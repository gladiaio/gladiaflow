//! Optional MLX OpenPII sidecar client (Apple Silicon).
//!
//! The Python MLX server is started by GladiaFlow when HF weights are present.
//! Requests stay on 127.0.0.1 and must finish inside a short timeout.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const DEFAULT_PORT: u16 = 8765;
const REQUEST_TIMEOUT: Duration = Duration::from_millis(80);

static SIDECAR: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
static ENDPOINT: OnceLock<String> = OnceLock::new();

fn port() -> u16 {
    std::env::var("OPENPII_MLX_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

fn endpoint() -> &'static str {
    ENDPOINT.get_or_init(|| format!("127.0.0.1:{}", port()))
}

/// Resolve HF weights used by the MLX Python runtime.
pub fn resolve_mlx_model_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("GLADIAFLOW_NER_MLX_MODEL_DIR") {
        let path = PathBuf::from(dir);
        if looks_like_hf_dir(&path) {
            return Some(path);
        }
    }
    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(".models/openpii-xlmrL2-hf"));
        candidates.push(cwd.join("../.models/openpii-xlmrL2-hf"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.models/openpii-xlmrL2-hf"));
    if let Some(data) = dirs::data_dir() {
        candidates.push(data.join("gladiaflow/models/openpii-xlmrL2-hf"));
    }
    candidates.into_iter().find(|p| looks_like_hf_dir(p))
}

fn looks_like_hf_dir(path: &Path) -> bool {
    path.join("model.safetensors").is_file()
        && path.join("tokenizer.json").is_file()
        && path.join("config.json").is_file()
}

fn serve_script() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/openpii_mlx/serve.py"));
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("tools/openpii_mlx/serve.py"));
        candidates.push(cwd.join("../tools/openpii_mlx/serve.py"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn python_bin() -> String {
    std::env::var("GLADIAFLOW_PYTHON").unwrap_or_else(|_| "python3".to_string())
}

/// Spawn the MLX sidecar if weights + script exist. Idempotent.
pub fn warmup() {
    if health_ok() {
        log::info!("[screen_context] OpenPII MLX sidecar already healthy");
        return;
    }
    let Some(model_dir) = resolve_mlx_model_dir() else {
        log::info!("[screen_context] OpenPII MLX weights not found");
        return;
    };
    let Some(script) = serve_script() else {
        log::warn!("[screen_context] openpii_mlx/serve.py not found");
        return;
    };

    let slot = SIDECAR.get_or_init(|| Mutex::new(None));
    let mut guard = match slot.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if guard.is_some() && health_ok() {
        return;
    }

    match Command::new(python_bin())
        .arg(&script)
        .arg("--model-dir")
        .arg(&model_dir)
        .arg("--port")
        .arg(port().to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            log::info!(
                "[screen_context] started OpenPII MLX sidecar pid={} model={}",
                child.id(),
                model_dir.display()
            );
            *guard = Some(child);
        }
        Err(error) => {
            log::warn!("[screen_context] failed to start MLX sidecar: {error}");
            return;
        }
    }

    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if health_ok() {
            log::info!("[screen_context] OpenPII MLX sidecar ready");
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    log::warn!("[screen_context] OpenPII MLX sidecar did not become healthy in time");
}

fn health_ok() -> bool {
    http_get_ok(&format!("http://{}/health", endpoint()), Duration::from_millis(50))
}

/// True when the MLX sidecar answers health checks.
pub fn is_ready() -> bool {
    health_ok()
}

/// Extract vocabulary terms via the MLX sidecar. Empty on any failure/timeout.
pub fn extract_entity_terms(text: &str) -> Vec<String> {
    if text.trim().is_empty() || !health_ok() {
        return Vec::new();
    }
    let body = serde_json::json!({ "text": text }).to_string();
    match http_post_json(
        &format!("http://{}/ner/terms", endpoint()),
        &body,
        REQUEST_TIMEOUT,
    ) {
        Some(raw) => serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|v| {
                v.get("terms")
                    .and_then(|t| t.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
            })
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

fn http_get_ok(url: &str, timeout: Duration) -> bool {
    http_exchange("GET", url, None, timeout).is_some()
}

fn http_post_json(url: &str, body: &str, timeout: Duration) -> Option<String> {
    http_exchange("POST", url, Some(body), timeout)
}

fn http_exchange(
    method: &str,
    url: &str,
    body: Option<&str>,
    timeout: Duration,
) -> Option<String> {
    // Tiny HTTP/1.0 client — avoids pulling reqwest into the harvest hot path.
    let url = url.strip_prefix("http://")?;
    let (host_port, path) = url.split_once('/')?;
    let path = format!("/{path}");
    let mut stream = TcpStream::connect(host_port).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;

    let mut req = format!(
        "{method} {path} HTTP/1.0\r\nHost: {host_port}\r\nConnection: close\r\n"
    );
    if let Some(body) = body {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
        req.push_str(body);
    } else {
        req.push_str("\r\n");
    }
    stream.write_all(req.as_bytes()).ok()?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let mut parts = text.splitn(2, "\r\n\r\n");
    let headers = parts.next().unwrap_or("");
    let body = parts.next().unwrap_or("").to_string();
    if !headers.starts_with("HTTP/1.0 200") && !headers.starts_with("HTTP/1.1 200") {
        return None;
    }
    Some(body)
}
