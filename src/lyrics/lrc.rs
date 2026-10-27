//! Parser del formato LRC (`[mm:ss.xx] testo`), quello usato dai testi sincronizzati.

#[derive(Debug, Clone, PartialEq)]
pub struct LyricLine {
    pub time_ms: u64,
    /// Vuota per le pause strumentali.
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
}

impl Lyrics {
    pub fn parse(src: &str) -> Self {
        let mut offset_ms: i64 = 0;
        let mut lines = Vec::new();

        for raw in src.lines() {
            let mut rest = raw.trim();
            let mut times = Vec::new();
            while let Some(after) = rest.strip_prefix('[') {
                let Some(end) = after.find(']') else { break };
                let tag = &after[..end];
                if let Some(t) = parse_timestamp(tag) {
                    times.push(t);
                } else if let Some(v) = tag.strip_prefix("offset:") {
                    offset_ms = v.trim().parse().unwrap_or(0);
                }
                rest = after[end + 1..].trim_start();
            }
            if times.is_empty() {
                continue; // metadati ([ar:...], [ti:...]) o righe senza tempo
            }
            let text = strip_word_timestamps(rest).trim().to_string();
            lines.extend(times.into_iter().map(|time_ms| LyricLine {
                time_ms,
                text: text.clone(),
            }));
        }

        // Da specifica, un offset positivo fa apparire il testo prima.
        if offset_ms != 0 {
            for l in &mut lines {
                let shifted = i64::try_from(l.time_ms)
                    .unwrap_or(i64::MAX)
                    .saturating_sub(offset_ms);
                l.time_ms = shifted.max(0) as u64;
            }
        }
        lines.sort_by_key(|l| l.time_ms);
        Self { lines }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Indice della riga in corso alla posizione `pos_ms`, se il testo è già iniziato.
    pub fn index_at(&self, pos_ms: u64) -> Option<usize> {
        self.lines
            .partition_point(|l| l.time_ms <= pos_ms)
            .checked_sub(1)
    }

    /// Istante in cui cambierà riga dopo `pos_ms`.
    pub fn next_time_after(&self, pos_ms: u64) -> Option<u64> {
        let i = self.lines.partition_point(|l| l.time_ms <= pos_ms);
        self.lines.get(i).map(|l| l.time_ms)
    }
}

/// Accetta `mm:ss`, `mm:ss.x`, `mm:ss.xx`, `mm:ss.xxx` e la variante `mm:ss:xx`.
fn parse_timestamp(tag: &str) -> Option<u64> {
    let (min, rest) = tag.split_once(':')?;
    let (sec, frac) = match rest.find(['.', ':']) {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !all_digits(min) || !all_digits(sec) || !(frac.is_empty() || all_digits(frac)) {
        return None;
    }
    let min: u64 = min.parse().ok()?;
    let sec: u64 = sec.parse().ok()?;
    let frac_ms = match frac.len() {
        0 => 0,
        1 => frac.parse::<u64>().ok()? * 100,
        2 => frac.parse::<u64>().ok()? * 10,
        _ => frac[..3].parse::<u64>().ok()?,
    };
    // Testi della comunità: valori assurdi si scartano invece di traboccare.
    min.checked_mul(60_000)?
        .checked_add(sec.checked_mul(1000)?)?
        .checked_add(frac_ms)
}

/// Rimuove i tempi per parola del formato "enhanced", es. `<00:12.34>parola`.
fn strip_word_timestamps(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        match rest[start..].find('>') {
            Some(len) if parse_timestamp(&rest[start + 1..start + len]).is_some() => {
                out.push_str(&rest[..start]);
                rest = &rest[start + len + 1..];
            }
            _ => {
                out.push_str(&rest[..=start]);
                rest = &rest[start + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times(l: &Lyrics) -> Vec<u64> {
        l.lines.iter().map(|l| l.time_ms).collect()
    }

    #[test]
    fn parses_basic_lrc_and_skips_metadata() {
        let l = Lyrics::parse(
            "[ar:Lumen Coast]\n[ti:Paper Sky]\n[00:12.00]prima riga\n[00:17.20] seconda riga \n",
        );
        assert_eq!(l.lines.len(), 2);
        assert_eq!(
            l.lines[0],
            LyricLine {
                time_ms: 12_000,
                text: "prima riga".into()
            }
        );
        assert_eq!(
            l.lines[1],
            LyricLine {
                time_ms: 17_200,
                text: "seconda riga".into()
            }
        );
    }

    #[test]
    fn supports_every_fraction_format() {
        let l = Lyrics::parse(
            "[01:02]a\n[01:02.5]b\n[01:02.05]c\n[01:02.005]d\n[01:02:50]e\n[01:02.12345]f",
        );
        assert_eq!(
            times(&l),
            vec![62_000, 62_005, 62_050, 62_123, 62_500, 62_500]
        );
    }

    #[test]
    fn repeated_timestamps_become_separate_lines_in_order() {
        let l = Lyrics::parse("[00:30.00][00:10.00]ritornello\n[00:20.00]strofa");
        assert_eq!(times(&l), vec![10_000, 20_000, 30_000]);
        assert_eq!(l.lines[0].text, "ritornello");
        assert_eq!(l.lines[2].text, "ritornello");
    }

    #[test]
    fn keeps_empty_lines_as_instrumental_gaps() {
        let l = Lyrics::parse("[00:01.00]ciao\n[00:05.00]\n[00:09.00]di nuovo");
        assert_eq!(l.lines.len(), 3);
        assert_eq!(l.lines[1].text, "");
    }

    #[test]
    fn applies_offset() {
        let l = Lyrics::parse("[offset:+500]\n[00:10.00]a\n[00:00.20]b");
        assert_eq!(times(&l), vec![0, 9_500]);
        let l = Lyrics::parse("[offset:-250]\n[00:10.00]a");
        assert_eq!(times(&l), vec![10_250]);
    }

    #[test]
    fn strips_word_level_timestamps() {
        let l = Lyrics::parse("[00:01.00]<00:01.00>uno <00:01.50>due <a>tre");
        assert_eq!(l.lines[0].text, "uno due <a>tre");
    }

    #[test]
    fn rejects_malformed_tags() {
        let l = Lyrics::parse("[aa:bb]x\n[00:1x]y\n[length: 03:20]\n[00:03.00 senza chiusura\n");
        assert!(l.is_empty());
    }

    #[test]
    fn survives_absurd_values() {
        let l = Lyrics::parse(
            "[99999999999999999999:00.00]x\n[18446744073709551:00.00]y\n[offset:-9223372036854775808]\n[00:01.00]z",
        );
        assert_eq!(l.lines.len(), 1);
        assert_eq!(l.lines[0].text, "z");
    }

    #[test]
    fn finds_current_and_next_line() {
        let l = Lyrics::parse("[00:10.00]a\n[00:20.00]b\n[00:30.00]c");
        assert_eq!(l.index_at(0), None);
        assert_eq!(l.index_at(9_999), None);
        assert_eq!(l.index_at(10_000), Some(0));
        assert_eq!(l.index_at(25_000), Some(1));
        assert_eq!(l.index_at(99_000), Some(2));
        assert_eq!(l.next_time_after(0), Some(10_000));
        assert_eq!(l.next_time_after(10_000), Some(20_000));
        assert_eq!(l.next_time_after(30_000), None);
    }
}
