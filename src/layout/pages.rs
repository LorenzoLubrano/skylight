//! Pagine del notch aperto: cornice comune (intestazione, frecce, pallini e
//! barra delle icone) e disposizione di ogni pagina.
//!
//! Il contenuto di ogni pagina comincia sotto l'intestazione (`HEADER_H`) ed è
//! largo `CONTENT_W` a partire da `content_x()`.

use super::{CONTENT_W, FOOTER_H, HEADER_H, OPEN_W, Rect, content_x, open_x};
use crate::features::pomodoro::Phase;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Music,
    Focus,
    Water,
    Shelf,
    Todo,
    System,
}

impl Page {
    pub const ALL: [Page; 6] = [
        Page::Music,
        Page::Focus,
        Page::Water,
        Page::Shelf,
        Page::Todo,
        Page::System,
    ];

    /// Glifo di Segoe Fluent Icons / MDL2 Assets.
    pub fn glyph(self) -> char {
        match self {
            Page::Music => '\u{EC4F}',
            Page::Focus => '\u{E916}',
            Page::Water => '\u{EB42}',
            Page::Shelf => '\u{E8B7}',
            Page::Todo => '\u{E9D5}',
            Page::System => '\u{EC4A}',
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Page::Music => "Musica",
            Page::Focus => "Focus",
            Page::Water => "Acqua",
            Page::Shelf => "Shelf",
            Page::Todo => "To-do",
            Page::System => "Sistema",
        }
    }

    pub fn index(self) -> usize {
        Page::ALL.iter().position(|&p| p == self).unwrap_or(0)
    }

    /// Pagina successiva (`+1`) o precedente (`-1`), senza giro (rotella).
    pub fn step(self, direction: i32) -> Page {
        let i = (self.index() as i32 + direction).clamp(0, Page::ALL.len() as i32 - 1);
        Page::ALL[i as usize]
    }

    /// Come `step`, ma dopo l'ultima si torna alla prima (frecce).
    pub fn cycle(self, direction: i32) -> Page {
        let n = Page::ALL.len() as i32;
        Page::ALL[(self.index() as i32 + direction).rem_euclid(n) as usize]
    }
}

/// Tutto ciò che si può cliccare nel notch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    // Musica
    Prev,
    PlayPause,
    Next,
    Lyrics,
    Seek,
    /// Riga del testo relativa a quella corrente: -1 precedente, 0 corrente, 1 successiva.
    LyricLine(i8),
    /// Pulsante dell'altoparlante: la barra diventa quella del volume.
    Volume,
    VolumeBar,
    // Navigazione
    Page(Page),
    PrevPage,
    NextPage,
    Settings,
    // Focus
    FocusToggle,
    FocusReset,
    FocusSkip,
    FocusPhase(Phase),
    // Acqua
    Drink,
    UndoDrink,
    WaterReminders,
    /// Riquadro a destra: indicatore circolare o grafico per ora.
    WaterView,
    // Shelf (indici tra gli elementi visibili)
    ShelfItem(usize),
    ShelfRemove(usize),
    ShelfAction,
    // To-do (indici tra le righe visibili)
    TaskCheck(usize),
    TaskDelete(usize),
    TaskAdd,
    TodoAction,
    // Avviso
    AlertAction(usize),
    AlertBody,
}

fn find(targets: &[(Rect, Hit)], x: f32, y: f32) -> Option<Hit> {
    targets
        .iter()
        .find(|(r, _)| r.contains(x, y))
        .map(|(_, h)| *h)
}

// ---- Cornice --------------------------------------------------------------------

/// Titolo della pagina a sinistra, stato (batteria, data, ora) al centro e
/// pulsante d'azione della pagina a destra.
pub struct Header {
    pub title: Rect,
    pub status: Rect,
    pub action: Rect,
}

pub fn header() -> Header {
    let x = open_x();
    Header {
        title: Rect::new(x + 18.0, 4.0, 100.0, 24.0),
        status: Rect::new(x + 110.0, 4.0, OPEN_W - 220.0, 24.0),
        action: Rect::new(x + OPEN_W - 18.0 - 88.0, 6.0, 88.0, 20.0),
    }
}

