use serde::{Deserialize, Serialize};
use serde_json::Value;

// Enough for a big change, small enough for one command line and one report.
pub const MAX_FILES: usize = 300;
const MAX_PROBLEMS: usize = 200;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Problem {
    pub file: String,
    pub line: Option<u64>,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Status {
    Ok,
    Problems,
    Skipped,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Outcome {
    pub id: String,
    pub label: String,
    pub status: Status,
    pub problems: Vec<Problem>,
    pub note: String,
}

pub struct Check {
    pub id: &'static str,
    pub label: &'static str,
    pub image: Image,
    pub exts: &'static [&'static str],
    pub script: &'static str,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Image {
    Php,
    Composer,
    Node,
}

// Each script exits 99 when the project does not use the tool.
pub const CHECKS: &[Check] = &[
    Check {
        id: "lint",
        label: "php -l",
        image: Image::Php,
        exts: &["php"],
        script: r#"for f; do php -d display_errors=stdout -d error_reporting=E_ALL -l "$f"; done | grep -v '^No syntax errors' || true"#,
    },
    Check {
        id: "phpstan",
        label: "PHPStan",
        image: Image::Php,
        exts: &["php"],
        script: r#"[ -f vendor/bin/phpstan ] || exit 99; [ -f phpstan.neon ] || [ -f phpstan.neon.dist ] || [ -f phpstan.dist.neon ] || exit 99; php -d memory_limit=1G vendor/bin/phpstan analyse --no-progress --no-interaction --error-format=json -- "$@""#,
    },
    Check {
        id: "cs",
        label: "PHP-CS-Fixer",
        image: Image::Php,
        exts: &["php"],
        script: r#"B=vendor/bin/php-cs-fixer; [ -f "$B" ] || B=tools/php-cs-fixer/vendor/bin/php-cs-fixer; [ -f "$B" ] || exit 99; C=.php-cs-fixer.php; [ -f "$C" ] || C=.php-cs-fixer.dist.php; [ -f "$C" ] || exit 99; PHP_CS_FIXER_IGNORE_ENV=1 php "$B" fix -v --dry-run --config="$C" --format=json --using-cache=no --path-mode=intersection --no-interaction -- "$@""#,
    },
    Check {
        id: "yaml",
        label: "YAML",
        image: Image::Php,
        exts: &["yaml", "yml"],
        script: r#"[ -f vendor/bin/yaml-lint ] || exit 99; php vendor/bin/yaml-lint --parse-tags --format=json -- "$@""#,
    },
    Check {
        id: "twig",
        label: "Twig",
        image: Image::Php,
        exts: &["twig"],
        script: r#"[ -f bin/console ] || exit 99; php bin/console lint:twig --format=json --no-interaction -- "$@""#,
    },
    Check {
        id: "composer",
        label: "composer validate",
        image: Image::Composer,
        exts: &["composer.json", "composer.lock"],
        script: r#"[ -f composer.json ] || exit 99; composer validate --no-check-publish --no-interaction 2>&1"#,
    },
    Check {
        id: "eslint",
        label: "ESLint",
        image: Image::Node,
        exts: &["js", "mjs", "cjs", "jsx", "ts", "tsx", "vue"],
        script: r#"[ -f node_modules/eslint/bin/eslint.js ] || exit 99; for c in eslint.config.js eslint.config.mjs eslint.config.cjs eslint.config.ts .eslintrc .eslintrc.js .eslintrc.cjs .eslintrc.json .eslintrc.yml .eslintrc.yaml; do [ -f "$c" ] && found=1; done; [ "$found" ] || exit 99; node node_modules/eslint/bin/eslint.js -f json -- "$@""#,
    },
];

pub fn applies(check: &Check, file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    check.exts.iter().any(|e| if e.contains('.') { name == *e } else { name.rsplit_once('.').is_some_and(|(_, x)| x == *e) })
}

// Changed against the base, plus untracked files, without vendored or built folders.
pub fn changed(diff: &str, untracked: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in diff.lines().chain(untracked.lines()).map(str::trim) {
        let skip = f.is_empty() || ["vendor/", "node_modules/", "var/", "public/build/"].iter().any(|p| f.starts_with(p) || f.contains(&format!("/{p}")));
        if !skip && !out.iter().any(|x| x == f) {
            out.push(f.to_string());
        }
    }
    out.truncate(MAX_FILES);
    out
}

fn major_minor(text: &str) -> Option<String> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let v: String = text[start..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let mut parts = v.split('.').filter(|p| !p.is_empty());
    let (major, minor) = (parts.next()?, parts.next().unwrap_or("0"));
    (major.len() == 1).then(|| format!("{major}.{minor}"))
}

// The PHP the project runs on: Lando's config, then Composer's platform, then its requirement.
pub fn php_version(lando: &str, composer: &str) -> Option<String> {
    for line in lando.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("php:").map(str::trim) {
            if let Some(found) = major_minor(v.trim_matches(['\'', '"'])) {
                return Some(found);
            }
        }
    }
    let c: Value = serde_json::from_str(composer).unwrap_or_default();
    c["config"]["platform"]["php"].as_str().and_then(major_minor).or_else(|| c["require"]["php"].as_str().and_then(major_minor))
}

