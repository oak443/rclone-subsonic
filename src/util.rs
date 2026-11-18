use md5::{Digest, Md5};

pub fn md5_hex(input: &str) -> String {
    hex::encode(Md5::digest(input.as_bytes()))
}

pub fn constant_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right) {
        diff |= a ^ b;
    }
    diff == 0
}
