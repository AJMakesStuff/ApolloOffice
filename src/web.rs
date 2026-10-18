use crate::app::{Ax, WS};
use crate::db::now;
use crate::pa;
use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::{HeaderMap, Method, Request, Response, StatusCode};
use rust_embed::RustEmbed;
use serde_json::{json, Value};

#[derive(RustEmbed)]
#[folder = "ui/"]
struct Ui;

fn rs(c: StatusCode, ct: &str, b: impl Into<Body>) -> Response<Body> {
    Response::builder()
        .status(c)
        .header("content-type", ct)
        .header("cache-control", "no-store")
        .header("x-content-type-options", "nosniff")
        .header("x-frame-options", "DENY")
        .header("referrer-policy", "same-origin")
        .header("content-security-policy", "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; form-action 'self'; base-uri 'none'")
        .body(b.into())
        .unwrap()
}

fn js(v: Value) -> Response<Body> {
    rs(StatusCode::OK, "application/json", v.to_string())
}

fn er(c: StatusCode, m: &str) -> Response<Body> {
    rs(c, "application/json", json!({"error": m}).to_string())
}

fn ck(h: &HeaderMap, n: &str) -> Option<String> {
    h.get_all("cookie").iter().filter_map(|v| v.to_str().ok()).flat_map(|s| s.split(';')).map(|s| s.trim()).find_map(|s| s.strip_prefix(&format!("{n}=")).map(String::from))
}

fn host_ok(a: &Ax, h: &HeaderMap) -> bool {
    let hs = h.get("host").and_then(|v| v.to_str().ok()).unwrap_or("");
    hs == format!("127.0.0.1:{}", a.port) || hs == format!("localhost:{}", a.port) || hs == format!("[::1]:{}", a.port)
}

fn origin_ok(a: &Ax, h: &HeaderMap) -> bool {
    match h.get("origin").and_then(|v| v.to_str().ok()) {
        None => h.get("sec-fetch-site").and_then(|v| v.to_str().ok()).map(|s| s == "same-origin" || s == "none").unwrap_or(true),
        Some(o) => o == format!("http://127.0.0.1:{}", a.port) || o == format!("http://localhost:{}", a.port) || o == format!("http://[::1]:{}", a.port),
    }
}

fn authed(a: &Ax, h: &HeaderMap) -> bool {
    ck(h, "ao").map(|s| a.db.n("select count(*) from ses where id=? and exp>?", pa![s, now()]) > 0).unwrap_or(false)
}

pub fn link_new(a: &crate::db::Db) -> String {
    let c = hex::encode(rand::random::<[u8; 16]>());
    a.run("delete from lnk where exp<?", pa![now()]);
    a.run("insert into lnk(c,exp) values(?,?)", pa![c.clone(), now() + 600_000]);
    c
}

const LP: &str = "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>ApolloOffice</title><style>body{margin:0;min-height:100vh;display:grid;place-items:center;background:#0b0c0e;color:#ecebe6;font:14px/1.5 ui-monospace,Menlo,Consolas,monospace}div{max-width:420px;padding:24px}button{font:inherit;color:#0b0c0e;background:#ecebe6;border:0;border-radius:6px;padding:8px 16px;cursor:pointer}code{color:#4fb3e0}</style><div>";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn page(b: &str) -> Response<Body> {
    rs(StatusCode::OK, "text/html; charset=utf-8", format!("{LP}{b}</div>"))
}

