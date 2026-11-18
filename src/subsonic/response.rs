use std::io::Write;

use axum::body::Body;
use axum::http::{StatusCode, header};
use axum::response::Response;
use quick_xml::Writer;
use quick_xml::escape::escape;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use serde_json::{Map, Value, json};

pub const VERSION: &str = "1.16.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Xml,
}

#[derive(Debug, Clone)]
pub struct ApiError {
    pub code: i64,
    pub message: String,
}

impl ApiError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn generic(message: impl Into<String>) -> Self {
        Self::new(0, message)
    }

    pub fn missing(parameter: &str) -> Self {
        Self::new(10, format!("Required parameter is missing: {parameter}"))
    }

    pub fn wrong_credentials() -> Self {
        Self::new(40, "Wrong username or password")
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(70, message)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        tracing::warn!("API 内部错误: {error:#}");
        Self::generic(error.to_string())
    }
}

pub fn ok(format: Format, mut value: Value) -> Response {
    strip_nulls(&mut value);
    match format {
        Format::Json => json_response(&value, None),
        Format::Xml => match xml_body(&value, false, None) {
            Ok(bytes) => response_with(header_value("text/xml; charset=utf-8"), bytes),
            Err(error) => {
                tracing::error!("生成 XML 失败: {error}");
                plain(StatusCode::INTERNAL_SERVER_ERROR, "XML error")
            }
        },
    }
}

pub fn fail(format: Format, error: ApiError) -> Response {
    match format {
        Format::Json => json_response(&Value::Object(Map::new()), Some(&error)),
        Format::Xml => match xml_body(&Value::Object(Map::new()), true, Some(&error)) {
            Ok(bytes) => response_with(header_value("text/xml; charset=utf-8"), bytes),
            Err(write_error) => {
                tracing::error!("生成错误 XML 失败: {write_error}");
                plain(StatusCode::INTERNAL_SERVER_ERROR, "XML error")
            }
        },
    }
}

fn json_response(value: &Value, error: Option<&ApiError>) -> Response {
    let mut root = json_envelope(error.is_some());
    if let Some(error) = error {
        root.insert(
            "error".to_string(),
            json!({ "code": error.code, "message": error.message }),
        );
    } else if let Value::Object(map) = value {
        root.extend(map.clone());
    }
    let body = json!({ "subsonic-response": Value::Object(root) });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    response_with(header_value("application/json"), bytes)
}

fn json_envelope(failed: bool) -> Map<String, Value> {
    let mut root = Map::new();
    root.insert(
        "status".to_string(),
        json!(if failed { "failed" } else { "ok" }),
    );
    root.insert("version".to_string(), json!(VERSION));
    root.insert("type".to_string(), json!("rclone-subsonic"));
    root.insert(
        "serverVersion".to_string(),
        json!(env!("CARGO_PKG_VERSION")),
    );
    root.insert("openSubsonic".to_string(), json!(true));
    root
}

fn strip_nulls(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|_, child| !child.is_null());
            for child in map.values_mut() {
                strip_nulls(child);
            }
        }
        Value::Array(items) => {
            for child in items {
                strip_nulls(child);
            }
        }
        _ => {}
    }
}

fn xml_body(
    value: &Value,
    failed: bool,
    error: Option<&ApiError>,
) -> Result<Vec<u8>, quick_xml::Error> {
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    let mut root = BytesStart::new("subsonic-response");
    root.push_attribute(("xmlns", "http://subsonic.org/restapi"));
    root.push_attribute(("status", if failed { "failed" } else { "ok" }));
    root.push_attribute(("version", VERSION));
    root.push_attribute(("type", "rclone-subsonic"));
    let server_version = env!("CARGO_PKG_VERSION");
    root.push_attribute(("serverVersion", server_version));
    root.push_attribute(("openSubsonic", "true"));
    writer.write_event(Event::Start(root))?;

    if let Some(error) = error {
        let mut start = BytesStart::new("error");
        start.push_attribute(("code", error.code.to_string().as_str()));
        let message = escape(&error.message);
        start.push_attribute(("message", message.as_ref()));
        writer.write_event(Event::Empty(start))?;
    } else if let Value::Object(map) = value {
        for (key, item) in map {
            write_value(&mut writer, key, item)?;
        }
    }

    writer.write_event(Event::End(BytesEnd::new("subsonic-response")))?;
    Ok(writer.into_inner())
}

fn write_value<W: Write>(
    writer: &mut Writer<W>,
    key: &str,
    value: &Value,
) -> Result<(), quick_xml::Error> {
    match value {
        Value::Null => Ok(()),
        Value::Object(map) => {
            writer.write_event(Event::Start(BytesStart::new(key)))?;
            for (child_key, child) in map {
                write_value(writer, child_key, child)?;
            }
            writer.write_event(Event::End(BytesEnd::new(key)))?;
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                write_value(writer, key, item)?;
            }
            Ok(())
        }
        Value::String(text) => {
            writer.write_event(Event::Start(BytesStart::new(key)))?;
            writer.write_event(Event::Text(BytesText::new(text)))?;
            writer.write_event(Event::End(BytesEnd::new(key)))?;
            Ok(())
        }
        Value::Number(number) => {
            writer.write_event(Event::Start(BytesStart::new(key)))?;
            writer.write_event(Event::Text(BytesText::new(&number.to_string())))?;
            writer.write_event(Event::End(BytesEnd::new(key)))?;
            Ok(())
        }
        Value::Bool(boolean) => {
            writer.write_event(Event::Start(BytesStart::new(key)))?;
            writer.write_event(Event::Text(BytesText::new(if *boolean {
                "true"
            } else {
                "false"
            })))?;
            writer.write_event(Event::End(BytesEnd::new(key)))?;
            Ok(())
        }
    }
}

fn header_value(value: &'static str) -> axum::http::HeaderValue {
    axum::http::HeaderValue::from_static(value)
}

fn response_with(content_type: axum::http::HeaderValue, bytes: Vec<u8>) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| plain(StatusCode::INTERNAL_SERVER_ERROR, "response error"))
}

pub fn plain(status: StatusCode, message: &str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(message.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_null_object_fields() {
        let mut value = json!({ "a": null, "b": { "c": null, "d": 1 } });
        strip_nulls(&mut value);
        assert_eq!(value, json!({ "b": { "d": 1 } }));
    }
}
