use md5::{Digest, Md5};

use crate::config::AuthConfig;

use super::Params;
use super::response::ApiError;

pub fn verify(params: &Params, auth: &AuthConfig) -> Result<(), ApiError> {
    let username = params.get("u").ok_or_else(|| ApiError::missing("u"))?;
    if !constant_eq(username.as_bytes(), auth.username.as_bytes()) {
        return Err(ApiError::wrong_credentials());
    }

    if let (Some(token), Some(salt)) = (params.get("t"), params.get("s")) {
        let digest = md5_hex(&format!("{}{}", auth.password, salt));
        if constant_eq(token.to_ascii_lowercase().as_bytes(), digest.as_bytes()) {
            return Ok(());
        }
        return Err(ApiError::wrong_credentials());
    }

    if let Some(password) = params.get("p") {
        if !auth.allow_plaintext {
            return Err(ApiError::new(41, "Token authentication is required"));
        }
        let password = if let Some(encoded) = password.strip_prefix("enc:") {
            hex::decode(encoded)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .unwrap_or_default()
        } else {
            password.to_string()
        };
        if constant_eq(password.as_bytes(), auth.password.as_bytes()) {
            return Ok(());
        }
        return Err(ApiError::wrong_credentials());
    }

    Err(ApiError::new(40, "Missing authentication token"))
}

pub fn md5_hex(input: &str) -> String {
    hex::encode(Md5::digest(input.as_bytes()))
}

fn constant_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_subsonic_token() {
        // md5("sesame" + "c19b2d")，与 Subsonic 文档一致
        assert_eq!(md5_hex("sesamec19b2d"), "26719a1196d2a940705a59634eb18eab");
    }
}