fn state(a: &Ax) -> Value {
    let d = crate::db::day();
    let mut tk = json!({});
    for t in a.db.all("select w, tin+tout n from tok where day=?", pa![d]) {
        tk[t["w"].as_str().unwrap_or("")] = t["n"].clone();
    }
    let (w, o, fl, ln, cil) = {
        let l = a.lv.lock().unwrap();
        (json!(l.w), json!(l.o), json!(l.fl.iter().filter(|f| now() - f.t < 8000).collect::<Vec<_>>()), json!(l.ln.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<std::collections::HashMap<_, _>>()), json!(l.cil.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<std::collections::HashMap<_, _>>()))
    };
    let mut wv = w;
    for x in WS {
        wv[*x]["tk"] = tk[*x].clone();
        if wv[*x]["md"].as_str().map(|s| s.is_empty()).unwrap_or(true) && *x != "ci" && *x != "submit" {
            wv[*x]["md"] = json!(a.md(x));
        }
    }
    let lm = a.db.kg("lm");
    json!({
        "t": now(), "v": crate::app::V,
        "o": o, "w": wv, "fl": fl, "ln": ln, "cil": cil,
        "hs": a.db.all("select id, ln, dk, on_ from hs order by id='local' desc, id", pa![]),
        "q": a.db.all("select id,repo,num,kind,title,url,st,why,pr,t0,t1,json_extract(d,'$.h') h from jobs where st not in ('done','drop') order by st='approve' desc, st='vet', id limit 60", pa![]),
        "dn": a.db.all("select id,repo,num,kind,title,url,st,why,pr,t1 from jobs where st in ('done','drop') order by t1 desc limit 25", pa![]),
        "pr": a.db.all("select repo,num,st,title,url,upd,merged from prs order by t0 desc limit 40", pa![]),
        "mg": a.db.all("select substr(merged,1,10) d, count(*) n from prs where merged is not null and merged >= date('now','-30 day') group by 1 order by 1", pa![]),
        "ev": a.db.all("select t,w,m,j from ev order by id desc limit 60", pa![]),
        "n": Value::Object(a.db.all("select st, count(*) n from jobs group by st", pa![]).into_iter().map(|x| (x["st"].as_str().unwrap_or("").to_string(), x["n"].clone())).collect()),
        "lm": if now() - lm["t"].as_i64().unwrap_or(0) < 180_000 { lm } else { Value::Null },
        "nr": a.db.n("select count(*) from rp", pa![]),
        "star": crate::star::show(a),
    })
}

async fn checks(a: &Ax) -> Value {
    use crate::sh::{run, tool, Rq};
    let v = |p: &str, x: &[&str]| {
        let p = p.to_string();
        let x: Vec<String> = x.iter().map(|s| s.to_string()).collect();
        async move { run(&p, &x, Rq { t: 30, ..Default::default() }).await }
    };
    let (g, h, c, d, b) = tokio::join!(v(&tool("git"), &["--version"]), v(&tool("gh"), &["api", "user", "--jq", ".login"]), v(&tool("claude"), &["--version"]), v(&tool("docker"), &["info", "--format", "{{.ServerVersion}}"]), v(&crate::sh::bash(), &["-c", "echo ok"]));
    let _ = a;
    json!({
        "git": {"ok": g.c == 0, "m": if g.c == 0 { g.o.trim().to_string() } else { "Install git: https://git-scm.com".into() }},
        "gh": {"ok": h.c == 0, "m": if h.c == 0 { format!("logged in as {}", h.o.trim()) } else { "Install the GitHub CLI and run: gh auth login".into() }},
        "claude": {"ok": c.c == 0, "m": if c.c == 0 { c.o.trim().to_string() } else { "Install Claude Code and log in: https://claude.com/claude-code".into() }},
        "docker": {"ok": d.c == 0, "m": if d.c == 0 { format!("Docker {}", d.o.trim()) } else { "Docker is not running (optional, recommended)".into() }},
        "bash": {"ok": b.c == 0, "m": if b.c == 0 { "bash ok".to_string() } else { "bash not found (on Windows it comes with Git for Windows)".to_string() }},
    })
}

fn cfg(a: &Ax) -> Value {
    json!({"repos": a.db.all("select * from rp order by full", pa![]), "hosts": a.db.all("select * from hs order by id='local' desc, id", pa![]), "set": a.set(), "data": a.dd.display().to_string(), "auto": crate::sys::autostart_on(), "prov": crate::prov::status(a), "google": crate::goog::linked(a)["email"], "upd": a.db.kg("upd"), "asset": crate::upd::asset()})
}

fn rid(s: &str) -> Option<String> {
    let s = s.trim().trim_start_matches("https://github.com/").trim_end_matches('/').trim_end_matches(".git");
    let re = regex::Regex::new(r"^[A-Za-z0-9-]+/[A-Za-z0-9._-]+$").unwrap();
    if re.is_match(s) {
        Some(s.to_string())
    } else {
        None
    }
}

