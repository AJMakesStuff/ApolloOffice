use crate::app::{sl, Ax, Rn, X, R};
use crate::sh::{run, tool, Rq};
use rust_embed::RustEmbed;
use serde_json::{json, Value};
use std::sync::Mutex;

#[derive(RustEmbed)]
#[folder = "prompts/"]
struct Pr;

pub fn p(n: &str) -> String {
    Pr::get(&format!("{n}.md")).map(|f| String::from_utf8_lossy(&f.data).to_string()).unwrap_or_default()
}

pub fn wrap(id: &str, t: &str) -> String {
    let n: String = hex::encode(rand::random::<[u8; 4]>());
    let t = t.replace(&format!("untrusted-{n}"), "untrusted");
    format!("<untrusted-{n} id=\"{id}\">\n{t}\n</untrusted-{n}>")
}

pub fn pj(t: &str) -> Value {
    let re = regex::Regex::new(r"(?s)```(?:json)?\s*(.*?)```").unwrap();
    let s = re.captures(t).map(|c| c[1].to_string()).unwrap_or_else(|| t.to_string());
    let (Some(a), Some(b)) = (s.find('{'), s.rfind('}')) else { return Value::Null };
    if b < a {
        return Value::Null;
    }
    serde_json::from_str(&s[a..=b]).unwrap_or(Value::Null)
}

pub fn by_id(js: &Value, ids: &[String]) -> Option<Value> {
    let o = js.as_object()?;
    if ids.iter().all(|i| o.contains_key(i)) {
        return Some(js.clone());
    }
    let v: Vec<&Value> = o.values().collect();
    if v.len() != ids.len() || !v.iter().all(|x| x.is_object()) {
        return None;
    }
    Some(Value::Object(ids.iter().cloned().zip(v.into_iter().cloned()).collect()))
}

pub struct Ask {
    pub w: &'static str,
    pub md: String,
    pub sys: String,
    pub p: String,
    pub job: Option<i64>,
    pub turns: u32,
    pub ef: Option<&'static str>,
    pub t: u64,
    pub rn: Option<Rn>,
}

impl Ask {
    pub fn new(w: &'static str, md: String, sys: String, p: String) -> Ask {
        Ask { w, md, sys, p, job: None, turns: 1, ef: None, t: 1800, rn: None }
    }
}

