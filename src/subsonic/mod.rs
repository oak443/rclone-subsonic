pub mod auth;
pub mod handlers;
pub mod response;

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;
use sqlx::SqlitePool;
use tower_http::cors::CorsLayer;

use crate::config::Config;
use crate::ids;
use crate::rclone::RcloneClient;
use crate::scanner::Scanner;
use response::Format;

pub struct AppState {
    pub config: Arc<Config>,
    pub db: SqlitePool,
    pub scanner: Arc<Scanner>,
    pub rclone: Arc<RcloneClient>,
    pub cover_cache: std::path::PathBuf,
    pub image_secret: Vec<u8>,
}

#[derive(Debug, Default, Clone)]
pub struct Params {
    pairs: Vec<(String, String)>,
}

impl Params {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|value| value.parse().ok())
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/rest/{file}", get(dispatch).post(dispatch))
        .route("/share/img/{id}/{token}", get(public_image))
        .route("/", get(index))
        .route("/healthz", get(health))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn index() -> &'static str {
    "rclone-subsonic\n"
}

async fn health(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let status = state.scanner.status();
    Json(json!({
        "status": "ok",
        "scanning": status.scanning,
        "count": status.count,
        "error": status.error,
    }))
}

async fn dispatch(
    State(state): State<Arc<AppState>>,
    Path(file): Path<String>,
    RawQuery(raw_query): RawQuery,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let method_name = file.strip_suffix(".view").unwrap_or(&file);
    if method_name.is_empty() {
        return response::plain(StatusCode::NOT_FOUND, "not found");
    }

    let mut pairs: Vec<(String, String)> = raw_query
        .map(|query| {
            url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_default();

    let is_form = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.starts_with("application/x-www-form-urlencoded"))
        .unwrap_or(false);
    if is_form && !body.is_empty() {
        pairs.extend(
            url::form_urlencoded::parse(&body)
                .into_owned()
                .collect::<Vec<_>>(),
        );
    }

    let params = Params { pairs };
    let format = request_format(&params);
    tracing::debug!(api = method_name, http = %method, "API 请求");

    if let Err(error) = auth::verify(&params, &state.config.auth) {
        tracing::debug!(api = method_name, "认证失败");
        return response::fail(format, error);
    }

    match method_name {
        "stream" => handlers::stream(&state, &params, &headers, &method, format, false).await,
        "download" => handlers::stream(&state, &params, &headers, &method, format, true).await,
        "getCoverArt" => handlers::cover(&state, &params, &headers, format).await,
        other => {
            let base_url = public_base_url(&state, &headers);
            match handlers::api(&state, other, &params, &base_url).await {
                Ok(value) => response::ok(format, value),
                Err(error) => response::fail(format, error),
            }
        }
    }
}

async fn public_image(
    State(state): State<Arc<AppState>>,
    Path((id, token)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if !id.starts_with(ids::ARTIST_PREFIX) || !verify_image_token(&state.image_secret, &id, &token)
    {
        return response::plain(StatusCode::NOT_FOUND, "not found");
    }
    handlers::serve_art(&state, &id, &headers).await
}

/// 对外访问根地址：优先 config.server.public_url，其次按请求的 Host 推导。
fn public_base_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(url) = state
        .config
        .server
        .public_url
        .as_deref()
        .filter(|url| !url.is_empty())
    {
        return url.trim_end_matches('/').to_string();
    }
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("http");
    match headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(host) => format!("{scheme}://{host}"),
        None => format!("http://{}", state.config.server.bind),
    }
}

type HmacSha256 = Hmac<Sha256>;

pub fn image_token(secret: &[u8], id: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC 接受任意长度密钥");
    mac.update(id.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn image_url(base_url: &str, secret: &[u8], id: &str, size: u32) -> String {
    format!(
        "{base_url}/share/img/{id}/{}?size={size}",
        image_token(secret, id)
    )
}

fn verify_image_token(secret: &[u8], id: &str, token: &str) -> bool {
    let expected = image_token(secret, id);
    constant_eq(expected.as_bytes(), token.as_bytes())
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

fn request_format(params: &Params) -> Format {
    match params.get("f").or_else(|| params.get("format")) {
        Some("json") => Format::Json,
        _ => Format::Xml,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_tokens_are_stable_and_verified() {
        let secret = b"test-secret";
        let token = image_token(secret, "ar-abc");
        assert_eq!(token, image_token(secret, "ar-abc"));
        assert!(verify_image_token(secret, "ar-abc", &token));
        assert!(!verify_image_token(secret, "ar-other", &token));
        assert!(!verify_image_token(b"other-secret", "ar-abc", &token));
    }
}
