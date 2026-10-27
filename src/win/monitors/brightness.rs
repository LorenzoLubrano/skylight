//! Luminosità dello schermo del portatile: evento WMI `WmiMonitorBrightnessEvent`.
//! Gli schermi esterni di solito non lo inviano: in quel caso il thread dorme e basta.

use std::sync::Arc;

use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Wmi::{
    WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
};
use windows::core::{BSTR, Result};

use super::{Events, SystemEvent, emit};
use crate::win::{Notifier, wmi};

pub fn run(inbox: Arc<Events>, notifier: Notifier) -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let services = wmi::connect("ROOT\\WMI")?;
        let events = services.ExecNotificationQuery(
            &BSTR::from("WQL"),
            &BSTR::from("SELECT * FROM WmiMonitorBrightnessEvent"),
            WBEM_FLAG_RETURN_IMMEDIATELY | WBEM_FLAG_FORWARD_ONLY,
            None,
        )?;
        let _ = wmi::secure(&events);

        loop {
            let mut objects = [None];
            let mut returned = 0;
            // Bloccante: il thread dorme finché non arriva un evento.
            events
                .Next(WBEM_INFINITE, &mut objects, &mut returned)
                .ok()?;
            let Some(object) = objects[0].take() else {
                continue;
            };
            if let Some(level) = wmi::number(&object, "Brightness") {
                emit(
                    &inbox,
                    &notifier,
                    SystemEvent::Brightness(level.clamp(0.0, 100.0) as u8),
                );
            }
        }
    }
}