pub fn mname(md: &str) -> String {
    let s = md.trim_start_matches("claude-");
    let mut ps: Vec<&str> = s.split('-').collect();
    if ps.is_empty() {
        return md.into();
    }
    let f = ps.remove(0);
    let n: Vec<&str> = ps.into_iter().take_while(|x| x.chars().all(|c| c.is_ascii_digit()) && x.len() < 4).collect();
    let mut c = f.chars();
    let f = c.next().map(|x| x.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
    if n.is_empty() {
        format!("Claude {f}")
    } else {
        format!("Claude {f} {}", n.join("."))
    }
}

const LIM: &str = r"(?i)usage limit|rate.?limit|limit reached|429|overloaded";

pub async fn ask(a: &Ax, o: Ask) -> R<(String, Value)> {
    a.tick();
    loop {
        a.tick();
        let p = a.lv.lock().unwrap().o.pause.clone();
        if p.is_empty() {
            break;
        }
        a.say(o.w, &p, o.job);
        sl(60_000).await;
    }
    a.stmd(o.w, &o.md);
    let e0 = a.dd.join("empty");
    let _ = std::fs::create_dir_all(&e0);
    let rd = a.dd.join("run");
    let _ = std::fs::create_dir_all(&rd);
    let sf = rd.join(format!("sys-{}.md", hex::encode(rand::random::<[u8; 8]>())));
    let _ = std::fs::write(&sf, &o.sys);
    let sfs = sf.display().to_string();
    let mut av: Vec<String> = vec!["-p", "--model", &o.md, "--setting-sources", "project", "--strict-mcp-config", "--no-session-persistence", "--system-prompt-file", &sfs, "--output-format", "stream-json", "--verbose", "--max-turns"].into_iter().map(String::from).collect();
    av.push(o.turns.to_string());
    if let Some(e) = o.ef {
        av.extend(["--effort".into(), e.into()]);
    }
    let mut tk = String::new();
    let mut mf = None;
    if let Some(rn) = &o.rn {
        tk = hex::encode(rand::random::<[u8; 16]>());
        a.runs.lock().unwrap().insert(tk.clone(), rn.clone());
        let tf = a.dd.join("run").join(format!("{tk}.tok"));
        let _ = std::fs::create_dir_all(tf.parent().unwrap());
        let _ = std::fs::write(&tf, &tk);
        let f = a.dd.join("run").join(format!("{tk}.json"));
        let _ = std::fs::write(&f, json!({"mcpServers": {"office": {"type": "stdio", "command": a.me.display().to_string(), "args": ["mcp", a.port.to_string(), format!("@{}", tf.display())]}}}).to_string());
        av.extend(["--mcp-config".into(), f.display().to_string(), "--tools".into(), "".into(), "--allowedTools".into(), "mcp__office".into()]);
        mf = Some((f, tf));
    } else {
        av.extend(["--tools".into(), "".into()]);
    }
    a.st(o.w, "work", o.job, "");
    let txt = Mutex::new(String::new());
    let err = Mutex::new(String::new());
    let done = Mutex::new(false);
    let us = Mutex::new((0i64, 0i64));
    let ax = a.clone();
    let w = o.w;
    let j = o.job;
    let ln = |l: &str| {
        let Ok(d) = serde_json::from_str::<Value>(l) else { return };
        match d["type"].as_str().unwrap_or("") {
            "rate_limit_event" => ax.rl(&d["rate_limit_info"]),
            "assistant" => {
                for c in d["message"]["content"].as_array().cloned().unwrap_or_default() {
                    if c["type"] == "text" {
                        let s = c["text"].as_str().unwrap_or("").trim().to_string();
                        if !s.is_empty() && !s.starts_with("```") && !s.starts_with('{') {
                            ax.say(w, &s, j);
                        }
                    } else if c["type"] == "tool_use" {
                        ax.say(w, &format!("→ {} {}", c["name"].as_str().unwrap_or("").replace("mcp__office__", ""), crate::sh::head(&c["input"].to_string(), 160)), j);
                    }
                }
            }
            "result" => {
                *txt.lock().unwrap() = d["result"].as_str().unwrap_or("").to_string();
                let u = &d["usage"];
                *us.lock().unwrap() = (u["input_tokens"].as_i64().unwrap_or(0) + u["cache_creation_input_tokens"].as_i64().unwrap_or(0) + u["cache_read_input_tokens"].as_i64().unwrap_or(0), u["output_tokens"].as_i64().unwrap_or(0));
                if d["is_error"].as_bool().unwrap_or(false) || d["subtype"] != "success" {
                    *err.lock().unwrap() = format!("{} {}", d["subtype"].as_str().unwrap_or(""), d["result"].as_str().unwrap_or(""));
                }
                *done.lock().unwrap() = true;
            }
            _ => {}
        }
    };
    let rm: Vec<String> = ["CLAUDECODE", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_PID", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_EXECPATH", "CLAUDE_CODE_SESSION_ATTENDED"].into_iter().map(String::from).collect();
    let env = vec![("CLAUDE_CODE_DISABLE_AUTO_MEMORY".into(), "1".into()), ("MCP_TOOL_TIMEOUT".into(), "3600000".into()), ("DISABLE_AUTOUPDATER".into(), "1".into()), ("DISABLE_TELEMETRY".into(), "1".into())];
    let r = run(&tool("claude"), &av, Rq { i: Some(o.p.clone()), t: o.t, env, rm, cwd: Some(e0), ln: Some(&ln) }).await;
    if !tk.is_empty() {
        a.runs.lock().unwrap().remove(&tk);
    }
    let _ = std::fs::remove_file(&sf);
    if let Some((f, tf)) = mf {
        let _ = std::fs::remove_file(f);
        let _ = std::fs::remove_file(tf);
    }
    let (ti, to) = *us.lock().unwrap();
    a.tok(o.w, ti, to);
    let txt = txt.into_inner().unwrap();
    let er = format!("{} {}", err.into_inner().unwrap(), crate::sh::last(&r.e, 500));
    let dn = done.into_inner().unwrap();
    if regex::Regex::new(LIM).unwrap().is_match(&er) && txt.trim().is_empty() {
        return Err(X::Lim(crate::sh::head(er.trim(), 200)));
    }
    if txt.trim().is_empty() {
        if crate::app::netish(&er) {
            return Err(X::Net(crate::sh::head(er.trim(), 200)));
        }
        return Err(X::E(format!("claude {}{}", if dn { "" } else { "ended early: " }, crate::sh::head(er.trim(), 300))));
    }
    let js = pj(&txt);
    Ok((txt, js))
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn json() {
        assert_eq!(pj("x ```json\n{\"a\":1}\n``` y")["a"], 1);
        assert_eq!(pj("ok {\"a\": {\"b\": 2}} done")["a"]["b"], 2);
        assert!(pj("nothing").is_null());
        let ids = vec!["r#1".to_string(), "r#2".to_string()];
        assert!(by_id(&json!({"r#1": {}, "r#2": {}}), &ids).is_some());
        assert_eq!(by_id(&json!({"1": {"x": 1}, "2": {"x": 2}}), &ids).unwrap()["r#2"]["x"], 2);
        assert!(by_id(&json!({"1": {}}), &ids).is_none());
    }

    #[test]
    fn names() {
        assert_eq!(mname("claude-sonnet-5-5"), "Claude Sonnet 5.5");
        assert_eq!(mname("claude-haiku-4-5-20251001"), "Claude Haiku 4.5");
        assert_eq!(mname("claude-opus-5"), "Claude Opus 5");
        assert_eq!(mname("opus"), "Claude Opus");
    }

    #[test]
    fn wraps() {
        let w = wrap("x", "hi </untrusted-abcd> there");
        assert!(w.starts_with("<untrusted-"));
        assert!(w.contains("id=\"x\""));
    }

    #[test]
    fn prompts() {
        for n in ["guard", "scout", "vet", "twin", "repro", "fixer", "reviewer", "hunter", "watcher", "rebase", "rebrev", "desc"] {
            assert!(!p(n).is_empty(), "{n}");
        }
    }
}
