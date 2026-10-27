//! Dispositivi Bluetooth associati: un `DeviceWatcher` segnala quando uno si
//! connette o si disconnette. Si avvisa solo per cuffie e altoparlanti (classe
//! "audio/video") o dispositivi di classe sconosciuta, non per mouse e tastiere.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use windows::Devices::Bluetooth::BluetoothDevice;
use windows::Devices::Enumeration::Pnp::{PnpObject, PnpObjectType};
use windows::Devices::Enumeration::{
    DeviceInformation, DeviceInformationKind, DeviceInformationUpdate, DeviceWatcher,
    DeviceWatcherStatus,
};
use windows::Foundation::{IReference, TypedEventHandler};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::core::{GUID, HSTRING, IInspectable, Interface, Ref, Result};
use windows_collections::{IIterable, IMapView};

use super::{Events, SystemEvent, emit};
use crate::win::Notifier;

const IS_CONNECTED: &str = "System.Devices.Aep.IsConnected";
const MAJOR_CLASS: &str = "System.Devices.Aep.Bluetooth.Cod.Major";
const CONTAINER: &str = "System.Devices.Aep.ContainerId";
/// Livello batteria che Windows mostra in Impostazioni > Bluetooth (non documentato ufficialmente).
const BATTERY: &str = "{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2";
/// Classe "Audio/Video" del Bluetooth Class of Device.
const MAJOR_AUDIO: u32 = 4;

enum Msg {
    Added {
        id: String,
        name: String,
        connected: bool,
        major: Option<u32>,
        container: Option<GUID>,
    },
    Updated {
        id: String,
        connected: Option<bool>,
    },
    Removed {
        id: String,
    },
    Ready,
    Stopped,
}

struct Device {
    name: String,
    connected: bool,
    major: Option<u32>,
    container: Option<GUID>,
}

pub fn run(inbox: Arc<Events>, notifier: Notifier) -> Result<()> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    // Se il servizio Bluetooth si riavvia il watcher si interrompe: se ne crea uno nuovo.
    loop {
        let _ = watch(&inbox, &notifier);
        std::thread::sleep(Duration::from_secs(10));
    }
}

/// Segue i dispositivi finché il watcher resta attivo.
fn watch(inbox: &Events, notifier: &Notifier) -> Result<()> {
    let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(true)?;
    let properties: IIterable<HSTRING> = vec![
        HSTRING::from(IS_CONNECTED),
        HSTRING::from(MAJOR_CLASS),
        HSTRING::from(CONTAINER),
    ]
    .into();
    let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
        &selector,
        &properties,
        DeviceInformationKind::AssociationEndpoint,
    )?;

    let (tx, rx) = mpsc::channel();
    subscribe(&watcher, tx)?;
    watcher.Start()?;

    let mut devices: HashMap<String, Device> = HashMap::new();
    let mut ready = false;
    loop {
        let msg = match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(msg) => msg,
            // Nessun evento da un po': si controlla che il watcher sia ancora vivo.
            Err(RecvTimeoutError::Timeout) => match watcher.Status()? {
                DeviceWatcherStatus::Aborted | DeviceWatcherStatus::Stopped => break,
                _ => continue,
            },
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match msg {
            Msg::Added {
                id,
                name,
                connected,
                major,
                container,
            } => {
                devices.insert(
                    id,
                    Device {
                        name,
                        connected,
                        major,
                        container,
                    },
                );
            }
            Msg::Updated {
                id,
                connected: Some(connected),
            } => {
                let Some(device) = devices.get_mut(&id) else {
                    continue;
                };
                if device.connected == connected {
                    continue;
                }
                device.connected = connected;
                let relevant = device.major.is_none_or(|m| m == MAJOR_AUDIO);
                if ready && relevant {
                    let name = device.name.clone();
                    let event = SystemEvent::Bluetooth {
                        name: name.clone(),
                        connected,
                        battery: None,
                    };
                    emit(inbox, notifier, event);
                    // La batteria compare qualche secondo dopo la connessione.
                    if connected && let Some(container) = device.container {
                        std::thread::sleep(Duration::from_secs(3));
                        if let Some(level) = battery_level(container) {
                            let event = SystemEvent::Bluetooth {
                                name,
                                connected,
                                battery: Some(level),
                            };
                            emit(inbox, notifier, event);
                        }
                    }
                }
            }
            Msg::Updated { .. } => {}
            Msg::Removed { id } => {
                devices.remove(&id);
            }
            // Fine dell'elenco iniziale: da qui in poi i cambi sono "veri".
            Msg::Ready => ready = true,
            Msg::Stopped => break,
        }
    }
    let _ = watcher.Stop();
    Ok(())
}

