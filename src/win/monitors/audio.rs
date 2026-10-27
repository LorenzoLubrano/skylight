//! Volume principale: Windows chiama `OnNotify` a ogni cambio (tasti, mixer,
//! altre app). Se cambia il dispositivo predefinito ci si riaggancia al nuovo.

use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::core::{PCWSTR, Result, implement};

use super::{Events, SystemEvent, emit};
use crate::win::Notifier;

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeCallback {
    inbox: Arc<Events>,
    notifier: Notifier,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeCallback_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        if let Some(data) = unsafe { data.as_ref() } {
            let event = SystemEvent::Volume {
                level: data.fMasterVolume,
                muted: data.bMuted.as_bool(),
            };
            emit(&self.inbox, &self.notifier, event);
        }
        Ok(())
    }
}

#[implement(IMMNotificationClient)]
struct DeviceCallback {
    changed: Sender<()>,
}

impl IMMNotificationClient_Impl for DeviceCallback_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> Result<()> {
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _: &PCWSTR) -> Result<()> {
        if flow == eRender && role == eConsole {
            let _ = self.changed.send(());
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> Result<()> {
        Ok(())
    }
}

pub fn run(inbox: Arc<Events>, notifier: Notifier) -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let (tx, rx) = mpsc::channel();
        let devices: IMMNotificationClient = DeviceCallback { changed: tx }.into();
        enumerator.RegisterEndpointNotificationCallback(&devices)?;
        let callback: IAudioEndpointVolumeCallback = VolumeCallback { inbox, notifier }.into();

        loop {
            let endpoint = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .and_then(|device| device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None));
            if let Ok(volume) = &endpoint {
                volume.RegisterControlChangeNotify(&callback)?;
            }
            // Dorme finché non cambia il dispositivo predefinito.
            if rx.recv().is_err() {
                return Ok(());
            }
            while rx.try_recv().is_ok() {}
            if let Ok(volume) = &endpoint {
                let _ = volume.UnregisterControlChangeNotify(&callback);
            }
        }
    }
}
