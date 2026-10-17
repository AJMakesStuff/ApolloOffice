use crate::app::{App, Ax};
use crate::pa;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, Default)]
pub struct Out {
    pub c: i32,
    pub o: String,
    pub e: String,
}

impl Out {
    pub fn all(&self) -> String {
        format!("{}{}", self.o, self.e)
    }
}

const MAXO: usize = 400_000;

pub fn which(n: &str) -> Option<PathBuf> {
    let ex: Vec<String> = if cfg!(windows) { vec![".exe".into(), ".cmd".into(), ".bat".into(), "".into()] } else { vec!["".into()] };
    let mut ps: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(h) = dirs::home_dir() {
        ps.push(h.join(".local/bin"));
        ps.push(h.join(".claude/local"));
    }
    if !cfg!(windows) {
        ps.push("/opt/homebrew/bin".into());
        ps.push("/usr/local/bin".into());
    }
    for d in ps {
        for e in &ex {
            let f = d.join(format!("{n}{e}"));
            if f.is_file() {
                return Some(f);
            }
        }
    }
    None
}

pub fn tool(n: &str) -> String {
    let k = format!("APOLLO_OFFICE_{}", n.to_uppercase());
    if let Ok(v) = std::env::var(&k) {
        if !v.is_empty() {
            return v;
        }
    }
    which(n).map(|p| p.display().to_string()).unwrap_or_else(|| n.to_string())
}

pub fn bash() -> String {
    if let Ok(v) = std::env::var("APOLLO_OFFICE_BASH") {
        return v;
    }
    if cfg!(windows) {
        if let Some(g) = which("git") {
            for up in [g.parent().and_then(|p| p.parent()), g.parent().and_then(|p| p.parent()).and_then(|p| p.parent())] {
                if let Some(r) = up {
                    let b = r.join("bin").join("bash.exe");
                    if b.is_file() {
                        return b.display().to_string();
                    }
                }
            }
        }
        for c in [r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files (x86)\Git\bin\bash.exe"] {
            if Path::new(c).is_file() {
                return c.into();
            }
        }
    }
    "bash".into()
}

pub fn cmd(p: &str, a: &[String]) -> tokio::process::Command {
    let lw = p.to_lowercase();
    let mut c = if cfg!(windows) && (lw.ends_with(".cmd") || lw.ends_with(".bat")) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(p);
        c
    } else {
        tokio::process::Command::new(p)
    };
    c.args(a);
    #[cfg(windows)]
    {
        c.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    {
        c.process_group(0);
    }
    c
}

