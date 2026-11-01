//! OpenCode's JSON-lines CLI protocol. This is not Claude's bidirectional stream-json.
use super::*;

pub(super) fn configure_args(
    args: &mut Vec<String>,
    request: &ExternalCliRunRequest,
) -> Result<(), String> {
    if !matches!(
        request.operation.trim(),
        "" | "ask" | "chat" | "create" | "send"
    ) {
        return Err(format!(
            "OpenCode adapter does not support operation '{}'",
            request.operation
        ));
    }
    if request
        .adapter_config
        .permission_mode
        .as_deref()
        .is_some_and(|mode| !mode.is_empty() && mode != "default")
        || request.adapter_config.danger_full_access.unwrap_or(false)
        || request.adapter_config.sandbox.is_some()
        || request.adapter_config.approval_policy.is_some()
    {
        return Err("OpenCode permission policy must be configured in OpenCode; Bifrost does not translate other runners' permission flags".into());
    }
    // Custom arguments can change the output protocol or select an unrelated session.
    // Keep the native adapter deterministic; use a custom adapter for wrappers.
    if !args.is_empty() {
        return Err("OpenCode native adapter does not support adapterConfig.args".to_string());
    }
    args.extend(["run".into(), "--format".into(), "json".into()]);
    if let Some(model) = request
        .adapter_config
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        args.extend(["--model".into(), model.into()]);
    }
    if let Some(session) = request
        .params
        .get("sessionId")
        .or_else(|| request.params.get("sessionID"))
        .or_else(|| request.params.get("session_id"))
        .or_else(|| request.params.get("threadId"))
        .or_else(|| request.params.get("thread_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        args.extend(["--session".into(), session.into()]);
    }
    if let Some(variant) = request
        .adapter_config
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        args.extend(["--variant".into(), variant.into()]);
    }
    Ok(())
}

pub(super) fn parse_model_catalog(stdout: &str) -> Result<Vec<ExternalCliModelInfo>, String> {
    let mut models: Vec<_> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.chars().any(char::is_whitespace))
        .filter(|line| {
            line.split_once('/')
                .is_some_and(|(provider, model)| !provider.is_empty() && !model.is_empty())
        })
        .map(|line| ExternalCliModelInfo {
            slug: line.into(),
            display_name: Some(line.into()),
            ..Default::default()
        })
        .collect();
    models.sort_by(|a, b| a.slug.cmp(&b.slug));
    models.dedup_by(|a, b| a.slug == b.slug);
    if models.is_empty() {
        return Err("OpenCode returned no provider/model identifiers".into());
    }
    Ok(models)
}

#[derive(Default)]
pub(super) struct ParseState {
    session: Option<String>,
    text: String,
    parts: HashSet<String>,
}

pub(super) fn is_event(raw: &serde_json::Value) -> bool {
    raw.get("sessionID").is_some()
        && matches!(
            raw.get("type").and_then(serde_json::Value::as_str),
            Some("step_start" | "text" | "step_finish" | "tool_use" | "reasoning" | "error")
        )
}

