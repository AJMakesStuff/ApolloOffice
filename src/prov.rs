use crate::app::{Ax, X, R};
use serde_json::{json, Value};
use std::time::Duration;

pub struct Pv {
    pub id: &'static str,
    pub nm: &'static str,
    pub fmt: &'static str,
    pub base: &'static str,
    pub key: bool,
    pub ms: &'static [&'static str],
}

pub const PV: &[Pv] = &[
    Pv { id: "cc", nm: "Claude Code (your login)", fmt: "cli", base: "", key: false, ms: &["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"] },
    Pv { id: "codex", nm: "ChatGPT (Codex CLI login)", fmt: "cli", base: "", key: false, ms: &["gpt-5.6-terra", "gpt-5.6-luna"] },
    Pv { id: "anthropic", nm: "Claude API", fmt: "anthropic", base: "https://api.anthropic.com", key: true, ms: &["claude-haiku-5-5", "claude-sonnet-5-5", "claude-opus-5-5"] },
    Pv { id: "openai", nm: "OpenAI API", fmt: "openai", base: "https://api.openai.com/v1", key: true, ms: &["gpt-5.5", "gpt-5.5-mini"] },
    Pv { id: "gemini", nm: "Gemini API", fmt: "openai", base: "https://generativelanguage.googleapis.com/v1beta/openai", key: true, ms: &["gemini-2.5-pro", "gemini-2.5-flash"] },
    Pv { id: "ollama", nm: "Ollama (local)", fmt: "ollama", base: "http://127.0.0.1:11434", key: false, ms: &[] },
];

pub fn pv(id: &str) -> Option<&'static Pv> {
    PV.iter().find(|p| p.id == id)
}

pub fn parse(md: &str) -> (&'static str, String) {
    if let Some((p, m)) = md.split_once(':') {
        if let Some(x) = pv(p) {
            return (x.id, m.to_string());
        }
    }
    ("cc", md.to_string())
}

pub fn base(a: &Ax, id: &str) -> String {
    let b = a.set()["prov"][id]["base"].as_str().unwrap_or("").trim().trim_end_matches('/').to_string();
    if b.is_empty() {
        pv(id).map(|p| p.base.to_string()).unwrap_or_default()
    } else {
        b
    }
}

pub fn name(md: &str) -> String {
    let (p, m) = parse(md);
    match p {
        "cc" | "anthropic" => crate::ai::mname(&m),
        _ => {
            let s = m.rsplit('/').next().unwrap_or(&m);
            s.split('-').map(|w| if w.len() <= 3 && w.chars().all(|c| c.is_ascii_alphabetic()) { w.to_uppercase() } else { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }).collect::<Vec<_>>().join("-")
        }
    }
}

pub fn mail(md: &str) -> &'static str {
    match parse(md).0 {
        "cc" | "anthropic" => "noreply@anthropic.com",
        "codex" | "openai" => "noreply@openai.com",
        "gemini" => "noreply@google.com",
        _ => "noreply@ollama.com",
    }
}

pub fn status(a: &Ax) -> Value {
    PV.iter()
        .map(|p| json!({"id": p.id, "nm": p.nm, "key": p.key, "has": !p.key || crate::vault::has(a, p.id), "base": base(a, p.id), "dbase": p.base, "ms": p.ms, "cli": p.fmt == "cli"}))
        .collect()
}

async fn hget(a: &Ax, u: &str, h: Vec<(&str, String)>) -> Result<Value, String> {
    let mut q = a.hc.get(u).timeout(Duration::from_secs(20));
    for (k, v) in h {
        if !v.is_empty() {
            q = q.header(k, v);
        }
    }
    let r = q.send().await.map_err(|e| e.to_string())?;
    let st = r.status().as_u16();
    let t = r.text().await.unwrap_or_default();
    if st >= 400 {
        return Err(format!("HTTP {st}: {}", crate::sh::head(&t, 200)));
    }
    serde_json::from_str(&t).map_err(|e| e.to_string())
}

