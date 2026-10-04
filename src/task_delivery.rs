use crate::tasks::TaskRecord;
use chrono::{SecondsFormat, Utc};
use git2::{Repository, StatusOptions};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

const PULL_REQUEST_FIELDS: &str =
    "number,url,title,state,isDraft,mergedAt,headRefOid,headRefName,baseRefName,statusCheckRollup";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullRequestAction {
    Refresh,
    Associate,
    Create,
}

impl PullRequestAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Refresh => "Refreshing PR status",
            Self::Associate => "Associating PR",
            Self::Create => "Creating PR",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    NoChecks,
    Pending,
    Failed,
    Passed,
}

impl CheckState {
    pub fn label(self) -> &'static str {
        match self {
            Self::NoChecks => "No checks",
            Self::Pending => "Pending",
            Self::Failed => "Failed",
            Self::Passed => "Passed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestCheck {
    pub name: String,
    pub state: CheckState,
    pub details_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TaskSourceFreshness {
    Current { local_head_sha: String },
    Stale { local_head_sha: Option<String>, reason: String },
    Unavailable { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestSnapshot {
    pub number: u64,
    pub url: String,
    pub title: String,
    pub state: String,
    pub is_draft: bool,
    pub merged_at: Option<String>,
    pub head_sha: String,
    pub head_branch: String,
    pub base_branch: String,
    pub checks: Vec<PullRequestCheck>,
    pub check_state: CheckState,
    pub freshness: TaskSourceFreshness,
    pub source_fingerprint_at_fetch: Option<String>,
    pub fetched_at: String,
}

impl PullRequestSnapshot {
    pub fn is_merged(&self) -> bool {
        self.merged_at.is_some() || self.state.eq_ignore_ascii_case("MERGED")
    }
}

#[derive(Debug)]
pub struct PullRequestActionError {
    pub message: String,
    pub created_url: Option<String>,
}

impl From<String> for PullRequestActionError {
    fn from(message: String) -> Self {
        Self {
            message,
            created_url: None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum PullRequestRemoteView {
    Unlinked,
    Checking { action: &'static str },
    Refreshing {
        previous: Option<PullRequestSnapshot>,
        previous_at: Option<String>,
    },
    Unavailable {
        message: String,
        previous: Option<PullRequestSnapshot>,
        previous_at: Option<String>,
    },
    Ready {
        snapshot: PullRequestSnapshot,
    },
}

#[derive(Clone, Debug)]
pub struct PullRequestActionMessage {
    pub succeeded: bool,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct PullRequestStatusView {
    pub remote: PullRequestRemoteView,
    pub action_message: Option<PullRequestActionMessage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryInfo {
    name_with_owner: String,
    url: String,
    default_branch_ref: Option<BranchRef>,
}

#[derive(Deserialize)]
struct BranchRef {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestData {
    number: u64,
    url: String,
    title: String,
    state: String,
    is_draft: bool,
    merged_at: Option<String>,
    head_ref_oid: String,
    head_ref_name: String,
    base_ref_name: String,
}

pub fn perform_action(
    task: &TaskRecord,
    action: PullRequestAction,
    identifier: Option<&str>,
) -> Result<PullRequestSnapshot, PullRequestActionError> {
    match action {
        PullRequestAction::Refresh => {
            let url = task
                .pull_request_url
                .as_deref()
                .ok_or_else(|| "This task has no associated pull request".to_string())?;
            fetch_pull_request(task, url).map_err(Into::into)
        }
        PullRequestAction::Associate => {
            let identifier = identifier
                .ok_or_else(|| "Enter a pull request number or HTTPS URL".to_string())?;
            let identifier = validate_identifier(identifier)?;
            fetch_pull_request(task, &identifier).map_err(Into::into)
        }
        PullRequestAction::Create => create_pull_request(task),
    }
}

fn create_pull_request(task: &TaskRecord) -> Result<PullRequestSnapshot, PullRequestActionError> {
    let branch = task
        .branch
        .as_deref()
        .filter(|branch| !branch.trim().is_empty())
        .ok_or_else(|| "This task has no saved branch. Associate an existing PR instead.".to_string())?;
    ensure_task_branch_matches(task, branch)?;
    let repository = gh_json(
        &task.repository_path,
        &[
            "repo",
            "view",
            "--json",
            "nameWithOwner,url,defaultBranchRef",
        ],
        "Could not read the GitHub repository",
    )?;
    let repository: RepositoryInfo = serde_json::from_value(repository)
        .map_err(|error| format!("GitHub CLI returned incomplete repository information: {error}"))?;
    let base_branch = repository
        .default_branch_ref
        .map(|branch| branch.name)
        .filter(|branch| !branch.is_empty())
        .ok_or_else(|| "Could not determine the GitHub repository's default branch".to_string())?;
    let head = ensure_branch_is_published(&task.worktree_path, branch, &repository.url)?;

    let body = format!("Created from Git Manager task `{}`.", task.id);
    let output = run_gh(
        &task.repository_path,
        &[
            "pr",
            "create",
            "--repo",
            &repository.name_with_owner,
            "--head",
            &head,
            "--base",
            &base_branch,
            "--title",
            &task.title,
            "--body",
            &body,
        ],
        "Could not create the pull request",
    )?;
    let output_text = String::from_utf8_lossy(&output.stdout);
    let url = output_text
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("https://"))
        .map(str::to_owned);
    match url {
        Some(url) => fetch_pull_request(task, &url).map_err(|error| PullRequestActionError {
            message: format!("The PR was created, but GitHub CLI could not read it back: {error}"),
            created_url: Some(url),
        }),
        None => fetch_pull_request(task, &head).map_err(|error| PullRequestActionError {
            message: format!(
                "The PR may have been created, but GitHub CLI could not read it back: {error}. Git Manager could not capture its URL; use Associate PR with its number or URL if creation completed."
            ),
            created_url: None,
        }),
    }
}

fn ensure_task_branch_matches(task: &TaskRecord, saved_branch: &str) -> Result<(), String> {
    let repo = Repository::open(&task.worktree_path)
        .map_err(|_| "The task worktree is unavailable; an existing PR can still be associated".to_string())?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| "The task repository has no working directory".to_string())?;
    let workdir = std::fs::canonicalize(workdir)
        .map_err(|error| format!("Could not resolve the task worktree: {error}"))?;
    let requested_path = std::fs::canonicalize(&task.worktree_path)
        .map_err(|error| format!("Could not resolve the task worktree path: {error}"))?;
    if requested_path != workdir {
        return Err("The task path no longer points to its own Git worktree".into());
    }
    let repository_root = crate::tasks::repository_root(&repo, &workdir)?;
    let expected_repository_path = std::fs::canonicalize(&task.repository_path)
        .map_err(|error| format!("Could not resolve the task's saved repository path: {error}"))?;
    if repository_root != expected_repository_path {
        return Err("The task worktree is no longer linked to its saved repository".into());
    }
    let current_branch = repo
        .head()
        .ok()
        .and_then(|head| head.shorthand().map(str::to_owned));
    if current_branch.as_deref() != Some(saved_branch) {
        return Err(format!(
            "The task worktree is not on its saved branch `{saved_branch}`. Switch it back or associate an existing PR."
        ));
    }
    Ok(())
}

fn ensure_branch_is_published(
    worktree_path: &str,
    branch: &str,
    base_repository_url: &str,
) -> Result<String, String> {
    let base_repository = parse_repository_url(base_repository_url)
        .ok_or_else(|| "Could not identify the GitHub repository host".to_string())?;
    let local_head_output = Command::new("git")
        .current_dir(worktree_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|_| "Could not read the task branch's local commit".to_string())?;
    if !local_head_output.status.success() {
        return Err("Could not read the task branch's local commit".into());
    }
    let local_head = String::from_utf8_lossy(&local_head_output.stdout)
        .trim()
        .to_owned();
    let remote_output = Command::new("git")
        .current_dir(worktree_path)
        .args(["remote"])
        .output()
        .map_err(|_| "Could not list Git remotes to verify the published task branch".to_string())?;
    if !remote_output.status.success() {
        return Err("Could not list Git remotes to verify the published task branch".into());
    }
    let mut remotes = String::from_utf8_lossy(&remote_output.stdout)
        .lines()
        .map(str::trim)
        .filter(|remote| !remote.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if let Some(upstream) = configured_branch_remote(worktree_path, branch) {
        if let Some(index) = remotes.iter().position(|remote| remote == &upstream) {
            remotes.swap(0, index);
        }
    }
    if remotes.is_empty() {
        return Err("No Git remotes are configured; publish the task branch before creating a PR.".into());
    }

    let mut could_not_check_remote = false;
    let mut published_on_different_host = false;
    let mut published_on_unsupported_remote = false;
    let mut published_with_different_head = false;
    for remote in remotes {
        let output = Command::new("git")
            .current_dir(worktree_path)
            .args(["ls-remote", "--exit-code", "--heads", &remote])
            .arg(format!("refs/heads/{branch}"))
            .output()
            .map_err(|_| "Could not run Git to check whether the task branch is published".to_string())?;
        match output.status.code() {
            Some(0) => {
                let remote_head = String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .next()
                    .map(str::to_owned);
                if remote_head.as_deref() != Some(local_head.as_str()) {
                    published_with_different_head = true;
                    continue;
                }
                let url_output = Command::new("git")
                    .current_dir(worktree_path)
                    .args(["remote", "get-url", &remote])
                    .output()
                    .map_err(|_| "Could not read the published branch's remote URL".to_string())?;
                if !url_output.status.success() {
                    return Err("Could not read the published branch's remote URL".into());
                }
                let url = String::from_utf8_lossy(&url_output.stdout);
                let Some(head_repository) = parse_repository_url(url.trim()) else {
                    published_on_unsupported_remote = true;
                    continue;
                };
                if !head_repository.host.eq_ignore_ascii_case(&base_repository.host) {
                    published_on_different_host = true;
                    continue;
                }
                if head_repository.owner.eq_ignore_ascii_case(&base_repository.owner)
                    && head_repository.name.eq_ignore_ascii_case(&base_repository.name)
                {
                    return Ok(branch.to_owned());
                }
                return Ok(format!("{}:{branch}", head_repository.owner));
            }
            Some(2) => {}
            _ => could_not_check_remote = true,
        }
    }

    if could_not_check_remote {
        return Err("Could not verify the task branch on all configured remotes. Check the repository's Git remote access.".into());
    }
    if published_with_different_head {
        return Err(format!(
            "The task branch's local commit does not match any published remote branch. Publish or synchronize `{branch}` before creating a PR; Git Manager does not push branches."
        ));
    }
    if published_on_different_host {
        return Err("The task branch is published, but not on the GitHub host used by this repository. Select a matching GitHub remote before creating a PR.".into());
    }
    if published_on_unsupported_remote {
        return Err("The task branch is published, but no matching GitHub remote is configured. Select a GitHub remote for the repository before creating a PR.".into());
    }
    Err(format!(
        "Task branch `{branch}` is not published on a configured remote. Publish it before creating a PR; Git Manager does not push branches."
    ))
}

fn configured_branch_remote(worktree_path: &str, branch: &str) -> Option<String> {
    [
        format!("branch.{branch}.pushRemote"),
        "remote.pushDefault".to_owned(),
        format!("branch.{branch}.remote"),
    ]
    .into_iter()
    .find_map(|key| {
        let output = Command::new("git")
            .current_dir(worktree_path)
            .args(["config", "--get", &key])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let remote = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        (!remote.is_empty() && remote != ".").then_some(remote)
    })
}

struct ParsedRepositoryUrl {
    host: String,
    owner: String,
    name: String,
}

fn parse_repository_url(url: &str) -> Option<ParsedRepositoryUrl> {
    let (host, path) = if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ssh://"))
        .or_else(|| url.strip_prefix("git://"))
    {
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit('@').next()?.split(':').next()?;
        (host, path)
    } else if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        (host, path)
    } else {
        return None;
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let owner = parts.next()?.trim();
    let name = parts.next()?.trim();
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(ParsedRepositoryUrl {
        host: host.to_ascii_lowercase(),
        owner: owner.to_owned(),
        name: name.to_owned(),
    })
}

fn fetch_pull_request(task: &TaskRecord, identifier: &str) -> Result<PullRequestSnapshot, String> {
    let output = gh_json(
        &task.repository_path,
        &[
            "pr",
            "view",
            identifier,
            "--json",
            PULL_REQUEST_FIELDS,
        ],
        "Could not read the pull request",
    )?;
    parse_pull_request(task, output)
}

fn parse_pull_request(task: &TaskRecord, value: Value) -> Result<PullRequestSnapshot, String> {
    let rollup = value.get("statusCheckRollup").ok_or_else(|| {
        "GitHub CLI did not return remote check status. Update GitHub CLI and refresh again.".to_string()
    })?;
    let check_values = match rollup {
        Value::Null => Vec::new(),
        Value::Array(values) => values.clone(),
        _ => return Err("GitHub CLI returned an invalid remote check status".into()),
    };
    let data: PullRequestData = serde_json::from_value(value)
        .map_err(|error| format!("GitHub CLI returned incomplete pull request data: {error}"))?;
    if !data.url.starts_with("https://") || data.head_ref_oid.is_empty() {
        return Err("GitHub CLI returned an invalid pull request URL or head commit".into());
    }

    let checks = check_values
        .into_iter()
        .map(parse_check)
        .collect::<Vec<_>>();
    let check_state = aggregate_checks(&checks);
    let freshness = task_source_freshness(task, &data.head_ref_oid);

    Ok(PullRequestSnapshot {
        number: data.number,
        url: data.url,
        title: data.title,
        state: data.state,
        is_draft: data.is_draft,
        merged_at: data.merged_at,
        head_sha: data.head_ref_oid,
        head_branch: data.head_ref_name,
        base_branch: data.base_ref_name,
        checks,
        check_state,
        freshness,
        source_fingerprint_at_fetch: None,
        fetched_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
    })
}

pub fn refresh_snapshot_freshness(
    snapshot: &PullRequestSnapshot,
    current_source_fingerprint: Option<&str>,
) -> PullRequestSnapshot {
    let mut snapshot = snapshot.clone();
    let Some(fetched_fingerprint) = snapshot.source_fingerprint_at_fetch.as_deref() else {
        snapshot.freshness = TaskSourceFreshness::Unavailable {
            reason: "the task source snapshot was unavailable when GitHub was checked".into(),
        };
        return snapshot;
    };
    let Some(current_fingerprint) = current_source_fingerprint else {
        snapshot.freshness = TaskSourceFreshness::Unavailable {
            reason: "the current task source snapshot is unavailable".into(),
        };
        return snapshot;
    };
    if fetched_fingerprint != current_fingerprint {
        let local_head_sha = match &snapshot.freshness {
            TaskSourceFreshness::Current { local_head_sha } => Some(local_head_sha.clone()),
            TaskSourceFreshness::Stale { local_head_sha, .. } => local_head_sha.clone(),
            TaskSourceFreshness::Unavailable { .. } => None,
        };
        snapshot.freshness = TaskSourceFreshness::Stale {
            local_head_sha,
            reason: "the task source changed after the remote result was fetched".into(),
        };
    }
    snapshot
}

fn parse_check(value: Value) -> PullRequestCheck {
    let name = value
        .get("name")
        .or_else(|| value.get("context"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("Unnamed check")
        .to_string();
    let kind = value.get("__typename").and_then(Value::as_str).unwrap_or("");
    let state = if kind == "CheckRun" {
        match value.get("status").and_then(Value::as_str).unwrap_or("") {
            "COMPLETED" => match value
                .get("conclusion")
                .and_then(Value::as_str)
                .unwrap_or("")
            {
                "SUCCESS" | "NEUTRAL" | "SKIPPED" => CheckState::Passed,
                "FAILURE" | "STARTUP_FAILURE" | "STALE" | "CANCELLED" | "TIMED_OUT"
                | "ACTION_REQUIRED" => CheckState::Failed,
                _ => CheckState::Pending,
            },
            _ => CheckState::Pending,
        }
    } else if kind == "StatusContext" {
        match value.get("state").and_then(Value::as_str).unwrap_or("") {
            "SUCCESS" => CheckState::Passed,
            "FAILURE" | "ERROR" => CheckState::Failed,
            _ => CheckState::Pending,
        }
    } else {
        CheckState::Pending
    };
    let details_url = value
        .get("detailsUrl")
        .or_else(|| value.get("targetUrl"))
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("https://"))
        .map(str::to_owned);
    PullRequestCheck {
        name,
        state,
        details_url,
    }
}

fn aggregate_checks(checks: &[PullRequestCheck]) -> CheckState {
    if checks.is_empty() {
        CheckState::NoChecks
    } else if checks.iter().any(|check| check.state == CheckState::Failed) {
        CheckState::Failed
    } else if checks.iter().any(|check| check.state == CheckState::Pending) {
        CheckState::Pending
    } else {
        CheckState::Passed
    }
}

fn task_source_freshness(task: &TaskRecord, remote_head_sha: &str) -> TaskSourceFreshness {
    let repo = match Repository::open(&task.worktree_path) {
        Ok(repo) => repo,
        Err(_) => {
            return TaskSourceFreshness::Unavailable {
                reason: "the task worktree is unavailable".into(),
            }
        }
    };
    let head = match repo.head().ok().and_then(|head| head.target()) {
        Some(head) => head.to_string(),
        None => {
            return TaskSourceFreshness::Unavailable {
                reason: "the task worktree has no readable HEAD commit".into(),
            }
        }
    };
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(true);
    let dirty = match repo.statuses(Some(&mut options)) {
        Ok(statuses) => !statuses.is_empty(),
        Err(_) => {
            return TaskSourceFreshness::Unavailable {
                reason: "the task worktree changes could not be read".into(),
            }
        }
    };
    if dirty {
        TaskSourceFreshness::Stale {
            local_head_sha: Some(head),
            reason: "the task worktree contains files that are not part of its PR head".into(),
        }
    } else if head != remote_head_sha {
        TaskSourceFreshness::Stale {
            local_head_sha: Some(head),
            reason: "the task HEAD differs from the PR head commit".into(),
        }
    } else {
        TaskSourceFreshness::Current {
            local_head_sha: head,
        }
    }
}

fn validate_identifier(identifier: &str) -> Result<String, String> {
    let identifier = identifier.trim();
    if !identifier.is_empty() && identifier.chars().all(|character| character.is_ascii_digit()) {
        return Ok(identifier.to_string());
    }
    let https_host = identifier
        .strip_prefix("https://")
        .and_then(|remainder| remainder.split('/').next());
    if https_host.is_some_and(|host| !host.is_empty() && !host.contains('@'))
        && !identifier.chars().any(char::is_whitespace)
    {
        return Ok(identifier.to_string());
    }
    Err("Enter a pull request number or HTTPS pull request URL".into())
}

fn gh_json(repository_path: &str, args: &[&str], context: &str) -> Result<Value, String> {
    let output = run_gh(repository_path, args, context)?;
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("GitHub CLI returned invalid JSON: {error}"))
}

fn run_gh(repository_path: &str, args: &[&str], context: &str) -> Result<Output, String> {
    let output = Command::new("gh")
        .current_dir(Path::new(repository_path))
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "GitHub CLI (`gh`) is not installed or not on PATH".to_string()
            } else {
                format!("Could not start GitHub CLI: {error}")
            }
        })?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(format_gh_error(context, &output))
    }
}

fn format_gh_error(context: &str, output: &Output) -> String {
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail = detail.trim();
    let normalized = detail.to_ascii_lowercase();
    if normalized.contains("not logged into")
        || normalized.contains("authentication")
        || normalized.contains("auth login")
        || normalized.contains("http 401")
        || normalized.contains("http 403")
    {
        return format!(
            "{context}: GitHub CLI authentication or repository access is unavailable. Sign in with `gh auth login` and confirm access."
        );
    }
    let detail = detail
        .chars()
        .filter(|character| !character.is_control() || *character == '\n')
        .take(400)
        .collect::<String>();
    if detail.is_empty() {
        format!("{context}: GitHub CLI exited with {}", output.status)
    } else {
        format!("{context}: {detail}")
    }
}