async fn api(a: &Ax, m: &Method, p: &str, q: &str, b: Value) -> Response<Body> {
    let qv = |k: &str| q.split('&').find_map(|x| x.strip_prefix(&format!("{k}="))).unwrap_or("").to_string();
    match (m.as_str(), p) {
        ("GET", "/api/state") => js(state(a)),
        ("GET", "/api/cfg") => js(cfg(a)),
        ("GET", "/api/check") => js(checks(a).await),
        ("GET", "/api/job") => {
            let id: i64 = qv("id").parse().unwrap_or(0);
            match a.db.one("select * from jobs where id=?", pa![id]) {
                Some(j) => js(json!({"id": j["id"], "repo": j["repo"], "num": j["num"], "kind": j["kind"], "title": j["title"], "url": j["url"], "st": j["st"], "why": j["why"], "pt": j["d"]["pt"], "pb": j["d"]["pb"], "df": j["d"]["df"], "proof": j["d"]["proof"], "known": j["d"]["known"], "h": j["d"]["h"], "ev": a.db.all("select t,w,m from ev where j=? order by id desc limit 80", pa![id])})),
                None => er(StatusCode::NOT_FOUND, "no such job"),
            }
        }
        ("POST", "/api/act") => {
            let id = b["id"].as_i64().unwrap_or(0);
            match b["a"].as_str().unwrap_or("") {
                "pause" => {
                    let on = b["on"].as_bool().unwrap_or(false);
                    a.lv.lock().unwrap().o.man = on;
                    a.db.ks("man", &json!(on));
                    a.say("scout", if on { "office paused" } else { "office resumed" }, None);
                }
                "hold" => {
                    let w = b["w"].as_str().unwrap_or("").to_string();
                    let on = b["on"].as_bool().unwrap_or(false);
                    if let Some(x) = a.lv.lock().unwrap().w.get_mut(&w) {
                        x.hold = on;
                    }
                    let mut h = a.db.kg("hold");
                    if !h.is_object() {
                        h = json!({});
                    }
                    h[&w] = json!(on);
                    a.db.ks("hold", &h);
                }
                "drop" => {
                    if let Some(j) = a.db.jg(id) {
                        if ["ready", "vet", "approve"].contains(&j.st.as_str()) {
                            a.db.jset(id, &[("st", json!("drop")), ("why", json!("dropped by the owner"))]);
                        } else if !["done", "drop"].contains(&j.st.as_str()) {
                            a.db.ks(&format!("kill:{id}"), &json!(true));
                        }
                    }
                }
                "approve" => {
                    if let Some(mut j) = a.db.jg(id) {
                        if j.st != "approve" {
                            return er(StatusCode::CONFLICT, "job is not waiting for approval");
                        }
                        if let Some(t) = b["pt"].as_str().filter(|s| !s.trim().is_empty()) {
                            j.d["pt"] = json!(t.trim());
                        }
                        if let Some(t) = b["pb"].as_str().filter(|s| !s.trim().is_empty()) {
                            j.d["pb"] = json!(t.trim());
                        }
                        j.d["ap"] = json!(true);
                        a.db.jset(id, &[("st", json!("submit")), ("why", json!("approved, submitting")), ("d", j.d)]);
                    }
                }
                "requeue" => {
                    if let Some(j) = a.db.jg(id) {
                        if j.st == "drop" && j.kind == "fix" {
                            let mut d = j.d.clone();
                            for k in ["h", "rs", "rp", "br", "f", "f0", "sq", "known", "vt"] {
                                d.as_object_mut().map(|o| o.remove(k));
                            }
                            a.db.kd(&format!("kill:{id}"));
                            a.db.kd(&format!("cired:{}", j.repo));
                            a.db.jset(id, &[("st", json!(if j.why.starts_with("Guard") || j.why.starts_with("Vet") { "vet" } else { "ready" })), ("why", json!("requeued by the owner")), ("d", d)]);
                        }
                    }
                }
                _ => return er(StatusCode::BAD_REQUEST, "unknown action"),
            }
            js(state(a))
        }
        ("POST", "/api/repo") => {
            let op = b["op"].as_str().unwrap_or("");
            let Some(r) = rid(b["full"].as_str().unwrap_or("")) else { return er(StatusCode::BAD_REQUEST, "use owner/name") };
            match op {
                "add" => {
                    let mt = match crate::gh::meta(a, &r).await {
                        Ok(v) => v,
                        Err(e) => return er(StatusCode::BAD_REQUEST, &format!("cannot read {r}: {e}")),
                    };
                    if mt["arch"].as_bool().unwrap_or(false) {
                        return er(StatusCode::BAD_REQUEST, "repository is archived");
                    }
                    let r = mt["full"].as_str().unwrap_or(&r).to_string();
                    let (c, s) = crate::gh::detect(&r).await;
                    a.db.run("insert or ignore into rp(full,ci,setup,mode,t0) values(?,?,?,?,?)", pa![r.clone(), c.clone(), s.clone(), "ask".to_string(), now()]);
                    a.sw.notify_one();
                    js(json!({"full": r, "ci": c, "setup": s, "cfg": cfg(a)}))
                }
                "set" => {
                    for (k, kind) in [("iss", 0), ("hunt", 0), ("nm", 0), ("on_", 0), ("maxo", 1), ("maxd", 1), ("ci", 2), ("setup", 2), ("rules", 2), ("mode", 2)] {
                        let v = &b[k];
                        if v.is_null() {
                            continue;
                        }
                        match kind {
                            0 => {
                                let x = v.as_bool().map(|x| x as i64).or(v.as_i64()).unwrap_or(0).clamp(0, 1);
                                a.db.run(&format!("update rp set {k}=? where full=?"), pa![x, r.clone()]);
                            }
                            1 => {
                                let x = v.as_i64().unwrap_or(3).clamp(0, 10000);
                                a.db.run(&format!("update rp set {k}=? where full=?"), pa![x, r.clone()]);
                            }
                            _ => {
                                let mut s = v.as_str().unwrap_or("").trim().to_string();
                                if k == "mode" && s != "auto" {
                                    s = "ask".into();
                                }
                                s = crate::sh::head(&s, 4000);
                                a.db.run(&format!("update rp set {k}=? where full=?"), pa![s, r.clone()]);
                            }
                        }
                    }
                    a.db.kd(&format!("cired:{r}"));
                    a.sw.notify_one();
                    js(cfg(a))
                }
                "detect" => {
                    let (c, s) = crate::gh::detect(&r).await;
                    js(json!({"ci": c, "setup": s}))
                }
                "del" => {
                    a.db.run("delete from rp where full=?", pa![r.clone()]);
                    a.db.run("update jobs set st='drop', why='repository removed' where repo=? and st in ('vet','ready','approve')", pa![r.clone()]);
                    js(cfg(a))
                }
                _ => er(StatusCode::BAD_REQUEST, "unknown op"),
            }
        }
        ("POST", "/api/host") => {
            let op = b["op"].as_str().unwrap_or("");
            let id = b["id"].as_str().unwrap_or("").trim().to_string();
            if !regex::Regex::new(r"^[A-Za-z0-9._-]{1,40}$").unwrap().is_match(&id) {
                return er(StatusCode::BAD_REQUEST, "name: letters, digits, . _ - only");
            }
            match op {
                "add" | "set" => {
                    let ssh = if id == "local" { String::new() } else { b["ssh"].as_str().unwrap_or(&id).trim().to_string() };
                    if !ssh.is_empty() && !regex::Regex::new(r"^[A-Za-z0-9._@-]{1,100}$").unwrap().is_match(&ssh) {
                        return er(StatusCode::BAD_REQUEST, "ssh: use a host alias from ~/.ssh/config or user@host");
                    }
                    let dk = b["dk"].as_bool().unwrap_or(false);
                    if !dk && !b["raw"].as_bool().unwrap_or(false) {
                        return er(StatusCode::BAD_REQUEST, "without Docker the workers run repository code directly on this machine; confirm that first");
                    }
                    let no = crate::sh::head(b["no"].as_str().unwrap_or("").trim(), 2000);
                    a.db.run("insert into hs(id,ssh,dk,ln,ci,gb,raw,on_,t0,no,cpu) values(?,?,?,?,?,?,?,?,?,?,?) on conflict(id) do update set ssh=excluded.ssh, dk=excluded.dk, ln=excluded.ln, ci=excluded.ci, gb=excluded.gb, raw=excluded.raw, on_=excluded.on_, no=excluded.no, cpu=excluded.cpu", pa![id.clone(), ssh, dk as i64, b["ln"].as_i64().unwrap_or(1).clamp(1, 8), b["ci"].as_i64().unwrap_or(1).clamp(1, 8), b["gb"].as_i64().unwrap_or(50).clamp(5, 10000), (!dk) as i64, b["on"].as_bool().unwrap_or(true) as i64, now(), no, b["cpu"].as_i64().unwrap_or(0).clamp(0, 256)]);
                    a.db.kd(&format!("up:{id}"));
                    match crate::sh::ensure(a, &id).await {
                        Ok(()) => {
                            if dk {
                                a.db.ks(&format!("up:{id}"), &json!(true));
                            }
                            a.lv.lock().unwrap().o.net.insert(id.clone(), true);
                            js(json!({"ok": true, "cfg": cfg(a)}))
                        }
                        Err(m) => js(json!({"ok": false, "m": m, "cfg": cfg(a)})),
                    }
                }
                "del" => {
                    a.db.run("delete from hs where id=?", pa![id.clone()]);
                    js(cfg(a))
                }
                _ => er(StatusCode::BAD_REQUEST, "unknown op"),
            }
        }
        ("POST", "/api/set") => {
            let mut s = a.db.kg("set");
            if !s.is_object() {
                s = json!({});
            }
            if let Some(m) = b["md"].as_object() {
                for (k, v) in m {
                    if let Some(x) = v.as_str() {
                        if regex::Regex::new(r"^[A-Za-z0-9._:/\[\]-]{0,120}$").unwrap().is_match(x.trim()) {
                            s["md"][k] = json!(x.trim());
                        }
                    }
                }
            }
            if let Some(x) = b["big"].as_bool() {
                s["big"] = json!(x);
            }
            if let Some(x) = b["hunters"].as_i64() {
                s["hunters"] = json!(x.clamp(0, 12));
            }
            if let Some(x) = b["updates"].as_bool() {
                s["updates"] = json!(x);
            }
            if let Some(x) = b["cckc"].as_str() {
                if regex::Regex::new(r"^[A-Za-z0-9._ -]{0,100}$").unwrap().is_match(x.trim()) {
                    s["prov"]["cc"]["kc"] = json!(x.trim());
                }
            }
            if let Some(x) = b["octx"].as_i64() {
                s["prov"]["ollama"]["ctx"] = json!(x.clamp(2048, 1_048_576));
            }
            if let Some(x) = b["ollama"].as_str() {
                let x = x.trim();
                if x.is_empty() || x.starts_with("http://") || x.starts_with("https://") {
                    s["prov"]["ollama"]["base"] = json!(x);
                }
            }
            if let Some(x) = b["tokday"].as_i64() {
                s["tokday"] = json!(x.max(0));
            }
            for k in ["ntfy", "discord"] {
                if let Some(x) = b[k].as_str() {
                    let x = x.trim();
                    if x.is_empty() || x.starts_with("https://") || k == "ntfy" && x.starts_with("http://") {
                        s[k] = json!(x);
                    }
                }
            }
            a.db.ks("set", &s);
            if b["test"].as_bool().unwrap_or(false) {
                crate::ntf::send(a, "ApolloOffice", "Test notification", 3).await;
            }
            if let Some(x) = b["auto"].as_bool() {
                let _ = crate::sys::autostart(x, &a.me, &a.dd, a.port);
            }
            js(cfg(a))
        }
        ("POST", "/api/prov") => {
            let id = b["id"].as_str().unwrap_or("");
            let Some(p) = crate::prov::pv(id) else { return er(StatusCode::BAD_REQUEST, "unknown provider") };
            if let Some(k) = b["key"].as_str() {
                if !p.key && id != "cc" {
                    return er(StatusCode::BAD_REQUEST, "this provider has no API key");
                }
                if let Err(e) = crate::vault::put(a, id, k.trim()) {
                    return er(StatusCode::INTERNAL_SERVER_ERROR, &e);
                }
            }
            js(cfg(a))
        }
        ("GET", "/api/models") => match crate::prov::models(a, &qv("p")).await {
            Ok(v) => js(json!({"ms": v})),
            Err(e) => js(json!({"ms": [], "error": e})),
        },
        ("POST", "/api/test") => {
            let md = b["md"].as_str().unwrap_or("").to_string();
            match crate::prov::test(a, &md).await {
                Ok(t) => js(json!({"ok": true, "m": t})),
                Err(e) => js(json!({"ok": false, "m": crate::wk::errm(&e)})),
            }
        }
        ("POST", "/api/google") => match b["op"].as_str().unwrap_or("") {
            "link" => js(json!({"url": crate::goog::start(a, "link")})),
            "unlink" => {
                a.db.kd("google");
                js(cfg(a))
            }
            _ => er(StatusCode::BAD_REQUEST, "unknown op"),
        },
        ("POST", "/api/star") => match b["op"].as_str().unwrap_or("") {
            "star" => match crate::star::star(a).await {
                Ok(()) => js(state(a)),
                Err(e) => er(StatusCode::BAD_GATEWAY, &e),
            },
            "later" => {
                crate::star::later(a);
                js(state(a))
            }
            _ => er(StatusCode::BAD_REQUEST, "unknown op"),
        },
        ("POST", "/api/update") => match b["op"].as_str().unwrap_or("") {
            "check" => match crate::upd::check(a).await {
                Ok(v) => js(json!({"ok": true, "r": v, "cfg": cfg(a)})),
                Err(e) => js(json!({"ok": false, "m": e, "cfg": cfg(a)})),
            },
            "apply" => {
                if !crate::upd::ready(a) {
                    return er(StatusCode::CONFLICT, "no update downloaded");
                }
                crate::upd::apply(a);
                js(json!({"ok": true}))
            }
            _ => er(StatusCode::BAD_REQUEST, "unknown op"),
        },
        _ => er(StatusCode::NOT_FOUND, "not found"),
    }
}