pub(super) fn parse_event(
    mut raw: serde_json::Value,
    state: &mut ParseState,
) -> Option<ExternalCliProgressEvent> {
    let session = raw.get("sessionID")?.as_str()?.to_string();
    if state.session.as_ref().is_some_and(|root| root != &session) {
        return None; // A child/foreign session must not become the channel's response.
    }
    if !session.is_empty() && state.session.is_none() {
        state.session = Some(session.clone());
    }
    let kind = raw.get("type")?.as_str()?.to_string();
    let part = raw.get("part").cloned().unwrap_or_default();
    let (event_type, content, title) = match kind.as_str() {
        "step_start" => {
            state.text.clear();
            state.parts.clear();
            (
                ExternalCliProgressEventType::Status,
                "OpenCode step started".into(),
                None,
            )
        }
        "text" => {
            let text = part.get("text")?.as_str()?;
            if let Some(id) = part.get("id").and_then(serde_json::Value::as_str) {
                if !state.parts.insert(id.into()) {
                    return None;
                }
            }
            state.text.push_str(text);
            (
                ExternalCliProgressEventType::AssistantDelta,
                text.into(),
                None,
            )
        }
        "reasoning" => return None, // Do not surface private reasoning as the final answer.
        "step_finish" => {
            let reason = part
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if matches!(reason, "stop" | "length" | "content-filter") {
                (
                    ExternalCliProgressEventType::RunFinished,
                    state.text.clone(),
                    None,
                )
            } else {
                (
                    ExternalCliProgressEventType::Status,
                    format!("OpenCode step finished: {reason}"),
                    None,
                )
            }
        }
        "tool_use" => {
            let status = part
                .pointer("/state/status")
                .and_then(serde_json::Value::as_str)?;
            let title = part
                .get("tool")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            let (event_type, content) = match status {
                "pending" | "running" => (
                    ExternalCliProgressEventType::ToolStarted,
                    "tool running".into(),
                ),
                "completed" => (
                    ExternalCliProgressEventType::ToolFinished,
                    part.pointer("/state/output")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("tool completed")
                        .into(),
                ),
                "error" => (
                    ExternalCliProgressEventType::ToolFinished,
                    part.pointer("/state/error")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("tool failed")
                        .into(),
                ),
                _ => return None,
            };
            (event_type, content, title)
        }
        "error" => {
            let message = raw
                .pointer("/error/data/message")
                .or_else(|| raw.pointer("/error/message"))
                .or_else(|| raw.get("error"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("OpenCode failed");
            (
                ExternalCliProgressEventType::RunFailed,
                message.into(),
                None,
            )
        }
        _ => return None,
    };
    raw["session_id"] = session.into();
    Some(ExternalCliProgressEvent {
        event_type,
        content,
        title,
        raw,
    })
}

pub(super) fn completed_successfully(events: &[ExternalCliProgressEvent]) -> bool {
    events.iter().any(|e| {
        e.event_type == ExternalCliProgressEventType::RunFinished && !e.content.trim().is_empty()
    }) && !events
        .iter()
        .any(|e| e.event_type == ExternalCliProgressEventType::RunFailed)
}

pub(super) fn terminal_response(events: &[ExternalCliProgressEvent]) -> String {
    if let Some(error) = events
        .iter()
        .rev()
        .find(|e| e.event_type == ExternalCliProgressEventType::RunFailed)
    {
        return error.content.clone();
    }
    events
        .iter()
        .rev()
        .find(|e| e.event_type == ExternalCliProgressEventType::RunFinished)
        .map(|e| e.content.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "OpenCode exited without a completed text response.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[cfg(unix)]
    #[tokio::test]
    async fn opencode_exec_runtime_preserves_response_and_session() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("opencode-mock");
        std::fs::write(&executable, r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo 1.19.3; exit 0; fi
cat >/dev/null
printf '%s\n' '{"type":"step_start","sessionID":"ses_test","part":{}}' '{"type":"text","sessionID":"ses_test","part":{"id":"a","text":"hello "}}' '{"type":"text","sessionID":"ses_test","part":{"id":"b","text":"world"}}' '{"type":"step_finish","sessionID":"ses_test","part":{"reason":"stop"}}'
"#).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let request: ExternalCliRunRequest = serde_json::from_value(json!({
            "message":"hello", "adapter":"opencode", "workDir":temp.path(),
            "adapterConfig":{"executable":executable, "timeoutSecs":5}
        }))
        .unwrap();
        let runtime = ExternalCliRuntime::new(temp.path().join("runs"));
        let result = runtime
            .run_in_current_process_with_progress(request, None)
            .await
            .unwrap();
        assert_eq!(result.status, ExternalCliRunStatus::Succeeded);
        assert_eq!(result.response, "hello world");
        assert_eq!(
            result.metadata.get("threadId").map(String::as_str),
            Some("ses_test")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn opencode_exec_runtime_fails_closed_and_preserves_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        for (name, body, status, response) in [
            (
                "error",
                r#"printf '%s\n' '{"type":"error","sessionID":"s","error":{"message":"quota denied"}}'"#,
                ExternalCliRunStatus::Failed,
                "quota denied",
            ),
            (
                "incomplete",
                r#"printf '%s\n' '{"type":"text","sessionID":"s","part":{"text":"partial"}}'"#,
                ExternalCliRunStatus::Failed,
                "OpenCode exited without a completed text response.",
            ),
            (
                "timeout",
                "sleep 20",
                ExternalCliRunStatus::TimedOut,
                "OpenCode exited without a completed text response.",
            ),
        ] {
            let executable = temp.path().join(name);
            std::fs::write(&executable, format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo test; exit 0; fi\ncat >/dev/null\n{body}\n")).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let request: ExternalCliRunRequest = serde_json::from_value(json!({"message":"hello", "adapter":"opencode", "operation":"chat", "workDir":temp.path(), "adapterConfig":{"executable":executable, "timeoutSecs":1}})).unwrap();
            let runtime = ExternalCliRuntime::new(temp.path().join(format!("runs-{name}")));
            let result = runtime
                .run_in_current_process_with_progress(request, None)
                .await
                .unwrap();
            assert_eq!(result.status, status, "{name}");
            assert_eq!(result.response, response, "{name}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn opencode_model_catalog_runs_models_without_debug_prefix() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("opencode");
        std::fs::write(
            &executable,
            "#!/bin/sh\n[ \"$1\" = models ] && [ \"$#\" = 1 ] || exit 2\necho opencode/test\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let models = load_external_cli_model_catalog(
            OPENCODE_ADAPTER,
            &ExternalCliAdapterConfig {
                executable: Some(executable.display().to_string()),
                ..Default::default()
            },
            Some(temp.path()),
        )
        .await
        .unwrap();
        assert_eq!(models[0].slug, "opencode/test");
    }

    #[test]
    fn opencode_incomplete_and_error_are_not_success() {
        let mut state = ParseState::default();
        let text = parse_event(
            json!({"type":"text","sessionID":"s","part":{"text":"partial"}}),
            &mut state,
        )
        .unwrap();
        assert!(!completed_successfully(&[text]));
        assert_eq!(
            terminal_response(&[]),
            "OpenCode exited without a completed text response."
        );
        let error = parse_event(
            json!({"type":"error","sessionID":"s","error":{"message":"denied"}}),
            &mut state,
        )
        .unwrap();
        assert!(!completed_successfully(std::slice::from_ref(&error)));
        assert_eq!(terminal_response(&[error]), "denied");
    }

    #[test]
    fn opencode_command_uses_explicit_session_and_model() {
        let mut request: ExternalCliRunRequest = serde_json::from_value(json!({
            "message":"secret prompt stays on stdin", "adapter":"opencode",
            "workDir":"work", "params":{"threadId":"ses_123"},
            "adapterConfig":{"model":"opencode/deepseek-v4.1-flash-free", "reasoningEffort":"high"}
        }))
        .unwrap();
        let spec = build_command_spec(&request, Path::new("last.md")).unwrap();
        assert_eq!(spec.executable, "opencode");
        assert_eq!(spec.work_dir, Some(PathBuf::from("work")));
        assert_eq!(
            spec.args,
            [
                "run",
                "--format",
                "json",
                "--model",
                "opencode/deepseek-v4.1-flash-free",
                "--session",
                "ses_123",
                "--variant",
                "high"
            ]
        );
        request.params = json!({});
        let spec = build_command_spec(&request, Path::new("last.md")).unwrap();
        assert!(!spec
            .args
            .iter()
            .any(|s| s == "--session" || s == "--continue" || s == "--auto"));
        request.adapter_config.args = vec!["--continue".into()];
        assert!(build_command_spec(&request, Path::new("last.md")).is_err());
    }

    #[test]
    fn opencode_default_is_registered_without_changing_selected_runner() {
        let config = normalized_gateway_config(ExternalCliGatewayConfig::default());
        assert_eq!(
            config.runners[DEFAULT_OPENCODE_RUNNER_ID].adapter,
            OPENCODE_ADAPTER
        );
        assert_ne!(config.default_runner_id, DEFAULT_OPENCODE_RUNNER_ID);
        assert!(supports_external_cli_model_slash(OPENCODE_ADAPTER));
    }

    #[test]
    fn opencode_text_parts_and_terminal_step() {
        let events = parse_progress_events(
            &[
                json!({"type":"step_start","sessionID":"ses_root","part":{}}),
                json!({"type":"text","sessionID":"ses_root","part":{"id":"a","text":"Hello "}}),
                json!({"type":"text","sessionID":"ses_root","part":{"id":"a","text":"Hello "}}),
                json!({"type":"text","sessionID":"ses_child","part":{"id":"c","text":"SECRET"}}),
                json!({"type":"text","sessionID":"ses_root","part":{"id":"b","text":"world"}}),
                json!({"type":"step_finish","sessionID":"ses_root","part":{"reason":"stop"}}),
            ]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        );
        assert_eq!(events.len(), 4);
        assert_eq!(
            events.last().unwrap().event_type,
            ExternalCliProgressEventType::RunFinished
        );
        assert_eq!(events.last().unwrap().content, "Hello world");
        let mut metadata = BTreeMap::new();
        append_external_cli_metadata(OPENCODE_ADAPTER, &events, &mut metadata);
        assert_eq!(
            metadata.get("threadId").map(String::as_str),
            Some("ses_root")
        );
    }

    #[test]
    fn opencode_tool_cycle_is_not_terminal() {
        let mut state = ParseState::default();
        let intermediate = parse_event(
            json!({"type":"step_finish","sessionID":"s","part":{"reason":"tool-calls"}}),
            &mut state,
        )
        .unwrap();
        assert_eq!(
            intermediate.event_type,
            ExternalCliProgressEventType::Status
        );
        let tool = parse_event(json!({"type":"tool_use","sessionID":"s","part":{"tool":"bash","state":{"status":"error","error":"denied"}}}), &mut state).unwrap();
        assert_eq!(tool.event_type, ExternalCliProgressEventType::ToolFinished);
        let error = parse_event(
            json!({"type":"error","sessionID":"s","error":{"data":{"message":"quota exceeded"}}}),
            &mut state,
        )
        .unwrap();
        assert_eq!(error.event_type, ExternalCliProgressEventType::RunFailed);
        assert_eq!(error.content, "quota exceeded");
        assert!(parse_event(
            json!({"type":"reasoning","sessionID":"s","part":{"text":"private"}}),
            &mut state
        )
        .is_none());
        assert!(parse_event(json!({"type":"unknown","sessionID":"s"}), &mut state).is_none());
    }

    #[test]
    fn opencode_boundaries_and_tools() {
        let mut request: ExternalCliRunRequest =
            serde_json::from_value(json!({"message":"test", "adapter":"opencode"})).unwrap();
        assert_eq!(
            app_server::resolved_transport(&request).unwrap(),
            ExternalCliTransport::Exec
        );
        for mode in ["plan", "bypass_permissions", "auto"] {
            request.adapter_config.permission_mode = Some(mode.into());
            assert!(configure_args(&mut vec![], &request).is_err());
        }
        request.adapter_config.permission_mode = None;
        request.operation = "fork".into();
        assert!(configure_args(&mut vec![], &request).is_err());
        let mut state = ParseState::default();
        for (status, expected) in [
            ("pending", ExternalCliProgressEventType::ToolStarted),
            ("running", ExternalCliProgressEventType::ToolStarted),
            ("completed", ExternalCliProgressEventType::ToolFinished),
        ] {
            let e = parse_event(json!({"type":"tool_use","sessionID":"s","part":{"tool":"bash","state":{"status":status,"output":"ok"}}}), &mut state).unwrap();
            assert_eq!(e.event_type, expected);
        }
        assert!(parse_event(
            json!({"type":"tool_use","sessionID":"s","part":{"state":{"status":"future"}}}),
            &mut state
        )
        .is_none());
        assert!(parse_event(json!({"type":"text","sessionID":123}), &mut state).is_none());
        let mut args = vec![];
        request.operation = "ask".into();
        request.params = json!({"sessionID":"ses_alt"});
        configure_args(&mut args, &request).unwrap();
        assert_eq!(args, ["run", "--format", "json", "--session", "ses_alt"]);
    }

    #[test]
    fn opencode_discriminator_preserves_custom_cli_events() {
        let events = parse_progress_events(
            r#"{"type":"assistant_final","sessionID":"custom","content":"custom reply"}"#,
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].content, "custom reply");
        assert_eq!(
            events[0].event_type,
            ExternalCliProgressEventType::AssistantFinal
        );
    }

    #[test]
    fn opencode_model_catalog_is_plain_lines() {
        let models = parse_model_catalog(
            "opencode/deepseek-v4.1-flash-free\ngoogle/a\ngoogle/a\nnot a model\n",
        )
        .unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[1].slug, "opencode/deepseek-v4.1-flash-free");
        assert!(parse_model_catalog("warning only").is_err());
    }
}
