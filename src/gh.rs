use crate::app::App;
use crate::db::now;
use crate::pa;
use crate::sh::{apip, ghj};
use serde_json::{json, Value};

pub fn cg(a: &App, k: &str, ttl: i64) -> Option<Value> {
    let c = a.cache.lock().unwrap();
    c.get(k).filter(|(t, _)| now() - t < ttl).map(|(_, v)| v.clone())
}

pub fn cs(a: &App, k: &str, v: &Value) {
    a.cache.lock().unwrap().insert(k.into(), (now(), v.clone()));
}

pub async fn me(a: &App) -> Result<Value, String> {
    if let Some(v) = cg(a, "me", 3_600_000) {
        return Ok(v);
    }
    let v = ghj(&["api", "user"], 60).await?;
    let l = v["login"].as_str().unwrap_or("").to_string();
    if l.is_empty() {
        return Err("gh is not logged in".into());
    }
    let id = v["id"].as_i64().unwrap_or(0);
    let nm = v["name"].as_str().filter(|x| !x.trim().is_empty()).unwrap_or(&l).to_string();
    let o = json!({"login": l, "name": nm, "mail": format!("{id}+{l}@users.noreply.github.com")});
    cs(a, "me", &o);
    Ok(o)
}

pub async fn login(a: &App) -> String {
    me(a).await.ok().and_then(|v| v["login"].as_str().map(String::from)).unwrap_or_default()
}

pub async fn meta(a: &App, r: &str) -> Result<Value, String> {
    let k = format!("meta:{r}");
    if let Some(v) = cg(a, &k, 600_000) {
        return Ok(v);
    }
    let v = ghj(&["api", &format!("repos/{r}")], 60).await?;
    let o = json!({"push": v["permissions"]["push"].as_bool().unwrap_or(false), "main": v["default_branch"].as_str().unwrap_or("main"), "private": v["private"].as_bool().unwrap_or(false), "fork": v["fork"].as_bool().unwrap_or(false), "arch": v["archived"].as_bool().unwrap_or(false), "full": v["full_name"].as_str().unwrap_or(r)});
    cs(a, &k, &o);
    Ok(o)
}

pub struct Rf {
    pub n: i64,
    pub rp: String,
    pub st: String,
    pub by: String,
}

pub async fn refs(r: &str, n: i64) -> Result<Vec<Rf>, String> {
    let mut o = vec![];
    for e in apip(&format!("repos/{r}/issues/{n}/timeline?per_page=100")).await? {
        let i = &e["source"]["issue"];
        if e["event"] == "cross-referenced" && i["pull_request"].is_object() {
            o.push(Rf {
                n: i["number"].as_i64().unwrap_or(0),
                rp: i["repository"]["full_name"].as_str().unwrap_or("").into(),
                st: if i["pull_request"]["merged_at"].is_string() { "merged".into() } else { i["state"].as_str().unwrap_or("").into() },
                by: i["user"]["login"].as_str().unwrap_or("").into(),
            });
        }
    }
    let s = ghj(&["pr", "list", "-R", r, "--state", "all", "--search", &n.to_string(), "--json", "number,state,title,body,author", "--limit", "40"], 60).await?;
    let re = regex::Regex::new(&format!(r"#{n}(\D|$)|issues/{n}(\D|$)")).unwrap();
    for p in s.as_array().cloned().unwrap_or_default() {
        if re.is_match(&format!("{} {}", p["title"].as_str().unwrap_or(""), p["body"].as_str().unwrap_or(""))) {
            o.push(Rf { n: p["number"].as_i64().unwrap_or(0), rp: r.into(), st: p["state"].as_str().unwrap_or("").to_lowercase(), by: p["author"]["login"].as_str().unwrap_or("").into() });
        }
    }
    Ok(o)
}

pub fn live(x: Vec<Rf>) -> Vec<Rf> {
    let mut seen = std::collections::HashSet::new();
    x.into_iter().filter(|p| (p.st == "open" || p.st == "merged") && seen.insert((p.rp.clone(), p.n))).collect()
}

pub async fn twins(a: &App, r: &str, w: &str) -> Result<(Vec<Value>, Vec<Value>), String> {
    let re = regex::Regex::new(r"[\w./-]+\.\w+").unwrap();
    let fs: Vec<String> = re.find_iter(w).map(|m| m.as_str().split(':').next().unwrap_or("").to_string()).collect();
    let k = format!("prs:{r}");
    let l = match cg(a, &k, 900_000) {
        Some(v) => v,
        None => {
            let v = ghj(&["pr", "list", "-R", r, "--state", "open", "--limit", "1000", "--json", "number,title,body,files"], 240).await?;
            cs(a, &k, &v);
            v
        }
    };
    let (mut h, mut o) = (vec![], vec![]);
    for p in l.as_array().cloned().unwrap_or_default() {
        let hit = p["files"].as_array().map(|f| f.iter().any(|x| fs.iter().any(|y| x["path"].as_str() == Some(y.as_str()) || y.ends_with(x["path"].as_str().unwrap_or("\u{0}"))))).unwrap_or(false);
        if hit {
            h.push(p);
        } else {
            o.push(p);
        }
    }
    Ok((h, o))
}

pub async fn files(r: &str, p: &str) -> Vec<Value> {
    ghj(&["api", &format!("repos/{r}/contents/{p}")], 60).await.ok().and_then(|v| v.as_array().cloned()).unwrap_or_default()
}