fn rel(file: &str) -> String {
    file.strip_prefix("/app/").unwrap_or(file).to_string()
}

pub fn parse_lint(out: &str) -> Vec<Problem> {
    out.lines()
        .filter_map(|l| {
            let at = l.find(" error: ")?;
            let rest = &l[at + 8..];
            let (message, place) = rest.rsplit_once(" in ")?;
            let (file, line) = place.rsplit_once(" on line ")?;
            Some(Problem { file: rel(file.trim()), line: line.trim().parse().ok(), message: message.trim().to_string() })
        })
        .collect()
}

fn json_start(out: &str) -> Option<Value> {
    let at = out.find(['{', '['])?;
    serde_json::from_str(&out[at..]).ok().or_else(|| out.lines().rev().find_map(|l| serde_json::from_str(l.trim()).ok()))
}

pub fn parse_phpstan(out: &str) -> Option<Vec<Problem>> {
    let v = json_start(out)?;
    let mut problems = Vec::new();
    for (file, f) in v["files"].as_object()? {
        for m in f["messages"].as_array().into_iter().flatten() {
            problems.push(Problem { file: rel(file), line: m["line"].as_u64(), message: m["message"].as_str().unwrap_or_default().to_string() });
        }
    }
    for e in v["errors"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        problems.push(Problem { file: String::new(), line: None, message: e.to_string() });
    }
    Some(problems)
}

pub fn parse_cs(out: &str) -> Option<Vec<Problem>> {
    let v = json_start(out)?;
    Some(
        v["files"]
            .as_array()?
            .iter()
            .map(|f| {
                let fixers: Vec<&str> = f["appliedFixers"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                let message = if fixers.is_empty() { "needs formatting".to_string() } else { format!("format: {}", fixers.join(", ")) };
                Problem { file: rel(f["name"].as_str().unwrap_or_default()), line: None, message }
            })
            .collect(),
    )
}

// yaml-lint and lint:twig share this JSON shape.
pub fn parse_lint_json(out: &str) -> Option<Vec<Problem>> {
    let v = json_start(out)?;
    Some(
        v.as_array()?
            .iter()
            .filter(|f| f["valid"] == false)
            .map(|f| Problem { file: rel(f["file"].as_str().unwrap_or_default()), line: f["line"].as_u64(), message: f["message"].as_str().unwrap_or_default().trim().to_string() })
            .collect(),
    )
}

pub fn parse_composer(out: &str) -> Vec<Problem> {
    let mut in_errors = false;
    let mut problems = Vec::new();
    for l in out.lines() {
        if let Some(head) = l.strip_prefix('#') {
            in_errors = head.to_lowercase().contains("error");
        } else if let (true, Some(msg)) = (in_errors, l.strip_prefix("- ")) {
            problems.push(Problem { file: "composer.json".into(), line: None, message: msg.trim().to_string() });
        }
    }
    problems
}

// Composer stops on an exception, not its usual list, when composer.json breaks its schema.
fn composer_failure(out: &str) -> Option<Vec<Problem>> {
    let message = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("In ") && !l.starts_with("validate ["))
        .collect::<Vec<_>>()
        .join(" ");
    message.contains("composer.json").then(|| vec![Problem { file: "composer.json".into(), line: None, message }])
}

pub fn parse_eslint(out: &str) -> Option<Vec<Problem>> {
    let v = json_start(out)?;
    let mut problems = Vec::new();
    for f in v.as_array()? {
        for m in f["messages"].as_array().into_iter().flatten().filter(|m| m["severity"].as_u64() == Some(2)) {
            let rule = m["ruleId"].as_str().map(|r| format!(" ({r})")).unwrap_or_default();
            problems.push(Problem { file: rel(f["filePath"].as_str().unwrap_or_default()), line: m["line"].as_u64(), message: format!("{}{rule}", m["message"].as_str().unwrap_or_default()) });
        }
    }
    Some(problems)
}

