use crate::app::Ax;
use crate::db::now;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const CLAUDE_TTL: i64 = 10 * 60 * 1000;
const CODEX_TTL: i64 = 5 * 60 * 1000;

pub async fn read(a: &Ax) -> Value {
    let _guard = a.usage_lock.lock().await;
    let ids = selected(a);
    let rows = futures::future::join_all(ids.into_iter().map(|id| provider(a, id))).await;
    json!({"providers": rows})
}

fn selected(a: &Ax) -> Vec<&'static str> {
    let mut found = HashSet::new();
    if let Some(models) = a.set()["md"].as_object() {
        for model in models.values().filter_map(Value::as_str) {
            let (id, _) = crate::prov::parse(model);
            if matches!(id, "cc" | "codex") {
                found.insert(id);
            }
        }
    }
    ["cc", "codex"].into_iter().filter(|id| found.contains(id)).collect()
}

async fn provider(a: &Ax, id: &'static str) -> Value {
    let (name, ttl) = match id {
        "cc" => ("Claude", CLAUDE_TTL),
        "codex" => ("Codex", CODEX_TTL),
        _ => return json!({"id": id, "available": false, "windows": [], "message": "Usage is not supported for this provider."}),
    };
    let key = format!("quota:{id}");
    if let Some(v) = crate::gh::cg(a, &key, ttl) {
        return v;
    }

    match fetch(a, id).await {
        Ok(windows) => {
            let t = now();
            let v = json!({"id": id, "name": name, "available": true, "windows": windows, "updatedAt": t, "checkedAt": t, "stale": false});
            crate::gh::cs(a, &key, &v);
            v
        }
        Err(message) => {
            let stale = { a.cache.lock().unwrap().get(&key).cloned() };
            if let Some((_, mut old)) = stale {
                old["stale"] = json!(true);
                old["checkedAt"] = json!(now());
                old["message"] = json!(message);
                crate::gh::cs(a, &key, &old);
                old
            } else {
                let t = now();
                let v = json!({"id": id, "name": name, "available": false, "windows": [], "message": message, "updatedAt": Value::Null, "checkedAt": t, "stale": false});
                crate::gh::cs(a, &key, &v);
                v
            }
        }
    }
}

async fn fetch(a: &Ax, id: &str) -> Result<Vec<Value>, String> {
    match id {
        "cc" => claude(a).await,
        "codex" => codex(a).await,
        _ => Err("Usage is not supported for this provider.".into()),
    }
}

fn claude_token() -> Result<String, String> {
    if let Some(token) = std::env::var_os("CLAUDE_CODE_OAUTH_TOKEN")
        .and_then(|value| value.into_string().ok())
        .filter(|token| !token.trim().is_empty())
    {
        return Ok(token.trim().to_string());
    }
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude")))
        .ok_or_else(|| "Claude Code login credentials are not available in the local config file.".to_string())?;
    let content = std::fs::read_to_string(dir.join(".credentials.json"))
        .map_err(|_| "Claude Code login credentials are not available in the local config file.".to_string())?;
    let credentials: Value = serde_json::from_str(&content)
        .map_err(|_| "Claude Code login credentials could not be read.".to_string())?;
    credentials["claudeAiOauth"]["accessToken"]
        .as_str()
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "A Claude Code account login is required to read subscription usage.".into())
}