pub fn kill(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
        libc::kill(pid as i32, libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill").args(["/T", "/F", "/PID", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

pub struct Rq<'a> {
    pub i: Option<String>,
    pub t: u64,
    pub env: Vec<(String, String)>,
    pub rm: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub ln: Option<&'a (dyn Fn(&str) + Send + Sync)>,
    pub keep: bool,
}

impl<'a> Default for Rq<'a> {
    fn default() -> Self {
        Rq { i: None, t: 120, env: vec![], rm: vec![], cwd: None, ln: None, keep: false }
    }
}

pub async fn run(p: &str, a: &[String], r: Rq<'_>) -> Out {
    let mut c = cmd(p, a);
    c.stdin(if r.i.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    for k in &r.rm {
        c.env_remove(k);
    }
    for (k, v) in &r.env {
        c.env(k, v);
    }
    if let Some(d) = &r.cwd {
        c.current_dir(d);
    }
    let mut ch = match c.spawn() {
        Ok(x) => x,
        Err(e) => return Out { c: -1, o: String::new(), e: format!("{p}: {e}") },
    };
    let pid = ch.id().unwrap_or(0);
    if let (Some(i), Some(mut si)) = (r.i.clone(), ch.stdin.take()) {
        tokio::spawn(async move {
            let _ = si.write_all(i.as_bytes()).await;
            let _ = si.shutdown().await;
        });
    }
    let so = ch.stdout.take().unwrap();
    let mut se = ch.stderr.take().unwrap();
    let ln = r.ln;
    let kp = r.keep;
    let rd = async move {
        let mut o = String::new();
        let mut l = BufReader::new(so);
        let mut b = Vec::new();
        loop {
            b.clear();
            match l.read_until(b'\n', &mut b).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let s = String::from_utf8_lossy(&b);
                    if let Some(f) = ln {
                        f(s.trim_end());
                    }
                    o.push_str(&s);
                    if !kp && o.len() > MAXO * 2 {
                        o = o.split_off(o.len() - MAXO);
                    }
                }
            }
        }
        o
    };
    let re = async move {
        let mut b = Vec::new();
        let _ = se.read_to_end(&mut b).await;
        let s = String::from_utf8_lossy(&b).to_string();
        if s.len() > MAXO {
            s[s.len() - MAXO..].to_string()
        } else {
            s
        }
    };
    let w = async {
        let (o, e) = tokio::join!(rd, re);
        let st = ch.wait().await;
        (o, e, st)
    };
    match tokio::time::timeout(Duration::from_secs(r.t), w).await {
        Ok((o, e, st)) => Out { c: st.ok().and_then(|s| s.code()).unwrap_or(-1), o: if kp { o } else { tail(&o) }, e },
        Err(_) => {
            kill(pid);
            Out { c: -1, o: String::new(), e: format!("timeout after {} s", r.t) }
        }
    }
}

fn tail(s: &str) -> String {
    if s.len() <= MAXO {
        return s.to_string();
    }
    let mut i = s.len() - MAXO;
    while !s.is_char_boundary(i) {
        i += 1;
    }
    s[i..].to_string()
}

pub fn head(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut i = n;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    s[..i].to_string()
}

pub fn last(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut i = s.len() - n;
    while !s.is_char_boundary(i) {
        i += 1;
    }
    s[i..].to_string()
}

pub fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[derive(Clone, Debug)]
pub struct Ho {
    pub id: String,
    pub ssh: String,
    pub dk: bool,
    pub ln: i64,
    pub ci: i64,
    pub gb: i64,
    pub on: bool,
}

impl Ho {
    fn of(v: &Value) -> Ho {
        Ho {
            id: v["id"].as_str().unwrap_or("").into(),
            ssh: v["ssh"].as_str().unwrap_or("").into(),
            dk: v["dk"].as_i64().unwrap_or(0) == 1,
            ln: v["ln"].as_i64().unwrap_or(1).clamp(1, 8),
            ci: v["ci"].as_i64().unwrap_or(1).clamp(1, 8),
            gb: v["gb"].as_i64().unwrap_or(50).max(5),
            on: v["on_"].as_i64().unwrap_or(1) == 1,
        }
    }
}

pub fn hosts(a: &App) -> Vec<Ho> {
    a.db.all("select * from hs order by id='local' desc, id", pa![]).iter().map(Ho::of).collect()
}

pub fn ho(a: &App, id: &str) -> Option<Ho> {
    a.db.one("select * from hs where id=?", pa![id.to_string()]).map(|v| Ho::of(&v))
}

pub const CT: &str = "apollo-office";
pub const IM: &str = "apollo-office-build";

pub fn bp(p: &Path) -> String {
    let s = p.display().to_string();
    if cfg!(windows) {
        let s = s.replace('\\', "/");
        if s.len() > 2 && s.as_bytes()[1] == b':' {
            return format!("/{}{}", s[..1].to_lowercase(), &s[2..]);
        }
        return s;
    }
    s
}

pub fn hb(a: &App, h: &Ho) -> String {
    if h.dk {
        "/work".into()
    } else if h.ssh.is_empty() {
        bp(&a.dd.join("work"))
    } else {
        "$HOME/.apollo-office/work".into()
    }
}

