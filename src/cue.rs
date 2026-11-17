use encoding_rs::Encoding;

#[derive(Debug, Clone, Default)]
pub struct CueSheet {
    pub title: Option<String>,
    pub performer: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i64>,
    pub disc: u32,
    pub tracks: Vec<CueTrack>,
}

#[derive(Debug, Clone)]
pub struct CueTrack {
    /// CUE 中当前音轨所属的 FILE 名（可能和实际文件名不一致）
    pub file: Option<String>,
    pub number: u32,
    pub title: Option<String>,
    pub performer: Option<String>,
    /// INDEX 01 相对 FILE 的毫秒偏移
    pub index01_ms: Option<u64>,
}

pub fn parse(bytes: &[u8]) -> Result<CueSheet, String> {
    if bytes.is_empty() {
        return Err("CUE 文件为空".to_string());
    }
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("CUE 文件过大".to_string());
    }
    let text = decode(bytes);
    parse_text(&text)
}

pub fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    let encoding = detector.guess(None, true);
    decode_with(encoding, bytes)
}

fn decode_with(encoding: &'static Encoding, bytes: &[u8]) -> String {
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}

fn parse_text(text: &str) -> Result<CueSheet, String> {
    let mut sheet = CueSheet {
        disc: 1,
        ..Default::default()
    };
    let mut current_file: Option<String> = None;
    let mut current_track: Option<CueTrack> = None;

    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let key = parts.next().unwrap_or("").to_ascii_uppercase();
        let rest = parts.next().unwrap_or("").trim();

        match key.as_str() {
            "REM" => {
                let mut kv = rest.splitn(2, char::is_whitespace);
                let sub = kv.next().unwrap_or("").to_ascii_uppercase();
                let val = value(kv.next().unwrap_or(""));
                match sub.as_str() {
                    "DATE" | "YEAR" => sheet.year = extract_year(&val),
                    "GENRE" => sheet.genre = nonempty(val),
                    "DISC" => {
                        if let Ok(n) = val.parse::<u32>() {
                            sheet.disc = n.max(1);
                        }
                    }
                    _ => {}
                }
            }
            "FILE" => {
                let name = value(rest);
                if name.is_empty() {
                    continue;
                }
                if let Some(track) = current_track.as_mut() {
                    // 少数 CUE（如 cyanrip 产物）把 INDEX 01 放在 FILE 之后
                    if track.index01_ms.is_none() {
                        track.file = Some(name.clone());
                    }
                }
                current_file = Some(name);
            }
            "TRACK" => {
                if let Some(track) = current_track.take() {
                    sheet.tracks.push(track);
                }
                let number = rest
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(sheet.tracks.len() as u32 + 1);
                current_track = Some(CueTrack {
                    file: current_file.clone(),
                    number,
                    title: None,
                    performer: None,
                    index01_ms: None,
                });
            }
            "TITLE" => {
                let val = nonempty(value(rest));
                if let Some(track) = current_track.as_mut() {
                    track.title = val;
                } else {
                    sheet.title = val;
                }
            }
            "PERFORMER" => {
                let val = nonempty(value(rest));
                if let Some(track) = current_track.as_mut() {
                    track.performer = val;
                } else {
                    sheet.performer = val;
                }
            }
            "INDEX" => {
                let mut it = rest.split_whitespace();
                let index = it.next().unwrap_or("");
                let time = it.next().unwrap_or("");
                if index.trim_start_matches('0') == "1"
                    && let Some(track) = current_track.as_mut()
                {
                    track.index01_ms = parse_time_ms(time);
                    if track.file.is_none() {
                        track.file = current_file.clone();
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(track) = current_track.take() {
        sheet.tracks.push(track);
    }

    if sheet.tracks.is_empty() {
        return Err("CUE 中没有 TRACK".to_string());
    }
    Ok(sheet)
}

fn value(input: &str) -> String {
    let input = input.trim();
    if let Some(rest) = input.strip_prefix('"') {
        if let Some(end) = rest.rfind('"') {
            return rest[..end].to_string();
        }
        return rest.trim_end_matches('"').to_string();
    }
    input.to_string()
}

fn nonempty(value: String) -> Option<String> {
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn extract_year(input: &str) -> Option<i64> {
    let chars: Vec<char> = input.chars().collect();
    for window in chars.windows(4) {
        if !window.iter().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let text: String = window.iter().collect();
        if let Ok(year) = text.parse::<i64>()
            && (1900..=2100).contains(&year)
        {
            return Some(year);
        }
    }
    None
}

fn parse_time_ms(input: &str) -> Option<u64> {
    let parts: Vec<&str> = input.trim().split(':').collect();
    let (minutes, seconds, frames) = match parts.as_slice() {
        [m, s] => (m.parse::<u64>().ok()?, s.parse::<u64>().ok()?, 0),
        [m, s, f] => (
            m.parse::<u64>().ok()?,
            s.parse::<u64>().ok()?,
            f.parse::<u64>().ok()?,
        ),
        _ => return None,
    };
    Some((minutes * 60 + seconds) * 1000 + frames * 1000 / 75)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
REM DATE "2008-06-25"
REM GENRE "J-Pop"
PERFORMER "ランカ・リー＝中島愛"
TITLE "星間飛行"
FILE "1 - 星間飛行.flac" WAVE
  TRACK 01 AUDIO
    TITLE "星間飛行"
    PERFORMER "ランカ・リー＝中島愛"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "ねこ日記"
    PERFORMER "ランカ・リー＝中島愛"
    INDEX 00 03:51:22
FILE "2 - ねこ日記.flac" WAVE
    INDEX 01 00:00:00
TRACK 03 AUDIO
  TITLE "愛・おぼえていますか"
  INDEX 01 00:00:00
"#;

    #[test]
    fn parses_sample_with_mismatched_file_names() {
        let sheet = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(sheet.year, Some(2008));
        assert_eq!(sheet.genre.as_deref(), Some("J-Pop"));
        assert_eq!(sheet.performer.as_deref(), Some("ランカ・リー＝中島愛"));
        assert_eq!(sheet.title.as_deref(), Some("星間飛行"));
        assert_eq!(sheet.tracks.len(), 3);
        assert_eq!(sheet.tracks[0].number, 1);
        assert_eq!(sheet.tracks[0].title.as_deref(), Some("星間飛行"));
        assert_eq!(sheet.tracks[1].file.as_deref(), Some("2 - ねこ日記.flac"));
        assert_eq!(sheet.tracks[2].number, 3);
    }

    #[test]
    fn decodes_shift_jis_and_bom() {
        let (encoded, _, _) = encoding_rs::SHIFT_JIS.encode("TITLE \"星間飛行\"\nTRACK 01 AUDIO\n");
        let sheet = parse(&encoded).unwrap();
        assert_eq!(sheet.title.as_deref(), Some("星間飛行"));

        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(b"TITLE \"BOM\"\nTRACK 01 AUDIO\n");
        let sheet = parse(&with_bom).unwrap();
        assert_eq!(sheet.title.as_deref(), Some("BOM"));
    }

    #[test]
    fn parses_time() {
        assert_eq!(parse_time_ms("03:51:22"), Some(231_293));
        assert_eq!(parse_time_ms("00:00:00"), Some(0));
        assert_eq!(parse_time_ms("bad"), None);
    }
}
