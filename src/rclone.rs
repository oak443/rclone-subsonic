use std::net::TcpListener;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use futures_util::StreamExt;
use percent_encoding::percent_decode_str;
use rand::distributions::{Alphanumeric, DistString};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, RANGE};
use reqwest::{Method, Response};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteItem {
    #[serde(rename = "Path")]
    pub path: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Size", default)]
    pub size: i64,
    #[serde(rename = "ModTime", default)]
    pub mod_time: String,
    #[serde(rename = "IsDir", default)]
    pub is_dir: bool,
}

impl RemoteItem {
    pub fn mtime_ms(&self) -> i64 {
        chrono::DateTime::parse_from_rfc3339(&self.mod_time)
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0)
    }
}

#[derive(Clone)]
pub struct RcloneClient {
    http: reqwest::Client,
    base_http: Url,
    base_rc: String,
    http_user: String,
    http_pass: String,
    rc_user: String,
    rc_pass: String,
}

impl RcloneClient {
    pub fn new(
        http_url: &str,
        rc_url: &str,
        http_user: impl Into<String>,
        http_pass: impl Into<String>,
        rc_user: impl Into<String>,
        rc_pass: impl Into<String>,
    ) -> Result<Self> {
        let base_http =
            Url::parse(http_url).with_context(|| format!("无效的 storage.http_url: {http_url}"))?;
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(8)
            .build()
            .context("创建 HTTP 客户端失败")?;
        Ok(Self {
            http,
            base_http,
            base_rc: rc_url.trim_end_matches('/').to_string(),
            http_user: http_user.into(),
            http_pass: http_pass.into(),
            rc_user: rc_user.into(),
            rc_pass: rc_pass.into(),
        })
    }

    fn url_for(&self, path: &str) -> Result<Url> {
        let mut url = self.base_http.clone();
        let base = percent_decode_str(url.path())
            .decode_utf8_lossy()
            .trim_end_matches('/')
            .to_string();
        let path = path.trim_start_matches('/');
        let joined = if base.is_empty() {
            format!("/{path}")
        } else {
            format!("{base}/{path}")
        };
        url.set_path(&joined);
        url.set_query(None);
        url.set_fragment(None);
        Ok(url)
    }

