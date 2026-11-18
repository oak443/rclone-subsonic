use crate::util::md5_hex;

pub const ARTIST_PREFIX: &str = "ar-";
pub const ALBUM_PREFIX: &str = "al-";
pub const TRACK_PREFIX: &str = "tr-";

pub fn artist_id(name: &str) -> String {
    format!("{ARTIST_PREFIX}{}", md5_hex(name))
}

pub fn album_id(dir_path: &str) -> String {
    format!("{ALBUM_PREFIX}{}", md5_hex(dir_path))
}

pub fn track_id(file_path: &str) -> String {
    format!("{TRACK_PREFIX}{}", md5_hex(file_path))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Artist,
    Album,
    Track,
}

pub fn entity_kind(id: &str) -> Option<EntityKind> {
    if id.starts_with(ARTIST_PREFIX) {
        Some(EntityKind::Artist)
    } else if id.starts_with(ALBUM_PREFIX) {
        Some(EntityKind::Album)
    } else if id.starts_with(TRACK_PREFIX) {
        Some(EntityKind::Track)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_ids() {
        assert_eq!(artist_id("a"), artist_id("a"));
        assert_ne!(artist_id("a"), artist_id("b"));
        assert!(album_id("x/y").starts_with(ALBUM_PREFIX));
        assert_eq!(entity_kind(&track_id("p")), Some(EntityKind::Track));
        assert_eq!(entity_kind("1"), None);
    }
}