// None when the output is not what the tool gives once it ran: it failed to start.
pub fn parse(id: &str, code: i32, out: &str) -> Option<Vec<Problem>> {
    let mut problems = match id {
        "lint" => Some(parse_lint(out)),
        "phpstan" => parse_phpstan(out),
        "cs" => parse_cs(out),
        "yaml" | "twig" => parse_lint_json(out),
        "composer" => Some(parse_composer(out)).filter(|p| code == 0 || !p.is_empty()).or_else(|| composer_failure(out)),
        "eslint" => parse_eslint(out),
        _ => None,
    }?;
    problems.truncate(MAX_PROBLEMS);
    Some(problems)
}

pub fn count(outcomes: &[Outcome]) -> usize {
    outcomes.iter().map(|o| o.problems.len()).sum()
}

pub fn report(outcomes: &[Outcome]) -> String {
    outcomes
        .iter()
        .filter(|o| o.status == Status::Problems)
        .map(|o| {
            let lines: Vec<String> = o
                .problems
                .iter()
                .map(|p| match (p.file.is_empty(), p.line) {
                    (true, _) => format!("- {}", p.message),
                    (false, Some(n)) => format!("- {}:{n} — {}", p.file, p.message),
                    (false, None) => format!("- {} — {}", p.file, p.message),
                })
                .collect();
            format!("{}\n{}", o.label, lines.join("\n"))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

// What a failed tool said first, without Docker's download lines.
pub fn first_lines(text: &str) -> String {
    let noise = ["Unable to find image", "Pulling", "Pull complete", "Waiting", "Digest:", "Status:", "Downloading", "Verifying", "Extracting", "Already exists", "Note: Using configuration"];
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty() && !noise.iter().any(|n| l.contains(n)) && !(l.len() > 13 && l[..12].chars().all(|c| c.is_ascii_hexdigit()) && &l[12..13] == ":"))
        .take(8)
        .collect();
    lines.join("\n").chars().take(700).collect()
}

// Lando's PHP image of the project's version has the usual extensions, which Symfony's console needs.
pub fn lando_tag(tags: &str, version: &str) -> Option<String> {
    tags.lines().map(str::trim).find(|t| t.starts_with(&format!("{version}-"))).map(|t| format!("devwithlando/php:{t}"))
}

pub fn hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(id: &str) -> &'static Check {
        CHECKS.iter().find(|c| c.id == id).unwrap()
    }

    #[test]
    fn files_per_check() {
        assert!(applies(check("lint"), "src/A.php"));
        assert!(!applies(check("lint"), "src/php"));
        assert!(applies(check("composer"), "composer.lock"));
        assert!(!applies(check("composer"), "src/composer.json.php"));
        assert!(applies(check("yaml"), "config/services.yaml"));
    }

    #[test]
    fn changed_files_skip_vendor_and_repeats() {
        assert_eq!(changed("src/A.php\nvendor/x/B.php\nsrc/A.php\n", "new.twig\nplugins/p/node_modules/x.js\n"), ["src/A.php", "new.twig"]);
    }

    #[test]
    fn php_version_from_lando_then_composer() {
        assert_eq!(php_version("name: km\nconfig:\n    php: 8.4\n    webroot: public", "").as_deref(), Some("8.4"));
        assert_eq!(php_version("  php: '8.2'", "").as_deref(), Some("8.2"));
        assert_eq!(php_version("", r#"{"require":{"php":"^8.1 || ^8.2"}}"#).as_deref(), Some("8.1"));
        assert_eq!(php_version("services:\n  php:\n    type: php", r#"{"config":{"platform":{"php":"8.3.4"}},"require":{"php":">=7.4"}}"#).as_deref(), Some("8.3"));
        assert_eq!(php_version("", "{}"), None);
    }

    #[test]
    fn php_lint_errors() {
        let out = "PHP Parse error:  syntax error, unexpected token \"}\" in /app/src/A.php on line 12\nErrors parsing /app/src/A.php\n";
        assert_eq!(parse_lint(out), [Problem { file: "src/A.php".into(), line: Some(12), message: "syntax error, unexpected token \"}\"".into() }]);
        assert!(parse_lint("").is_empty());
    }

    #[test]
    fn phpstan_json() {
        let out = r#"Note: Using configuration file /app/phpstan.neon.
{"totals":{"errors":0,"file_errors":1},"files":{"/app/src/A.php":{"errors":1,"messages":[{"message":"Undefined variable: $x","line":7,"ignorable":true}]}},"errors":["Child process error"]}"#;
        let p = parse_phpstan(out).unwrap();
        assert_eq!(p[0], Problem { file: "src/A.php".into(), line: Some(7), message: "Undefined variable: $x".into() });
        assert_eq!(p[1].message, "Child process error");
        assert!(parse_phpstan("Fatal error: Allowed memory size").is_none());
    }

    #[test]
    fn cs_fixer_json() {
        let out = r#"{"about":"PHP CS Fixer","files":[{"name":"src/B.php","appliedFixers":["ordered_imports","no_unused_imports"]}],"time":{"total":0.1}}"#;
        assert_eq!(parse_cs(out).unwrap(), [Problem { file: "src/B.php".into(), line: None, message: "format: ordered_imports, no_unused_imports".into() }]);
        let real = r#"{"about":"PHP CS Fixer 3.64.0","files":[{"name":"src\/X.php","appliedFixers":["no_unused_imports"]},{"name":"src\/Y.php"}],"time":{"total":0.024},"memory":16}"#;
        let p = parse_cs(real).unwrap();
        assert_eq!((p[0].file.as_str(), p[1].message.as_str()), ("src/X.php", "needs formatting"));
    }

    #[test]
    fn yaml_and_twig_json() {
        let out = r#"[{"file":"config/a.yaml","valid":true},{"file":"templates/b.html.twig","line":3,"valid":false,"message":"Unexpected \"}\"."}]"#;
        assert_eq!(parse_lint_json(out).unwrap(), [Problem { file: "templates/b.html.twig".into(), line: Some(3), message: "Unexpected \"}\".".into() }]);
    }

    #[test]
    fn composer_errors_only() {
        let out = "./composer.json is valid but your composer.lock has some errors\n# Lock file errors\n- The lock file is not up to date with the latest changes in composer.json.\n# General warnings\n- No license specified.\n";
        assert_eq!(parse_composer(out).len(), 1);
        assert_eq!(parse("composer", 0, "./composer.json is valid\n"), Some(vec![]));
        assert_eq!(parse("composer", 1, "Could not open input file"), None);
        let schema = "\nIn Factory.php line 319:\n  \"./composer.json\" does not match the expected JSON schema:  \n   - name : Does not match the regex pattern  \n\nvalidate [--no-check-all] [--strict] [--] [<file>]\n";
        let p = parse("composer", 1, schema).unwrap();
        assert_eq!(p[0].message, "\"./composer.json\" does not match the expected JSON schema: - name : Does not match the regex pattern");
    }

    #[test]
    fn eslint_errors_only() {
        let out = r#"[{"filePath":"/app/assets/a.js","messages":[{"ruleId":"no-undef","severity":2,"message":"'x' is not defined.","line":4},{"ruleId":"semi","severity":1,"message":"Missing semicolon.","line":5}]}]"#;
        assert_eq!(parse_eslint(out).unwrap(), [Problem { file: "assets/a.js".into(), line: Some(4), message: "'x' is not defined. (no-undef)".into() }]);
    }

    #[test]
    fn failure_text_without_docker_noise() {
        let err = "Unable to find image 'php:8.4-cli' locally\n40748add9f4f: Pulling fs layer\nDigest: sha256:f1\nNote: Using configuration file /app/phpstan.neon.\nNo rules detected\nYou have the following choices:\n";
        assert_eq!(first_lines(err), "No rules detected\nYou have the following choices:");
    }

    #[test]
    fn lando_image_of_the_version() {
        assert_eq!(lando_tag("8.4-fpm-7\n8.4-apache-7\n8.2-apache-7\n", "8.2").as_deref(), Some("devwithlando/php:8.2-apache-7"));
        assert_eq!(lando_tag("8.4-fpm-7\n", "8.1"), None);
    }

    #[test]
    fn report_lists_problems_by_tool() {
        let o = Outcome { id: "lint".into(), label: "php -l".into(), status: Status::Problems, problems: vec![Problem { file: "a.php".into(), line: Some(2), message: "boom".into() }], note: String::new() };
        let ok = Outcome { id: "cs".into(), label: "PHP-CS-Fixer".into(), status: Status::Ok, problems: vec![], note: String::new() };
        assert_eq!(report(&[o.clone(), ok]), "php -l\n- a.php:2 — boom");
        assert_eq!(count(&[o]), 1);
        assert_ne!(hash("a"), hash("b"));
    }
}