/// Frecce per la pagina precedente e successiva. Stanno alla stessa altezza in
/// tutte le pagine (a metà della più bassa), così dopo un clic restano sotto il cursore.
pub fn arrows() -> (Rect, Rect) {
    let size = 22.0;
    let y = HEADER_H + FOCUS_H / 2.0 - size / 2.0;
    (
        Rect::new(open_x() + 8.0, y, size, size),
        Rect::new(open_x() + OPEN_W - 8.0 - size, y, size, size),
    )
}

/// Piè di pagina: i pallini (uno per pagina) e, quando il mouse ci arriva,
/// la barra con le icone delle pagine e l'ingranaggio delle impostazioni.
pub struct Footer {
    /// Zona che fa comparire la barra.
    pub zone: Rect,
    pub dots: [(Page, Rect); 6],
    pub bar: Rect,
    pub tabs: [(Page, Rect); 6],
    pub divider: Rect,
    pub settings: Rect,
}

const DOT_SLOT: f32 = 14.0;
const TAB_W: f32 = 30.0;
const TAB_GAP: f32 = 2.0;

pub fn footer(content_h: f32) -> Footer {
    let y = HEADER_H + content_h;
    let cx = open_x() + OPEN_W / 2.0;
    let n = Page::ALL.len() as f32;

    let dots_x = cx - n * DOT_SLOT / 2.0;
    let dots = Page::ALL.map(|p| {
        let i = p.index() as f32;
        (
            p,
            Rect::new(dots_x + i * DOT_SLOT, y + 2.0, DOT_SLOT, FOOTER_H - 6.0),
        )
    });

    let tabs_w = n * TAB_W + (n - 1.0) * TAB_GAP;
    let bar_w = 5.0 + tabs_w + 11.0 + TAB_W + 5.0;
    let bar = Rect::new(cx - bar_w / 2.0, y + 1.0, bar_w, 24.0);
    let tabs = Page::ALL.map(|p| {
        let i = p.index() as f32;
        (
            p,
            Rect::new(
                bar.x + 5.0 + i * (TAB_W + TAB_GAP),
                bar.y + 1.0,
                TAB_W,
                22.0,
            ),
        )
    });
    let divider = Rect::new(bar.x + 5.0 + tabs_w + 5.0, bar.y + 6.0, 1.0, 12.0);
    Footer {
        zone: Rect::new(open_x(), y, OPEN_W, FOOTER_H),
        dots,
        bar,
        tabs,
        divider,
        settings: Rect::new(bar.right() - 5.0 - TAB_W, bar.y + 1.0, TAB_W, 22.0),
    }
}

impl Footer {
    /// Con la barra visibile si cliccano le icone, altrimenti i pallini.
    pub fn hit(&self, x: f32, y: f32, bar_visible: bool) -> Option<Hit> {
        if bar_visible {
            if self.settings.contains(x, y) {
                return Some(Hit::Settings);
            }
            self.tabs
                .iter()
                .find(|(_, r)| r.contains(x, y))
                .map(|(p, _)| Hit::Page(*p))
        } else {
            self.dots
                .iter()
                .find(|(_, r)| r.contains(x, y))
                .map(|(p, _)| Hit::Page(*p))
        }
    }
}

// ---- Focus ----------------------------------------------------------------------

pub const FOCUS_H: f32 = 104.0;

/// Anello di avanzamento a sinistra; a destra tempo, didascalia, pulsanti per
/// passare alle altre due fasi e, in mezzo, avvio/pausa.
pub struct FocusLayout {
    pub ring: Rect,
    pub timer: Rect,
    pub caption: Rect,
    pub reset: Rect,
    pub skip: Rect,
    /// Le due fasi diverse da quella attuale, a sinistra e a destra di avvio/pausa.
    pub chips: [Rect; 2],
    pub toggle: Rect,
}

