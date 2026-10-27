//! Pagina Musica: copertina, titolo, avanzamento, controlli e testo sincronizzato.

use windows::core::Result;

use super::*;
use crate::lyrics::LyricsState;
use crate::util::fmt_time;
use crate::win::sys;

const GLYPH_PREV: char = '\u{E892}';
const GLYPH_NEXT: char = '\u{E893}';
const GLYPH_LYRICS: char = '\u{E8F2}';
const GLYPH_MUSIC: char = '\u{EC4F}';
const GLYPH_VOLUME: [char; 4] = ['\u{E992}', '\u{E993}', '\u{E994}', '\u{E995}'];
const GLYPH_MUTE: char = '\u{E74F}';

/// Pagina Musica. Senza niente in riproduzione resta uguale, con i controlli spenti.
pub(super) fn draw(g: &mut Gfx, app: &App, alpha: f32) -> Result<()> {
    let l = &app.music;
    let hot = |h: Hit| app.hot == Some(h);
    let media = app.media.as_ref().filter(|m| m.has_track());

    match media.and_then(|m| m.art.as_ref()) {
        Some(art) => image(g, art, l.art, 12.0, alpha, false)?,
        None => {
            fill_round(g, l.art, 12.0, HOVER, alpha);
            glyph(g, GLYPH_MUSIC, &g.fonts.icon_xl, l.art, TEXT_DIM, alpha);
        }
    }
    match media {
        Some(m) => {
            text(g, &m.title, &g.fonts.title, l.title, TEXT, alpha);
            text(g, &m.artist, &g.fonts.artist, l.artist, TEXT_DIM, alpha);
        }
        None => {
            text(
                g,
                "Nessuna riproduzione",
                &g.fonts.title,
                l.title,
                TEXT,
                alpha,
            );
            text(
                g,
                "Avvia la musica in Spotify, YouTube o un'altra app",
                &g.fonts.artist,
                l.artist,
                TEXT_DIM,
                alpha,
            );
        }
    }

    // Riga centrale: avanzamento del brano, oppure il volume.
    if app.volume_mode {
        let (level, muted) = app.volume.unwrap_or((0.0, false));
        let icon = if muted {
            GLYPH_MUTE
        } else {
            GLYPH_VOLUME[((level * 3.0).ceil() as usize).min(3)]
        };
        glyph(g, icon, &g.fonts.icon_small, l.time_left, PINK, alpha);
        let shown = if muted { 0.0 } else { level };
        progress(g, l.bar, shown, TEXT, alpha);
        knob(g, l, shown, alpha);
        text(
            g,
            &format!("{:.0}%", shown * 100.0),
            &g.fonts.small_right,
            l.time_right,
            TEXT_DIM,
            alpha,
        );
    } else {
        let (position, duration) = media.map_or((0, 0), |m| {
            (m.position_at(sys::now_filetime()), m.duration_ms)
        });
        let fraction = if duration > 0 {
            position as f32 / duration as f32
        } else {
            0.0
        };
        progress(g, l.bar, fraction, TEXT, alpha);
        if hot(Hit::Seek) && media.is_some() {
            knob(g, l, fraction, alpha);
        }
        let remaining = duration.saturating_sub(position);
        text(
            g,
            &fmt_time(position),
            &g.fonts.small,
            l.time_left,
            TEXT_DIM,
            alpha,
        );
        text(
            g,
            &format!("-{}", fmt_time(remaining)),
            &g.fonts.small_right,
            l.time_right,
            TEXT_DIM,
            alpha,
        );
    }

    // Controlli: testo a sinistra, brano precedente/play/successivo al centro, volume a destra.
    let enabled = if media.is_some() { TEXT } else { TEXT_FAINT };
    let playing = media.is_some_and(|m| m.playing);
    let play = if playing { GLYPH_PAUSE } else { GLYPH_PLAY };
    icon_button(
        g,
        l.prev,
        GLYPH_PREV,
        &g.fonts.icon,
        hot(Hit::Prev),
        enabled,
        alpha,
    );
    icon_button(
        g,
        l.play,
        play,
        &g.fonts.icon_big,
        hot(Hit::PlayPause),
        enabled,
        alpha,
    );
    icon_button(
        g,
        l.next,
        GLYPH_NEXT,
        &g.fonts.icon,
        hot(Hit::Next),
        enabled,
        alpha,
    );
    let lyrics_color = if app.settings.lyrics { PINK } else { TEXT_DIM };
    icon_button(
        g,
        l.lyrics_btn,
        GLYPH_LYRICS,
        &g.fonts.icon,
        hot(Hit::Lyrics),
        lyrics_color,
        alpha,
    );
    let volume_icon = match app.volume {
        Some((_, true)) => GLYPH_MUTE,
        Some((level, false)) => GLYPH_VOLUME[((level * 3.0).ceil() as usize).min(3)],
        None => GLYPH_VOLUME[2],
    };
    icon_button(
        g,
        l.volume_btn,
        volume_icon,
        &g.fonts.icon,
        hot(Hit::Volume) || app.volume_mode,
        if app.volume_mode { PINK } else { TEXT_DIM },
        alpha,
    );

    if app.settings.lyrics && media.is_some() {
        draw_lyrics(g, app, alpha);
    }
    Ok(())
}