async fn claude(a: &Ax) -> Result<Vec<Value>, String> {
    let token = claude_token()?;
    // Claude Code uses this OAuth route for subscription windows; keep the poll interval conservative.
    let response = a
        .hc
        .get("https://api.anthropic.com/api/oauth/usage")
        .timeout(Duration::from_secs(10))
        .header("authorization", format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .await
        .map_err(|_| "Claude usage could not be reached.".to_string())?;
    let status = response.status().as_u16();
    if status >= 400 {
        return Err(match status {
            401 | 403 => "Claude Code login expired or does not include subscription usage access.".into(),
            429 => "Claude is rate limiting usage checks.".into(),
            _ => format!("Claude usage returned HTTP {status}."),
        });
    }
    let data: Value = response.json().await.map_err(|_| "Claude returned unreadable usage data.".to_string())?;
    let mut windows = Vec::new();
    if let Some(w) = quota_window(&data["five_hour"], "five_hour") {
        windows.push(w);
    }
    if let Some(w) = quota_window(&data["seven_day"], "weekly") {
        windows.push(w);
    }
    if windows.is_empty() {
        return Err("Claude did not return 5-hour or weekly subscription windows.".into());
    }
    Ok(windows)
}

fn quota_window(data: &Value, kind: &str) -> Option<Value> {
    let used = data["utilization"]
        .as_f64()
        .or_else(|| data["used_percentage"].as_f64())?;
    let reset = timestamp(&data["resets_at"]);
    Some(json!({"kind": kind, "remaining": (100.0 - used).clamp(0.0, 100.0), "resetAt": reset}))
}

fn timestamp(value: &Value) -> Option<i64> {
    if let Some(n) = value.as_i64() {
        return Some(n);
    }
    if let Some(n) = value.as_f64() {
        return Some(n as i64);
    }
    value
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp())
}

async fn codex(_: &Ax) -> Result<Vec<Value>, String> {
    let result = codex_rate_limits().await?;
    let snapshots = result["rateLimitsByLimitId"]["codex"]
        .as_object()
        .map(|_| &result["rateLimitsByLimitId"]["codex"])
        .unwrap_or(&result["rateLimits"]);
    let mut windows = Vec::new();
    for field in ["primary", "secondary"] {
        let w = &snapshots[field];
        let kind = match w["windowDurationMins"].as_i64() {
            Some(300) => "five_hour",
            Some(10080) => "weekly",
            _ => continue,
        };
        let Some(used) = w["usedPercent"].as_f64() else { continue };
        windows.push(json!({
            "kind": kind,
            "remaining": (100.0 - used).clamp(0.0, 100.0),
            "resetAt": w["resetsAt"].as_i64(),
        }));
    }
    if windows.is_empty() {
        return Err("Codex account did not report 5-hour or weekly plan windows.".into());
    }
    Ok(windows)
}

async fn codex_rate_limits() -> Result<Value, String> {
    let args = ["app-server".to_string(), "--stdio".to_string()];
    let mut command = crate::sh::cmd(&crate::sh::tool("codex"), &args);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|_| "Codex CLI could not start its local usage reader.".to_string())?;
    let mut input = child.stdin.take().ok_or_else(|| "Codex usage reader did not open input.".to_string())?;
    let output = child.stdout.take().ok_or_else(|| "Codex usage reader did not open output.".to_string())?;
    let mut output = BufReader::new(output);

    rpc_send(
        &mut input,
        json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": {"name": "apollo-office", "title": "ApolloOffice", "version": crate::app::V},
                "capabilities": {"experimentalApi": true}
            }
        }),
    )
    .await?;
    rpc_response(&mut output, 1).await?;
    rpc_send(&mut input, json!({"method": "initialized", "params": {}})).await?;
    rpc_send(&mut input, json!({
        "method": "account/rateLimits/read",
        "id": 2,
        "params": {"excludeResetCreditDetails": true}
    })).await?;
    let response = rpc_response(&mut output, 2).await?;
    drop(input);
    let _ = child.kill().await;
    let _ = child.wait().await;
    Ok(response)
}

async fn rpc_send(input: &mut tokio::process::ChildStdin, value: Value) -> Result<(), String> {
    let mut line = serde_json::to_vec(&value).map_err(|_| "Could not prepare the Codex usage request.".to_string())?;
    line.push(b'\n');
    input.write_all(&line).await.map_err(|_| "Could not send a request to Codex CLI.".to_string())
}

async fn rpc_response(output: &mut BufReader<tokio::process::ChildStdout>, id: i64) -> Result<Value, String> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let mut line = String::new();
            if output.read_line(&mut line).await.map_err(|_| "Could not read a response from Codex CLI.".to_string())? == 0 {
                return Err("Codex CLI ended before returning usage data.".to_string());
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
            if value["id"].as_i64() != Some(id) {
                continue;
            }
            if !value["error"].is_null() {
                return Err("Codex CLI could not read account usage. Check that it is signed in.".to_string());
            }
            return Ok(value["result"].clone());
        }
    })
    .await
    .map_err(|_| "Codex CLI usage request timed out.".to_string())?
}
