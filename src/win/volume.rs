//! Volume dell'uscita audio predefinita, letto e regolato dalla pagina Musica.
//! I cambi fatti da qui arrivano anche al monitor del volume come quelli fatti
//! con la tastiera.

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::{GUID, Result};

fn endpoint() -> Result<IAudioEndpointVolume> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)?
            .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
    }
}

/// Livello (0..1) e silenzioso sì/no.
pub fn get() -> Option<(f32, bool)> {
    let volume = endpoint().ok()?;
    unsafe {
        let level = volume.GetMasterVolumeLevelScalar().ok()?;
        let muted = volume.GetMute().ok()?.as_bool();
        Some((level, muted))
    }
}

/// Imposta il livello; alzarlo toglie anche il silenzioso.
pub fn set(level: f32) -> Result<()> {
    let volume = endpoint()?;
    let level = level.clamp(0.0, 1.0);
    unsafe {
        volume.SetMasterVolumeLevelScalar(level, &GUID::zeroed())?;
        if level > 0.0 {
            volume.SetMute(false, &GUID::zeroed())?;
        }
    }
    Ok(())
}
