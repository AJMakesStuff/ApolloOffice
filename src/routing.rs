//! Opt-in deployment policy: free author, subscription reviewer, bounded repair.
use crate::app::{App, Ax, X, R};
use serde_json::{json, Value};

pub const FREE: [&str; 2] = ["nvidia:nvidia/nemotron-3-super-120b-a12b", "mistral:ministral-14b-latest"];
pub const OPENCODE: [&str; 2] = ["opencode:opencode/beam-free", "opencode:opencode/deepseek-v4.1-flash-free"];
pub fn is_opencode(md: &str) -> bool { OPENCODE.contains(&md) }
pub const REVIEWERS: [&str; 2] = ["codex:", "cc:sonnet"];
pub fn enabled(a: &App) -> bool { std::env::var("APOLLO_OFFICE_FREE_FIRST").as_deref() == Ok("1") || a.set()["free_first"].as_bool() == Some(true) }
pub fn is_free(md: &str) -> bool { FREE.contains(&md) || is_opencode(md) || md == "groq:openai/gpt-oss-120b" }
pub fn plan(a: &Ax) -> Vec<String> {
    let settings = a.set();
    let extra = settings["opencode_authors"].as_array().map(|rows| rows.iter()
        .filter_map(|v| v.as_str()).filter(|md| is_opencode(md)).map(String::from)
        .collect::<Vec<_>>()).unwrap_or_default();
    let mut result = extra;
    if let Ok(models)=std::env::var("APOLLO_OFFICE_OPENCODE_AUTHORS") {
        result.extend(models.split(',').map(str::trim).filter(|m|is_opencode(m)).map(str::to_string));
    }
    let mut seen=std::collections::BTreeSet::new();result.retain(|m|seen.insert(m.clone()));
    result.extend(FREE.iter().chain(REVIEWERS.iter()).map(|s| s.to_string()));
    result
}

pub fn plan_for(a: &Ax, role: &str) -> Vec<String> {
    if matches!(role, "fixer" | "repro") { plan(a) }
    else { FREE.iter().chain(REVIEWERS.iter()).map(|s| s.to_string()).collect() }
}

pub fn validate(a: &Ax, md: &str) -> R<()> {
    if !enabled(a) { return Ok(()); }
    if !is_free(md) && !REVIEWERS.contains(&md) { return Err(X::E("route denied by free-first policy".into())); }
    if is_free(md) {
        let (id, model) = crate::prov::parse(md);
        if matches!(id, "nvidia" | "mistral" | "groq") {
            if let Ok(path) = std::env::var("APOLLO_OFFICE_FREE_APPROVALS") {
                let data = std::fs::read_to_string(path).map_err(|_|X::E("free-tier approval missing".into()))?;
                let approvals:Value = serde_json::from_str(&data).map_err(|_|X::E("free-tier approval malformed".into()))?;
                let approved = approvals["providers"].as_array().map(|rows| rows.iter().any(|r|
                    r["provider"] == id && r["model"] == model && r["base_url"] == crate::prov::base(a,id) &&
                    r["user_approved_free_only"] == true && r["billing_disabled_verified"] == true &&
                    r["tool_calling_verified"] == true && r["expires_at"].as_f64().unwrap_or(0.0) > crate::db::now() as f64 / 1000.0
                )).unwrap_or(false);
                if !approved { return Err(X::E("free-tier approval expired or route not approved; paid inference denied".into())); }
            } else if !cfg!(test) { return Err(X::E("free-tier approval path missing".into())); }
        }
        if crate::prov::pv(id).map(|p| p.base) != Some(crate::prov::base(a, id).as_str()) {
            return Err(X::E("free route endpoint override denied".into()));
        }
    }
    Ok(())
}

// Independent records avoid overwriting a pipeline Job snapshot during async work.
pub fn event(a: &Ax, job: Option<i64>, role: &str, attempt: usize, md: &str, feedback: &str) {
    if !enabled(a) { return; }
    let key = format!("routing:{}:{role}:{attempt}", job.unwrap_or(0));
    a.db.ks(&key, &json!({"job":job,"role":role,"attempt":attempt + 1,"model":md,
        "t":crate::db::now(),"feedback":crate::sh::head(feedback,6000),
        "subscription_repair": role != "reviewer" && REVIEWERS.contains(&md)}));
    a.say(role, &format!("attempt {}: {md}{}", attempt+1, if !feedback.is_empty() { "; prior diagnostics handed over" } else { "" }), job);
}

