/// 从 FLAC 开头的 STREAMINFO 元数据块解析时长。
///
/// 只需要前 42 字节：4 字节 "fLaC" + 4 字节块头 + 34 字节 STREAMINFO。
/// 返回毫秒；数据不完整或不是 FLAC 时返回 None。
pub fn duration_ms(head: &[u8]) -> Option<u64> {
    if head.len() < 42 || &head[0..4] != b"fLaC" {
        return None;
    }
    let block_type = head[4] & 0x7f;
    if block_type != 0 {
        return None;
    }
    let block_len = u32::from_be_bytes([0, head[5], head[6], head[7]]) as usize;
    if block_len < 34 || head.len() < 8 + 34 {
        return None;
    }
    let b = &head[8..42];
    let bits = u64::from_be_bytes(b[10..18].try_into().ok()?);
    let sample_rate = (bits >> 44) & 0xF_FFFF;
    let total_samples = bits & ((1u64 << 36) - 1);
    if sample_rate == 0 || total_samples == 0 {
        return None;
    }
    Some(total_samples * 1000 / sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(sample_rate: u64, total_samples: u64) -> Vec<u8> {
        let mut head = Vec::new();
        head.extend_from_slice(b"fLaC");
        head.extend_from_slice(&[0x00, 0x00, 0x00, 34]); // STREAMINFO, len 34
        // min/max block size + min/max frame size
        head.extend_from_slice(&[0u8; 10]);
        let bits = (sample_rate << 44) | (1 << 41) | (15 << 36) | total_samples;
        head.extend_from_slice(&bits.to_be_bytes());
        head.extend_from_slice(&[0u8; 16]); // MD5 signature
        assert_eq!(head.len(), 42);
        head
    }

    #[test]
    fn parses_streaminfo() {
        let head = header(44_100, 10_319_400);
        assert_eq!(duration_ms(&head), Some(234_000));
    }

    #[test]
    fn rejects_short_or_wrong() {
        assert_eq!(duration_ms(b"fLaC"), None);
        assert_eq!(duration_ms(&[0u8; 42]), None);
    }
}
