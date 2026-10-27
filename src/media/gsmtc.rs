//! Musica da qualsiasi player tramite le Global System Media Transport Controls:
//! lo stesso sistema che alimenta il riquadro multimediale di Windows (Spotify,
//! Apple Music, browser con YouTube, ...).
//!
//! Tutto gira su un thread dedicato: gli eventi di Windows arrivano su thread
//! del sistema, vengono messi in coda e qui trasformati in `MediaSnapshot`
//! completi, consegnati all'interfaccia tramite `Inbox` + `Notifier`.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as SessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};
use windows::Storage::Streams::IRandomAccessStreamReference;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IStream};
use windows::Win32::System::WinRT::{
    CreateStreamOverRandomAccessStream, RO_INIT_MULTITHREADED, RoInitialize,
};
use windows::core::{Result, RuntimeType};

use super::{Artwork, MediaCommand, MediaSnapshot};
use crate::util::Inbox;
use crate::win::{Notifier, now_filetime};

/// Lato della copertina decodificata: basta per 70 DIP a scala 200%.
const ART_PX: u32 = 144;

enum Event {
    SessionChanged,
    Properties,
    Playback,
    Timeline,
    Command(MediaCommand),
}

/// Per inviare comandi (play, salta, ...) al player attivo.
#[derive(Clone)]
pub struct MediaControl(Sender<Event>);

impl MediaControl {
    pub fn send(&self, command: MediaCommand) {
        let _ = self.0.send(Event::Command(command));
    }
}

pub fn spawn(inbox: Arc<Inbox<Option<MediaSnapshot>>>, notifier: Notifier) -> MediaControl {
    let (tx, rx) = mpsc::channel();
    let events = tx.clone();
    std::thread::Builder::new()
        .name("media".into())
        .spawn(move || {
            if let Err(e) = run(events, rx, &inbox, notifier)
                && cfg!(debug_assertions)
            {
                eprintln!("media: {e}");
            }
        })
        .expect("impossibile avviare il thread media");
    MediaControl(tx)
}

/// Eventi accumulati: in un colpo solo se ne possono ricevere molti.
#[derive(Default)]
struct Pending {
    session: bool,
    properties: bool,
    playback: bool,
    timeline: bool,
    commands: Vec<MediaCommand>,
}

impl Pending {
    fn add(&mut self, e: Event) {
        match e {
            Event::SessionChanged => self.session = true,
            Event::Properties => self.properties = true,
            Event::Playback => self.playback = true,
            Event::Timeline => self.timeline = true,
            Event::Command(c) => self.commands.push(c),
        }
    }
}

fn run(
    tx: Sender<Event>,
    rx: Receiver<Event>,
    inbox: &Inbox<Option<MediaSnapshot>>,
    notifier: Notifier,
) -> Result<()> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let wic: IWICImagingFactory =
        unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? };
    let manager = SessionManager::RequestAsync()?.join()?;
    let events = tx.clone();
    manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
        let _ = events.send(Event::SessionChanged);
        Ok(())
    }))?;

    let mut worker = Worker {
        tx,
        wic,
        attached: None,
        snapshot: None,
        art_id: 0,
        rebased_at: 0,
        status_known: false,
    };
    let _ = worker.tx.send(Event::SessionChanged);

    while let Ok(first) = rx.recv() {
        let mut pending = Pending::default();
        pending.add(first);
        while let Ok(more) = rx.try_recv() {
            pending.add(more);
        }

        if pending.session {
            worker.attach(&manager);
            pending.properties = true;
            pending.playback = true;
            pending.timeline = true;
        }
        // Brano nuovo = durata nuova: rileggiamo subito anche la timeline.
        pending.timeline |= pending.properties;
        for command in pending.commands {
            worker.execute(command);
        }
        if worker.attached.is_some() {
            // L'ordine conta: lo stato di riproduzione serve per valutare la timeline.
            if pending.properties {
                let _ = worker.refresh_properties();
            }
            if pending.playback {
                let _ = worker.refresh_playback();
            }
            if pending.timeline {
                let _ = worker.refresh_timeline();
            }
        }
        inbox.push(worker.snapshot.clone());
        notifier.notify();
    }
    Ok(())
}

