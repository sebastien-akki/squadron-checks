mod checks;

use checks::{CHECKS, Image, Outcome, Status};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use squadron_extension_api::{Agent, Extension, host, squadron_extension};
use std::time::{SystemTime, UNIX_EPOCH};

struct Checks;

fn en() -> bool {
    host::state_get("lang").as_deref() == Some("en")
}

macro_rules! t {
    ($fr:literal, $en:literal $(, $a:expr)*) => {
        if en() { format!($en $(, $a)*) } else { format!($fr $(, $a)*) }
    };
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct Run {
    at: u64,
    sig: String,
    files: usize,
    outcomes: Vec<Outcome>,
}

struct Work {
    root: String,
    files: Vec<String>,
    sig: String,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn option(id: &str) -> Value {
    serde_json::from_str::<Value>(&host::options()).unwrap_or_default()[id].clone()
}

fn git(dir: &str, args: &[&str]) -> Result<String, String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let out = host::exec("git", &args, Some(dir))?;
    if out.code != 0 {
        return Err(out.stderr.trim().to_string());
    }
    Ok(out.stdout)
}

// The files this branch changed: against where it left the main branch, plus what is not committed.
fn work(dir: &str) -> Result<Work, String> {
    let root = git(dir, &["rev-parse", "--show-toplevel"])?.trim().to_string();
    let base = ["origin/HEAD", "origin/main", "origin/master"]
        .iter()
        .find_map(|b| git(&root, &["merge-base", "HEAD", b]).ok())
        .map_or_else(|| "HEAD".to_string(), |s| s.trim().to_string());
    let diff = git(&root, &["diff", "--name-only", "--diff-filter=ACMR", &base])?;
    let untracked = git(&root, &["ls-files", "--others", "--exclude-standard"])?;
    let files = checks::changed(&diff, &untracked);
    let head = git(&root, &["rev-parse", "HEAD"]).unwrap_or_default();
    let full = git(&root, &["diff", &base]).unwrap_or_default();
    let sig = checks::hash(&format!("{head}\n{full}\n{untracked}"));
    Ok(Work { root, files, sig })
}

// The set image, else Lando's of the project's version when Docker has it, else the official one.
fn php_image(root: &str) -> String {
    let set = option("php-image").as_str().unwrap_or_default().trim().to_string();
    if !set.is_empty() {
        return set;
    }
    let lando = git(root, &["show", "HEAD:.lando.yml"]).unwrap_or_default();
    let composer = git(root, &["show", "HEAD:composer.json"]).unwrap_or_default();
    let version = checks::php_version(&lando, &composer).unwrap_or_else(|| "8.3".into());
    let tags = host::exec("docker", &["images".into(), "devwithlando/php".into(), "--format".into(), "{{.Tag}}".into()], None).map(|o| o.stdout).unwrap_or_default();
    checks::lando_tag(&tags, &version).unwrap_or_else(|| format!("php:{version}-cli"))
}

fn user() -> String {
    let id = |flag: &str| host::exec("id", &[flag.to_string()], None).map(|o| o.stdout.trim().to_string()).unwrap_or_default();
    format!("{}:{}", id("-u"), id("-g"))
}

fn run_check(root: &str, user: &str, image: &str, check: &checks::Check, files: &[String]) -> Outcome {
    let mut args: Vec<String> = ["run", "--rm", "--network", "none", "-u", user, "-e", "HOME=/tmp", "-v", &format!("{root}:/app"), "-w", "/app", image, "sh", "-c", check.script, "sh"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.extend(files.iter().cloned());
    let outcome = |status, problems, note: String| Outcome { id: check.id.into(), label: check.label.into(), status, problems, note };
    match host::exec("docker", &args, None) {
        Err(e) => outcome(Status::Failed, vec![], e),
        Ok(out) if out.code == 99 => outcome(Status::Skipped, vec![], String::new()),
        Ok(out) => match checks::parse(check.id, out.code, &out.stdout) {
            Some(p) if p.is_empty() => outcome(Status::Ok, p, String::new()),
            Some(p) => outcome(Status::Problems, p, String::new()),
            None => outcome(Status::Failed, vec![], checks::first_lines(&format!("{}\n{}", out.stderr, out.stdout))),
        },
    }
}

fn check_all(w: &Work) -> Run {
    let mut run = Run { at: now(), sig: w.sig.clone(), files: w.files.len(), outcomes: Vec::new() };
    if w.files.is_empty() {
        return run;
    }
    let user = user();
    let php = php_image(&w.root);
    let node = option("node-image").as_str().filter(|s| !s.trim().is_empty()).unwrap_or("node:22").to_string();
    for check in CHECKS {
        let files: Vec<String> = w.files.iter().filter(|f| checks::applies(check, f)).cloned().collect();
        if files.is_empty() {
            continue;
        }
        let image = match check.image {
            Image::Php => php.as_str(),
            Image::Composer => "composer:2",
            Image::Node => node.as_str(),
        };
        run.outcomes.push(run_check(&w.root, &user, image, check, &files));
    }
    run
}

fn key(agent: &str) -> String {
    format!("run:{agent}")
}

fn last(agent: &str) -> Option<Run> {
    host::state_get(&key(agent)).and_then(|s| serde_json::from_str(&s).ok())
}

// Keeps the newest results within the 1 MB of state.
fn save(agent: &str, run: &Run) {
    let mut seen: Vec<String> = host::state_get("agents").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    seen.retain(|a| a != agent);
    seen.push(agent.to_string());
    while seen.len() > 40 {
        host::state_set(&key(&seen.remove(0)), None);
    }
    host::state_set("agents", Some(&serde_json::to_string(&seen).unwrap_or_default()));
    host::state_set(&key(agent), serde_json::to_string(run).ok().as_deref());
}

fn summary(run: &Run) -> String {
    run.outcomes
        .iter()
        .filter(|o| o.status != Status::Skipped)
        .map(|o| match o.status {
            Status::Problems => t!("{}\u{a0}: {}", "{}: {}", o.label, o.problems.len()),
            Status::Failed => t!("{}\u{a0}: n’a pas pu tourner", "{}: could not run", o.label),
            _ => t!("{}\u{a0}: ok", "{}: ok", o.label),
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn chips(agent: &str, run: &Run) {
    let n = checks::count(&run.outcomes);
    let ran = run.outcomes.iter().any(|o| matches!(o.status, Status::Ok | Status::Problems));
    let failed = run.outcomes.iter().any(|o| o.status == Status::Failed);
    let title = summary(run);
    let chip = if n > 0 {
        json!([{ "key": if n > 1 { t!("{} problèmes", "{} problems", n) } else { t!("1 problème", "1 problem") }, "title": title, "tone": "err" }])
    } else if ran {
        json!([{ "key": t!("Contrôles OK", "Checks OK"), "title": title, "tone": "ok" }])
    } else if failed {
        json!([{ "key": t!("Contrôles en échec", "Checks failed"), "title": title, "tone": "info" }])
    } else {
        json!([])
    };
    host::set_agent_chips(agent, &chip.to_string());
}

fn post(message: Value) {
    host::panel_post("main", &message.to_string());
}

fn check_agent(agent: &Agent, force: bool) -> Result<Option<Run>, String> {
    if agent.dir.is_empty() {
        return Err(t!("Cet agent n’a pas de dossier.", "This agent has no folder."));
    }
    let w = work(&agent.dir)?;
    if !force && last(&agent.id).is_some_and(|r| r.sig == w.sig) {
        return Ok(None);
    }
    post(json!({ "type": "running", "to": "*", "agent": agent.id }));
    let run = check_all(&w);
    save(&agent.id, &run);
    chips(&agent.id, &run);
    post(json!({ "type": "result", "to": "*", "agent": agent.id, "run": run }));
    Ok(Some(run))
}

fn fix_prompt(run: &Run) -> String {
    let n = checks::count(&run.outcomes);
    format!(
        "{}\n\n{}\n\n{}",
        t!("Les contrôles ont trouvé {} problème(s) dans les fichiers modifiés\u{a0}:", "The checks found {} problem(s) in the changed files:", n),
        checks::report(&run.outcomes),
        t!("Corrige-les, puis relance les contrôles avec l’outil checks_run.", "Fix them, then run the checks again with the checks_run tool.")
    )
}

fn answer(kind: &str, agent: &Agent, message: &Value) -> Result<Value, String> {
    Ok(match kind {
        "ready" => {
            if let Some(lang) = message["lang"].as_str() {
                host::state_set("lang", Some(if lang.starts_with("fr") { "fr" } else { "en" }));
            }
            json!({ "type": "init", "agent": agent.id, "run": last(&agent.id), "auto": option("auto") })
        }
        "run" => match check_agent(agent, true) {
            Ok(_) => json!({ "type": "done" }),
            Err(e) => json!({ "type": "error", "agent": agent.id, "message": e }),
        },
        "fix" => {
            let run = last(&agent.id).ok_or_else(|| t!("Aucun résultat.", "No result."))?;
            host::insert_prompt(&agent.id, &fix_prompt(&run));
            json!({ "type": "inserted" })
        }
        _ => return Err(format!("Unknown request {kind}.")),
    })
}

fn tool_report(run: &Run) -> String {
    if run.files == 0 {
        return "No changed file to check in this worktree.".into();
    }
    let ran: Vec<&str> = run.outcomes.iter().filter(|o| matches!(o.status, Status::Ok | Status::Problems)).map(|o| o.label.as_str()).collect();
    let failed: Vec<String> = run.outcomes.iter().filter(|o| o.status == Status::Failed).map(|o| format!("{} could not run: {}", o.label, o.note)).collect();
    let n = checks::count(&run.outcomes);
    let head = if ran.is_empty() {
        format!("{} changed file(s), but no check applies to them in this project.", run.files)
    } else if n == 0 {
        format!("No problem in the {} changed file(s). Checks run: {}.", run.files, ran.join(", "))
    } else {
        format!("{n} problem(s) in the {} changed file(s):\n\n{}", run.files, checks::report(&run.outcomes))
    };
    if failed.is_empty() { head } else { format!("{head}\n\n{}", failed.join("\n")) }
}

impl Extension for Checks {
    fn on_event(name: String, agent: Option<Agent>, data: String) {
        match name.as_str() {
            "agent.done" if option("auto") != false => {
                if let Some(agent) = agent {
                    let _ = check_agent(&agent, false);
                }
            }
            "panel.message" => {
                let data: Value = serde_json::from_str(&data).unwrap_or_default();
                let message = &data["message"];
                let Some(agent) = agent else { return };
                let kind = message["type"].as_str().unwrap_or_default();
                let mut reply = answer(kind, &agent, message).unwrap_or_else(|e| json!({ "type": "error", "agent": agent.id, "message": e }));
                reply["to"] = message["to"].clone();
                post(reply);
            }
            _ => {}
        }
    }

    fn call_tool(name: String, _input: String, agent: Option<Agent>) -> Result<String, String> {
        let agent = agent.ok_or("Call this tool from an agent started by Squadron.")?;
        match name.as_str() {
            "run" => check_agent(&agent, true).map(|run| tool_report(&run.unwrap_or_default())),
            _ => Err(format!("Unknown tool {name}.")),
        }
    }
}

squadron_extension!(Checks);
