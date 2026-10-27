//! Piccolo aiuto per WMI: connessione a un namespace e lettura delle proprietà.

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoSetProxyBlanket, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::{
    VARIANT, VT_BOOL, VT_BSTR, VT_I1, VT_I2, VT_I4, VT_R4, VT_R8, VT_UI1, VT_UI2, VT_UI4,
    VariantClear,
};
use windows::Win32::System::Wmi::{
    IEnumWbemClassObject, IWbemClassObject, IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WbemLocator,
};
use windows::core::{BSTR, HSTRING, IUnknown, PCWSTR, Param, Result};

/// Si collega a un namespace (es. `ROOT\WMI`). Il thread deve avere COM inizializzato.
pub fn connect(namespace: &str) -> Result<IWbemServices> {
    unsafe {
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
        let empty = BSTR::new();
        let services = locator.ConnectServer(
            &BSTR::from(namespace),
            &empty,
            &empty,
            &empty,
            0,
            &empty,
            None,
        )?;
        secure(&services)?;
        Ok(services)
    }
}

/// WMI vuole l'impersonazione per rispondere e consegnare eventi. Va impostata
/// sul proxy dell'interfaccia usata (servizi, enumeratore), non su un altro.
pub fn secure<P: Param<IUnknown>>(proxy: P) -> Result<()> {
    unsafe {
        CoSetProxyBlanket(
            proxy,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
    }
}

/// Esegue una query WQL e restituisce tutti gli oggetti trovati.
pub fn query(services: &IWbemServices, wql: &str) -> Vec<IWbemClassObject> {
    let flags = WBEM_FLAG_RETURN_IMMEDIATELY | WBEM_FLAG_FORWARD_ONLY;
    let Ok(results) =
        (unsafe { services.ExecQuery(&BSTR::from("WQL"), &BSTR::from(wql), flags, None) })
    else {
        return Vec::new();
    };
    let _ = secure(&results);
    collect(&results)
}

fn collect(results: &IEnumWbemClassObject) -> Vec<IWbemClassObject> {
    let mut out = Vec::new();
    loop {
        let mut objects = [None];
        let mut returned = 0;
        let status = unsafe { results.Next(2_000, &mut objects, &mut returned) };
        match objects[0].take() {
            Some(object) if returned == 1 => out.push(object),
            _ => break,
        }
        if status.is_err() {
            break;
        }
    }
    out
}

/// Legge una proprietà come numero (interi e decimali).
pub fn number(object: &IWbemClassObject, name: &str) -> Option<f64> {
    read(object, name, |v| unsafe {
        let inner = &v.Anonymous.Anonymous;
        let data = &inner.Anonymous;
        match inner.vt {
            VT_UI1 => Some(data.bVal as f64),
            VT_I1 => Some(data.cVal as u8 as f64),
            VT_UI2 => Some(data.uiVal as f64),
            VT_I2 => Some(data.iVal as f64),
            VT_UI4 => Some(data.ulVal as f64),
            VT_I4 => Some(data.lVal as f64),
            VT_R4 => Some(data.fltVal as f64),
            VT_R8 => Some(data.dblVal),
            _ => None,
        }
    })
}

pub fn boolean(object: &IWbemClassObject, name: &str) -> Option<bool> {
    read(object, name, |v| unsafe {
        let inner = &v.Anonymous.Anonymous;
        (inner.vt == VT_BOOL).then(|| inner.Anonymous.boolVal.as_bool())
    })
}

pub fn string(object: &IWbemClassObject, name: &str) -> Option<String> {
    read(object, name, |v| unsafe {
        let inner = &v.Anonymous.Anonymous;
        (inner.vt == VT_BSTR).then(|| inner.Anonymous.bstrVal.to_string())
    })
}

fn read<T>(
    object: &IWbemClassObject,
    name: &str,
    convert: impl FnOnce(&VARIANT) -> Option<T>,
) -> Option<T> {
    let mut value = VARIANT::default();
    let name = HSTRING::from(name);
    let result = unsafe { object.Get(&name, 0, &mut value, None, None) }
        .ok()
        .and_then(|()| convert(&value));
    let _ = unsafe { VariantClear(&mut value) };
    result
}
