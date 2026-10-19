(() => {
  const A = window.AO, { get, post, esc, $ } = A;
  const dr = $("dr"), T = $("drT"), B = $("drB");
  let cur = "", cfg = null, ck = null, jid = 0;

  const show = (t, k) => { cur = k; T.textContent = t; dr.hidden = false; };
  const hide = () => { dr.hidden = true; cur = ""; jid = 0; };
  $("drX").onclick = hide;
  dr.onclick = (e) => { if (e.target === dr) hide(); };
  document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !dr.hidden) hide(); });

  const yes = (v) => v === 1 || v === true;
  const tg = (n, v, l) => `<label class="ck"><input type="checkbox" name="${n}" ${yes(v) ? "checked" : ""}> ${l}</label>`;
  const fv = (f) => {
    const o = {};
    for (const e of f.querySelectorAll("[name]")) o[e.name] = e.type === "checkbox" ? e.checked : e.type === "number" ? Number(e.value) : e.value;
    return o;
  };
  const busy = (b, on, t) => { b.disabled = on; if (t) b.textContent = t; };

  const diff = (d) => esc(d || "").split("\n").map((l) => `<span class="${l.startsWith("diff --git") ? "fh" : l.startsWith("@@") ? "hh" : l.startsWith("+") && !l.startsWith("+++") ? "ad" : l.startsWith("-") && !l.startsWith("---") ? "rm" : ""}">${l}</span>`).join("\n");

  const repos = async () => {
    cfg = await get("/api/cfg");
    show("Repos", "repos");
    const rs = cfg.repos;
    B.innerHTML = `<div class="sec"><h4>Add a repository</h4><div class="box"><form id="fa" class="acts" style="margin:0"><input type="text" name="full" placeholder="owner/name or GitHub link" style="flex:1;min-width:200px"><button class="pri">Add</button></form><div class="msg" id="am"></div></div></div>
    <div class="sec"><h4>${rs.length} ${rs.length === 1 ? "repository" : "repositories"}</h4>${rs.map((r) => `<form class="box" data-r="${esc(r.full)}"><div class="hd2"><b>${esc(r.full)}</b>${tg("on_", r.on_, "active")}</div>
      ${r.ci ? "" : `<div class="warn">No test command found. Enter the command that runs this repository's tests, otherwise nothing happens here.</div>`}
      <div class="fm">
        <label>Work on</label><div class="acts" style="margin:0">${tg("iss", r.iss, "open issues")}${tg("hunt", r.hunt, "hunt for bugs itself")}</div>
        <label>Test command</label><input type="text" name="ci" value="${esc(r.ci)}" placeholder="e.g. cargo test, npm test, pytest">
        <div class="hint">Runs from the repository root. A fix is only submitted when this passes.</div>
        <label>Setup command</label><input type="text" name="setup" value="${esc(r.setup)}" placeholder="optional, e.g. npm ci">
        <div class="hint">Runs before every job, after checkout.</div>
        <label>Pull requests</label><select name="mode"><option value="ask" ${r.mode !== "auto" ? "selected" : ""}>wait for my approval</option><option value="auto" ${r.mode === "auto" ? "selected" : ""}>submit automatically</option></select>
        <label>Max open PRs</label><input type="number" name="maxo" min="0" max="10000" value="${r.maxo}">
        <label>Max new PRs per day</label><input type="number" name="maxd" min="0" max="10000" value="${r.maxd}">
        <label>PR footer</label>${tg("nm", r.nm ?? 1, "mention ApolloOffice with a link")}
        <div class="hint">The AI note and the Co-Authored-By trailers are always added.</div>
        <label>Your rules</label><textarea name="rules" placeholder="optional, e.g. only issues labeled bug; never touch the plugin API">${esc(r.rules)}</textarea>
      </div>
      <div class="acts"><button class="pri" data-op="set">Save</button><button type="button" data-op="detect">Detect test command</button><span class="sp"></span><button type="button" class="dng" data-op="del">Remove</button></div><div class="msg"></div></form>`).join("") || `<div class="empty">No repository yet.</div>`}</div>`;
    $("fa").onsubmit = async (e) => {
      e.preventDefault();
      const b = e.target.querySelector("button"), m = $("am");
      busy(b, true, "Adding…");
      const r = await post({ op: "add", full: e.target.full.value }, "/api/repo");
      busy(b, false, "Add");
      if (r.error) { m.className = "msg bad2"; m.textContent = r.error; return; }
      await repos();
      const n = $("am");
      n.className = "msg";
      n.textContent = r.ci ? `Added ${r.full}. Test command detected: ${r.ci}${r.setup ? " (setup: " + r.setup + ")" : ""}. Check it below.` : `Added ${r.full}. No test command detected, please enter one below.`;
    };
    for (const f of B.querySelectorAll("form[data-r]")) {
      f.onsubmit = (e) => e.preventDefault();
      f.onclick = async (e) => {
        const b = e.target.closest("button[data-op]");
        if (!b) return;
        e.preventDefault();
        const m = f.querySelector(".msg"), op = b.dataset.op, full = f.dataset.r;
        if (op === "del") {
          if (b.dataset.sure !== "1") { b.dataset.sure = "1"; b.textContent = "Click again to remove"; return; }
          await post({ op: "del", full }, "/api/repo");
          return repos();
        }
        if (op === "detect") {
          busy(b, true, "Detecting…");
          const r = await post({ op: "detect", full }, "/api/repo");
          busy(b, false, "Detect test command");
          if (r.ci) { f.ci.value = r.ci; if (r.setup && !f.setup.value) f.setup.value = r.setup; m.textContent = "Detected. Save to keep it."; }
          else m.textContent = "Nothing detected.";
          return;
        }
        busy(b, true, "Saving…");
        const r = await post({ op: "set", full, ...fv(f) }, "/api/repo");
        busy(b, false, "Save");
        m.className = r.error ? "msg bad2" : "msg";
        m.textContent = r.error || "Saved.";
      };
    }
  };

  const hosts = async () => {
    cfg = await get("/api/cfg");
    show("Machines", "hosts");
    const hs = cfg.hosts;
    const hl = hs.some((h) => h.id === "local");
    const form = (h, nw) => `<form class="box" data-h="${esc(h.id || "")}" data-new="${nw ? 1 : ""}"><div class="hd2"><b>${nw ? (h.id === "local" ? "This computer" : "SSH machine") : esc(h.id) + (h.ssh ? " · ssh " + esc(h.ssh) : " · this computer")}</b>${nw ? "" : tg("on", h.on_, "active")}</div>
      <div class="fm">
        ${nw && h.id !== "local" ? `<label>SSH host</label><input type="text" name="ssh" placeholder="alias from ~/.ssh/config or user@host"><div class="hint">Key login must work without a password prompt.</div>` : ""}
        <label>Isolation</label><select name="dk"><option value="1" ${h.dk !== 0 ? "selected" : ""}>Docker container (recommended)</option><option value="0" ${h.dk === 0 ? "selected" : ""}>directly on the machine</option></select>
        <div class="hint raw" ${h.dk === 0 ? "" : "hidden"}><span style="color:var(--bad)">Workers build and run code from the repositories you add, directly with your user account, which can reach your GitHub, Claude and SSH logins on this machine. Only do this for repositories you trust.</span><br>${tg("raw", h.dk === 0 && !nw, "I understand")}</div>
        <label>Parallel jobs</label><input type="number" name="ln" min="1" max="8" value="${h.ln || 1}">
        <label>Parallel test runs</label><input type="number" name="ci" min="1" max="8" value="${h.ci || 1}">
        <label>CPU limit</label><input type="number" name="cpu" min="0" value="${h.cpu || 0}"><div class="hint">Cores for the Docker container. 0 = all but one.</div>
        <label>Skip repos</label><input type="text" name="no" value="${esc(h.no || "")}" placeholder="optional, e.g. owner/big-repo, other-repo"><div class="hint">Repositories this machine never builds, for example because they are too large for it.</div>
        <label>Disk limit (GB)</label><input type="number" name="gb" min="5" value="${h.gb || 50}">
        <div class="hint">Old clones and build folders are deleted above this, or when less than 20 GB are free.</div>
      </div>
      <div class="acts"><button class="pri" data-op="set">${nw ? "Add" : "Save"}</button>${nw ? "" : `<span class="sp"></span><button type="button" class="dng" data-op="del">Remove</button>`}</div><div class="msg"></div></form>`;
    B.innerHTML = `<div class="sec"><h4>Machines that build and test</h4>${hs.map((h) => form(h, false)).join("") || `<div class="empty">No machine yet. Add this computer below.</div>`}</div>
    <div class="sec"><h4>Add</h4>${hl ? "" : form({ id: "local", dk: 1 }, true)}${form({ id: "", dk: 1 }, true)}</div>`;
    for (const f of B.querySelectorAll("form[data-h]")) {
      f.dk.onchange = () => { f.querySelector(".raw").hidden = f.dk.value === "1"; };
      f.onsubmit = (e) => e.preventDefault();
      f.onclick = async (e) => {
        const b = e.target.closest("button[data-op]");
        if (!b) return;
        e.preventDefault();
        const m = f.querySelector(".msg"), v = fv(f);
        let id = f.dataset.h;
        if (!id) id = (v.ssh || "").replace(/^.*@/, "").replace(/[^A-Za-z0-9._-]/g, "-").slice(0, 40);
        if (b.dataset.op === "del") {
          if (b.dataset.sure !== "1") { b.dataset.sure = "1"; b.textContent = "Click again to remove"; return; }
          await post({ op: "del", id }, "/api/host");
          return hosts();
        }
        const ex = hs.find((x) => x.id === id);
        const body = { op: "set", id, ssh: v.ssh ?? ex?.ssh ?? "", dk: v.dk === "1", raw: !!v.raw, ln: v.ln, ci: v.ci, gb: v.gb, cpu: v.cpu, no: v.no, on: v.on ?? true };
        busy(b, true, body.dk ? "Setting up Docker (first time takes several minutes)…" : "Checking…");
        m.className = "msg"; m.textContent = "";
        const r = await post(body, "/api/host");
        busy(b, false, f.dataset.new ? "Add" : "Save");
        if (r.error) { m.className = "msg bad2"; m.textContent = r.error; return; }
        if (!r.ok) { m.className = "msg bad2"; m.textContent = "Saved, but the machine is not ready: " + (r.m || ""); return; }
        hosts();
      };
    }
  };

  const RK = { scout: [["scout", "Model"]], guard: [["guard", "Model"]], vet: [["vet", "Model"]], watcher: [["watcher", "Model"]], hunter: [["hunter", "Model"]], reviewer: [["reviewer", "Model"]], repro: [["repro", "First try"], ["repro2", "Second try"]], fixer: [["fixer", "Model"], ["big", "Complex bugs"]] };
  const TOOLS = ["hunter", "repro", "fixer"];
  const split = (v) => { const i = (v || "").indexOf(":"), p = i > 0 ? v.slice(0, i) : ""; return cfg.prov.some((x) => x.id === p) ? [p, v.slice(i + 1)] : ["cc", v || ""]; };
  const join = (p, m) => (p === "cc" ? m : p + ":" + m);

  const role = async (w) => {
    cfg = await get("/api/cfg");
    show((window.AO.NM?.[w] || w) + " · model", "role");
    const s = cfg.set, ks = RK[w];
    const popt = (sel) => cfg.prov.map((p) => `<option value="${p.id}" ${p.id === sel ? "selected" : ""}>${esc(p.nm)}${p.has ? "" : " (no key yet)"}</option>`).join("");
    B.innerHTML = `<form id="fr">${ks.map(([k, l]) => { const [p, m] = split(s.md[k]); return `<div class="sec"><h4>${esc(l)}</h4><div class="box" data-k="${k}"><div class="fm" style="margin:0">
        <label>Provider</label><select data-p>${popt(p)}</select>
        <label>Model</label><input type="text" data-m list="dl-${k}" value="${esc(m)}" placeholder="model id"><datalist id="dl-${k}"></datalist>
        <div class="hint" data-h></div>
      </div><div class="acts"><button type="button" data-t>Test</button></div><div class="msg" data-r></div></div></div>`; }).join("")}
      ${w === "fixer" ? `<div class="sec"><div class="box">${tg("big", s.big, "use the complex-bug model for Repro and Fixer when Vet marks a bug complex")}</div></div>` : ""}
      ${TOOLS.includes(w) ? `<div class="msg">This worker reads and edits code with tools. Small local models often fail here; a strong model saves time and tokens.</div>` : ""}
      <div class="acts"><button class="pri">Save</button></div><div class="msg" id="rm"></div></form>`;
    for (const bx of B.querySelectorAll(".box[data-k]")) {
      const ps = bx.querySelector("[data-p]"), mi = bx.querySelector("[data-m]"), dl = bx.querySelector("datalist"), h = bx.querySelector("[data-h]");
      const fill = async (reset) => {
        const pv = cfg.prov.find((x) => x.id === ps.value);
        h.textContent = !pv.has ? "Add the API key for this provider in Settings first." : pv.id === "ollama" ? "Runs on " + pv.base : pv.cli ? "Uses the CLI login on this computer." : "";
        dl.innerHTML = pv.ms.map((m) => `<option value="${esc(m)}">`).join("");
        if (reset) mi.value = pv.ms[0] || "";
        if (!pv.has) return;
        const r = await get("/api/models?p=" + pv.id);
        if (r.ms?.length) { dl.innerHTML = r.ms.map((m) => `<option value="${esc(m)}">`).join(""); if (reset && !mi.value) mi.value = r.ms[0]; }
        else if (r.error) h.textContent = "Could not list models: " + r.error;
      };
      ps.onchange = () => fill(true);
      fill(false);
      bx.querySelector("[data-t]").onclick = async (e) => {
        const b = e.target, m = bx.querySelector("[data-r]");
        busy(b, true, "Testing…"); m.className = "msg"; m.textContent = "";
        const r = await post({ md: join(ps.value, mi.value.trim()) }, "/api/test");
        busy(b, false, "Test");
        m.className = r.ok ? "msg" : "msg bad2"; m.textContent = r.ok ? "Answer: " + r.m : r.m;
      };
    }
    $("fr").onsubmit = async (e) => {
      e.preventDefault();
      const md = {};
      for (const bx of B.querySelectorAll(".box[data-k]")) md[bx.dataset.k] = join(bx.querySelector("[data-p]").value, bx.querySelector("[data-m]").value.trim());
      const body = { md };
      const bg = $("fr").querySelector("[name=big]");
      if (bg) body.big = bg.checked;
      const r = await post(body, "/api/set");
      $("rm").className = r.error ? "msg bad2" : "msg";
      $("rm").textContent = r.error || "Saved.";
    };
  };
  A.role = role;

  const settings = async () => {
    cfg = await get("/api/cfg");
    show("Settings", "set");
    const s = cfg.set, u = cfg.upd || {};
    const keys = cfg.prov.filter((p) => p.key);
    const ol = cfg.prov.find((p) => p.id === "ollama");
    const ust = u.ready && u.ready !== A.S?.v ? `Update ${esc(u.ready)} is downloaded.` : u.err ? `Last check: ${esc(u.err)}` : u.latest ? `Latest release: ${esc(u.latest)}` : "Not checked yet.";
    B.innerHTML = `<form id="fs">
      <div class="sec"><h4>Providers</h4><div class="box"><div class="fm" style="margin:0">
        ${keys.map((p) => `<label>${esc(p.nm)}</label><input type="password" autocomplete="off" data-key="${p.id}" placeholder="${p.has ? "saved · type to replace, clear with a single space" : "API key"}">`).join("")}
        <label>Claude Code token</label><input type="password" autocomplete="off" data-key="cc" placeholder="${cfg.prov.find((p) => p.id === "cc").tok ? "saved · type to replace, clear with a single space" : "optional, from claude setup-token"}">
        <label>or keychain item</label><input type="text" name="cckc" value="${esc(s.prov?.cc?.kc || "")}" placeholder="optional, macOS keychain service name">
        <div class="hint">Without a token Claude Code uses its own login on this computer.</div>
        <label>Ollama address</label><input type="text" name="ollama" value="${esc(s.prov?.ollama?.base || "")}" placeholder="${esc(ol.dbase)}">
        <label>Ollama context</label><input type="number" name="octx" min="2048" step="1024" value="${s.prov?.ollama?.ctx || 32768}"><div class="hint">Tokens per request. Ollama's default of 4096 is too small for the workers.</div>
        <div class="hint">Keys are stored encrypted on this computer. Claude Code and Codex use their own CLI logins. Pick a model per worker under Models below.</div>
      </div></div></div>
      <div class="sec"><h4>Models</h4><div class="box"><div class="fm" style="margin:0">
        ${["scout", "guard", "vet", "hunter", "repro", "fixer", "reviewer", "watcher"].map((w) => `<label>${esc(A.NM?.[w] || w)}</label><div class="ck"><span style="flex:1">${RK[w].map(([k, l]) => esc((RK[w].length > 1 ? l + ": " : "") + (A.mdn ? A.mdn(s.md[k]) : s.md[k]))).join(" · ")}</span><button type="button" data-role="${w}">Change</button></div>`).join("")}
        <div class="hint">Also reachable with the gear on each worker's card.</div>
      </div></div></div>
      <div class="sec"><h4>Sign in</h4><div class="box">${cfg.google ? `<div class="ck"><b class="ok2">Google</b> ${esc(cfg.google)} can sign in.</div><div class="acts"><button type="button" class="dng" id="gu">Disconnect Google</button></div>` : `<div class="ck">Sign in with a one-time link from <code>apollo-office login-link</code>, or connect a Google account.</div><div class="acts"><button type="button" id="gl">Connect Google account</button></div>`}</div></div>
      <div class="sec"><h4>Work</h4><div class="box"><div class="fm" style="margin:0">
        <label>Hunters</label><input type="number" name="hunters" min="0" max="12" value="${s.hunters}"><div class="hint">How many bug hunts may run at once (only in repos with hunting on).</div>
        <label>Daily token limit</label><input type="number" name="tokday" min="0" value="${s.tokday}"><div class="hint">0 = no limit. The office always pauses when a provider reports a usage limit.</div>
      </div></div></div>
      <div class="sec"><h4>Notifications</h4><div class="box"><div class="fm" style="margin:0">
        <label>ntfy topic URL</label><input type="text" name="ntfy" value="${esc(s.ntfy)}" placeholder="https://ntfy.sh/your-secret-topic">
        <label>Discord webhook</label><input type="text" name="discord" value="${esc(s.discord)}" placeholder="https://discord.com/api/webhooks/…">
      </div><div class="acts"><button type="button" id="bt">Send test</button></div></div></div>
      <div class="sec"><h4>Updates</h4><div class="box">${tg("updates", s.updates, "install updates automatically when the office is idle")}<div class="msg" id="um">${ust}</div><div class="acts"><button type="button" id="uc">Check now</button>${u.ready && u.ready !== A.S?.v ? `<button type="button" class="pri" id="ua">Install and restart</button>` : ""}</div></div></div>
      <div class="sec"><h4>This installation</h4><div class="box"><div class="fm" style="margin:0">
        <label>Start at login</label>${tg("auto", cfg.auto, "start ApolloOffice when I log in")}
        <label>Data folder</label><span class="mono" style="font-size:12px;word-break:break-all">${esc(cfg.data)}</span>
        <label>Version</label><span class="mono" style="font-size:12px">${esc(A.S?.v || "")} · ${esc(cfg.asset)}</span>
      </div></div></div>
      <div class="acts"><button class="pri">Save</button></div><div class="msg" id="sm"></div></form>`;
    const send = async (test) => {
      const f = $("fs"), v = fv(f);
      for (const e of f.querySelectorAll("[data-key]")) {
        if (!e.value) continue;
        const r = await post({ id: e.dataset.key, key: e.value.trim() }, "/api/prov");
        if (r.error) { $("sm").className = "msg bad2"; $("sm").textContent = r.error; return; }
      }
      const r = await post({ hunters: v.hunters, tokday: v.tokday, ntfy: v.ntfy, discord: v.discord, auto: v.auto, updates: v.updates, ollama: v.ollama, octx: v.octx, cckc: v.cckc, test }, "/api/set");
      if (r.error) { $("sm").className = "msg bad2"; $("sm").textContent = r.error; return; }
      await settings();
      $("sm").textContent = test ? "Saved and test sent." : "Saved.";
    };
    $("fs").onsubmit = (e) => { e.preventDefault(); send(false); };
    for (const b of B.querySelectorAll("[data-role]")) b.onclick = () => role(b.dataset.role);
    $("bt").onclick = () => send(true);
    if ($("gl")) $("gl").onclick = async () => { const r = await post({ op: "link" }, "/api/google"); if (r.url) location.href = r.url; };
    if ($("gu")) $("gu").onclick = async (e) => { if (e.target.dataset.sure !== "1") { e.target.dataset.sure = "1"; e.target.textContent = "Click again to disconnect"; return; } await post({ op: "unlink" }, "/api/google"); settings(); };
    $("uc").onclick = async (e) => { busy(e.target, true, "Checking…"); const r = await post({ op: "check" }, "/api/update"); await settings(); $("um").textContent = r.ok ? (r.r.new ? `Update ${r.r.latest} downloaded.` : `Up to date (latest ${r.r.latest}).`) : r.m; };
    if ($("ua")) $("ua").onclick = async (e) => { busy(e.target, true, "Restarting…"); await post({ op: "apply" }, "/api/update"); setTimeout(() => location.reload(), 6000); };
  };

  const job = async (id) => {
    const j = await get("/api/job?id=" + id);
    if (j.error) return;
    jid = id;
    show(j.st === "approve" ? "Review pull request" : "Job " + id, "job");
    const tag = (j.repo.split("/")[1] || j.repo) + (j.num ? "#" + j.num : "");
    const ev = (j.ev || []).map((e) => `<div><span class="tm">${new Date(e.t).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" })}</span><span>${esc(e.w)}</span><span>${esc(e.m)}</span></div>`).join("");
    if (j.st === "approve") {
      B.innerHTML = `<div class="sec"><div class="box"><div class="hd2"><b>${esc(j.repo)}${j.num ? " #" + j.num : " (found by Hunter)"}</b>${j.url ? `<a class="a" href="#" data-u="${esc(j.url)}">open issue</a>` : ""}</div><div class="msg">Built and tested on ${esc(j.h || "")}. Nothing is on GitHub yet.</div></div></div>
      <div class="sec"><h4>Pull request</h4><input type="text" id="pt" value="${esc(j.pt || j.title)}"><textarea id="pb" style="min-height:220px;margin-top:8px">${esc(j.pb || "")}</textarea></div>
      ${j.proof ? `<div class="sec"><h4>Proof</h4><pre class="df">${esc((j.proof.a || "") + "\n\n" + (j.proof.b || ""))}</pre></div>` : ""}
      <div class="sec"><h4>Diff</h4><pre class="df">${diff(j.df)}</pre></div>
      <div class="acts"><button class="pri" id="ba">Submit pull request</button><button class="dng" id="bd">Drop</button></div><div class="msg" id="jm"></div>
      <div class="sec" style="margin-top:20px"><h4>Log</h4><div class="log">${ev}</div></div>`;
      $("ba").onclick = async () => {
        busy($("ba"), true, "Submitting…");
        const r = await post({ a: "approve", id, pt: $("pt").value, pb: $("pb").value });
        if (r.error) { busy($("ba"), false, "Submit pull request"); $("jm").className = "msg bad2"; $("jm").textContent = r.error; return; }
        A.apply(r); hide();
      };
      $("bd").onclick = async () => {
        if ($("bd").dataset.sure !== "1") { $("bd").dataset.sure = "1"; $("bd").textContent = "Click again to drop"; return; }
        A.apply(await post({ a: "drop", id })); hide();
      };
      return;
    }
    B.innerHTML = `<div class="sec"><div class="box"><div class="hd2"><b>${esc(j.title)}</b><span class="pill">${esc(j.st)}</span></div><div class="msg">${esc(j.repo)} ${esc(tag)} · ${esc(j.kind)}${j.h ? " · " + esc(j.h) : ""}\n${esc(j.why || "")}</div>
      <div class="acts">${j.url ? `<button data-u="${esc(j.url)}">Open on GitHub</button>` : ""}${!["done", "drop"].includes(j.st) ? `<button class="dng" data-d="${id}">Drop</button>` : ""}${j.st === "drop" && j.kind === "fix" ? `<button data-rq="${id}">Try again</button>` : ""}</div></div></div>
      ${j.df ? `<div class="sec"><h4>Diff</h4><pre class="df">${diff(j.df)}</pre></div>` : ""}
      <div class="sec"><h4>Log</h4><div class="log">${ev || `<div class="empty">nothing yet</div>`}</div></div>`;
  };
  A.job = job;

  const setup = async () => {
    ck = ck || (await get("/api/check"));
    cfg = await get("/api/cfg");
    show("Set up ApolloOffice", "setup");
    const row = (k, l) => `<div class="ck" style="margin:4px 0"><b class="${ck[k].ok ? "ok2" : "bad2"}">${ck[k].ok ? "ok" : "missing"}</b> ${l}: ${esc(ck[k].m)}</div>`;
    const hs = cfg.hosts.length, rs = cfg.repos.length;
    B.innerHTML = `<div class="sec"><h4>1 · Tools</h4><div class="box">${row("git", "git")}${row("bash", "bash")}${row("gh", "GitHub CLI")}${row("claude", "Claude Code")}${row("docker", "Docker")}<div class="acts"><button id="br">Check again</button></div></div></div>
      <div class="sec"><h4>2 · Where code is built</h4><div class="box"><div class="ck"><b class="${hs ? "ok2" : "bad2"}">${hs ? "ok" : "todo"}</b> ${hs ? hs + " machine(s)" : "Add this computer (Docker recommended)."}</div><div class="acts"><button id="bh">${hs ? "Machines" : "Add machine"}</button></div></div></div>
      <div class="sec"><h4>3 · Repositories</h4><div class="box"><div class="ck"><b class="${rs ? "ok2" : "bad2"}">${rs ? "ok" : "todo"}</b> ${rs ? rs + " repository(s)" : "Add a GitHub repository to work on."}</div><div class="acts"><button id="bp">${rs ? "Repos" : "Add repository"}</button></div></div></div>
      <div class="msg">Workers use your own Claude Code login and your own GitHub account. Pull requests wait for your approval unless you change that per repository.</div>`;
    $("br").onclick = async () => { ck = await get("/api/check"); setup(); };
    $("bh").onclick = hosts;
    $("bp").onclick = repos;
  };

  $("bRepos").onclick = repos;
  $("bHosts").onclick = hosts;
  $("bSet").innerHTML = A.GEAR;
  $("bSet").onclick = settings;
  let asked = false;
  A.onState = (s) => {
    if (!asked && (s.nr === 0 || !s.hs.length)) { asked = true; setup(); }
    if (cur === "job" && jid && !dr.hidden) {
      const q = s.q.find((x) => x.id === jid);
      if (!q && T.textContent.startsWith("Review")) hide();
    }
  };
})();
