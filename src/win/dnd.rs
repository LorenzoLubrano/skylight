//! Trascinamento per lo Shelf: ricevere file e testo (`IDropTarget`) e
//! trascinarli fuori verso Esplora file, email o chat (`DoDragDrop`).
//! Più appunti e apertura dei file.

use std::cell::Cell;
use std::mem::ManuallyDrop;

use windows::Win32::Foundation::GlobalFree;
use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HANDLE, HGLOBAL, HWND,
    POINTL, S_OK,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::{
    DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::System::Ole::{
    CF_HDROP, CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_NONE,
    DoDragDrop, IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl, ReleaseStgMedium,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    BHID_DataObject, DragQueryFileW, HDROP, SHCreateDataObject, SHCreateShellItemArrayFromIDLists,
    SHParseDisplayName, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{BOOL, HRESULT, HSTRING, PCWSTR, Ref, Result, implement, w};

use super::app;

/// Ciò che arriva trascinando sul notch.
pub enum Payload {
    Files(Vec<String>),
    Text(String),
}

/// Riceve file e testo trascinati sul notch e li passa all'app.
#[implement(IDropTarget)]
#[derive(Default)]
pub struct DropTarget {
    /// Il trascinamento in corso è accettato (deciso in DragEnter).
    accepting: Cell<bool>,
}

fn set_effect(effect: *mut DROPEFFECT, accept: bool) {
    if let Some(effect) = unsafe { effect.as_mut() } {
        // "Copia": il file resta dov'è, lo Shelf ne ricorda solo il percorso.
        *effect = if accept {
            DROPEFFECT_COPY
        } else {
            DROPEFFECT_NONE
        };
    }
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        _: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let supported = data.ok().is_ok_and(has_payload);
        let accept = supported && app::drag_enter();
        self.accepting.set(accept);
        set_effect(effect, accept);
        Ok(())
    }

    fn DragOver(&self, _: MODIFIERKEYS_FLAGS, _: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        // Va reimpostato a ogni chiamata, altrimenti resta quello proposto dalla
        // sorgente (che potrebbe essere "sposta").
        set_effect(effect, self.accepting.get());
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        self.accepting.set(false);
        app::drag_leave();
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        _: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> Result<()> {
        let payload = data.ok().ok().and_then(read_payload);
        self.accepting.set(false);
        let accepted = app::drop(payload);
        set_effect(effect, accepted);
        Ok(())
    }
}

fn format(cf: u16) -> FORMATETC {
    FORMATETC {
        cfFormat: cf,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn has_payload(data: &IDataObject) -> bool {
    [CF_HDROP.0, CF_UNICODETEXT.0]
        .iter()
        .any(|&cf| unsafe { data.QueryGetData(&format(cf)) } == S_OK)
}

fn read_payload(data: &IDataObject) -> Option<Payload> {
    unsafe {
        if let Ok(mut medium) = data.GetData(&format(CF_HDROP.0)) {
            let drop = HDROP(medium.u.hGlobal.0);
            let count = DragQueryFileW(drop, u32::MAX, None);
            let files = (0..count)
                .map(|i| {
                    let len = DragQueryFileW(drop, i, None) as usize;
                    let mut buf = vec![0u16; len + 1];
                    DragQueryFileW(drop, i, Some(&mut buf));
                    String::from_utf16_lossy(&buf[..len])
                })
                .collect();
            ReleaseStgMedium(&mut medium);
            return Some(Payload::Files(files));
        }
        let mut medium = data.GetData(&format(CF_UNICODETEXT.0)).ok()?;
        let text = global_text(medium.u.hGlobal);
        ReleaseStgMedium(&mut medium);
        text.map(Payload::Text)
    }
}

/// Testo UTF-16 terminato da zero dentro un HGLOBAL.
unsafe fn global_text(handle: HGLOBAL) -> Option<String> {
    unsafe {
        let ptr = GlobalLock(handle) as *const u16;
        if ptr.is_null() {
            return None;
        }
        let max = GlobalSize(handle) / 2;
        let slice = std::slice::from_raw_parts(ptr, max);
        let len = slice.iter().position(|&c| c == 0).unwrap_or(max);
        let text = String::from_utf16_lossy(&slice[..len]);
        let _ = GlobalUnlock(handle);
        Some(text)
    }
}

fn text_to_global(text: &str) -> Result<HGLOBAL> {
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2)?;
        let ptr = GlobalLock(handle) as *mut u16;
        if ptr.is_null() {
            let _ = GlobalFree(Some(handle));
            return Err(windows::core::Error::from_thread());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
        let _ = GlobalUnlock(handle);
        Ok(handle)
    }
}

// ---- Trascinare fuori ---------------------------------------------------------------

#[implement(IDropSource)]
struct DropSource;

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if keys.0 & MK_LBUTTON.0 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Avvia il trascinamento (ritorna quando l'utente lascia il mouse).
pub fn drag_out(files: &[String], text: Option<&str>) -> Result<()> {
    let data = if !files.is_empty() {
        files_data_object(files)?
    } else if let Some(text) = text {
        text_data_object(text)?
    } else {
        return Ok(());
    };
    let source: IDropSource = DropSource.into();
    let mut effect = DROPEFFECT_NONE;
    unsafe {
        DoDragDrop(
            &data,
            &source,
            DROPEFFECT_COPY | DROPEFFECT_LINK,
            &mut effect,
        )
        .ok()
    }
}

/// Lo stesso oggetto che crea Esplora file: funziona con cartelle, email e chat.
fn files_data_object(paths: &[String]) -> Result<IDataObject> {
    unsafe {
        let mut pidls: Vec<*const ITEMIDLIST> = Vec::new();
        for path in paths {
            let mut pidl = std::ptr::null_mut();
            if SHParseDisplayName(&HSTRING::from(path.as_str()), None, &mut pidl, 0, None).is_ok() {
                pidls.push(pidl);
            }
        }
        let array = SHCreateShellItemArrayFromIDLists(&pidls);
        for pidl in pidls {
            CoTaskMemFree(Some(pidl as *const _));
        }
        array?.BindToHandler(None, &BHID_DataObject)
    }
}

fn text_data_object(text: &str) -> Result<IDataObject> {
    unsafe {
        let data: IDataObject = SHCreateDataObject(None, None, None)?;
        let medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 {
                hGlobal: text_to_global(text)?,
            },
            pUnkForRelease: ManuallyDrop::new(None),
        };
        data.SetData(&format(CF_UNICODETEXT.0), &medium, true)?;
        Ok(data)
    }
}

// ---- Appunti e apertura ---------------------------------------------------------------

pub fn copy_text(hwnd: HWND, text: &str) -> Result<()> {
    unsafe {
        OpenClipboard(Some(hwnd))?;
        let result = EmptyClipboard().and_then(|()| {
            let handle = text_to_global(text)?;
            SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(handle.0)))
                .map(|_| ())
                .inspect_err(|_| {
                    let _ = GlobalFree(Some(handle));
                })
        });
        let _ = CloseClipboard();
        result
    }
}

pub fn paste_text(hwnd: HWND) -> Option<String> {
    unsafe {
        OpenClipboard(Some(hwnd)).ok()?;
        let text = GetClipboardData(CF_UNICODETEXT.0 as u32)
            .ok()
            .and_then(|h| global_text(HGLOBAL(h.0)));
        let _ = CloseClipboard();
        text
    }
}

/// Apre un file o una cartella con il programma predefinito.
pub fn open_path(path: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(path),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}
