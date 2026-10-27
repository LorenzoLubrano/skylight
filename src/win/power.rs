//! Alimentazione: caricatore collegato/scollegato e batteria scarica. Windows
//! manda `WM_POWERBROADCAST` alla finestra; il primo valore dopo la
//! registrazione è lo stato attuale e non va mostrato.

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Power::{
    GetSystemPowerStatus, HPOWERNOTIFY, POWERBROADCAST_SETTING, RegisterPowerSettingNotification,
    SYSTEM_POWER_STATUS, UnregisterPowerSettingNotification,
};
use windows::Win32::System::SystemServices::{
    GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DEVICE_NOTIFY_WINDOW_HANDLE, PBT_POWERSETTINGCHANGE,
};

pub enum PowerEvent {
    Plugged(u32),
    Unplugged(u32),
    Low(u32),
}

pub struct Power {
    handles: Vec<HPOWERNOTIFY>,
    on_ac: Option<bool>,
    percent: Option<u32>,
    /// Soglia più bassa già segnalata in questa scarica (100 = nessuna).
    warned: u32,
}

impl Power {
    pub fn register(hwnd: HWND) -> Self {
        let mut status = SYSTEM_POWER_STATUS::default();
        let has_battery = unsafe { GetSystemPowerStatus(&mut status) }.is_ok()
            && status.BatteryFlag != 255
            && status.BatteryFlag & 128 == 0;
        let handles = if has_battery {
            [GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING]
                .iter()
                .filter_map(|guid| unsafe {
                    RegisterPowerSettingNotification(
                        HANDLE(hwnd.0),
                        guid,
                        DEVICE_NOTIFY_WINDOW_HANDLE,
                    )
                    .ok()
                })
                .collect()
        } else {
            Vec::new() // PC fisso: niente batteria, niente avvisi
        };
        // Stato di partenza: la prima notifica di Windows potrebbe arrivare
        // prima che l'app sia pronta a riceverla.
        let known = !handles.is_empty();
        Self {
            handles,
            on_ac: known.then_some(status.ACLineStatus == 1),
            percent: (known && status.BatteryLifePercent <= 100)
                .then_some(status.BatteryLifePercent as u32),
            warned: 100,
        }
    }

    pub fn on_message(&mut self, wp: WPARAM, lp: LPARAM) -> Option<PowerEvent> {
        if wp.0 as u32 != PBT_POWERSETTINGCHANGE || lp.0 == 0 || self.handles.is_empty() {
            return None;
        }
        let setting = unsafe { &*(lp.0 as *const POWERBROADCAST_SETTING) };
        if setting.DataLength < 4 {
            return None;
        }
        let value = unsafe { std::ptr::read_unaligned(setting.Data.as_ptr() as *const u32) };

        if setting.PowerSetting == GUID_ACDC_POWER_SOURCE {
            let on_ac = value == 0; // 0 = rete, 1 = batteria, 2 = UPS
            let previous = self.on_ac.replace(on_ac);
            if on_ac {
                self.warned = 100;
            }
            let percent = current_percent().or(self.percent).unwrap_or(0);
            return match previous {
                Some(was) if was != on_ac => Some(if on_ac {
                    PowerEvent::Plugged(percent)
                } else {
                    PowerEvent::Unplugged(percent)
                }),
                _ => None,
            };
        }
        if setting.PowerSetting == GUID_BATTERY_PERCENTAGE_REMAINING {
            self.percent = Some(value);
            if self.on_ac == Some(false) {
                for threshold in [10, 20] {
                    if value <= threshold && self.warned > threshold {
                        self.warned = threshold;
                        return Some(PowerEvent::Low(value));
                    }
                }
            }
        }
        None
    }
}

impl Drop for Power {
    fn drop(&mut self) {
        for handle in self.handles.drain(..) {
            let _ = unsafe { UnregisterPowerSettingNotification(handle) };
        }
    }
}

fn current_percent() -> Option<u32> {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    (status.BatteryLifePercent <= 100).then_some(status.BatteryLifePercent as u32)
}

/// Stato della batteria da mostrare nel notch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

/// Carica attuale; `None` sui PC senza batteria (o se Windows non la conosce).
pub fn battery() -> Option<Battery> {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    let present = status.BatteryFlag != 255 && status.BatteryFlag & 128 == 0;
    (present && status.BatteryLifePercent <= 100).then_some(Battery {
        percent: status.BatteryLifePercent,
        charging: status.ACLineStatus == 1,
    })
}
