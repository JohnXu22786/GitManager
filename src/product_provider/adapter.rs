use super::{input, protocol::*};
use serde_json::{json, Value};
use std::{collections::HashMap, ffi::OsString, path::Path};

pub(crate) fn wire(request: &ProviderRequest, nonce: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
    let correlation = json!({"request_id":request.request_id,"request_digest":request.digest()?,"source_digest":request.source_digest,"provider":request.provider,"nonce":nonce});
    let mut properties = serde_json::Map::new();
    for (key, value) in correlation.as_object().unwrap() {
        properties.insert(key.clone(), json!({"type":"string","enum":[value]}));
    }
    properties.insert("payload".into(), relocated_schema(&request.schema)?);
    let mut required: Vec<String> = properties.keys().cloned().collect();
    required.sort();
    let schema = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let stdin = json!({"transport_version":RECEIPT_VERSION,"instruction":"Return only an object matching the supplied output schema. Echo the correlation fields exactly; put the requested result in payload. Tool output and fields named passed never constitute independent execution evidence.","correlation":correlation,"prompt":std::str::from_utf8(&request.prompt).map_err(|e|e.to_string())?});
    Ok((
        serde_json::to_vec(&stdin).unwrap(),
        serde_json::to_vec(&schema).unwrap(),
    ))
}
pub(crate) fn arguments(
    kind: ProviderKind,
    directory: &Path,
    schema: &[u8],
    nonce: &str,
) -> Vec<OsString> {
    let words: Vec<&str> = match kind {
        ProviderKind::Codex => vec![
            "--no-daemon",
            "exec",
            "--ignore-user-config",
            "--ignore-rules",
            "--ephemeral",
            "--skip-git-repo-check",
            "--json",
            "--color",
            "never",
            "--sandbox",
            "read-only",
            "--config",
            "approval_policy=\"never\"",
            "--config",
            "project_doc_max_bytes=0",
            "--config",
            "shell_environment_policy.inherit=\"none\"",
            "--config",
            "web_search=\"disabled\"",
            "--disable",
            "shell_tool",
            "--disable",
            "unified_exec",
            "--disable",
            "hooks",
            "--disable",
            "apps",
            "--disable",
            "plugins",
            "--disable",
            "remote_plugin",
            "--disable",
            "browser_use",
            "--disable",
            "computer_use",
            "--disable",
            "view_image",
            "--disable",
            "multi_agent",
            "--disable",
            "code_mode_host",
            "--disable",
            "image_generation",
            "--disable",
            "skill_search",
            "--disable",
            "skill_mcp_dependency_install",
        ],
        ProviderKind::Claude => vec![
            "--print",
            "--restricted",
            "--safe-mode",
            "--tools",
            "",
            "--disallowedTools",
            "*",
            "--strict-mcp-config",
            "--mcp-config",
            "{\"mcpServers\":{}}",
            "--setting-sources",
            "",
            "--settings",
            "{\"disableAllHooks\":true}",
            "--no-chrome",
            "--no-session-persistence",
            "--permission-prompts",
            "none",
            "--output-format",
            "json",
            "--max-turns",
            "1",
        ],
    };
    let mut args: Vec<OsString> = words.into_iter().map(Into::into).collect();
    match kind {
        ProviderKind::Codex => {
            args.extend([
                OsString::from("--cd"),
                directory.into(),
                "--output-schema".into(),
                directory.join("schema.json").into(),
                "--output-last-message".into(),
                directory.join("final.json").into(),
                "-".into(),
            ]);
        }
        ProviderKind::Claude => {
            let session = session_id(nonce);
            args.extend([
                "--session-id".into(),
                session.into(),
                "--json-schema".into(),
                String::from_utf8(schema.to_vec())
                    .expect("JSON UTF-8")
                    .into(),
            ]);
        }
    }
    args
}
pub(crate) fn session_id(nonce: &str) -> String {
    format!(
        "{}-{}-4{}-a{}-{}",
        &nonce[..8],
        &nonce[8..12],
        &nonce[13..16],
        &nonce[17..20],
        &nonce[20..]
    )
}
pub(crate) struct Parsed {
    pub final_bytes: Vec<u8>,
    pub session: String,
    pub usage: Option<Value>,
}
pub(crate) fn envelope(value: Value, disclosure: &DataDisclosure) -> Result<Vec<u8>, String> {
    let map = value.as_object().ok_or("final envelope is not an object")?;
    if map.len() != 6
        || map.get("request_id") != Some(&json!(disclosure.request_id))
        || map.get("request_digest") != Some(&json!(disclosure.request_digest))
        || map.get("source_digest") != Some(&json!(disclosure.source_digest))
        || map.get("provider") != Some(&json!(disclosure.provider))
        || map.get("nonce") != Some(&json!(disclosure.nonce))
    {
        return Err("final result correlation mismatch".into());
    }
    serde_json::to_vec(map.get("payload").ok_or("missing opaque payload")?)
        .map_err(|e| e.to_string())
}
pub(crate) fn parse(
    kind: ProviderKind,
    stdout: &[u8],
    final_file: Option<&[u8]>,
    disclosure: &DataDisclosure,
) -> Result<Parsed, String> {
    match kind {
        ProviderKind::Codex => codex(
            stdout,
            final_file.ok_or("missing designated final output")?,
            disclosure,
        ),
        ProviderKind::Claude => claude(stdout, disclosure),
    }
}
fn codex(stdout: &[u8], file: &[u8], disclosure: &DataDisclosure) -> Result<Parsed, String> {
    if !stdout.ends_with(b"\n") {
        return Err("truncated JSONL stream".into());
    }
    let mut phase = 0;
    let mut session = None;
    let mut final_text = None;
    let mut usage = None;
    let mut items = HashMap::new();
    for line in stdout
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
    {
        if line.len() > MAX_RESULT_BYTES {
            return Err("JSONL event exceeds byte limit".into());
        }
        let event = input::parse_json_bytes(line).map_err(|e| e.to_string())?;
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .ok_or("event type missing")?;
        match kind {
            "thread.started" if phase == 0 => {
                session = Some(bounded_id(&event, "thread_id")?);
                phase = 1;
            }
            "turn.started" if phase == 1 => phase = 2,
            "item.started" | "item.updated" | "item.completed" if phase == 2 => {
                let item = event.get("item").ok_or("missing event item")?;
                let id = bounded_id(item, "id")?;
                let item_type = item
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or("missing item type")?;
                if !matches!(item_type, "agent_message" | "reasoning") {
                    return Err("provider attempted a tool or unsupported item".into());
                }
                if items.get(&id) == Some(&true) {
                    return Err("duplicate completed item ID".into());
                }
                if kind == "item.started" && items.contains_key(&id) {
                    return Err("duplicate started item ID".into());
                }
                if kind == "item.updated" && !items.contains_key(&id) {
                    return Err("out-of-order item update".into());
                }
                items.insert(id, kind == "item.completed");
                if kind == "item.completed" && item_type == "agent_message" {
                    final_text = Some(
                        item.get("text")
                            .and_then(Value::as_str)
                            .ok_or("agent message missing text")?
                            .to_owned(),
                    );
                }
            }
            "turn.completed" if phase == 2 => {
                if items.values().any(|v| !*v) {
                    return Err("unfinished item at terminal event".into());
                }
                phase = 3;
                usage = bounded_usage(event.get("usage"))?;
            }
            "error" | "turn.failed" => return Err("provider reported a failed turn".into()),
            _ => return Err("unsupported or out-of-order event".into()),
        }
    }
    if phase != 3 {
        return Err("missing terminal completion event".into());
    }
    let value = input::parse_json_bytes(file).map_err(|e| e.to_string())?;
    let text = input::parse_json_bytes(final_text.ok_or("missing final agent message")?.as_bytes())
        .map_err(|e| e.to_string())?;
    if value != text {
        return Err("designated output does not match the completed stream".into());
    }
    Ok(Parsed {
        final_bytes: envelope(value, disclosure)?,
        session: session.unwrap(),
        usage,
    })
}
fn claude(stdout: &[u8], disclosure: &DataDisclosure) -> Result<Parsed, String> {
    let value = input::parse_json_bytes(stdout).map_err(|e| e.to_string())?;
    if value.get("type") != Some(&json!("result"))
        || value.get("subtype") != Some(&json!("success"))
        || value.get("is_error") != Some(&json!(false))
    {
        return Err("Claude did not report a successful result".into());
    }
    let session = bounded_id(&value, "session_id")?;
    if session != session_id(&disclosure.nonce) {
        return Err("Claude session correlation mismatch".into());
    }
    Ok(Parsed {
        final_bytes: envelope(
            value
                .get("structured_output")
                .ok_or("missing structured_output")?
                .clone(),
            disclosure,
        )?,
        session,
        usage: bounded_usage(value.get("usage"))?,
    })
}
fn bounded_usage(value: Option<&Value>) -> Result<Option<Value>, String> {
    if let Some(value) = value {
        if !value.is_object() || serde_json::to_vec(value).map_err(|e| e.to_string())?.len() > 8192
        {
            return Err("unsupported or oversized usage metadata".into());
        }
    }
    Ok(value.cloned())
}
fn bounded_id(value: &Value, key: &str) -> Result<String, String> {
    let id = value
        .get(key)
        .and_then(Value::as_str)
        .ok_or("missing provider identity")?;
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return Err("invalid provider identity".into());
    }
    Ok(id.into())
}
pub(crate) fn provider_failure(stdout: &[u8]) -> Option<JobState> {
    for line in stdout
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
    {
        if let Ok(value) = input::parse_json_bytes(line) {
            if value.get("type") == Some(&json!("error"))
                || value.get("type") == Some(&json!("turn.failed"))
            {
                return Some(match value.pointer("/error/code").and_then(Value::as_str) {
                    Some("rate_limit_exceeded" | "insufficient_quota") => JobState::Quota,
                    Some("network_error" | "connection_error") => JobState::NetworkError,
                    Some("authentication_error" | "unauthorized") => JobState::AuthRequired,
                    _ => JobState::ProviderError,
                });
            }
            if value.get("type") == Some(&json!("result"))
                && value.get("is_error") == Some(&json!(true))
            {
                return Some(JobState::ProviderError);
            }
        }
    }
    None
}