impl FocusLayout {
    pub fn new() -> Self {
        let (x, top) = (content_x(), HEADER_H);
        let ring = Rect::new(x + 2.0, top + 4.0, 92.0, 92.0);
        let xr = ring.right() + 16.0;
        let wr = x + CONTENT_W - xr;
        let (chip_w, toggle_w, gap) = (92.0, 52.0, 6.0);
        let start = xr + (wr - 2.0 * chip_w - toggle_w - 2.0 * gap) / 2.0;
        let row = top + 68.0;
        Self {
            ring,
            timer: Rect::new(xr, top, wr, 46.0),
            caption: Rect::new(xr, top + 44.0, wr, 16.0),
            reset: Rect::new(xr + wr - 54.0, top + 11.0, 24.0, 24.0),
            skip: Rect::new(xr + wr - 26.0, top + 11.0, 24.0, 24.0),
            chips: [
                Rect::new(start, row, chip_w, 30.0),
                Rect::new(start + chip_w + 2.0 * gap + toggle_w, row, chip_w, 30.0),
            ],
            toggle: Rect::new(start + chip_w + gap, row, toggle_w, 30.0),
        }
    }

    /// Le fasi proposte nei due pulsanti quando è attiva `current`.
    pub fn other_phases(current: Phase) -> [Phase; 2] {
        match current {
            Phase::Focus => [Phase::ShortBreak, Phase::LongBreak],
            Phase::ShortBreak => [Phase::Focus, Phase::LongBreak],
            Phase::LongBreak => [Phase::ShortBreak, Phase::Focus],
        }
    }

    pub fn hit(&self, x: f32, y: f32, current: Phase) -> Option<Hit> {
        let [a, b] = Self::other_phases(current);
        find(
            &[
                (self.chips[0], Hit::FocusPhase(a)),
                (self.chips[1], Hit::FocusPhase(b)),
                (self.toggle, Hit::FocusToggle),
                (self.reset, Hit::FocusReset),
                (self.skip, Hit::FocusSkip),
            ],
            x,
            y,
        )
    }
}

impl Default for FocusLayout {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Acqua ----------------------------------------------------------------------

pub const WATER_H: f32 = 104.0;

/// A sinistra quantità, interruttore dei promemoria, barra e pulsanti; a destra
/// un riquadro con l'indicatore circolare o il grafico ora per ora.
pub struct WaterLayout {
    pub amount: Rect,
    pub reminders: Rect,
    pub caption: Rect,
    pub bar: Rect,
    pub drink: Rect,
    pub undo: Rect,
    pub divider: Rect,
    pub panel: Rect,
    /// Due pallini in alto a destra: quale vista del riquadro è attiva.
    pub view_dots: Rect,
    pub chart: Rect,
    pub labels_y: f32,
}

impl WaterLayout {
    pub fn new() -> Self {
        let (x, top) = (content_x(), HEADER_H);
        let left_w = 216.0;
        let panel_x = x + left_w + 22.0;
        let panel = Rect::new(panel_x, top + 2.0, x + CONTENT_W - panel_x, 98.0);
        Self {
            amount: Rect::new(x, top, left_w - 44.0, 36.0),
            reminders: Rect::new(x + left_w - 38.0, top + 7.0, 38.0, 22.0),
            caption: Rect::new(x, top + 37.0, left_w, 16.0),
            bar: Rect::new(x, top + 59.0, left_w, 5.0),
            drink: Rect::new(x, top + 72.0, 124.0, 28.0),
            undo: Rect::new(x + 130.0, top + 72.0, 28.0, 28.0),
            divider: Rect::new(x + left_w + 10.0, top + 8.0, 1.0, 88.0),
            panel,
            view_dots: Rect::new(panel.right() - 20.0, panel.y + 1.0, 20.0, 8.0),
            chart: Rect::new(panel.x + 4.0, panel.y + 16.0, panel.w - 8.0, 60.0),
            labels_y: panel.y + 80.0,
        }
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        find(
            &[
                (self.drink, Hit::Drink),
                (self.undo, Hit::UndoDrink),
                (self.reminders, Hit::WaterReminders),
                (self.panel, Hit::WaterView),
            ],
            x,
            y,
        )
    }

