use crate::Result;
use regex::Regex;
use serde_json::{Value, json};
use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    process::Command,
};

trait Api {
    fn call(&mut self, endpoint: &str, body: Option<Value>) -> Result<Value>;
}
struct Gh;
impl Api for Gh {
    fn call(&mut self, endpoint: &str, body: Option<Value>) -> Result<Value> {
        let mut cmd = Command::new("gh");
        cmd.args(["api", endpoint]);
        if let Some(body) = body {
            cmd.args(["--method", "PUT", "--input", "-"])
                .stdin(std::process::Stdio::piped());
            let mut child = cmd
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?;
            child
                .stdin
                .take()
                .ok_or("gh stdin unavailable")?
                .write_all(serde_json::to_string(&body)?.as_bytes())?;
            let out = child.wait_with_output()?;
            if !out.status.success() {
                let message = String::from_utf8_lossy(&out.stderr);
                if ["403", "405", "409", "422"]
                    .iter()
                    .any(|code| message.contains(&format!("HTTP {code}")))
                {
                    return Ok(json!({"merged":false}));
                }
                return Err("GitHub merge request failed".into());
            }
            return Ok(serde_json::from_slice(&out.stdout)?);
        }
        let out = cmd.output()?;
        if !out.status.success() {
            return Err(format!("GitHub read failed: {endpoint}").into());
        }
        Ok(serde_json::from_slice(&out.stdout)?)
    }
}
fn list(api: &mut impl Api, endpoint: &str, key: Option<&str>) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    for page in 1.. {
        let separator = if endpoint.contains('?') { '&' } else { '?' };
        let data = api.call(
            &format!("{endpoint}{separator}per_page=100&page={page}"),
            None,
        )?;
        let batch = key
            .map_or(&data, |key| &data[key])
            .as_array()
            .ok_or("GitHub list missing")?;
        result.extend(batch.iter().cloned());
        if batch.len() < 100 {
            break;
        }
    }
    Ok(result)
}
fn changed(file: &Value) -> Vec<&str> {
    file["patch"]
        .as_str()
        .unwrap_or("")
        .lines()
        .filter(|line| {
            (line.starts_with('+') || line.starts_with('-'))
                && !line.starts_with("+++")
                && !line.starts_with("---")
        })
        .collect()
}
fn version_only(file: &Value) -> bool {
    if file["status"] != "modified" {
        return false;
    }
    let lines = changed(file);
    let before = lines
        .iter()
        .filter(|s| s.starts_with('-'))
        .map(|s| &s[1..])
        .collect::<Vec<_>>();
    let after = lines
        .iter()
        .filter(|s| s.starts_with('+'))
        .map(|s| &s[1..])
        .collect::<Vec<_>>();
    let valid = Regex::new(r"^\s*(?:-\s*)?uses:\s*[^\s@]+@[A-Za-z0-9._/-]+(?:\s*#.*)?$").unwrap();
    let version = Regex::new(r"(uses:\s*[^\s@]+@)[A-Za-z0-9._/-]+").unwrap();
    let comment = Regex::new(r"\s+#.*$").unwrap();
    !before.is_empty()
        && before.len() == after.len()
        && before.iter().zip(after).all(|(a, b)| {
            valid.is_match(a)
                && valid.is_match(b)
                && comment.replace(&version.replace_all(a, "${1}<VERSION>"), "")
                    == comment.replace(&version.replace_all(b, "${1}<VERSION>"), "")
        })
}
fn dependency(
    api: &mut impl Api,
    prefix: &str,
    file: &Value,
    base: &str,
    head: &str,
) -> Result<bool> {
    let name = file["filename"].as_str().unwrap_or("");
    if Regex::new(r"^\.github/workflows/[^/]+\.ya?ml$")?.is_match(name) {
        return Ok(version_only(file));
    }
    if Regex::new(r"(^|/)Dockerfile(?:\.[^/]+)?$|(^|/)(?:docker-)?compose(?:\.[^/]+)?\.ya?ml$")?
        .is_match(name)
    {
        let lines = changed(file);
        let pattern = Regex::new(r"(?i)^[+-]\s*(?:FROM\s|image:\s)")?;
        return Ok(file["status"] == "modified"
            && !lines.is_empty()
            && lines.iter().all(|line| pattern.is_match(line)));
    }
    if Regex::new(r"(^|/)(?:Package\.swift|Package\.resolved|vcpkg\.json|vcpkg-configuration\.json|rust-toolchain(?:\.toml)?|Directory\.Build\.props|package\.json|package-lock\.json|npm-shrinkwrap\.json|pnpm-lock\.yaml|pnpm-workspace\.yaml|yarn\.lock|bun\.lockb?|Cargo\.toml|Cargo\.lock|Gemfile|Gemfile\.lock|pubspec\.yaml|pubspec\.lock|global\.json|packages\.config|packages\.lock\.json|Directory\.Packages\.props|requirements[^/]*\.txt|setup\.py|setup\.cfg|pyproject\.toml|poetry\.lock|uv\.lock|Pipfile(?:\.lock)?|build\.gradle(?:\.kts)?|settings\.gradle(?:\.kts)?|gradle\.properties|libs\.versions\.toml|gradle-wrapper\.properties|gradle\.lockfile|[^/]+\.lockfile|\.terraform\.lock\.hcl)$|\.(?:csproj|fsproj|vbproj|tf)$")?.is_match(name){return Ok(true);}
    let lines = changed(file);
    if file["status"] != "modified" || lines.len() != 2 {
        return Ok(false);
    }
    let old = lines
        .iter()
        .find_map(|line| line.strip_prefix("-Subproject commit "));
    let new = lines
        .iter()
        .find_map(|line| line.strip_prefix("+Subproject commit "));
    let sha = Regex::new(r"^[0-9a-f]{40}$")?;
    let (Some(old), Some(new)) = (old, new) else {
        return Ok(false);
    };
    if !sha.is_match(old) || !sha.is_match(new) {
        return Ok(false);
    }
    // 不明なsubmoduleや取得失敗は安全側で保留する。
    let Ok(a) = api.call(&format!("{prefix}/contents/{name}?ref={base}"), None) else {
        return Ok(false);
    };
    let Ok(b) = api.call(&format!("{prefix}/contents/{name}?ref={head}"), None) else {
        return Ok(false);
    };
    Ok(a["submodule_git_url"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
        && a["submodule_git_url"] == b["submodule_git_url"]
        && a["sha"] == old
        && b["sha"] == new
        && file["sha"] == b["sha"])
}
fn candidate(pr: &Value, repository: &Value, branch: &str) -> bool {
    pr["state"] == "open"
        && pr["draft"] == false
        && pr["user"]["login"] == "dependabot[bot]"
        && pr["user"]["id"] == 49699333u64
        && pr["head"]["repo"]["id"] == repository["id"]
        && pr["base"]["ref"] == branch
}

fn process(
    api: &mut impl Api,
    repo: &str,
    run: &str,
    required: &[String],
    event: &str,
    payload: &Value,
) -> Result<Vec<String>> {
    if required.is_empty() {
        return Ok(vec!["検証workflowが未設定のため保留".into()]);
    }
    let prefix = format!("repos/{repo}");
    let repository = api.call(&prefix, None)?;
    let branch = repository["default_branch"]
        .as_str()
        .ok_or("default branch missing")?;
    let owner = repo.split('/').next().ok_or("owner missing")?;
    let prs = if event == "pull_request_target" {
        vec![payload["pull_request"].clone()]
    } else {
        list(
            api,
            &format!("{prefix}/pulls?state=open&base={branch}"),
            None,
        )?
    };
    let mut workflows = Vec::new();
    for path in required {
        workflows.push(api.call(
            &format!(
                "{prefix}/actions/workflows/{}",
                path.rsplit('/').next().unwrap()
            ),
            None,
        )?);
    }
    let mut report = Vec::new();
    for item in prs {
        if !candidate(&item, &repository, branch) {
            continue;
        }
        let number = item["number"].as_u64().ok_or("PR number missing")?;
        let reason = verify_merge(
            api,
            &prefix,
            &repository,
            branch,
            owner,
            number,
            run,
            &workflows,
        )?;
        report.push(format!("- #{number}: {reason}"));
    }
    Ok(report)
}

#[allow(clippy::too_many_arguments)]
fn verify_merge(
    api: &mut impl Api,
    prefix: &str,
    repository: &Value,
    branch: &str,
    owner: &str,
    number: u64,
    run: &str,
    workflows: &[Value],
) -> Result<String> {
    let prpath = format!("{prefix}/pulls/{number}");
    let pr = api.call(&prpath, None)?;
    if !candidate(&pr, repository, branch) {
        return Ok("対象条件が変化したため保留".into());
    }
    let sha = pr["head"]["sha"].as_str().ok_or("head missing")?;
    let basepath = format!("{prefix}/branches/{branch}");
    let base = api.call(&basepath, None)?;
    let base_sha = base["commit"]["sha"].as_str().ok_or("base missing")?;
    let files = list(api, &format!("{prpath}/files"), None)?;
    if files.is_empty() || Some(files.len() as u64) != pr["changed_files"].as_u64() {
        return Ok("変更ファイルの取得範囲が不完全なため保留".into());
    }
    for file in &files {
        if !dependency(api, prefix, file, base_sha, sha)? {
            return Ok("依存更新以外または自動確認できない差分があるため保留".into());
        }
    }
    if files.iter().any(|f| {
        Regex::new(r"(^|/)Cargo\.(toml|lock)$")
            .unwrap()
            .is_match(f["filename"].as_str().unwrap_or(""))
    }) {
        return Ok("Rust依存はWindows/macOS配布物の更新が必要なため手動対応".into());
    }
    let commits = list(api, &format!("{prpath}/commits"), None)?;
    if commits.is_empty()
        || Some(commits.len() as u64) != pr["commits"].as_u64()
        || commits.iter().any(|c| {
            c["commit"]["verification"]["verified"] != true
                || (c["author"]["id"] != 49699333u64
                    && !(c["parents"].as_array().is_some_and(|p| p.len() == 2)
                        && ["github-actions[bot]", owner]
                            .contains(&c["author"]["login"].as_str().unwrap_or(""))))
        })
    {
        return Ok("署名またはコミット作成者を確認できないため保留".into());
    }
    if pr["mergeable"] == false || pr["mergeable_state"] == "dirty" {
        return Ok("競合があるため保留".into());
    }
    for workflow in workflows {
        let runs = list(
            api,
            &format!(
                "{prefix}/actions/workflows/{}/runs?head_sha={sha}",
                workflow["id"]
            ),
            Some("workflow_runs"),
        )?;
        let latest = runs
            .iter()
            .filter(|r| {
                ["pull_request", "workflow_dispatch"].contains(&r["event"].as_str().unwrap_or(""))
            })
            .max_by_key(|r| (r["id"].as_u64(), r["run_attempt"].as_u64()));
        if latest.is_none_or(|r| r["status"] != "completed" || r["conclusion"] != "success") {
            return Ok("当該headの最新CIが成功していないため保留".into());
        }
    }
    let checks = list(
        api,
        &format!("{prefix}/commits/{sha}/check-runs?filter=latest"),
        Some("check_runs"),
    )?;
    let self_run = format!("/actions/runs/{run}");
    if checks
        .iter()
        .filter(|c| !c["details_url"].as_str().unwrap_or("").contains(&self_run))
        .any(|c| {
            c["status"] != "completed"
                || !["success", "neutral", "skipped"]
                    .contains(&c["conclusion"].as_str().unwrap_or(""))
        })
    {
        return Ok("他のcheckが未完了または失敗しているため保留".into());
    }
    let statuses = api.call(&format!("{prefix}/commits/{sha}/status?per_page=100"), None)?;
    if statuses["total_count"]
        .as_u64()
        .ok_or("commit status count missing")?
        > 0
        && statuses["state"] != "success"
    {
        return Ok("commit statusが成功していないため保留".into());
    }
    let current_base = api.call(&basepath, None)?;
    let current = api.call(&prpath, None)?;
    if !candidate(&current, repository, branch)
        || current["head"]["sha"] != sha
        || current_base["commit"]["sha"] != base_sha
        || current["mergeable"] != true
    {
        return Ok("head/baseまたはマージ可能状態が変化したため再検証を待機".into());
    }
    let method = if repository["allow_squash_merge"] == true {
        "squash"
    } else if repository["allow_merge_commit"] == true {
        "merge"
    } else {
        "rebase"
    };
    let result = api.call(
        &format!("{prpath}/merge"),
        Some(json!({"sha":sha,"merge_method":method})),
    )?;
    Ok(if result["merged"] == true {
        format!(
            "CI成功後にマージ済み ({})",
            result["sha"].as_str().unwrap_or("")
        )
    } else {
        "GitHubのマージ要件を満たすまで保留".into()
    })
}

pub fn execute() -> Result {
    let repo = env::var("GITHUB_REPOSITORY")?;
    if !repo.starts_with("1llum1n4t1s/") {
        return Err("repository owner is not authorized".into());
    }
    let event = env::var("GITHUB_EVENT_NAME")?;
    let payload: Value = serde_json::from_slice(&fs::read(env::var("GITHUB_EVENT_PATH")?)?)?;
    if (event == "pull_request_target" && payload["pull_request"]["user"]["id"] != 49699333u64)
        || (event == "status" && payload["state"] != "success")
        || (event == "check_run" && payload["check_run"]["app"]["slug"] == "github-actions")
    {
        return Ok(());
    }
    let required: Vec<String> = serde_json::from_str(&env::var("REQUIRED_WORKFLOWS")?)?;
    let report = process(
        &mut Gh,
        &repo,
        &env::var("GITHUB_RUN_ID")?,
        &required,
        &event,
        &payload,
    )?;
    let text = if report.is_empty() {
        "対象PRなし".into()
    } else {
        report.join("\n")
    };
    println!("{text}");
    if let Some(path) = env::var_os("GITHUB_STEP_SUMMARY") {
        writeln!(
            OpenOptions::new().append(true).create(true).open(path)?,
            "# Dependabot auto-merge\n\n{text}"
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Mock {
        responses: std::collections::VecDeque<Value>,
        writes: usize,
    }
    impl Api for Mock {
        fn call(&mut self, _: &str, body: Option<Value>) -> Result<Value> {
            if body.is_some() {
                self.writes += 1;
            }
            self.responses
                .pop_front()
                .ok_or_else(|| "unexpected request".into())
        }
    }
    fn pr() -> Value {
        json!({"number":1,"state":"open","draft":false,"user":{"id":49699333u64,"login":"dependabot[bot]"},"head":{"sha":"head","repo":{"id":1}},"base":{"ref":"main"},"changed_files":1,"commits":1,"mergeable":true})
    }
    fn repository() -> Value {
        json!({"id":1,"default_branch":"main","allow_squash_merge":true})
    }
    fn fixtures() -> Vec<Value> {
        vec![
            pr(),
            json!({"commit":{"sha":"base"}}),
            json!([{"filename":".github/workflows/test.yml","status":"modified","patch":"- uses: actions/checkout@old\n+ uses: actions/checkout@new # update"}]),
            json!([{"commit":{"verification":{"verified":true}},"author":{"id":49699333u64},"parents":[]}]),
            json!({"workflow_runs":[{"id":2,"run_attempt":1,"event":"pull_request","status":"completed","conclusion":"success"}]}),
            json!({"check_runs":[]}),
            json!({"total_count":0,"state":"pending"}),
            json!({"commit":{"sha":"base"}}),
            pr(),
            json!({"merged":true,"sha":"merged"}),
        ]
    }
    fn verify(values: Vec<Value>) -> (String, usize) {
        let mut mock = Mock {
            responses: values.into(),
            writes: 0,
        };
        let result = verify_merge(
            &mut mock,
            "repos/owner/repo",
            &repository(),
            "main",
            "owner",
            1,
            "123",
            &[json!({"id":2})],
        )
        .unwrap();
        (result, mock.writes)
    }
    #[test]
    fn trusted_dependency_update_merges() {
        let (r, w) = verify(fixtures());
        assert!(r.contains("マージ済み"));
        assert_eq!(w, 1);
    }
    #[test]
    fn guards_prevent_mutation() {
        for index in 0..23 {
            let mut values = fixtures();
            match index {
                0 => values[0]["user"]["id"] = json!(1),
                1 => values[2] = json!([{"filename":"src/main.rs"}]),
                2 => values[2][0]["filename"] = json!("Cargo.lock"),
                3 => values[3][0]["commit"]["verification"]["verified"] = json!(false),
                4 => values[4]["workflow_runs"][0]["conclusion"] = json!("failure"),
                5 => values[5]["check_runs"] = json!([{"status":"in_progress"}]),
                6 => values[6] = json!({"total_count":1,"state":"failure"}),
                7 => values[7]["commit"]["sha"] = json!("changed"),
                8 => values[8]["head"]["sha"] = json!("changed"),
                9 => values[0]["head"]["repo"]["id"] = json!(2),
                10 => values[0]["base"]["ref"] = json!("other"),
                11 => values[0]["draft"] = json!(true),
                12 => values[0]["changed_files"] = json!(2),
                13 => values[0]["commits"] = json!(2),
                14 => values[3][0]["author"]["id"] = json!(2),
                15 => values[4]["workflow_runs"] = json!([]),
                16 => values[4]["workflow_runs"].as_array_mut().unwrap().push(json!({"id":3,"run_attempt":1,"event":"pull_request","status":"completed","conclusion":"failure"})),
                17 => values[4]["workflow_runs"][0]["status"] = json!("in_progress"),
                18 => values[6] = json!({"total_count":1,"state":"pending"}),
                19 => values[8]["state"] = json!("closed"),
                20 => values[8]["mergeable"] = Value::Null,
                21 => values[0]["mergeable"] = json!(false),
                22 => values[2][0]["patch"] = Value::Null,
                _ => unreachable!(),
            }
            let (_, writes) = verify(values);
            assert_eq!(writes, 0, "guard {index}");
        }
    }
    #[test]
    fn workflow_patch_cannot_run_code() {
        assert!(!version_only(
            &json!({"status":"modified","patch":"- uses: actions/checkout@old\n+ run: malicious"})
        ));
        assert!(!version_only(
            &json!({"status":"modified","patch":"- uses: owner/good@old\n+ uses: owner/evil@new"})
        ));
    }
}