pub fn hbid(a: &App, h: &str) -> String {
    ho(a, h).map(|x| hb(a, &x)).unwrap_or_else(|| "/work".into())
}

pub const PRE: &str = "ulimit -n 10240 2>/dev/null; export PATH=\"$HOME/.cargo/bin:$HOME/.local/bin:$HOME/go/bin:/usr/local/go/bin:/opt/homebrew/bin:/usr/local/bin:$PATH\" CARGO_TERM_COLOR=never NO_COLOR=1 CI=1 GIT_TERMINAL_PROMPT=0 CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_PROFILE_TEST_DEBUG=line-tables-only CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_BENCH_LTO=off; to() { s=$1; shift; if command -v timeout >/dev/null 2>&1; then timeout -k 10 \"$s\" \"$@\"; else \"$@\"; fi; }\n";

fn sshv(h: &Ho) -> Vec<String> {
    let mut v: Vec<String> = vec!["-o".into(), "ConnectTimeout=15".into(), "-o".into(), "ServerAliveInterval=15".into(), "-o".into(), "ServerAliveCountMax=4".into(), "-o".into(), "BatchMode=yes".into()];
    if cfg!(unix) {
        if let Some(d) = dirs::home_dir() {
            v.extend(["-o".into(), "ControlMaster=auto".into(), "-o".into(), format!("ControlPath={}/.ssh/cm-ao-%r@%h:%p", d.display()), "-o".into(), "ControlPersist=10m".into()]);
        }
    }
    v.push(h.ssh.clone());
    v
}

pub async fn hraw(h: &Ho, s: &str, t: u64) -> Out {
    let i = format!("{PRE}{s}");
    if h.ssh.is_empty() {
        run(&bash(), &["-s".into()], Rq { i: Some(i), t, ..Default::default() }).await
    } else {
        let mut v = sshv(h);
        v.extend(["bash".into(), "-s".into()]);
        run(&tool("ssh"), &v, Rq { i: Some(i), t, ..Default::default() }).await
    }
}

pub async fn hxl(a: &App, id: &str, s: &str, t: u64, ln: Option<&(dyn Fn(&str) + Send + Sync)>) -> Out {
    hxo(a, id, s, t, ln, false).await
}

pub async fn hxk(a: &App, id: &str, s: &str, t: u64) -> Out {
    hxo(a, id, s, t, None, true).await
}

async fn hxo(a: &App, id: &str, s: &str, t: u64, ln: Option<&(dyn Fn(&str) + Send + Sync)>, keep: bool) -> Out {
    let Some(h) = ho(a, id) else { return Out { c: -1, o: String::new(), e: format!("machine {id} is gone") } };
    let i = format!("{PRE}{s}");
    let mut v: Vec<String> = vec![];
    let p = if h.ssh.is_empty() {
        if h.dk {
            v.extend(["exec".into(), "-i".into(), CT.into(), "bash".into(), "-s".into()]);
            tool("docker")
        } else {
            v.push("-s".into());
            bash()
        }
    } else {
        v = sshv(&h);
        if h.dk {
            v.extend(["docker".into(), "exec".into(), "-i".into(), CT.into(), "bash".into(), "-s".into()]);
        } else {
            v.extend(["bash".into(), "-s".into()]);
        }
        tool("ssh")
    };
    let r = run(&p, &v, Rq { i: Some(i), t, ln, keep, ..Default::default() }).await;
    if h.dk && r.c != 0 && (r.e.contains("is not running") || r.e.contains("No such container")) {
        a.lv.lock().unwrap().o.net.insert(id.into(), false);
    }
    r
}

pub async fn hx(a: &App, id: &str, s: &str, t: u64) -> Out {
    hxl(a, id, s, t, None).await
}

