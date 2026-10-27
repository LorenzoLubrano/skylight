//! Anteprime per lo Shelf: le stesse di Esplora file (miniature delle immagini,
//! icone dei programmi), generate su un thread a parte per non bloccare il notch.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{DeleteObject, HGDIOBJ, HPALETTE};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK,
};
use windows::core::{HSTRING, Result};

use super::Notifier;
use crate::media::Artwork;
use crate::util::Inbox;

/// Lato massimo delle anteprime: 44 DIP fino a scala 200%.
const THUMB_PX: u32 = 88;
/// Le chiavi delle anteprime hanno il bit alto acceso, per non confondersi con
/// quelle delle copertine nella cache delle immagini della GPU.
pub const THUMB_KEY: u64 = 1 << 63;

pub type Thumbnails = Inbox<(u64, Option<Artwork>)>;

/// Richieste: (id dell'elemento dello Shelf, percorso).
pub fn spawn(inbox: Arc<Thumbnails>, notifier: Notifier) -> Sender<(u64, String)> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("thumbnails".into())
        .spawn(move || run(rx, &inbox, notifier));
    tx
}

fn run(rx: Receiver<(u64, String)>, inbox: &Thumbnails, notifier: Notifier) {
    // Thread "MTA": può restare fermo in attesa senza far girare una coda di
    // messaggi; i generatori di anteprime che vogliono un STA li gestisce COM.
    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
        return;
    }
    let Ok(wic) = (unsafe {
        CoCreateInstance::<_, IWICImagingFactory>(
            &CLSID_WICImagingFactory,
            None,
            CLSCTX_INPROC_SERVER,
        )
    }) else {
        return;
    };
    while let Ok((id, path)) = rx.recv() {
        let art = thumbnail(&wic, &path).ok().map(|mut art| {
            art.id = THUMB_KEY | id;
            art
        });
        inbox.push((id, art));
        notifier.notify();
    }
}

fn thumbnail(wic: &IWICImagingFactory, path: &str) -> Result<Artwork> {
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(path), None)?;
        let size = SIZE {
            cx: THUMB_PX as i32,
            cy: THUMB_PX as i32,
        };
        let hbitmap = factory.GetImage(size, SIIGBF_BIGGERSIZEOK)?;
        let bitmap = wic.CreateBitmapFromHBITMAP(
            hbitmap,
            HPALETTE::default(),
            WICBitmapUsePremultipliedAlpha,
        );
        let _ = DeleteObject(HGDIOBJ(hbitmap.0));
        let bitmap = bitmap?;

        // Rimpicciolisce mantenendo le proporzioni.
        let (mut w, mut h) = (0, 0);
        bitmap.GetSize(&mut w, &mut h)?;
        let k = (THUMB_PX as f64 / w.max(h).max(1) as f64).min(1.0);
        let (sw, sh) = (
            ((w as f64 * k).round() as u32).max(1),
            ((h as f64 * k).round() as u32).max(1),
        );
        let scaler = wic.CreateBitmapScaler()?;
        scaler.Initialize(&bitmap, sw, sh, WICBitmapInterpolationModeHighQualityCubic)?;
        let converter = wic.CreateFormatConverter()?;
        converter.Initialize(
            &scaler,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let mut pixels = vec![0u8; (sw * sh * 4) as usize];
        converter.CopyPixels(std::ptr::null(), sw * 4, &mut pixels)?;
        Ok(Artwork {
            id: 0,
            width: sw,
            height: sh,
            pixels,
        })
    }
}