    /// Una colonna per ora, da sinistra a destra.
    pub fn chart_slots(&self, hours: usize) -> Vec<Rect> {
        if hours == 0 {
            return Vec::new();
        }
        let gap = 2.0;
        let w = (self.chart.w - gap * (hours as f32 - 1.0)) / hours as f32;
        (0..hours)
            .map(|i| {
                Rect::new(
                    self.chart.x + i as f32 * (w + gap),
                    self.chart.y,
                    w,
                    self.chart.h,
                )
            })
            .collect()
    }
}

impl Default for WaterLayout {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Sistema -----------------------------------------------------------------------

pub const SYSTEM_H: f32 = 3.0 * SYSTEM_TILE_H + 2.0 * SYSTEM_GAP + 6.0;
#[cfg(test)]
pub const SYSTEM_TILES: usize = 9;
const SYSTEM_TILE_H: f32 = 52.0;
const SYSTEM_GAP: f32 = 6.0;

/// Griglia 3×3 di riquadri: CPU, RAM, Disco / GPU, Download, Upload / Consumo, Temp. CPU, Temp. GPU.
pub fn system_tile(i: usize) -> Rect {
    let w = (CONTENT_W - 2.0 * SYSTEM_GAP) / 3.0;
    let (col, row) = ((i % 3) as f32, (i / 3) as f32);
    Rect::new(
        content_x() + col * (w + SYSTEM_GAP),
        HEADER_H + 2.0 + row * (SYSTEM_TILE_H + SYSTEM_GAP),
        w,
        SYSTEM_TILE_H,
    )
}

pub struct SystemTileParts {
    pub label: Rect,
    pub value: Rect,
    pub bottom: Rect,
}

pub fn system_tile_parts(tile: Rect) -> SystemTileParts {
    SystemTileParts {
        label: Rect::new(tile.x + 10.0, tile.y + 5.0, tile.w - 20.0, 13.0),
        value: Rect::new(tile.x + 10.0, tile.y + 17.0, tile.w - 20.0, 21.0),
        bottom: Rect::new(tile.x + 10.0, tile.y + 38.0, tile.w - 20.0, 10.0),
    }
}

// ---- Shelf ------------------------------------------------------------------------

pub const SHELF_COLS: usize = 4;
pub const SHELF_ROWS: usize = 2;
pub const TILE_H: f32 = 72.0;
const TILE_GAP: f32 = 6.0;
const SHELF_EMPTY_H: f32 = 92.0;

pub fn tile_w() -> f32 {
    (CONTENT_W - TILE_GAP * (SHELF_COLS as f32 - 1.0)) / SHELF_COLS as f32
}

/// Righe di riquadri visibili per `items` elementi.
pub fn shelf_rows(items: usize) -> usize {
    items.div_ceil(SHELF_COLS).min(SHELF_ROWS)
}

pub fn shelf_height(items: usize) -> f32 {
    match shelf_rows(items) {
        0 => SHELF_EMPTY_H + 8.0,
        rows => rows as f32 * (TILE_H + TILE_GAP) - TILE_GAP + 8.0,
    }
}

pub fn shelf_drop_zone() -> Rect {
    Rect::new(content_x(), HEADER_H + 2.0, CONTENT_W, SHELF_EMPTY_H)
}

/// Riquadro dell'elemento visibile numero `i`.
pub fn shelf_tile(i: usize) -> Rect {
    let (col, row) = (i % SHELF_COLS, i / SHELF_COLS);
    let w = tile_w();
    Rect::new(
        content_x() + col as f32 * (w + TILE_GAP),
        HEADER_H + 2.0 + row as f32 * (TILE_H + TILE_GAP),
        w,
        TILE_H,
    )
}

pub struct TileParts {
    pub thumb: Rect,
    pub name: Rect,
    pub remove: Rect,
}

pub fn tile_parts(tile: Rect) -> TileParts {
    TileParts {
        thumb: Rect::new(tile.center_x() - 18.0, tile.y + 7.0, 36.0, 36.0),
        name: Rect::new(tile.x + 5.0, tile.y + 48.0, tile.w - 10.0, 18.0),
        remove: Rect::new(tile.right() - 21.0, tile.y + 3.0, 18.0, 18.0),
    }
}

/// `header_action`: il pulsante in alto a destra è disegnato (lo Shelf non è vuoto).
pub fn shelf_hit(x: f32, y: f32, visible: usize, header_action: bool) -> Option<Hit> {
    if header_action && header().action.contains(x, y) {
        return Some(Hit::ShelfAction);
    }
    (0..visible).find_map(|i| {
        let tile = shelf_tile(i);
        if tile_parts(tile).remove.contains(x, y) {
            Some(Hit::ShelfRemove(i))
        } else {
            tile.contains(x, y).then_some(Hit::ShelfItem(i))
        }
    })
}

// ---- To-do ------------------------------------------------------------------------

pub const TODO_ROWS: usize = 5;
pub const TODO_ROW_H: f32 = 26.0;
const TODO_LEFT_W: f32 = 120.0;
const TODO_EMPTY_H: f32 = 56.0;
const TODO_ADD_H: f32 = 28.0;

/// Colonna di sinistra: quante fatte oggi, prossima scadenza e "Pulisci".
pub struct TodoSummary {
    pub count: Rect,
    pub caption: Rect,
    pub next: Rect,
    pub clear: Rect,
}

pub fn todo_summary() -> TodoSummary {
    let (x, top) = (content_x(), HEADER_H);
    TodoSummary {
        count: Rect::new(x, top, TODO_LEFT_W, 36.0),
        caption: Rect::new(x, top + 36.0, TODO_LEFT_W, 16.0),
        next: Rect::new(x, top + 52.0, TODO_LEFT_W, 16.0),
        clear: Rect::new(x, top + 74.0, 96.0, 26.0),
    }
}

/// Riquadro a destra con le attività e la riga per aggiungerne.
pub fn todo_panel(visible_rows: usize) -> Rect {
    let x = content_x() + TODO_LEFT_W + 8.0;
    let rows_h = match visible_rows.min(TODO_ROWS) {
        0 => TODO_EMPTY_H,
        n => n as f32 * TODO_ROW_H,
    };
    Rect::new(
        x,
        HEADER_H + 2.0,
        content_x() + CONTENT_W - x,
        6.0 + rows_h + TODO_ADD_H + 6.0,
    )
}

pub fn todo_height(visible_rows: usize) -> f32 {
    (todo_panel(visible_rows).h + 6.0).max(104.0)
}

pub struct TodoRow {
    pub row: Rect,
    pub check: Rect,
    pub title: Rect,
    pub due: Rect,
    pub delete: Rect,
}

pub fn todo_row(i: usize) -> TodoRow {
    let panel = todo_panel(TODO_ROWS);
    let row = Rect::new(
        panel.x + 6.0,
        panel.y + 6.0 + i as f32 * TODO_ROW_H,
        panel.w - 12.0,
        TODO_ROW_H,
    );
    let check = Rect::new(row.x + 5.0, row.y + 5.0, 16.0, 16.0);
    let due = Rect::new(row.right() - 5.0 - 56.0, row.y + 5.0, 56.0, 16.0);
    let title_x = check.right() + 8.0;
    TodoRow {
        row,
        check,
        title: Rect::new(title_x, row.y, due.x - 6.0 - title_x, TODO_ROW_H),
        due,
        delete: Rect::new(row.right() - 26.0, row.y + 2.0, 22.0, 22.0),
    }
}

/// Messaggio "Nessuna attività" quando la lista è vuota.
pub fn todo_empty() -> Rect {
    let panel = todo_panel(0);
    Rect::new(panel.x, panel.y + 6.0, panel.w, TODO_EMPTY_H)
}

/// Riga "Nuova attività" (o campo di testo mentre si scrive).
pub fn todo_add_row(visible_rows: usize) -> Rect {
    let panel = todo_panel(visible_rows);
    Rect::new(
        panel.x + 6.0,
        panel.bottom() - 6.0 - TODO_ADD_H,
        panel.w - 12.0,
        TODO_ADD_H,
    )
}

/// `header_action`: "Pulisci" è disegnato (c'è almeno un'attività fatta).
pub fn todo_hit(x: f32, y: f32, visible: usize, header_action: bool) -> Option<Hit> {
    if header_action && todo_summary().clear.contains(x, y) {
        return Some(Hit::TodoAction);
    }
    if todo_add_row(visible).contains(x, y) {
        return Some(Hit::TaskAdd);
    }
    (0..visible.min(TODO_ROWS)).find_map(|i| {
        let r = todo_row(i);
        if r.delete.contains(x, y) {
            Some(Hit::TaskDelete(i))
        } else {
            r.row.contains(x, y).then_some(Hit::TaskCheck(i))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{BOUNCE_ROOM, CANVAS_H, LYRICS_H, MUSIC_H, open_height};

    #[test]
    fn footer_hits_dots_tabs_and_settings() {
        let f = footer(100.0);
        for (page, r) in f.tabs {
            assert_eq!(
                f.hit(r.center_x(), r.center_y(), true),
                Some(Hit::Page(page))
            );
            assert!(!r.overlaps(&f.settings));
            assert!(f.bar.contains(r.center_x(), r.center_y()));
        }
        for (page, r) in f.dots {
            assert_eq!(
                f.hit(r.center_x(), r.center_y(), false),
                Some(Hit::Page(page))
            );
        }
        assert_eq!(
            f.hit(f.settings.center_x(), f.settings.center_y(), true),
            Some(Hit::Settings)
        );
        assert_eq!(
            f.hit(f.settings.center_x(), f.settings.center_y(), false),
            None
        );
        assert!(f.bar.right() <= open_x() + OPEN_W);
    }

    #[test]
    fn page_steps_and_cycles() {
        assert_eq!(Page::Music.step(-1), Page::Music);
        assert_eq!(Page::Shelf.step(1), Page::Todo);
        assert_eq!(Page::System.step(1), Page::System);
        assert_eq!(Page::System.cycle(1), Page::Music);
        assert_eq!(Page::Music.cycle(-1), Page::System);
    }

    #[test]
    fn arrows_sit_beside_the_content() {
        let (left, right) = arrows();
        assert!(left.right() <= content_x());
        assert!(right.x >= content_x() + CONTENT_W);
        // Dentro anche la pagina più bassa.
        for h in [FOCUS_H, WATER_H, shelf_height(0), todo_height(0)] {
            assert!(left.bottom() <= HEADER_H + h);
        }
    }

    #[test]
    fn footer_zone_does_not_cover_the_page() {
        for h in [FOCUS_H, WATER_H, SYSTEM_H] {
            assert!(footer(h).zone.y >= HEADER_H + h);
        }
    }

    #[test]
    fn every_page_fits_in_the_canvas() {
        for h in [
            MUSIC_H + LYRICS_H,
            FOCUS_H,
            WATER_H,
            shelf_height(100),
            todo_height(100),
            SYSTEM_H,
        ] {
            assert!(
                open_height(h) + BOUNCE_ROOM <= CANVAS_H,
                "pagina alta {h} non entra"
            );
        }
    }

    #[test]
    fn focus_and_water_controls() {
        let f = FocusLayout::new();
        assert_eq!(
            f.hit(f.toggle.center_x(), f.toggle.center_y(), Phase::Focus),
            Some(Hit::FocusToggle)
        );
        assert_eq!(
            f.hit(f.chips[0].center_x(), f.chips[0].center_y(), Phase::Focus),
            Some(Hit::FocusPhase(Phase::ShortBreak))
        );
        assert_eq!(
            f.hit(
                f.chips[1].center_x(),
                f.chips[1].center_y(),
                Phase::LongBreak
            ),
            Some(Hit::FocusPhase(Phase::Focus))
        );
        assert!(!f.chips[0].overlaps(&f.toggle) && !f.toggle.overlaps(&f.chips[1]));
        assert!(f.chips[1].right() <= content_x() + CONTENT_W);
        assert!(!f.timer.overlaps(&f.ring));
        for phase in Phase::ALL {
            assert!(!FocusLayout::other_phases(phase).contains(&phase));
        }

        let w = WaterLayout::new();
        assert_eq!(
            w.hit(w.drink.center_x(), w.drink.center_y()),
            Some(Hit::Drink)
        );
        assert_eq!(
            w.hit(w.reminders.center_x(), w.reminders.center_y()),
            Some(Hit::WaterReminders)
        );
        assert_eq!(
            w.hit(w.panel.center_x(), w.panel.center_y()),
            Some(Hit::WaterView)
        );
        let slots = w.chart_slots(13);
        assert_eq!(slots.len(), 13);
        assert!(slots.last().unwrap().right() <= w.chart.right() + 1e-3);
        assert!(w.panel.right() <= content_x() + CONTENT_W + 1e-3);
        assert!(w.panel.bottom() <= HEADER_H + WATER_H);
    }

    #[test]
    fn system_grid_fits() {
        let last = system_tile(SYSTEM_TILES - 1);
        assert!(last.bottom() <= HEADER_H + SYSTEM_H);
        assert!((last.right() - (content_x() + CONTENT_W)).abs() < 1e-3);
        assert!(!system_tile(0).overlaps(&system_tile(1)));
        assert!(!system_tile(0).overlaps(&system_tile(3)));
    }

    #[test]
    fn shelf_grid() {
        assert_eq!(shelf_rows(0), 0);
        assert_eq!(shelf_rows(4), 1);
        assert_eq!(shelf_rows(5), 2);
        assert_eq!(shelf_rows(50), 2);
        let last = shelf_tile(SHELF_COLS * SHELF_ROWS - 1);
        assert!(last.bottom() <= HEADER_H + shelf_height(8));
        assert!((last.right() - (content_x() + CONTENT_W)).abs() < 1e-3);
        let t = shelf_tile(1);
        let parts = tile_parts(t);
        assert_eq!(
            shelf_hit(parts.remove.center_x(), parts.remove.center_y(), 3, true),
            Some(Hit::ShelfRemove(1))
        );
        assert_eq!(
            shelf_hit(parts.thumb.center_x(), parts.thumb.center_y(), 3, true),
            Some(Hit::ShelfItem(1))
        );
        assert_eq!(
            shelf_hit(parts.thumb.center_x(), parts.thumb.center_y(), 1, true),
            None
        );
        let action = header().action;
        assert_eq!(
            shelf_hit(action.center_x(), action.center_y(), 0, false),
            None
        );
    }

    #[test]
    fn todo_rows() {
        let r = todo_row(2);
        assert_eq!(
            todo_hit(r.check.center_x(), r.check.center_y(), 3, true),
            Some(Hit::TaskCheck(2))
        );
        assert_eq!(
            todo_hit(r.delete.center_x(), r.delete.center_y(), 3, true),
            Some(Hit::TaskDelete(2))
        );
        let add = todo_add_row(3);
        assert_eq!(
            todo_hit(add.center_x(), add.center_y(), 3, true),
            Some(Hit::TaskAdd)
        );
        assert!(!add.overlaps(&todo_row(2).row));
        assert!(add.bottom() <= HEADER_H + todo_height(3));
        assert!(todo_add_row(0).y >= todo_empty().bottom());
        assert!(r.title.w > 100.0);
        let clear = todo_summary().clear;
        assert_eq!(
            todo_hit(clear.center_x(), clear.center_y(), 0, true),
            Some(Hit::TodoAction)
        );
        assert_eq!(
            todo_hit(clear.center_x(), clear.center_y(), 0, false),
            None,
            "senza attività fatte \"Pulisci\" non c'è"
        );
        assert!(todo_panel(5).right() <= content_x() + CONTENT_W + 1e-3);
    }
}