pub async fn file(r: &str, p: &str) -> Option<String> {
    let v = ghj(&["api", &format!("repos/{r}/contents/{p}")], 60).await.ok()?;
    let b = v["content"].as_str()?.replace(['\n', '\r'], "");
    use base64::Engine;
    String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b).ok()?).ok()
}

pub fn pick_ci(names: &[String], pkg: Option<&str>, mk: Option<&str>, xtask: bool) -> (String, String) {
    let has = |n: &str| names.iter().any(|x| x == n);
    if has("Cargo.toml") {
        if xtask {
            return ("cargo xtask ci".into(), String::new());
        }
        return ("cargo test --workspace".into(), String::new());
    }
    if has("package.json") {
        let p: Value = pkg.and_then(|s| serde_json::from_str(s).ok()).unwrap_or(json!({}));
        let t = p["scripts"]["test"].as_str().unwrap_or("");
        if !t.is_empty() && !t.contains("no test specified") {
            let (m, i) = if has("pnpm-lock.yaml") {
                ("pnpm", "pnpm install --frozen-lockfile")
            } else if has("yarn.lock") {
                ("yarn", "yarn install --frozen-lockfile")
            } else if has("package-lock.json") {
                ("npm", "npm ci")
            } else {
                ("npm", "npm install")
            };
            return (format!("{m} test"), i.into());
        }
    }
    if has("go.mod") {
        return ("go test ./...".into(), String::new());
    }
    if has("pyproject.toml") || has("pytest.ini") || has("setup.py") || has("setup.cfg") || has("tox.ini") {
        let e = if has("pyproject.toml") || has("setup.py") { " -e ." } else { "" };
        let i = if has("requirements-dev.txt") {
            format!("python3 -m venv .venv && . .venv/bin/activate && pip install -q -r requirements-dev.txt{e}")
        } else if has("requirements.txt") {
            format!("python3 -m venv .venv && . .venv/bin/activate && pip install -q pytest -r requirements.txt{e}")
        } else {
            format!("python3 -m venv .venv && . .venv/bin/activate && pip install -q pytest{e}")
        };
        return (". .venv/bin/activate && pytest".into(), i);
    }
    if has("Makefile") && mk.map(|m| m.lines().any(|l| l.starts_with("test:"))).unwrap_or(false) {
        return ("make test".into(), String::new());
    }
    (String::new(), String::new())
}

pub async fn detect(r: &str) -> (String, String) {
    let fs = files(r, "").await;
    let names: Vec<String> = fs.iter().filter_map(|f| f["name"].as_str().map(String::from)).collect();
    let pkg = if names.iter().any(|x| x == "package.json") { file(r, "package.json").await } else { None };
    let mk = if names.iter().any(|x| x == "Makefile") { file(r, "Makefile").await } else { None };
    let xt = names.iter().any(|x| x == "xtask") || names.iter().any(|x| x == ".cargo") && file(r, ".cargo/config.toml").await.map(|c| c.contains("xtask")).unwrap_or(false);
    pick_ci(&names, pkg.as_deref(), mk.as_deref(), xt)
}

pub async fn opened(a: &App, r: &str) -> i64 {
    a.db.n("select count(*) from prs where repo=? and st='open'", pa![r.to_string()]) + a.db.n("select count(*) from jobs where repo=? and st in ('approve') ", pa![r.to_string()])
}

pub async fn today(a: &App, r: &str) -> i64 {
    a.db.n("select count(*) from prs where repo=? and t0>=?", pa![r.to_string(), crate::db::day0()])
}

#[cfg(test)]
mod t {
    use super::*;

    #[test]
    fn lives() {
        let r = |n: i64, st: &str| Rf { n, rp: "o/r".into(), st: st.into(), by: "x".into() };
        let l = live(vec![r(1, "merged"), r(1, "merged"), r(2, "closed"), r(3, "open")]);
        assert_eq!(l.iter().map(|p| p.n).collect::<Vec<_>>(), vec![1, 3]);
    }

    #[test]
    fn ci() {
        let n = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_ci(&n(&["Cargo.toml", "xtask"]), None, None, true).0, "cargo xtask ci");
        assert_eq!(pick_ci(&n(&["Cargo.toml"]), None, None, false).0, "cargo test --workspace");
        assert_eq!(pick_ci(&n(&["package.json", "pnpm-lock.yaml"]), Some(r#"{"scripts":{"test":"vitest run"}}"#), None, false), ("pnpm test".into(), "pnpm install --frozen-lockfile".into()));
        assert_eq!(pick_ci(&n(&["package.json"]), Some(r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#), None, false).0, "");
        assert_eq!(pick_ci(&n(&["go.mod"]), None, None, false).0, "go test ./...");
        assert!(pick_ci(&n(&["pyproject.toml"]), None, None, false).0.ends_with("pytest"));
        assert!(pick_ci(&n(&["pyproject.toml"]), None, None, false).1.ends_with(" -e ."));
        assert!(pick_ci(&n(&["pytest.ini", "requirements.txt"]), None, None, false).1.ends_with("-r requirements.txt"));
        assert_eq!(pick_ci(&n(&["Makefile"]), None, Some("all:\n\ttrue\ntest:\n\t./t.sh\n"), false).0, "make test");
        assert_eq!(pick_ci(&n(&["README.md"]), None, None, false).0, "");
    }
}
