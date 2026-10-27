//! Impostazioni dell'app e loro descrizione (etichetta, limiti, passo), usata
//! sia per salvarle nel registro sia per costruire la finestra Impostazioni.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub autostart: bool,
    /// Con un'app a schermo intero: vedi `FULLSCREEN_MODES`.
    pub fullscreen_mode: u32,
    /// Le finestre ingrandite contano come schermo intero.
    pub shrink_maximized: bool,
    /// Data accanto all'ora nel notch chiuso.
    pub closed_date: bool,
    /// Anello dei download in corso, e notch normale anche a schermo intero.
    pub downloads: bool,
    /// 0 = monitor principale, poi 1, 2, ... nell'ordine di Windows.
    pub monitor: u32,
    /// Dimensione del notch in percentuale.
    pub notch_scale: u32,
    pub lyrics: bool,

    pub hud_volume: bool,
    pub hud_brightness: bool,
    pub hud_keyboard: bool,
    pub hud_power: bool,
    pub hud_network: bool,
    pub hud_bluetooth: bool,

    pub focus_min: u32,
    pub short_break_min: u32,
    pub long_break_min: u32,
    pub long_break_every: u32,
    pub focus_auto_start: bool,

    pub water_reminders: bool,
    pub water_goal_ml: u32,
    pub water_glass_ml: u32,
    pub water_interval_min: u32,
    pub water_from_h: u32,
    pub water_to_h: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            autostart: false,
            fullscreen_mode: FULLSCREEN_SHRINK,
            shrink_maximized: false,
            closed_date: false,
            downloads: true,
            monitor: 0,
            notch_scale: 100,
            // I testi mandano titolo e artista a LRCLIB: si attivano su richiesta.
            lyrics: false,
            hud_volume: true,
            hud_brightness: true,
            hud_keyboard: true,
            hud_power: true,
            hud_network: true,
            hud_bluetooth: true,
            focus_min: 25,
            short_break_min: 5,
            long_break_min: 15,
            long_break_every: 4,
            focus_auto_start: false,
            water_reminders: true,
            water_goal_ml: 2000,
            water_glass_ml: 250,
            water_interval_min: 60,
            water_from_h: 9,
            water_to_h: 21,
        }
    }
}

/// Cosa fa il notch quando un'app è a schermo intero.
pub const FULLSCREEN_MODES: &[&str] = &["Si riduce", "Si nasconde", "Resta com'è"];
pub const FULLSCREEN_SHRINK: u32 = 0;
pub const FULLSCREEN_HIDE: u32 = 1;
pub const FULLSCREEN_NORMAL: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    General,
    Notifications,
    Focus,
    Water,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::General => "Generale",
            Section::Notifications => "Avvisi nel notch",
            Section::Focus => "Focus",
            Section::Water => "Acqua",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Percent,
    Minutes,
    Sessions,
    Ml,
    Hour,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Toggle,
    Number {
        min: u32,
        max: u32,
        step: u32,
        unit: Unit,
    },
    /// Scelta del monitor: i nomi arrivano dal sistema.
    Monitor,
    /// Una voce tra quelle elencate (il valore è l'indice).
    Choice(&'static [&'static str]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Autostart,
    Fullscreen,
    ShrinkMaximized,
    ClosedDate,
    Downloads,
    Monitor,
    NotchScale,
    Lyrics,
    HudVolume,
    HudBrightness,
    HudKeyboard,
    HudPower,
    HudNetwork,
    HudBluetooth,
    FocusMin,
    ShortBreakMin,
    LongBreakMin,
    LongBreakEvery,
    FocusAutoStart,
    WaterReminders,
    WaterGoal,
    WaterGlass,
    WaterInterval,
    WaterFrom,
    WaterTo,
}

impl Field {
    pub const ALL: [Field; 25] = [
        Field::Autostart,
        Field::Fullscreen,
        Field::ShrinkMaximized,
        Field::ClosedDate,
        Field::Downloads,
        Field::Monitor,
        Field::NotchScale,
        Field::Lyrics,
        Field::HudVolume,
        Field::HudBrightness,
        Field::HudKeyboard,
        Field::HudPower,
        Field::HudNetwork,
        Field::HudBluetooth,
        Field::FocusMin,
        Field::ShortBreakMin,
        Field::LongBreakMin,
        Field::LongBreakEvery,
        Field::FocusAutoStart,
        Field::WaterReminders,
        Field::WaterGoal,
        Field::WaterGlass,
        Field::WaterInterval,
        Field::WaterFrom,
        Field::WaterTo,
    ];