/// Pallino bianco sulla barra, nel punto `fraction`.
fn knob(g: &Gfx, l: &MusicLayout, fraction: f32, alpha: f32) {
    let r = Rect::new(
        l.bar.x + l.bar.w * fraction.clamp(0.0, 1.0) - 5.0,
        l.bar.center_y() - 5.0,
        10.0,
        10.0,
    );
    fill_round(g, r, 5.0, TEXT, alpha);
}

fn draw_lyrics(g: &Gfx, app: &App, alpha: f32) {
    let l = &app.music;
    let status = |message: &str| {
        text(
            g,
            message,
            &g.fonts.lyric_dim,
            l.lyric_lines[1],
            TEXT_DIM,
            alpha,
        )
    };
    let lyrics = match app.current_lyrics() {
        None | Some(LyricsState::Loading) => return status("Cerco il testo…"),
        Some(LyricsState::Instrumental) => return status("Brano strumentale ♪"),
        Some(LyricsState::Unsynced) => return status("C'è solo il testo non sincronizzato"),
        Some(LyricsState::Missing) => return status("Testo non trovato"),
        Some(LyricsState::Failed) => return status("LRCLIB non raggiungibile"),
        Some(LyricsState::Synced(lyrics)) => lyrics,
    };
    let line = |i: usize| lyrics.lines.get(i).map(|l| l.text.as_str());
    let (prev, current, next) = match app.line {
        Some(i) => (i.checked_sub(1).and_then(line), line(i), line(i + 1)),
        None => (None, None, line(0)),
    };
    // Le righe scorrono in su quando si passa alla successiva.
    let dy = app.scroll.value() * (l.lyric_lines[1].y - l.lyric_lines[0].y);
    let color = |offset: i8| {
        if app.hot == Some(Hit::LyricLine(offset)) {
            TEXT_DIM
        } else {
            TEXT_FAINT
        }
    };
    if let Some(prev) = prev {
        text(
            g,
            or_note(prev),
            &g.fonts.lyric_dim,
            l.lyric_lines[0].offset_y(dy),
            color(-1),
            alpha,
        );
    }
    text(
        g,
        or_note(current.unwrap_or("")),
        &g.fonts.lyric_current,
        l.lyric_lines[1].offset_y(dy),
        TEXT,
        alpha,
    );
    if let Some(next) = next {
        text(
            g,
            or_note(next),
            &g.fonts.lyric_dim,
            l.lyric_lines[2].offset_y(dy),
            color(1),
            alpha,
        );
    }
}

/// Le righe vuote del testo sono pause strumentali.
fn or_note(line: &str) -> &str {
    if line.trim().is_empty() { "♪" } else { line }
}