    async fn rc_call(&self, endpoint: &str, body: &Value) -> Result<Value> {
        let url = format!("{}/{endpoint}", self.base_rc);
        let mut request = self.http.post(url).json(body);
        if !self.rc_user.is_empty() {
            request = request.basic_auth(&self.rc_user, Some(&self.rc_pass));
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("请求 rclone RC {endpoint} 失败"))?;
        let status = response.status();
        let value: Value = response.json().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            bail!("rclone RC {endpoint} 返回 {status}: {value}");
        }
        Ok(value)
    }

    pub async fn rc_version(&self) -> Result<Value> {
        self.rc_call("core/version", &json!({})).await
    }

    pub async fn list(&self, fs: &str, remote: &str, opt: Value) -> Result<Vec<RemoteItem>> {
        #[derive(Serialize)]
        struct Request<'a> {
            fs: &'a str,
            remote: &'a str,
            opt: Value,
        }
        #[derive(Deserialize)]
        struct ListResponse {
            #[serde(default)]
            list: Vec<RemoteItem>,
        }
        let value = self
            .rc_call(
                "operations/list",
                &serde_json::to_value(Request { fs, remote, opt })?,
            )
            .await?;
        let parsed: ListResponse =
            serde_json::from_value(value).context("解析 rclone operations/list 响应失败")?;
        Ok(parsed.list)
    }

    fn request(&self, method: Method, path: &str) -> Result<reqwest::RequestBuilder> {
        let url = self.url_for(path)?;
        let mut request = self.http.request(method, url);
        if !self.http_user.is_empty() {
            request = request.basic_auth(&self.http_user, Some(&self.http_pass));
        }
        Ok(request)
    }

    async fn file_request(
        &self,
        method: Method,
        path: &str,
        headers: &HeaderMap,
    ) -> Result<Response> {
        self.request(method, path)?
            .headers(headers.clone())
            .send()
            .await
            .with_context(|| format!("请求 rclone 文件失败: {path}"))
    }

    pub async fn stream(
        &self,
        path: &str,
        method: Method,
        headers: &[(HeaderName, HeaderValue)],
    ) -> Result<Response> {
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(name.clone(), value.clone());
        }
        self.file_request(method, path, &map).await
    }

    pub async fn fetch_head(&self, path: &str, bytes: usize) -> Result<Bytes> {
        let bytes = bytes.max(1);
        let response = self
            .request(Method::GET, path)?
            .header(RANGE, format!("bytes=0-{}", bytes - 1))
            .send()
            .await
            .with_context(|| format!("读取文件头失败: {path}"))?;
        if !response.status().is_success() {
            bail!("读取文件头失败: {path} -> {}", response.status());
        }
        let mut data = response.bytes().await?;
        if data.len() > bytes {
            data.truncate(bytes);
        }
        Ok(data)
    }

    pub async fn get_bytes_limited(&self, path: &str, max: usize) -> Result<Bytes> {
        let response = self
            .request(Method::GET, path)?
            .send()
            .await
            .with_context(|| format!("下载文件失败: {path}"))?;
        if !response.status().is_success() {
            bail!("下载文件失败: {path} -> {}", response.status());
        }
        if let Some(length) = response.content_length()
            && length > max as u64
        {
            bail!("文件过大: {path} ({length} bytes)");
        }
        let mut stream = response.bytes_stream();
        let mut data = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if data.len() + chunk.len() > max {
                bail!("文件过大: {path}");
            }
            data.extend_from_slice(&chunk);
        }
        Ok(Bytes::from(data))
    }
}

pub struct ManagedRclone {
    child: tokio::process::Child,
}

impl Drop for ManagedRclone {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

pub async fn start_managed(remote: &str, binary: &str) -> Result<(RcloneClient, ManagedRclone)> {
    let http_port = free_port()?;
    let rc_port = free_port()?;
    let http_user = rand_token();
    let http_pass = rand_token();
    let rc_user = rand_token();
    let rc_pass = rand_token();

    let mut command = tokio::process::Command::new(binary);
    command
        .arg("serve")
        .arg("http")
        .arg(remote)
        .arg("--addr")
        .arg(format!("127.0.0.1:{http_port}"))
        .arg("--user")
        .arg(&http_user)
        .arg("--pass")
        .arg(&http_pass)
        .arg("--rc")
        .arg("--rc-addr")
        .arg(format!("127.0.0.1:{rc_port}"))
        .arg("--rc-user")
        .arg(&rc_user)
        .arg("--rc-pass")
        .arg(&rc_pass)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .with_context(|| format!("启动 rclone 失败（binary = {binary}）"))?;

    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(relay_logs(stdout));
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(relay_logs(stderr));
    }

    let client = RcloneClient::new(
        &format!("http://127.0.0.1:{http_port}"),
        &format!("http://127.0.0.1:{rc_port}"),
        http_user,
        http_pass,
        rc_user,
        rc_pass,
    )?;

    for _ in 0..200 {
        if client.rc_version().await.is_ok() {
            tracing::info!(http_port, rc_port, remote, "rclone 已就绪");
            return Ok((client, ManagedRclone { child }));
        }
        if let Some(status) = child.try_wait().context("检查 rclone 状态失败")? {
            bail!("rclone 提前退出: {status}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let _ = child.start_kill();
    bail!("等待 rclone 就绪超时（20s）")
}

async fn relay_logs<R>(reader: R)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    use tokio::io::AsyncBufReadExt;
    let mut lines = tokio::io::BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::debug!(target: "rclone", "{line}");
    }
}

fn free_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).context("申请空闲端口失败")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn rand_token() -> String {
    Alphanumeric.sample_string(&mut rand::thread_rng(), 24)
}