    pub fn section(self) -> Section {
        use Field::*;
        match self {
            Autostart | Fullscreen | ShrinkMaximized | ClosedDate | Downloads | Monitor
            | NotchScale | Lyrics => Section::General,
            HudVolume | HudBrightness | HudKeyboard | HudPower | HudNetwork | HudBluetooth => {
                Section::Notifications
            }
            FocusMin | ShortBreakMin | LongBreakMin | LongBreakEvery | FocusAutoStart => {
                Section::Focus
            }
            WaterReminders | WaterGoal | WaterGlass | WaterInterval | WaterFrom | WaterTo => {
                Section::Water
            }
        }
    }

    pub fn label(self) -> &'static str {
        use Field::*;
        match self {
            Autostart => "Avvia con Windows",
            Fullscreen => "A schermo intero il notch",
            ShrinkMaximized => "Anche con le finestre ingrandite",
            ClosedDate => "Data nel notch chiuso",
            Downloads => "Download in corso nel notch",
            Monitor => "Monitor",
            NotchScale => "Dimensione del notch",
            Lyrics => "Testi sincronizzati (LRCLIB)",
            HudVolume => "Volume",
            HudBrightness => "Luminosità (anche quella automatica)",
            HudKeyboard => "Bloc Maiusc e lingua della tastiera",
            HudPower => "Batteria e caricatore",
            HudNetwork => "Connessione di rete",
            HudBluetooth => "Dispositivi Bluetooth",
            FocusMin => "Concentrazione",
            ShortBreakMin => "Pausa breve",
            LongBreakMin => "Pausa lunga",
            LongBreakEvery => "Pausa lunga ogni",
            FocusAutoStart => "Avvia da sola la fase successiva",
            WaterReminders => "Promemoria per bere",
            WaterGoal => "Obiettivo giornaliero",
            WaterGlass => "Un bicchiere",
            WaterInterval => "Ricordamelo ogni",
            WaterFrom => "Dalle",
            WaterTo => "Alle",
        }
    }

    /// Nome del valore nel registro.
    pub fn key(self) -> &'static str {
        use Field::*;
        match self {
            Autostart => "Autostart",
            Fullscreen => "FullscreenMode",
            ShrinkMaximized => "ShrinkMaximized",
            ClosedDate => "ClosedDate",
            Downloads => "Downloads",
            Monitor => "Monitor",
            NotchScale => "NotchScale",
            Lyrics => "LyricsEnabled",
            HudVolume => "HudVolume",
            HudBrightness => "HudBrightness",
            HudKeyboard => "HudKeyboard",
            HudPower => "HudPower",
            HudNetwork => "HudNetwork",
            HudBluetooth => "HudBluetooth",
            FocusMin => "FocusMinutes",
            ShortBreakMin => "ShortBreakMinutes",
            LongBreakMin => "LongBreakMinutes",
            LongBreakEvery => "LongBreakEvery",
            FocusAutoStart => "FocusAutoStart",
            WaterReminders => "WaterReminders",
            WaterGoal => "WaterGoalMl",
            WaterGlass => "WaterGlassMl",
            WaterInterval => "WaterIntervalMinutes",
            WaterFrom => "WaterFromHour",
            WaterTo => "WaterToHour",
        }
    }

    pub fn kind(self) -> Kind {
        use Field::*;
        let number = |min, max, step, unit| Kind::Number {
            min,
            max,
            step,
            unit,
        };
        match self {
            Monitor => Kind::Monitor,
            Fullscreen => Kind::Choice(FULLSCREEN_MODES),
            NotchScale => number(70, 140, 10, Unit::Percent),
            FocusMin => number(5, 90, 5, Unit::Minutes),
            ShortBreakMin => number(1, 30, 1, Unit::Minutes),
            LongBreakMin => number(5, 60, 5, Unit::Minutes),
            LongBreakEvery => number(2, 8, 1, Unit::Sessions),
            WaterGoal => number(500, 5000, 250, Unit::Ml),
            WaterGlass => number(100, 1000, 50, Unit::Ml),
            WaterInterval => number(15, 240, 15, Unit::Minutes),
            WaterFrom => number(0, 23, 1, Unit::Hour),
            WaterTo => number(1, 24, 1, Unit::Hour),
            _ => Kind::Toggle,
        }
    }

    pub fn get(self, s: &Settings) -> u32 {
        use Field::*;
        match self {
            Autostart => s.autostart as u32,
            Fullscreen => s.fullscreen_mode,
            ShrinkMaximized => s.shrink_maximized as u32,
            ClosedDate => s.closed_date as u32,
            Downloads => s.downloads as u32,
            Monitor => s.monitor,
            NotchScale => s.notch_scale,
            Lyrics => s.lyrics as u32,
            HudVolume => s.hud_volume as u32,
            HudBrightness => s.hud_brightness as u32,
            HudKeyboard => s.hud_keyboard as u32,
            HudPower => s.hud_power as u32,
            HudNetwork => s.hud_network as u32,
            HudBluetooth => s.hud_bluetooth as u32,
            FocusMin => s.focus_min,
            ShortBreakMin => s.short_break_min,
            LongBreakMin => s.long_break_min,
            LongBreakEvery => s.long_break_every,
            FocusAutoStart => s.focus_auto_start as u32,
            WaterReminders => s.water_reminders as u32,
            WaterGoal => s.water_goal_ml,
            WaterGlass => s.water_glass_ml,
            WaterInterval => s.water_interval_min,
            WaterFrom => s.water_from_h,
            WaterTo => s.water_to_h,
        }
    }

    /// Imposta il valore rispettando limiti e passo.
    pub fn set(self, s: &mut Settings, value: u32) {
        use Field::*;
        let value = match self.kind() {
            Kind::Toggle => (value != 0) as u32,
            Kind::Number { min, max, step, .. } => {
                let snapped = min + (value.saturating_sub(min) + step / 2) / step * step;
                snapped.clamp(min, max)
            }
            Kind::Monitor => value.min(16),
            Kind::Choice(options) => value.min(options.len() as u32 - 1),
        };
        let on = value != 0;
        match self {
            Autostart => s.autostart = on,
            Fullscreen => s.fullscreen_mode = value,
            ShrinkMaximized => s.shrink_maximized = on,
            ClosedDate => s.closed_date = on,
            Downloads => s.downloads = on,
            Monitor => s.monitor = value,
            NotchScale => s.notch_scale = value,
            Lyrics => s.lyrics = on,
            HudVolume => s.hud_volume = on,
            HudBrightness => s.hud_brightness = on,
            HudKeyboard => s.hud_keyboard = on,
            HudPower => s.hud_power = on,
            HudNetwork => s.hud_network = on,
            HudBluetooth => s.hud_bluetooth = on,
            FocusMin => s.focus_min = value,
            ShortBreakMin => s.short_break_min = value,
            LongBreakMin => s.long_break_min = value,
            LongBreakEvery => s.long_break_every = value,
            FocusAutoStart => s.focus_auto_start = on,
            WaterReminders => s.water_reminders = on,
            WaterGoal => s.water_goal_ml = value,
            WaterGlass => s.water_glass_ml = value,
            WaterInterval => s.water_interval_min = value,
            // La fascia oraria deve restare valida: "dalle" prima di "alle".
            WaterFrom => s.water_from_h = value.min(s.water_to_h.saturating_sub(1)),
            WaterTo => s.water_to_h = value.max(s.water_from_h + 1),
        }
    }

    /// Un passo in su (`+1`) o in giù (`-1`); per gli interruttori inverte il valore.
    pub fn step(self, s: &mut Settings, direction: i32, monitors: u32) {
        let current = self.get(s);
        let next = match self.kind() {
            Kind::Toggle => (current == 0) as u32,
            Kind::Number { step, .. } => current.saturating_add_signed(direction * step as i32),
            Kind::Monitor => {
                let count = monitors.max(1) + 1; // "principale" + ogni monitor
                (current as i32 + direction).rem_euclid(count as i32) as u32
            }
            Kind::Choice(options) => {
                (current as i32 + direction).rem_euclid(options.len() as i32) as u32
            }
        };
        self.set(s, next);
    }

    /// Valore leggibile, es. "25 min", "2000 ml", "9:00".
    pub fn format(self, s: &Settings) -> String {
        let v = self.get(s);
        match self.kind() {
            Kind::Toggle => if v != 0 { "Sì" } else { "No" }.to_string(),
            Kind::Choice(options) => options.get(v as usize).copied().unwrap_or("").to_string(),
            Kind::Monitor => {
                if v == 0 {
                    "Principale".to_string()
                } else {
                    format!("Monitor {v}")
                }
            }
            Kind::Number { unit, .. } => match unit {
                Unit::Percent => format!("{v}%"),
                Unit::Minutes => format!("{v} min"),
                Unit::Sessions => format!("{v} sessioni"),
                Unit::Ml => format!("{v} ml"),
                Unit::Hour => format!("{v}:00"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_respect_limits() {
        let defaults = Settings::default();
        for field in Field::ALL {
            let mut s = defaults.clone();
            field.set(&mut s, field.get(&defaults));
            assert_eq!(s, defaults, "{field:?} non è stabile");
        }
    }

    #[test]
    fn numbers_are_clamped_and_snapped() {
        let mut s = Settings::default();
        Field::FocusMin.set(&mut s, 1000);
        assert_eq!(s.focus_min, 90);
        Field::FocusMin.set(&mut s, 0);
        assert_eq!(s.focus_min, 5);
        Field::FocusMin.set(&mut s, 23);
        assert_eq!(s.focus_min, 25);
        Field::WaterGoal.set(&mut s, 1880);
        assert_eq!(s.water_goal_ml, 2000);
    }

    #[test]
    fn stepping() {
        let mut s = Settings::default();
        Field::FocusMin.step(&mut s, 1, 1);
        assert_eq!(s.focus_min, 30);
        Field::FocusMin.step(&mut s, -1, 1);
        assert_eq!(s.focus_min, 25);
        Field::Lyrics.step(&mut s, 1, 1);
        assert!(s.lyrics);
        Field::Lyrics.step(&mut s, -1, 1);
        assert!(!s.lyrics);
    }

    #[test]
    fn monitor_choice_wraps_around() {
        let mut s = Settings::default();
        Field::Monitor.step(&mut s, 1, 2);
        assert_eq!(s.monitor, 1);
        Field::Monitor.step(&mut s, 1, 2);
        assert_eq!(s.monitor, 2);
        Field::Monitor.step(&mut s, 1, 2);
        assert_eq!(s.monitor, 0);
        Field::Monitor.step(&mut s, -1, 2);
        assert_eq!(s.monitor, 2);
        assert_eq!(Field::Monitor.format(&s), "Monitor 2");
    }

    #[test]
    fn fullscreen_choice_wraps_around() {
        let mut s = Settings::default();
        assert_eq!(Field::Fullscreen.format(&s), "Si riduce");
        Field::Fullscreen.step(&mut s, 1, 1);
        assert_eq!(s.fullscreen_mode, FULLSCREEN_HIDE);
        Field::Fullscreen.step(&mut s, 1, 1);
        Field::Fullscreen.step(&mut s, 1, 1);
        assert_eq!(s.fullscreen_mode, FULLSCREEN_SHRINK);
        Field::Fullscreen.set(&mut s, 99);
        assert_eq!(Field::Fullscreen.format(&s), "Resta com'è");
    }

    #[test]
    fn water_hours_stay_ordered() {
        let mut s = Settings::default();
        Field::WaterFrom.set(&mut s, 22);
        assert_eq!(s.water_from_h, 20);
        Field::WaterTo.set(&mut s, 3);
        assert_eq!(s.water_to_h, 21);
    }

    #[test]
    fn every_field_has_a_unique_key() {
        let mut keys: Vec<_> = Field::ALL.iter().map(|f| f.key()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), Field::ALL.len());
    }
}
