use crate::app::{sl, Ax, Rn, X, R};
use crate::sh::{run, tool, Rq};
use rust_embed::RustEmbed;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::Duration;

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
    ask1(a, o).await
}

pub async fn ask1(a: &Ax, mut o: Ask) -> R<(String, Value)> {
    a.stmd(o.w, &o.md);
    if let Some(r) = o.rn.as_mut() {
        r.w = o.w.to_string();
    }
    match crate::prov::parse(&o.md).0 {
        "cc" => cc(a, o).await,
        "codex" => codex(a, o).await,
        _ => api(a, o).await,
    }
}

fn reg(a: &Ax, rn: &Rn) -> (String, std::path::PathBuf) {
    let tk = hex::encode(rand::random::<[u8; 16]>());
    a.runs.lock().unwrap().insert(tk.clone(), rn.clone());
    let tf = a.dd.join("run").join(format!("{tk}.tok"));
    let _ = std::fs::create_dir_all(tf.parent().unwrap());
    let _ = std::fs::write(&tf, &tk);
    (tk, tf)
}

pub async fn cctok(a: &Ax) -> R<Option<String>> {
    let t = crate::vault::get(a, "cc").map_err(X::E)?;
    if !t.is_empty() {
        return Ok(Some(t));
    }
    let kc = a.set()["prov"]["cc"]["kc"].as_str().unwrap_or("").trim().to_string();
    if kc.is_empty() {
        return Ok(None);
    }
    let r = run("security", &["find-generic-password".into(), "-s".into(), kc.clone(), "-w".into()], Rq { t: 15, ..Default::default() }).await;
    if r.c != 0 || r.o.trim().is_empty() {
        return Err(X::E(format!("Claude Code token not found in keychain item {kc}")));
    }
    Ok(Some(r.o.trim().to_string()))
}

