//! Skylight: un notch vivo in cima allo schermo di Windows.
//!
//! I moduli `anim`, `layout`, `lyrics`, `media` e `util` non dipendono da
//! Windows e si testano ovunque con `cargo test`; tutto ciò che parla con il
//! sistema operativo sta in `win`.

// In release niente finestra di console dietro al notch.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Fuori da Windows compiliamo solo la logica pura per i test.
#![cfg_attr(not(windows), allow(dead_code))]

mod anim;
mod features;
mod layout;
mod lyrics;
mod media;
mod migrate;
mod settings;
mod store;
mod textedit;
mod time;
mod util;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    if let Err(e) = win::run() {
        win::fatal(&e);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Skylight funziona solo su Windows 10/11. Qui puoi eseguire `cargo test`.");
}
