//! One-shot registered-ID commands for an already authorized local handoff.
//! No arbitrary paths, listener, task registry, credentials or consent are added.
use super::*;
use journal::TaskAssociation;

pub(super) fn registered_task(link: &TaskAssociation) -> Result<crate::tasks::TaskRecord, String> {
    let registry = crate::tasks::TaskRegistry::load();
    if let Some(error) = registry.load_error() {
        return Err(format!("Task registry unavailable: {error}"));
    }
    let task = registry
        .entries()
        .iter()
        .find(|t| t.id == link.task_id)
        .ok_or("The linked task is unavailable or was removed")?;
    if task.created_at != link.created_at
        || Path::new(&task.repository_path) != link.repository
        || Path::new(&task.worktree_path) != link.worktree
        || task.worktree_cleanup_completed_at.is_some()
    {
        return Err("The linked task identity or location changed; relink explicitly".into());
    }
    Ok(task.clone())
}
fn association(root: &Path, task_id: &str, request_id: &str) -> Result<TaskAssociation, String> {
    let value = JournalFile::read_only(root)?;
    let task = value
        .task
        .filter(|t| t.task_id == task_id)
        .ok_or("No matching registered task handoff")?;
    registered_task(&task)?;
    let external = task
        .external
        .as_ref()
        .filter(|e| e.pending.request().id == request_id && e.launch_attempted)
        .ok_or(
            "No matching issued request; machine commands cannot create or renew authorization",
        )?;
    external.pending.request().validate().map_err(error)?;
    Ok(task)
}
pub(super) fn execute(
    root: &Path,
    operation: &str,
    task_id: &str,
    request_id: &str,
) -> Result<String, String> {
    execute_inner(
        root,
        operation,
        task_id,
        request_id,
        #[cfg(test)]
        None,
    )
}
fn execute_inner(
    root: &Path,
    operation: &str,
    task_id: &str,
    request_id: &str,
    #[cfg(test)] after_submit: Option<&mut dyn FnMut()>,
) -> Result<String, String> {
    if !valid_id(task_id) || !valid_id(request_id) {
        return Err("Invalid registered task or request ID".into());
    }
    let original = association(root, task_id, request_id)?;
    let external = original.external.as_ref().unwrap();
    let mut flow = task_source_flow::TaskSourceFlow::interrupted(
        &original.tool.identity.project_id,
        task_id,
        external.pending.clone(),
    )
    .map_err(error)?;
    let not_cancelled = AtomicBool::new(false);
    flow.reopen_external(
        &root.join("external-task-jobs"),
        &not_cancelled,
        &not_cancelled,
    )
    .map_err(error)?;
    // The GUI may have explicitly abandoned or replaced the association during
    // the read. An old ticket never grants authority against a newer journal.
    let fresh = association(root, task_id, request_id)?;
    if canonical_bytes(&fresh).map_err(error)? != canonical_bytes(&original).map_err(error)? {
        return Err("The registered request changed; nothing was submitted".into());
    }
    let mut published = None;
    let answer = match operation {
        "prepare" => serde_json::to_string(external.pending.request()).map_err(error)?,
        "inspect" => serde_json::to_string(&serde_json::json!({
            "request_id": request_id,
            "decisions": flow.inspect_decisions(request_id).map_err(error)?,
            "accepted_scenes": external.pending.request().accepted_scenes,
            "examples": external.pending.request().examples,
            "unknowns": external.pending.request().unknowns,
        }))
        .map_err(error)?,
        "submit" => {
            flow.submit_external(request_id, &not_cancelled)
                .map_err(error)?;
            // Validate the actual published completion without changing job
            // files. This is source correlation, not a runtime/service receipt.
            published = Some(
                flow.intake_external(request_id, &not_cancelled)
                    .map_err(error)?
                    .ok_or("Published completion was unavailable for correlation")?
                    .capture()
                    .clone(),
            );
            #[cfg(test)]
            if let Some(after_submit) = after_submit {
                after_submit();
            }
            "Source completion submitted for independent local intake; no execution, live author or adoption is certified".into()
        }
        _ => return Err("Unsupported machine operation".into()),
    };
    let after = JournalFile::read_only(root)?
        .task
        .ok_or("Local intake was abandoned or replaced")?;
    registered_task(&after)?;
    let unchanged =
        canonical_bytes(&after).map_err(error)? == canonical_bytes(&original).map_err(error)?;
    let consumed = if let (Some(published), Some(completed)) = (&published, &after.completed) {
        after.external.is_none()
            && task_source_host::Link::from(&after) == task_source_host::Link::from(&original)
            && canonical_bytes(&completed.external).map_err(error)?
                == canonical_bytes(external).map_err(error)?
            && &completed.capture == published
    } else {
        false
    };
    if !unchanged && !consumed {
        return Err("Local intake was abandoned or changed; any late completion is not authorized for intake".into());
    }
    if let Some(published) = &published {
        flow.analyze(published, &not_cancelled, |_| Ok(()))
            .map_err(error)?;
    }
    Ok(answer)
}
#[cfg(test)]
pub(super) fn test_execute_after_submit(
    root: &Path,
    task_id: &str,
    request_id: &str,
    after_submit: &mut dyn FnMut(),
) -> Result<String, String> {
    execute_inner(root, "submit", task_id, request_id, Some(after_submit))
}
/// Called before GUI initialization. The argument grammar intentionally accepts
/// only IDs already registered by the current profile's single host journal.
pub(super) fn command(args: &[std::ffi::OsString]) -> Option<Result<String, String>> {
    if args.first().is_none_or(|a| a != "--product-task") {
        return None;
    }
    Some((|| {
        if args.len() != 6 || args[2] != "--task" || args[4] != "--request" {
            return Err("Usage: --product-task prepare|inspect|submit --task ID --request ID; paths are not accepted".into());
        }
        let operation = args[1].to_str().ok_or("Invalid machine operation")?;
        if !matches!(operation, "prepare" | "inspect" | "submit") {
            return Err("Unsupported machine operation".into());
        }
        let task = args[3]
            .to_str()
            .filter(|s| valid_id(s))
            .ok_or("Invalid task ID")?;
        let request = args[5]
            .to_str()
            .filter(|s| valid_id(s))
            .ok_or("Invalid request ID")?;
        let locations = ToolLocations::default_location().map_err(error)?;
        execute(locations.path(), operation, task, request)
    })())
}