pub fn lesson(a: &Ax, job: i64, reviewer: &str, finding: &Value, evidence: &str, diff: &str) {
    if !enabled(a) || finding["ok"] != false { return; }
    use sha2::{Digest, Sha256};
    let diff_sha = hex::encode(Sha256::digest(diff.as_bytes()));
    let record = json!({"status":"pending_review","objective_verified":false,"job":job,
        "reviewer":reviewer,"finding":finding,"evidence":evidence,"diff_sha256":diff_sha,
        "t":crate::db::now(),"origin":"apollo_subscription_review","promotion":"manual_review_required"});
    let id = hex::encode(Sha256::digest(record.to_string().as_bytes()));
    a.db.ks(&format!("lesson:{job}:{id}"), &record);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> Ax {
        let db = crate::db::Db::mem();
        db.ks("set", &json!({"free_first":true,"md":{"big":"openai:paid","fixer":"cc:sonnet"}}));
        std::sync::Arc::new(crate::app::App::new(db, std::env::temp_dir(), 1, "apollo-office".into()))
    }
    #[test]
    fn author_free_even_for_complex_work_and_paid_config() {
        let a=app();
        assert_eq!(a.md("fixer"),FREE[0]); assert_eq!(a.md("big"),FREE[0]);
        let p=plan(&a); assert_eq!(&p[..2], &FREE); assert_eq!(&p[2..], &REVIEWERS);
        assert!(validate(&a,"openai:gpt-5.5").is_err());
        assert!(validate(&a,"anthropic:claude-sonnet-5-5").is_err());
        for md in p { assert!(validate(&a,&md).is_ok()); }
        a.db.ks("set",&json!({"free_first":true,"prov":{"nvidia":{"base":"https://attacker.invalid"}}}));
        assert!(validate(&a,FREE[0]).is_err());
    }
    #[test]
    fn opencode_is_opt_in_and_keeps_existing_authors_and_reviewers() {
        let a = app();
        assert_eq!(plan(&a)[0], FREE[0]);
        a.db.ks("set", &json!({"free_first":true, "opencode_authors":[OPENCODE[0], "opencode:paid"]}));
        let p = plan(&a);
        assert_eq!(p[0], OPENCODE[0]);
        assert_eq!(&p[1..3], &FREE);
        assert_eq!(&p[3..], &REVIEWERS);
        assert_eq!(a.md("fixer"), OPENCODE[0]);
        assert_eq!(a.md("guard"), FREE[0]);
        assert_eq!(plan_for(&a,"guard")[0],FREE[0]);
        assert_eq!(plan_for(&a,"repro")[0],OPENCODE[0]);
        assert!(validate(&a, OPENCODE[0]).is_ok());
        assert!(validate(&a, "opencode:opencode/paid-model").is_err());
        a.db.ks("set", &json!({"free_first":true,"prov":{"opencode":{"base":"https://external.invalid"}}}));
        assert!(validate(&a,OPENCODE[0]).is_err());
    }
    #[test]
    fn rejection_stages_attributed_lesson_only() {
        let a=app();
        lesson(&a,12,"codex:",&json!({"ok":true}),"tests pass","diff");
        assert_eq!(a.db.n("select count(*) from kv where k like 'lesson:%'",crate::pa![]),0);
        lesson(&a,12,"codex:",&json!({"ok":false,"issues":["fix assertion"]}),"real test exit 1","diff");
        let row=a.db.one("select v from kv where k like 'lesson:%'",crate::pa![]).unwrap();
        let v:Value=serde_json::from_str(row["v"].as_str().unwrap()).unwrap();
        assert_eq!(v["status"],"pending_review"); assert_eq!(v["objective_verified"],false);
        assert_eq!(v["reviewer"],"codex:"); assert_eq!(v["diff_sha256"].as_str().unwrap().len(),64);
    }
}