async fn cc(a: &Ax, o: Ask) -> R<(String, Value)> {
    let e0 = a.dd.join("empty");
    let _ = std::fs::create_dir_all(&e0);
    let rd = a.dd.join("run");
    let _ = std::fs::create_dir_all(&rd);
    let sf = rd.join(format!("sys-{}.md", hex::encode(rand::random::<[u8; 8]>())));
    let _ = std::fs::write(&sf, &o.sys);
    let sfs = sf.display().to_string();
    let md = crate::prov::parse(&o.md).1;
    let mut av: Vec<String> = vec!["-p", "--model", &md, "--setting-sources", "project", "--strict-mcp-config", "--no-session-persistence", "--system-prompt-file", &sfs, "--output-format", "stream-json", "--verbose", "--max-turns"].into_iter().map(String::from).collect();
    av.push(o.turns.to_string());
    if let Some(e) = o.ef {
        av.extend(["--effort".into(), e.into()]);
    }
    let mut tk = String::new();
    let mut mf = None;
    if let Some(rn) = &o.rn {
        let (t, tf) = reg(a, rn);
        tk = t;
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
    let mut env = vec![("CLAUDE_CODE_DISABLE_AUTO_MEMORY".into(), "1".into()), ("MCP_TOOL_TIMEOUT".into(), "3600000".into()), ("DISABLE_AUTOUPDATER".into(), "1".into()), ("DISABLE_TELEMETRY".into(), "1".into())];
    match cctok(a).await {
        Ok(Some(t)) => env.push(("CLAUDE_CODE_OAUTH_TOKEN".into(), t)),
        Ok(None) => {}
        Err(e) => {
            if !tk.is_empty() {
                a.runs.lock().unwrap().remove(&tk);
            }
            return Err(e);
        }
    }
    let r = run(&tool("claude"), &av, Rq { i: Some(o.p.clone()), t: o.t, env, rm, cwd: Some(e0), ln: Some(&ln), keep: false }).await;
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

async fn codex(a: &Ax, o: Ask) -> R<(String, Value)> {
    let md = crate::prov::parse(&o.md).1;
    let rd = a.dd.join("run").join(format!("cx-{}", hex::encode(rand::random::<[u8; 8]>())));
    let _ = std::fs::create_dir_all(&rd);
    let mut av: Vec<String> = ["exec", "--json", "--skip-git-repo-check", "--ephemeral", "--ignore-user-config", "--ignore-rules", "-s", "read-only", "--color", "never"].into_iter().map(String::from).collect();
    for f in ["shell_tool", "unified_exec", "browser_use", "browser_use_external", "computer_use", "image_generation", "hooks"] {
        av.extend(["--disable".into(), f.into()]);
    }
    av.extend(["-C".into(), rd.display().to_string()]);
    for c in ["approval_policy=\"never\"", "apps._default.enabled=false", "include_apply_patch_tool=false", "tools.web_search=false"] {
        av.extend(["-c".into(), c.into()]);
    }
    let lf = rd.join("last.txt");
    av.extend(["-o".into(), lf.display().to_string()]);
    let mut tk = String::new();
    if let Some(rn) = &o.rn {
        let (t, tf) = reg(a, rn);
        tk = t;
        av.extend(["-c".into(), format!("mcp_servers.office.command={}", json!(a.me.display().to_string()))]);
        av.extend(["-c".into(), format!("mcp_servers.office.args={}", json!(["mcp", a.port.to_string(), format!("@{}", tf.display())]))]);
        for c in ["mcp_servers.office.tool_timeout_sec=3600", "mcp_servers.office.default_tools_approval_mode=\"approve\""] {
            av.extend(["-c".into(), c.into()]);
        }
    }
    if !md.is_empty() {
        av.extend(["-m".into(), md]);
    }
    if let Some(e) = o.ef {
        av.extend(["-c".into(), format!("model_reasoning_effort=\"{e}\"")]);
    }
    av.push("-".into());
    a.st(o.w, "work", o.job, "");
    let txt = Mutex::new(String::new());
    let err = Mutex::new(String::new());
    let done = Mutex::new(false);
    let us = Mutex::new((0i64, 0i64));
    let ax = a.clone();
    let (w, j) = (o.w, o.job);
    let ln = |l: &str| {
        let Ok(d) = serde_json::from_str::<Value>(l) else { return };
        match d["type"].as_str().unwrap_or("") {
            "item.completed" | "item.started" => {
                let it = &d["item"];
                match it["type"].as_str().unwrap_or("") {
                    "agent_message" if d["type"] == "item.completed" => {
                        let t = it["text"].as_str().unwrap_or("").to_string();
                        if !t.trim().starts_with('{') && !t.trim().starts_with("```") {
                            ax.say(w, &t, j);
                        }
                        *txt.lock().unwrap() = t;
                    }
                    "mcp_tool_call" if d["type"] == "item.started" => ax.say(w, &format!("→ {} {}", it["tool"].as_str().unwrap_or(""), crate::sh::head(&it["arguments"].to_string(), 160)), j),
                    _ => {}
                }
            }
            "turn.completed" => {
                let u = &d["usage"];
                *us.lock().unwrap() = (u["input_tokens"].as_i64().unwrap_or(0), u["output_tokens"].as_i64().unwrap_or(0));
                *done.lock().unwrap() = true;
            }
            "turn.failed" | "error" => {
                let m = d["error"]["message"].as_str().or(d["message"].as_str()).unwrap_or("").to_string();
                err.lock().unwrap().push_str(&format!(" {m}"));
            }
            _ => {}
        }
    };
    let r = run(&tool("codex"), &av, Rq { i: Some(format!("{}\n\n---\n\n{}", o.sys, o.p)), t: o.t, cwd: Some(rd.clone()), ln: Some(&ln), ..Default::default() }).await;
    if !tk.is_empty() {
        a.runs.lock().unwrap().remove(&tk);
    }
    let mut txt = txt.into_inner().unwrap();
    if txt.trim().is_empty() {
        txt = std::fs::read_to_string(&lf).unwrap_or_default();
    }
    let _ = std::fs::remove_dir_all(&rd);
    let (ti, to) = *us.lock().unwrap();
    a.tok(o.w, ti, to);
    let er = format!("{} {}", err.into_inner().unwrap(), crate::sh::last(&r.e, 500));
    if regex::Regex::new(LIM).unwrap().is_match(&er) && txt.trim().is_empty() {
        return Err(X::Lim(crate::sh::head(er.trim(), 200)));
    }
    if txt.trim().is_empty() || !done.into_inner().unwrap() && r.c != 0 {
        if crate::app::netish(&er) {
            return Err(X::Net(crate::sh::head(er.trim(), 200)));
        }
        return Err(X::E(format!("codex returned no result: {}", crate::sh::last(er.trim(), 300))));
    }
    let js = pj(&txt);
    Ok((txt, js))
}

async fn post(a: &Ax, u: &str, h: &[(&'static str, String)], b: &Value, t: Duration) -> R<Value> {
    let mut q = a.hc.post(u).timeout(t).json(b);
    for (k, v) in h {
        if !v.is_empty() {
            q = q.header(*k, v);
        }
    }
    let r = q.send().await.map_err(|e| if e.is_timeout() { X::E(format!("request timed out after {} s", t.as_secs())) } else { X::Net(crate::sh::head(&e.to_string(), 200)) })?;
    let st = r.status().as_u16();
    let tx = r.text().await.unwrap_or_default();
    let m = serde_json::from_str::<Value>(&tx).ok().and_then(|v| v["error"]["message"].as_str().or(v[0]["error"]["message"].as_str()).map(String::from)).unwrap_or_else(|| crate::sh::head(&tx, 300));
    if st == 429 || st == 529 || st == 402 || m.to_lowercase().contains("quota") || m.to_lowercase().contains("credit balance") {
        return Err(X::Lim(format!("{st} {}", crate::sh::head(&m, 200))));
    }
    if st >= 500 {
        return Err(X::Net(format!("HTTP {st}: {}", crate::sh::head(&m, 200))));
    }
    if st >= 400 {
        return Err(X::E(format!("HTTP {st}: {m}")));
    }
    serde_json::from_str(&tx).map_err(|e| X::E(format!("unreadable answer: {e}")))
}

fn unthink(t: &str) -> String {
    regex::Regex::new(r"(?s)<think>.*?</think>").unwrap().replace_all(t, "").trim().to_string()
}

async fn api(a: &Ax, o: Ask) -> R<(String, Value)> {
    let (id, md) = crate::prov::parse(&o.md);
    let fmt = crate::prov::pv(id).map(|p| p.fmt).unwrap_or("openai");
    let h = crate::prov::hdr(a, id).map_err(X::E)?;
    let base = crate::prov::base(a, id);
    if md.is_empty() {
        return Err(X::E(format!("no model set for {}", o.w)));
    }
    let defs = if o.rn.is_some() { crate::mcp::defs() } else { vec![] };
    let mx = if defs.is_empty() { 1 } else { o.turns.max(1) };
    let dl = std::time::Instant::now() + Duration::from_secs(o.t);
    let left = || dl.saturating_duration_since(std::time::Instant::now()).max(Duration::from_secs(5));
    a.st(o.w, "work", o.job, "");
    let (mut ti, mut to) = (0i64, 0i64);
    let say = |t: &str| {
        let s = unthink(t);
        if !s.is_empty() && !s.starts_with("```") && !s.starts_with('{') {
            a.say(o.w, &s, o.job);
        }
    };
    let nt = std::sync::atomic::AtomicUsize::new(0);
    let ns = std::sync::atomic::AtomicUsize::new(0);
    let tool = |n: String, g: Value| {
        nt.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n == "sh" {
            ns.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let ax = a.clone();
        let rn = o.rn.clone();
        a.say(o.w, &format!("→ {n} {}", crate::sh::head(&g.to_string(), 160)), o.job);
        async move {
            match rn {
                Some(r) => crate::mcp::call(&ax, &r, &n, &g).await,
                None => Err("no tools".into()),
            }
        }
    };
    let res: R<String> = async {
        if fmt == "anthropic" {
            let tools: Vec<Value> = defs.iter().map(|(n, d, s)| json!({"name": n, "description": d, "input_schema": s})).collect();
            let mut ms = vec![json!({"role": "user", "content": o.p})];
            for _ in 0..mx {
                if std::time::Instant::now() > dl {
                    return Err(X::E(format!("timeout after {} s", o.t)));
                }
                let mut b = json!({"model": md, "max_tokens": 16000, "system": o.sys, "messages": ms});
                if !tools.is_empty() {
                    b["tools"] = json!(tools);
                }
                let r = post(a, &format!("{base}/v1/messages"), &h, &b, left()).await?;
                let u = &r["usage"];
                ti += u["input_tokens"].as_i64().unwrap_or(0) + u["cache_creation_input_tokens"].as_i64().unwrap_or(0) + u["cache_read_input_tokens"].as_i64().unwrap_or(0);
                to += u["output_tokens"].as_i64().unwrap_or(0);
                let ct = r["content"].as_array().cloned().unwrap_or_default();
                let txt: String = ct.iter().filter(|c| c["type"] == "text").filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n");
                say(&txt);
                if r["stop_reason"] == "max_tokens" {
                    return Err(X::E("answer was cut off (max_tokens)".into()));
                }
                let uses: Vec<Value> = ct.iter().filter(|c| c["type"] == "tool_use").cloned().collect();
                if uses.is_empty() {
                    return Ok(txt);
                }
                ms.push(json!({"role": "assistant", "content": ct}));
                let mut rs = vec![];
                for x in uses {
                    let (t, e) = match tool(x["name"].as_str().unwrap_or("").to_string(), x["input"].clone()).await {
                        Ok(t) => (t, false),
                        Err(e) => (format!("error: {e}"), true),
                    };
                    rs.push(json!({"type": "tool_result", "tool_use_id": x["id"], "content": crate::sh::head(&t, 100_000), "is_error": e}));
                }
                ms.push(json!({"role": "user", "content": rs}));
            }
        } else {
            let ol = fmt == "ollama";
            let tools: Vec<Value> = defs.iter().map(|(n, d, s)| json!({"type": "function", "function": {"name": n, "description": d, "parameters": s}})).collect();
            let mut ms = vec![json!({"role": "system", "content": o.sys}), json!({"role": "user", "content": o.p})];
            for _ in 0..mx {
                if std::time::Instant::now() > dl {
                    return Err(X::E(format!("timeout after {} s", o.t)));
                }
                let mut b = json!({"model": md, "messages": ms});
                if !tools.is_empty() {
                    b["tools"] = json!(tools);
                }
                if ol {
                    b["stream"] = json!(false);
                    b["options"] = json!({"num_ctx": a.set()["prov"]["ollama"]["ctx"].as_i64().unwrap_or(32768)});
                }
                let r = post(a, &if ol { format!("{base}/api/chat") } else { format!("{base}/chat/completions") }, &h, &b, left()).await?;
                ti += r["usage"]["prompt_tokens"].as_i64().or(r["prompt_eval_count"].as_i64()).unwrap_or(0);
                to += r["usage"]["completion_tokens"].as_i64().or(r["eval_count"].as_i64()).unwrap_or(0);
                let m = if ol { r["message"].clone() } else { r["choices"][0]["message"].clone() };
                let txt = m["content"].as_str().unwrap_or("").to_string();
                say(&txt);
                if r["choices"][0]["finish_reason"] == "length" || r["done_reason"] == "length" {
                    return Err(X::E("answer was cut off (length)".into()));
                }
                let calls = m["tool_calls"].as_array().cloned().unwrap_or_default();
                if calls.is_empty() {
                    return Ok(unthink(&txt));
                }
                let mut am = json!({"role": "assistant", "content": txt, "tool_calls": calls});
                if let Some(rc) = m.get("reasoning_content") {
                    am["reasoning_content"] = rc.clone();
                }
                ms.push(am);
                for c in calls {
                    let g: Value = match &c["function"]["arguments"] {
                        Value::String(s) => serde_json::from_str(s).unwrap_or(json!({})),
                        v if v.is_object() => v.clone(),
                        _ => json!({}),
                    };
                    let n = c["function"]["name"].as_str().unwrap_or("").to_string();
                    let t = tool(n.clone(), g).await.unwrap_or_else(|e| format!("error: {e}"));
                    ms.push(if ol { json!({"role": "tool", "tool_name": n, "content": crate::sh::head(&t, 100_000)}) } else { json!({"role": "tool", "tool_call_id": c["id"], "content": crate::sh::head(&t, 100_000)}) });
                }
            }
        }
        Err(X::E(format!("too many steps (limit {mx})")))
    }
    .await;
    a.tok(o.w, ti, to);
    let txt = res?;
    if txt.trim().is_empty() {
        return Err(X::E(format!("{} returned an empty answer", crate::prov::name(&o.md))));
    }
    if !defs.is_empty() && matches!(o.w, "repro" | "hunter") && ns.into_inner() == 0 {
        return Err(X::E(format!("{} reported a result without running any command; pick a stronger model for this worker", crate::prov::name(&o.md))));
    }
    if !defs.is_empty() && nt.into_inner() == 0 {
        return Err(X::E(format!("{} answered without running anything; it may not support tool calls, pick another model for this worker", crate::prov::name(&o.md))));
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

    async fn fake(fmt: &'static str) -> (std::sync::Arc<crate::app::App>, std::sync::Arc<std::sync::Mutex<Vec<Value>>>, std::path::PathBuf) {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
        let sv = seen.clone();
        let h = move |b: axum::Json<Value>| {
            let sv = sv.clone();
            async move {
                let n = { let mut g = sv.lock().unwrap(); g.push(b.0.clone()); g.len() };
                axum::Json(match (fmt, n) {
                    ("anthropic", 1) => json!({"content": [{"type": "text", "text": "looking"}, {"type": "tool_use", "id": "t1", "name": "ls", "input": {"path": "."}}], "stop_reason": "tool_use", "usage": {"input_tokens": 10, "output_tokens": 5}}),
                    ("anthropic", _) => json!({"content": [{"type": "text", "text": "{\"ok\": true}"}], "stop_reason": "end_turn", "usage": {"input_tokens": 20, "output_tokens": 5}}),
                    (_, 1) => json!({"message": {"role": "assistant", "content": "", "tool_calls": [{"function": {"name": "sh", "arguments": {"cmd": "echo hi"}}}]}, "done_reason": "stop", "prompt_eval_count": 7, "eval_count": 3}),
                    _ => json!({"message": {"role": "assistant", "content": "<think>x</think>{\"ok\": true}"}, "done_reason": "stop"}),
                })
            }
        };
        let r = axum::Router::new().route("/v1/messages", axum::routing::post(h.clone())).route("/api/chat", axum::routing::post(h));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
        let dd = std::env::temp_dir().join(format!("ao-t-{}", hex::encode(rand::random::<[u8; 6]>())));
        std::fs::create_dir_all(dd.join("repo")).unwrap();
        std::fs::write(dd.join("repo/marker.txt"), "x").unwrap();
        std::env::set_var("APOLLO_OFFICE_VAULT", "file");
        let db = crate::db::Db::mem();
        db.run("insert into hs(id,ssh,dk,raw,on_) values('local','',0,1,1)", crate::pa![]);
        db.ks("set", &json!({"prov": {"anthropic": {"base": format!("http://127.0.0.1:{port}")}, "ollama": {"base": format!("http://127.0.0.1:{port}")}}}));
        let a = std::sync::Arc::new(crate::app::App::new(db, dd.clone(), 1, "apollo-office".into()));
        crate::vault::put(&a, "anthropic", "sk-test").unwrap();
        (a, seen, dd)
    }

    #[tokio::test]
    async fn api_loops() {
        for (fmt, md) in [("anthropic", "anthropic:claude-sonnet-5-5"), ("ollama", "ollama:qwen3:14b")] {
            let (a, seen, dd) = fake(fmt).await;
            let mut o = Ask::new("fixer", md.into(), "sys".into(), "go".into());
            o.turns = 5;
            o.rn = Some(Rn { h: "local".into(), dir: dd.join("repo").display().to_string(), w: String::new() });
            let (_, js) = ask1(&a, o).await.unwrap();
            assert_eq!(js["ok"], true, "{fmt}");
            let g = seen.lock().unwrap();
            assert_eq!(g.len(), 2, "{fmt}");
            let last = g[1]["messages"].as_array().unwrap().last().unwrap().clone();
            if fmt == "anthropic" {
                assert_eq!(g[0]["system"], "sys");
                assert!(last["content"][0]["content"].as_str().unwrap().contains("marker.txt"), "{last}");
            } else {
                assert_eq!(g[0]["options"]["num_ctx"], 32768);
                assert_eq!(last["tool_name"], "sh");
                assert!(last["content"].as_str().unwrap().contains("hi"), "{last}");
            }
            let _ = std::fs::remove_dir_all(dd);
        }
    }

    #[test]
    fn prompts() {
        for n in ["guard", "scout", "vet", "twin", "repro", "fixer", "reviewer", "hunter", "watcher", "rebase", "rebrev", "desc"] {
            assert!(!p(n).is_empty(), "{n}");
        }
    }
}