/// Windows chiede di gestire Added, Updated e Removed: senza Removed il watcher
/// può smettere di mandare aggiornamenti dopo l'elenco iniziale.
fn subscribe(watcher: &DeviceWatcher, tx: Sender<Msg>) -> Result<()> {
    let added = tx.clone();
    watcher.Added(&TypedEventHandler::new(
        move |_, info: Ref<DeviceInformation>| {
            if let Ok(info) = info.ok() {
                let props = info.Properties()?;
                let _ = added.send(Msg::Added {
                    id: info.Id()?.to_string(),
                    name: info.Name()?.to_string(),
                    connected: lookup::<bool>(&props, IS_CONNECTED).unwrap_or(false),
                    major: lookup::<u32>(&props, MAJOR_CLASS)
                        .or_else(|| lookup::<u16>(&props, MAJOR_CLASS).map(u32::from))
                        .or_else(|| lookup::<u8>(&props, MAJOR_CLASS).map(u32::from)),
                    container: lookup::<GUID>(&props, CONTAINER),
                });
            }
            Ok(())
        },
    ))?;
    let updated = tx.clone();
    watcher.Updated(&TypedEventHandler::new(
        move |_, update: Ref<DeviceInformationUpdate>| {
            if let Ok(update) = update.ok() {
                let connected = lookup::<bool>(&update.Properties()?, IS_CONNECTED);
                let _ = updated.send(Msg::Updated {
                    id: update.Id()?.to_string(),
                    connected,
                });
            }
            Ok(())
        },
    ))?;
    let removed = tx.clone();
    watcher.Removed(&TypedEventHandler::new(
        move |_, update: Ref<DeviceInformationUpdate>| {
            if let Ok(update) = update.ok() {
                let _ = removed.send(Msg::Removed {
                    id: update.Id()?.to_string(),
                });
            }
            Ok(())
        },
    ))?;
    let ready = tx.clone();
    watcher.EnumerationCompleted(&TypedEventHandler::new(move |_, _: Ref<IInspectable>| {
        let _ = ready.send(Msg::Ready);
        Ok(())
    }))?;
    watcher.Stopped(&TypedEventHandler::new(move |_, _: Ref<IInspectable>| {
        let _ = tx.send(Msg::Stopped);
        Ok(())
    }))?;
    Ok(())
}

fn lookup<T>(props: &IMapView<HSTRING, IInspectable>, key: &str) -> Option<T>
where
    T: windows::core::RuntimeType + 'static,
    IReference<T>: Interface,
{
    let value = props.Lookup(&HSTRING::from(key)).ok()?;
    value.cast::<IReference<T>>().ok()?.Value().ok()
}

/// Cerca tra i nodi del dispositivo quello che espone il livello della batteria.
fn battery_level(container: GUID) -> Option<u8> {
    let properties: IIterable<HSTRING> = vec![HSTRING::from(BATTERY)].into();
    let filter = HSTRING::from(format!("System.Devices.ContainerId:=\"{{{container:?}}}\""));
    let objects = PnpObject::FindAllAsyncAqsFilter(PnpObjectType::Device, &properties, &filter)
        .ok()?
        .join()
        .ok()?;
    (0..objects.Size().ok()?)
        .filter_map(|i| objects.GetAt(i).ok())
        .find_map(|object| lookup::<u8>(&object.Properties().ok()?, BATTERY))
        .map(|level| level.min(100))
}