/// Gestore di evento che si limita a mettere in coda `event()`.
fn forward<A, B>(tx: &Sender<Event>, event: fn() -> Event) -> TypedEventHandler<A, B>
where
    A: RuntimeType + 'static,
    B: RuntimeType + 'static,
{
    let tx = tx.clone();
    TypedEventHandler::new(move |_, _| {
        let _ = tx.send(event());
        Ok(())
    })
}

struct Attached {
    session: Session,
    tokens: [i64; 3],
}

struct Worker {
    tx: Sender<Event>,
    wic: IWICImagingFactory,
    attached: Option<Attached>,
    snapshot: Option<MediaSnapshot>,
    art_id: u64,
    /// Ultima volta in cui abbiamo ricalcolato noi la posizione (pausa, salto, ...):
    /// una timeline più vecchia di così è superata e va ignorata.
    rebased_at: i64,
    status_known: bool,
}

impl Worker {
    fn attach(&mut self, manager: &SessionManager) {
        if let Some(old) = self.attached.take() {
            let _ = old.session.RemoveMediaPropertiesChanged(old.tokens[0]);
            let _ = old.session.RemovePlaybackInfoChanged(old.tokens[1]);
            let _ = old.session.RemoveTimelinePropertiesChanged(old.tokens[2]);
        }
        self.snapshot = None;
        self.rebased_at = 0;
        self.status_known = false;

        let Ok(session) = manager.GetCurrentSession() else {
            return;
        };
        let tokens = [
            session.MediaPropertiesChanged(&forward(&self.tx, || Event::Properties)),
            session.PlaybackInfoChanged(&forward(&self.tx, || Event::Playback)),
            session.TimelinePropertiesChanged(&forward(&self.tx, || Event::Timeline)),
        ];
        let [Ok(a), Ok(b), Ok(c)] = tokens else {
            return;
        };
        self.attached = Some(Attached {
            session,
            tokens: [a, b, c],
        });
        self.snapshot = Some(MediaSnapshot {
            rate: 1.0,
            ..Default::default()
        });
    }

    fn session(&self) -> Option<&Session> {
        self.attached.as_ref().map(|a| &a.session)
    }

    fn execute(&mut self, command: MediaCommand) {
        let Some(session) = self.session() else {
            return;
        };
        let op = match command {
            MediaCommand::TogglePlayPause => session.TryTogglePlayPauseAsync(),
            MediaCommand::Next => session.TrySkipNextAsync(),
            MediaCommand::Previous => session.TrySkipPreviousAsync(),
            MediaCommand::Seek(ms) => {
                let start = session
                    .GetTimelineProperties()
                    .and_then(|t| t.StartTime())
                    .map_or(0, |t| t.Duration);
                session.TryChangePlaybackPositionAsync(start + ms as i64 * 10_000)
            }
        };
        let accepted = op.and_then(|op| op.join()).unwrap_or(false);
        if let (true, MediaCommand::Seek(ms), Some(s)) = (accepted, command, self.snapshot.as_mut())
        {
            // Non tutti i player notificano subito il salto: aggiorniamo noi.
            let now = now_filetime();
            s.position_ms = ms;
            s.updated_at = now;
            self.rebased_at = now;
        }
    }

    fn refresh_properties(&mut self) -> Result<()> {
        let Some(session) = self.session() else {
            return Ok(());
        };
        let props = session.TryGetMediaPropertiesAsync()?.join()?;
        let art = props
            .Thumbnail()
            .ok()
            .and_then(|r| decode_artwork(&self.wic, &r).ok());
        let Some(s) = self.snapshot.as_mut() else {
            return Ok(());
        };
        s.title = props.Title()?.to_string();
        s.artist = props.Artist()?.to_string();
        s.album = props
            .AlbumTitle()
            .map(|a| a.to_string())
            .unwrap_or_default();
        s.art = art.map(|mut a| {
            self.art_id += 1;
            a.id = self.art_id;
            Arc::new(a)
        });
        Ok(())
    }