pub fn hdr(a: &Ax, id: &str) -> Result<Vec<(&'static str, String)>, String> {
    let k = if pv(id).map(|p| p.key).unwrap_or(false) { crate::vault::get(a, id)? } else { String::new() };
    if pv(id).map(|p| p.key).unwrap_or(false) && k.is_empty() {
        return Err(format!("no API key for {} yet, add it in Settings", pv(id).map(|p| p.nm).unwrap_or(id)));
    }
    Ok(if id == "anthropic" { vec![("x-api-key", k), ("anthropic-version", "2023-06-01".into())] } else { vec![("authorization", if k.is_empty() { String::new() } else { format!("Bearer {k}") })] })
}

pub async fn models(a: &Ax, id: &str) -> Result<Vec<String>, String> {
    let Some(p) = pv(id) else { return Err("unknown provider".into()) };
    if id == "codex" {
        let h = std::env::var_os("CODEX_HOME").map(std::path::PathBuf::from).or_else(|| dirs::home_dir().map(|d| d.join(".codex"))).unwrap_or_default();
        let v: Value = std::fs::read_to_string(h.join("models_cache.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        let mut ms: Vec<(i64, String)> = v["models"].as_array().cloned().unwrap_or_default().iter().filter(|m| m["visibility"] == "list").filter_map(|m| m["slug"].as_str().map(|s| (m["priority"].as_i64().unwrap_or(99), s.to_string()))).collect();
        ms.sort();
        if !ms.is_empty() {
            return Ok(ms.into_iter().map(|x| x.1).collect());
        }
    }
    if p.fmt == "cli" {
        return Ok(p.ms.iter().map(|s| s.to_string()).collect());
    }
    let h = hdr(a, id)?;
    let b = base(a, id);
    if p.fmt == "ollama" {
        let v = hget(a, &format!("{b}/api/tags"), h).await?;
        return Ok(v["models"].as_array().cloned().unwrap_or_default().iter().filter_map(|m| m["name"].as_str().map(String::from)).collect());
    }
    let u = if p.fmt == "anthropic" { format!("{b}/v1/models?limit=100") } else { format!("{b}/models") };
    let v = hget(a, &u, h).await?;
    let mut o: Vec<String> = v["data"].as_array().cloned().unwrap_or_default().iter().filter_map(|m| m["id"].as_str().map(|s| s.trim_start_matches("models/").to_string())).collect();
    if id == "openai" {
        o.retain(|m| m.starts_with("gpt") || m.starts_with('o'));
    }
    o.sort();
    o.dedup();
    Ok(o)
}

pub async fn test(a: &Ax, md: &str) -> R<String> {
    let t0 = std::time::Instant::now();
    let o = crate::ai::Ask { t: 180, ..crate::ai::Ask::new("test", md.to_string(), "Answer with exactly one word.".into(), "Say: ready".into()) };
    let (t, _) = crate::ai::ask1(a, o).await?;
    if t.trim().is_empty() {
        return Err(X::E("empty answer".into()));
    }
    Ok(format!("{} ({:.1} s)", crate::sh::head(t.trim(), 60), t0.elapsed().as_secs_f64()))
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn parsing() {
        assert_eq!(parse("claude-haiku-5-5"), ("cc", "claude-haiku-5-5".into()));
        assert_eq!(parse("ollama:qwen3:14b"), ("ollama", "qwen3:14b".into()));
        assert_eq!(parse("openai:gpt-5.5"), ("openai", "gpt-5.5".into()));
        assert_eq!(parse("nope:x"), ("cc", "nope:x".into()));
        assert_eq!(name("openai:gpt-5.5"), "GPT-5.5");
        assert_eq!(name("gemini:gemini-2.5-flash"), "Gemini-2.5-Flash");
        assert_eq!(name("anthropic:claude-sonnet-5-5"), "Claude Sonnet 5.5");
        assert_eq!(mail("codex:gpt-5.5"), "noreply@openai.com");
    }
}