pub async fn h(State(a): State<Ax>, req: Request<Body>) -> Response<Body> {
    let (pt, b) = req.into_parts();
    let m = pt.method.clone();
    let p = pt.uri.path().to_string();
    let q = pt.uri.query().unwrap_or("").to_string();
    let hd = pt.headers;
    if p == "/int/mcp" && m == Method::POST {
        let t = hd.get("x-run").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        let Ok(by) = to_bytes(b, 16 << 20).await else { return er(StatusCode::BAD_REQUEST, "body") };
        let v: Value = serde_json::from_slice(&by).unwrap_or(Value::Null);
        return js(crate::mcp::handle(&a, &t, &v).await);
    }
    if !host_ok(&a, &hd) {
        return er(StatusCode::MISDIRECTED_REQUEST, "use http://127.0.0.1");
    }
    if p == "/auth/google" && m == Method::GET {
        if crate::goog::linked(&a)["sub"].is_null() {
            return page("No Google account is connected yet. Log in with <code>apollo-office login-link</code>, then connect Google in Settings.");
        }
        return Response::builder().status(StatusCode::SEE_OTHER).header("location", crate::goog::start(&a, "login")).header("cache-control", "no-store").body(Body::empty()).unwrap();
    }
    if p == "/auth/google/cb" && m == Method::GET {
        let qv = |k: &str| q.split('&').find_map(|x| x.strip_prefix(&format!("{k}="))).map(|v| v.replace("%2F", "/").replace("%2f", "/")).unwrap_or_default();
        if !qv("error").is_empty() {
            return page("Google sign-in was cancelled. <a href='/'>Back</a>");
        }
        return match crate::goog::back(&a, &qv("code"), &qv("state")).await {
            Ok((md, sub, em)) if md == "link" => {
                crate::goog::link(&a, &sub, &em);
                a.say("scout", &format!("Google account {em} connected for sign-in"), None);
                rs(StatusCode::OK, "text/html; charset=utf-8", format!("{LP}<meta http-equiv=refresh content='1;url=/'><p>Connected <b>{}</b>. You can now sign in with this Google account.</p></div>", esc(&em)))
            }
            Ok((_, sub, em)) => {
                if !crate::goog::ok(&a, &sub) {
                    return page(&format!("<b>{}</b> is not the Google account connected to this ApolloOffice. <a href='/'>Back</a>", esc(&em)));
                }
                let s = hex::encode(rand::random::<[u8; 32]>());
                a.db.run("delete from ses where exp<?", pa![now()]);
                a.db.run("insert into ses(id,exp) values(?,?)", pa![s.clone(), now() + 30 * 86_400_000]);
                Response::builder().status(StatusCode::OK).header("content-type", "text/html; charset=utf-8").header("cache-control", "no-store").header("set-cookie", format!("ao={s}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}", 30 * 86400)).body(Body::from(format!("{LP}<meta http-equiv=refresh content='0;url=/'><p>Signed in as {}.</p></div>", esc(&em)))).unwrap()
            }
            Err(e) => page(&format!("{} <a href='/'>Back</a>", esc(&e))),
        };
    }
    if p == "/auth/link" {
        let c = q.split('&').find_map(|x| x.strip_prefix("c=")).unwrap_or("").to_string();
        if !c.chars().all(|x| x.is_ascii_hexdigit()) || c.len() != 32 {
            return page("This login link is not valid. Run <code>apollo-office login-link</code> for a new one.");
        }
        if m == Method::GET {
            return page(&format!("<p>Log in to ApolloOffice on this computer?</p><form method=post action='/auth/link?c={c}'><button>Log in</button></form>"));
        }
        if m == Method::POST && origin_ok(&a, &hd) {
            if a.db.ch("delete from lnk where c=? and exp>?", pa![c, now()]) == 1 {
                let s = hex::encode(rand::random::<[u8; 32]>());
                a.db.run("delete from ses where exp<?", pa![now()]);
                a.db.run("insert into ses(id,exp) values(?,?)", pa![s.clone(), now() + 30 * 86_400_000]);
                return Response::builder().status(StatusCode::SEE_OTHER).header("location", "/").header("set-cookie", format!("ao={s}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}", 30 * 86400)).body(Body::empty()).unwrap();
            }
            return page("This login link was already used or has expired. Run <code>apollo-office login-link</code> for a new one.");
        }
        return er(StatusCode::FORBIDDEN, "forbidden");
    }
    if p.starts_with("/api/") {
        if !authed(&a, &hd) {
            return er(StatusCode::UNAUTHORIZED, "login");
        }
        if m != Method::GET && (!origin_ok(&a, &hd) || hd.get("x-ao").is_none()) {
            return er(StatusCode::FORBIDDEN, "forbidden");
        }
        let b: Value = if m == Method::POST {
            match to_bytes(b, 1 << 20).await {
                Ok(by) => serde_json::from_slice(&by).unwrap_or(json!({})),
                Err(_) => return er(StatusCode::BAD_REQUEST, "body"),
            }
        } else {
            Value::Null
        };
        return api(&a, &m, &p, &q, b).await;
    }
    if p == "/api/wait" {
        return er(StatusCode::NOT_FOUND, "");
    }
    let f = if p == "/" { "index.html".to_string() } else { p.trim_start_matches('/').to_string() };
    if f == "index.html" && !authed(&a, &hd) {
        let g = if crate::goog::linked(&a)["sub"].is_string() { "<p><a href='/auth/google'><button>Sign in with Google</button></a></p><p>or, in a terminal, run</p>" } else { "<p>Not logged in. In a terminal, run</p>" };
        return page(&format!("<p><b>ApolloOffice</b></p>{g}<p><code>apollo-office login-link</code></p><p>and open the link it prints.</p>"));
    }
    match Ui::get(&f) {
        Some(x) => rs(StatusCode::OK, mime_guess::from_path(&f).first_or_octet_stream().as_ref(), x.data.into_owned()),
        None => er(StatusCode::NOT_FOUND, "not found"),
    }
}
