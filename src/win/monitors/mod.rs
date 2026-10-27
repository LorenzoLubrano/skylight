//! Osservatori di sistema: ognuno gira sul proprio thread, aspetta un evento di
//! Windows (niente polling) e lo consegna all'interfaccia come `SystemEvent`.

mod audio;
mod bluetooth;
mod brightness;
mod downloads;
mod network;

use std::sync::Arc;

use super::Notifier;
use crate::util::Inbox;

#[derive(Clone, Debug, PartialEq)]
pub enum SystemEvent {
    Volume {
        level: f32,
        muted: bool,
    },
    /// Luminosità dello schermo integrato, 0-100.
    Brightness(u8),
    Network {
        connected: bool,
        wifi: bool,
        name: String,
    },
    Bluetooth {
        name: String,
        connected: bool,
        battery: Option<u8>,
    },
    /// Un download è iniziato (o ripartito) oppure finito (o fermo).
    Downloading(bool),
}

pub type Events = Inbox<SystemEvent>;

/// Avvia tutti gli osservatori. Se uno non è disponibile (es. niente Bluetooth
/// o schermo esterno senza luminosità via WMI) il suo thread termina in silenzio.
pub fn spawn_all(inbox: &Arc<Events>, notifier: Notifier) {
    let start = |name: &str, run: fn(Arc<Events>, Notifier) -> windows::core::Result<()>| {
        let inbox = inbox.clone();
        let _ = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                if let Err(e) = run(inbox, notifier)
                    && cfg!(debug_assertions)
                {
                    eprintln!(
                        "monitor {}: {e}",
                        std::thread::current().name().unwrap_or("?")
                    );
                }
            });
    };
    start("audio", audio::run);
    start("brightness", brightness::run);
    start("network", network::run);
    start("bluetooth", bluetooth::run);
    start("downloads", downloads::run);
}

fn emit(inbox: &Events, notifier: &Notifier, event: SystemEvent) {
    inbox.push(event);
    notifier.notify();
}