pub const DF: &str = r#"FROM rust:1-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev clang cmake git curl ca-certificates procps xz-utils unzip \
    python3 python3-pip python3-venv python3-dev nodejs npm golang-go \
    libasound2-dev libfontconfig1-dev libfreetype-dev libgl1-mesa-dev libgtk-3-dev libwayland-dev \
    libx11-dev libxcursor-dev libxi-dev libxkbcommon-dev libxrandr-dev libudev-dev xvfb \
    && rm -rf /var/lib/apt/lists/*
RUN rustup component add rustfmt clippy && npm install -g pnpm yarn && pip3 install --break-system-packages pytest
ENV CARGO_TERM_COLOR=never CARGO_INCREMENTAL=0 PIP_BREAK_SYSTEM_PACKAGES=1
WORKDIR /work
"#;

pub fn dkup(h: &Ho, cpu: i64) -> String {
    format!(
        "command -v docker >/dev/null || {{ echo 'docker is not installed'; exit 2; }}\ndocker info >/dev/null 2>&1 || {{ echo 'docker is not running'; exit 2; }}\nif ! docker image inspect {IM} >/dev/null 2>&1; then docker build -q -t {IM} - <<'DOCKERFILE'\n{DF}DOCKERFILE\nfi\nr=$(docker inspect -f '{{{{.State.Running}}}}' {CT} 2>/dev/null)\nif [ -z \"$r\" ]; then docker run -d --name {CT} --restart unless-stopped --cpus {cpu} -v apollo-office-work:/work -v apollo-office-cargo:/usr/local/cargo/registry {IM} sleep infinity >/dev/null || exit 3; elif [ \"$r\" != true ]; then docker start {CT} >/dev/null || exit 3; fi\necho ok {}\n",
        h.id
    )
}

pub async fn ensure(a: &Ax, id: &str) -> Result<(), String> {
    let Some(h) = ho(a, id) else { return Err("unknown machine".into()) };
    if !h.dk {
        let r = hraw(&h, &format!("mkdir -p \"{}\" && git --version >/dev/null && echo ok", hb(a, &h)), 60).await;
        return if r.c == 0 { Ok(()) } else { Err(format!("{}{}", r.o, r.e).trim().chars().take(300).collect()) };
    }
    let n = hraw(&h, "getconf _NPROCESSORS_ONLN 2>/dev/null || nproc 2>/dev/null || echo 4", 30).await.o.trim().parse::<i64>().unwrap_or(4);
    let cpu = (n - 1).max(1);
    let r = hraw(&h, &dkup(&h, cpu), 3600).await;
    if r.c == 0 {
        Ok(())
    } else {
        Err(format!("{}{}", r.o, r.e).trim().chars().rev().take(400).collect::<String>().chars().rev().collect())
    }
}

pub async fn gh(a: &[&str], t: u64) -> Result<String, String> {
    let v: Vec<String> = a.iter().map(|x| x.to_string()).collect();
    let r = run(&tool("gh"), &v, Rq { t, env: vec![("GH_PROMPT_DISABLED".into(), "1".into()), ("NO_COLOR".into(), "1".into())], ..Default::default() }).await;
    if r.c != 0 {
        return Err(format!("gh {}: {}", a.iter().take(3).cloned().collect::<Vec<_>>().join(" "), if r.e.trim().is_empty() { r.o.trim() } else { r.e.trim() }).chars().take(400).collect());
    }
    Ok(r.o)
}

pub async fn ghj(a: &[&str], t: u64) -> Result<Value, String> {
    let o = gh(a, t).await?;
    if o.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&o).map_err(|e| format!("gh json: {e}"))
}

pub async fn apip(p: &str) -> Result<Vec<Value>, String> {
    let o = gh(&["api", "--paginate", "--slurp", "-H", "Accept: application/vnd.github+json", p], 180).await?;
    let v: Value = serde_json::from_str(&o).map_err(|e| format!("gh json: {e}"))?;
    Ok(v.as_array().cloned().unwrap_or_default().into_iter().flat_map(|x| x.as_array().cloned().unwrap_or_default()).collect())
}
