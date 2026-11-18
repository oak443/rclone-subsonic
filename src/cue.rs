use encoding_rs::Encoding;

use crate::media::extract_year;

pub const MAX_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct CueSheet {
    pub title: Option<String>,
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
    /// 是否出现 INDEX 01；用于识别把 INDEX 01 放在 FILE 之后的 CUE
    pub has_index01: bool,
}

pub fn parse(bytes: &[u8]) -> Result<CueSheet, String> {
    if bytes.is_empty() {
        return Err("CUE 文件为空".to_string());
    }
    if bytes.len() > MAX_BYTES {
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
                    if !track.has_index01 {
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
                    has_index01: false,
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
            "INDEX" => {
                let index = rest.split_whitespace().next().unwrap_or("");
                if index.trim_start_matches('0') == "1"
                    && let Some(track) = current_track.as_mut()
                {
                    track.has_index01 = true;
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
        assert_eq!(sheet.title.as_deref(), Some("星間飛行"));
        assert_eq!(sheet.tracks.len(), 3);
        assert_eq!(sheet.tracks[0].number, 1);
        assert_eq!(sheet.tracks[0].title.as_deref(), Some("星間飛行"));
        assert!(sheet.tracks.iter().all(|track| track.has_index01));
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
}
