//! Instrumentation for isolated E2E fixtures, compiled only with the test-fixture feature.
use crate::error::{AppError, Result};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

static CONFIG: OnceLock<Value> = OnceLock::new();
static COUNTS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static RESULTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());
const NAMES: [&str; 4] = [
    "configReads",
    "sessionChecks",
    "sessionReads",
    "decryptSpawns",
];

fn config() -> &'static Value {
    CONFIG.get_or_init(|| {
        std::env::var_os("PLUMIVOX_TEST_HOOKS")
            .and_then(|file| fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null)
    })
}
fn file(name: &str) -> Option<&'static Path> {
    config().get(name)?.as_str().map(Path::new)
}
pub fn count(name: &str) {
    if let Some(index) = NAMES.iter().position(|item| *item == name) {
        COUNTS[index].fetch_add(1, Ordering::SeqCst);
    }
}
pub fn begin_call() -> [u64; 4] {
    std::array::from_fn(|index| COUNTS[index].load(Ordering::SeqCst))
}
pub fn end_call(name: &str, result: &Value, before: [u64; 4]) {
    if let Some(path) = file("counts_path") {
        let mut item = json!({"name": name});
        for (index, key) in NAMES.iter().enumerate() {
            item[*key] = json!(
                COUNTS[index]
                    .load(Ordering::SeqCst)
                    .saturating_sub(before[index])
            );
        }
        if let Ok(mut results) = RESULTS.lock() {
            results.push(item);
            if let Ok(bytes) = serde_json::to_vec(&*results) {
                let _ = fs::write(path, bytes);
            }
        }
    }
    if name == "refresh_session" {
        // 規格上取り消されたMCP応答は抑制されるため、安全なerror codeだけを別途観測する。
        let content: Value = result
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or(Value::Null);
        let code = result
            .pointer("/error/code")
            .or_else(|| result.pointer("/structuredContent/error/code"))
            .or_else(|| content.pointer("/error/code"));
        if let (Some(path), Some(code)) = (file("result_path"), code.and_then(Value::as_str)) {
            let _ = fs::write(path, json!({"code": code}).to_string());
        }
    }
}
pub fn config_read(path: &Path, value: &Value) {
    count("configReads");
    if COUNTS[0].load(Ordering::SeqCst) == 1 {
        if let Some(account) = config()
            .get("mutate_default_after_first_read")
            .and_then(Value::as_str)
        {
            let mut changed = value.clone();
            changed["twitterapi_io"]["default_account"] = json!(account);
            let _ = fs::write(path, changed.to_string());
        }
    }
}
pub fn lock_lost_requested() -> bool {
    file("lose_lock_path").is_some_and(Path::exists)
}

pub async fn boundary(phase: &str, cancel: &CancellationToken) -> Result<()> {
    if config().get("gate").and_then(Value::as_str) != Some(phase) {
        return Ok(());
    }
    if let Some(path) = file("phase_path") {
        fs::write(
            path,
            json!({"phase":phase,"pid": std::process::id()}).to_string(),
        )
        .map_err(|_| AppError::new("INTERNAL_ERROR", "Fixture phase could not be recorded"))?;
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        if lock_lost_requested() {
            return Err(AppError::new(
                if phase == "saved" {
                    "AUTH_SAVE_UNCERTAIN"
                } else {
                    "AUTH_LOCK_LOST"
                },
                "Fixture lock ownership ended",
            ));
        }
        if cancel.is_cancelled() {
            return Err(AppError::new(
                if phase == "saved" {
                    "AUTH_SAVE_UNCERTAIN"
                } else {
                    "AUTH_CANCELLED"
                },
                "Fixture request was cancelled",
            ));
        }
        if file("resume_path").is_some_and(Path::exists) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::new("INTERNAL_ERROR", "Fixture gate timed out"));
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Replace only the OS decrypt boundary, retaining production byte limits, JSON and binding checks.
pub fn decrypted_fixture(encrypted: &[u8]) -> Option<Vec<u8>> {
    if config()
        .get("decrypted_output_fixture")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return None;
    }
    let marker: Value = serde_json::from_slice(encrypted).ok()?;
    let name = marker.get("name")?.as_str()?;
    let bytes = marker.get("bytes")?.as_u64()? as usize;
    if bytes > 4 * 1024 * 1024 {
        return None;
    }
    let binding = config().get("binding")?.as_str()?;
    let mut body = serde_json::to_vec(&json!({"schema_version":2,"cookie":"synthetic-🙂あe\u{0301}-cookie", "binding":binding,"user_name":name,"source":"plumivox_api","acquired_at":"2026-10-21T00:00:00.000Z"})).ok()?;
    body.resize(bytes.max(body.len()), b' ');
    Some(body)
}
