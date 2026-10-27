//! Connessione di rete: `NetworkStatusChanged` di WinRT. Gli eventi arrivano a
//! raffica durante una riconnessione, quindi si aspetta che la situazione si
//! stabilizzi e si avvisa solo se è davvero cambiato qualcosa.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use windows::Networking::Connectivity::{
    NetworkConnectivityLevel, NetworkInformation, NetworkStatusChangedEventHandler,
};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::core::Result;

use super::{Events, SystemEvent, emit};
use crate::win::Notifier;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct State {
    connected: bool,
    wifi: bool,
    name: String,
}

fn current() -> State {
    let Ok(profile) = NetworkInformation::GetInternetConnectionProfile() else {
        return State::default();
    };
    let level = profile
        .GetNetworkConnectivityLevel()
        .unwrap_or(NetworkConnectivityLevel::None);
    State {
        connected: level != NetworkConnectivityLevel::None,
        wifi: profile.IsWlanConnectionProfile().unwrap_or(false),
        // Per il Wi-Fi il nome del profilo è il nome della rete, senza
        // bisogno del permesso di posizione che richiede l'SSID.
        name: profile
            .ProfileName()
            .map(|n| n.to_string())
            .unwrap_or_default(),
    }
}

pub fn run(inbox: Arc<Events>, notifier: Notifier) -> Result<()> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let (tx, rx) = mpsc::channel();
    NetworkInformation::NetworkStatusChanged(&NetworkStatusChangedEventHandler::new(move |_| {
        let _ = tx.send(());
        Ok(())
    }))?;

    let mut last = current();
    while rx.recv().is_ok() {
        std::thread::sleep(Duration::from_millis(1500));
        while rx.try_recv().is_ok() {}
        let now = current();
        let changed = now.connected != last.connected || (now.connected && now.name != last.name);
        if changed {
            let event = SystemEvent::Network {
                connected: now.connected,
                wifi: now.wifi,
                name: now.name.clone(),
            };
            emit(&inbox, &notifier, event);
        }
        last = now;
    }
    Ok(())
}
