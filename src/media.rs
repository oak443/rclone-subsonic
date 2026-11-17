pub const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "mp3", "m4a", "aac", "ogg", "opus", "wav", "wma", "ape", "aiff", "aif", "alac", "dsf",
    "dff",
];

pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

pub fn extension(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn is_audio(name: &str) -> bool {
    AUDIO_EXTENSIONS.contains(&extension(name).as_str())
}

pub fn is_image(name: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&extension(name).as_str())
}

pub fn content_type(suffix: &str) -> &'static str {
    match suffix {
        "flac" => "audio/flac",
        "mp3" => "audio/mpeg",
        "m4a" | "alac" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
        "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "wma" => "audio/x-ms-wma",
        "ape" => "audio/x-ape",
        "aiff" | "aif" => "audio/aiff",
        "dsf" | "dff" => "audio/x-dsf",
        _ => "application/octet-stream",
    }
}

pub fn image_content_type(name: &str) -> &'static str {
    match extension(name).as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/jpeg",
    }
}

pub fn sort_name(name: &str) -> String {
    let lower = name.to_lowercase();
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = lower.strip_prefix(article) {
            let rest = rest.trim_start();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    lower
}

/// 从文件名提取 (音轨号, 标题)。例如 "01 星間飛行.flac" -> (Some(1), "星間飛行")
pub fn parse_filename(name: &str) -> (Option<u32>, String) {
    let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name);
    let stem = stem.trim();
    let digits_len = stem.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits_len == 0 {
        return (None, stem.to_string());
    }
    let number = stem[..digits_len].parse::<u32>().ok();
    let rest = &stem[digits_len..];
    let title = rest
        .trim_start_matches(|c: char| {
            c.is_whitespace() || matches!(c, '-' | '.' | '_' | '－' | '　' | '|')
        })
        .trim();
    let title = if title.is_empty() {
        stem.to_string()
    } else {
        title.to_string()
    };
    (number, title)
}

pub fn leading_number(name: &str) -> Option<u32> {
    let digits: String = name
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u32>().ok()
    }
}

/// 归一化文件名主干：小写、只保留字母数字、去掉数字串前导零。
pub fn norm_stem(name: &str) -> String {
    let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name);
    let mut out = String::new();
    let mut digits = String::new();
    let flush = |out: &mut String, digits: &mut String| {
        if !digits.is_empty() {
            let trimmed = digits.trim_start_matches('0');
            out.push_str(trimmed);
            digits.clear();
        }
    };
    for ch in stem.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            flush(&mut out, &mut digits);
            if ch.is_alphanumeric() {
                out.extend(ch.to_lowercase());
            }
        }
    }
    flush(&mut out, &mut digits);
    out
}

pub fn extract_year(text: &str) -> Option<i64> {
    let chars: Vec<char> = text.chars().collect();
    for window in chars.windows(4) {
        if !window.iter().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let value: String = window.iter().collect();
        if let Ok(year) = value.parse::<i64>()
            && (1900..=2100).contains(&year)
        {
            return Some(year);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_track_names() {
        assert_eq!(
            parse_filename("01 星間飛行.flac"),
            (Some(1), "星間飛行".to_string())
        );
        assert_eq!(
            parse_filename("1 - Hello.mp3"),
            (Some(1), "Hello".to_string())
        );
        assert_eq!(parse_filename("intro.flac"), (None, "intro".to_string()));
    }

    #[test]
    fn normalizes_cue_names() {
        assert_eq!(norm_stem("1 - 星間飛行.flac"), "1星間飛行");
        assert_eq!(norm_stem("01 星間飛行.flac"), "1星間飛行");
    }

    #[test]
    fn strips_articles_for_sorting() {
        assert_eq!(sort_name("The Beatles"), "beatles");
        assert_eq!(sort_name("A Perfect Circle"), "perfect circle");
    }
}