    fn refresh_playback(&mut self) -> Result<()> {
        let Some(session) = self.session() else {
            return Ok(());
        };
        let info = session.GetPlaybackInfo()?;
        let playing = info.PlaybackStatus()? == PlaybackStatus::Playing;
        let rate = info.PlaybackRate().and_then(|r| r.Value()).unwrap_or(1.0);
        let Some(s) = self.snapshot.as_mut() else {
            return Ok(());
        };
        if self.status_known && playing != s.playing {
            // Fissiamo la posizione prima del cambio, altrimenti alla ripresa
            // conteremmo come "ascoltato" anche il tempo passato in pausa.
            let now = now_filetime();
            s.rebase(now);
            self.rebased_at = now;
        }
        s.playing = playing;
        s.rate = if rate > 0.0 { rate } else { 1.0 };
        self.status_known = true;
        Ok(())
    }

    fn refresh_timeline(&mut self) -> Result<()> {
        let Some(session) = self.session() else {
            return Ok(());
        };
        let t = session.GetTimelineProperties()?;
        let start = t.StartTime()?.Duration;
        let end = t.EndTime()?.Duration;
        let position = t.Position()?.Duration;
        let updated = t.LastUpdatedTime()?.UniversalTime;
        let Some(s) = self.snapshot.as_mut() else {
            return Ok(());
        };

        s.duration_ms = ((end - start).max(0) / 10_000) as u64;
        if updated != 0 && updated < self.rebased_at {
            return Ok(()); // la nostra stima è più recente di quella del player
        }
        let now = now_filetime();
        s.position_ms = ((position - start).max(0) / 10_000) as u64;
        s.updated_at = if updated > 0 && updated <= now {
            updated
        } else {
            now
        };
        Ok(())
    }
}

/// Decodifica la copertina con WIC: lato corto a `ART_PX`, ritaglio quadrato al
/// centro (le miniature di YouTube sono 16:9), pixel premoltiplicati per Direct2D.
fn decode_artwork(
    wic: &IWICImagingFactory,
    reference: &IRandomAccessStreamReference,
) -> Result<Artwork> {
    let stream = reference.OpenReadAsync()?.join()?;
    unsafe {
        let stream: IStream = CreateStreamOverRandomAccessStream(&stream)?;
        let decoder =
            wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h)?;
        if w == 0 || h == 0 {
            return Err(E_FAIL.into());
        }

        let k = ART_PX as f64 / w.min(h) as f64;
        let sw = ((w as f64 * k).round() as u32).max(ART_PX);
        let sh = ((h as f64 * k).round() as u32).max(ART_PX);
        let scaler = wic.CreateBitmapScaler()?;
        scaler.Initialize(&frame, sw, sh, WICBitmapInterpolationModeHighQualityCubic)?;

        let clipper = wic.CreateBitmapClipper()?;
        let crop = WICRect {
            X: ((sw - ART_PX) / 2) as i32,
            Y: ((sh - ART_PX) / 2) as i32,
            Width: ART_PX as i32,
            Height: ART_PX as i32,
        };
        clipper.Initialize(&scaler, &crop)?;

        let converter = wic.CreateFormatConverter()?;
        converter.Initialize(
            &clipper,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;

        let stride = ART_PX * 4;
        let mut pixels = vec![0u8; (stride * ART_PX) as usize];
        converter.CopyPixels(std::ptr::null(), stride, &mut pixels)?;
        Ok(Artwork {
            id: 0,
            width: ART_PX,
            height: ART_PX,
            pixels,
        })
    }
}
