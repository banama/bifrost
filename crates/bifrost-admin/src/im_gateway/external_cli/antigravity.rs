//! Antigravity CLI (`agy`) headless JSON-lines protocol.
//!
//! The CLI streams `init`, `step_update` and `result` events on stdout when it
//! runs with `--output-format stream-json`. Prompts are accepted on stdin as
//! one `{"event":"user", ...}` message per line.
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
            "Antigravity CLI adapter does not support operation '{}'",
            request.operation
        ));
    }
    if request
        .adapter_config
        .permission_mode
        .as_deref()
        .is_some_and(|mode| !mode.is_empty() && mode != "default")
        || request.adapter_config.approval_policy.is_some()
    {
        return Err("Antigravity CLI permission policy must be configured in Antigravity; Bifrost does not translate other runners' permission flags".into());
    }
    // Custom arguments can change the output protocol or select an unrelated
    // conversation. Keep the native adapter deterministic.
    if !args.is_empty() || !request.adapter_config.args.is_empty() {
        return Err(
            "Antigravity CLI native adapter does not support adapterConfig.args".to_string(),
        );
    }

    args.extend([
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
    ]);
    if let Some(model) = request
        .adapter_config
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        args.extend(["--model".to_string(), model.to_string()]);
    }
    if let Some(effort) = request
        .adapter_config
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let effort = effort.to_ascii_lowercase();
        validate_external_effort_value(&effort)?;
        args.extend(["--effort".to_string(), effort]);
    }
    if let Some(conversation_id) = conversation_id_from_params(request) {
        args.extend(["--conversation".to_string(), conversation_id]);
    }
    if request
        .adapter_config
        .sandbox
        .as_deref()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "true" | "1" | "yes" | "sandbox"
            )
        })
    {
        args.push("--sandbox".to_string());
    }
    // Never inject the bypass automatically; only honour an explicit opt-in.
    if request.adapter_config.danger_full_access == Some(true) {
        args.push("--dangerously-skip-permissions".to_string());
    }
    Ok(())
}

fn conversation_id_from_params(request: &ExternalCliRunRequest) -> Option<String> {
    [
        "conversationId",
        "conversationID",
        "conversation_id",
        "threadId",
        "sessionId",
        "sessionID",
        "session_id",
    ]
    .iter()
    .find_map(|key| {
        request
            .params
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

/// Headless stream-json mode expects a `user` event on stdin rather than the
/// raw prompt bytes used by the other exec-based adapters.
pub(super) fn stdin_prompt(prompt: &str) -> Vec<u8> {
    let payload = serde_json::json!({
        "event": "user",
        "message": { "content": prompt },
    });
    let mut bytes = serde_json::to_vec(&payload).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

#[derive(Default)]
pub(super) struct ParseState {
    session: Option<String>,
    started_tools: HashSet<u64>,
}

pub(super) fn is_event(raw: &serde_json::Value) -> bool {
    matches!(
        raw.get("event").and_then(serde_json::Value::as_str),
        Some("init" | "step_update" | "result")
    )
}

fn event_conversation_id(value: &serde_json::Value) -> Option<&str> {
    value
        .get("conversation_id")
        .or_else(|| value.get("conversationId"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn parse_event(
    mut raw: serde_json::Value,
    state: &mut ParseState,
) -> Option<ExternalCliProgressEvent> {
    let kind = raw.get("event")?.as_str()?.to_string();
    let payload = match kind.as_str() {
        "init" => raw.get("init").cloned().unwrap_or_default(),
        "step_update" => raw.get("step_update")?.clone(),
        "result" => raw.get("result")?.clone(),
        _ => return None,
    };
    let session = event_conversation_id(&raw)
        .or_else(|| event_conversation_id(&payload))
        .map(str::to_string)?;
    if state.session.as_ref().is_some_and(|root| root != &session) {
        return None; // A child/foreign conversation must not become the reply.
    }
    if state.session.is_none() {
        state.session = Some(session.clone());
    }

    let (event_type, content, title) = match kind.as_str() {
        "init" => (
            ExternalCliProgressEventType::Status,
            "Antigravity session started".to_string(),
            None,
        ),
        "step_update" => parse_step_update(&payload, state)?,
        "result" => parse_result(&payload)?,
        _ => return None,
    };
    raw["session_id"] = serde_json::json!(session);
    Some(ExternalCliProgressEvent {
        event_type,
        content,
        title,
        raw,
    })
}

fn parse_step_update(
    payload: &serde_json::Value,
    state: &mut ParseState,
) -> Option<(ExternalCliProgressEventType, String, Option<String>)> {
    let step_type = payload.get("step_type")?.as_str()?;
    let step_state = payload
        .get("state")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("DONE");
    match step_type {
        "agent_response" => {
            let delta = payload
                .get("text_delta")
                .and_then(serde_json::Value::as_str)?;
            if delta.is_empty() {
                return None;
            }
            Some((
                ExternalCliProgressEventType::AssistantDelta,
                delta.to_string(),
                None,
            ))
        }
        "tool" => {
            let title = payload
                .get("tool_name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            let step_index = payload
                .get("step_index")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            if step_state == "ACTIVE" {
                if !state.started_tools.insert(step_index) {
                    return None;
                }
                return Some((
                    ExternalCliProgressEventType::ToolStarted,
                    "tool running".to_string(),
                    title,
                ));
            }
            state.started_tools.insert(step_index);
            let content = payload
                .pointer("/tool_info/error/message")
                .or_else(|| payload.pointer("/tool_info/output"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("tool completed");
            Some((
                ExternalCliProgressEventType::ToolFinished,
                content.to_string(),
                title,
            ))
        }
        "user_input" | "checkpoint" => None,
        _ => Some((
            ExternalCliProgressEventType::Status,
            format!("Antigravity step {step_type} {step_state}"),
            None,
        )),
    }
}

fn parse_result(
    payload: &serde_json::Value,
) -> Option<(ExternalCliProgressEventType, String, Option<String>)> {
    let status = payload
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("ERROR");
    let response = payload
        .get("response")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if status.eq_ignore_ascii_case("SUCCESS") && !response.is_empty() {
        return Some((ExternalCliProgressEventType::RunFinished, response, None));
    }
    let reason = payload
        .get("finished_reason")
        .or_else(|| payload.get("finishedReason"))
        .and_then(serde_json::Value::as_str);
    let message = payload
        .get("error")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| reason.map(|reason| format!("Antigravity run ended: {reason}")))
        .unwrap_or_else(|| {
            if status.eq_ignore_ascii_case("SUCCESS") {
                "Antigravity CLI returned an empty response.".to_string()
            } else {
                format!("Antigravity run ended with status {status}.")
            }
        });
    Some((ExternalCliProgressEventType::RunFailed, message, None))
}

pub(super) fn completed_successfully(events: &[ExternalCliProgressEvent]) -> bool {
    fail_closed_completed(events)
}

pub(super) fn terminal_response(
    events: &[ExternalCliProgressEvent],
    status: ExternalCliRunStatus,
    stderr: &str,
) -> String {
    fail_closed_terminal_response(
        events,
        status,
        stderr,
        "Antigravity CLI exited without a completed response.",
    )
}

pub(super) fn parse_model_catalog(stdout: &str) -> Result<Vec<ExternalCliModelInfo>, String> {
    let mut models: Vec<_> = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut parts = line.splitn(2, char::is_whitespace);
            let slug = parts.next()?.trim();
            if slug.is_empty()
                || !slug.chars().all(|ch| {
                    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':')
                })
            {
                return None;
            }
            let display_name = parts
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| slug.to_string());
            Some(ExternalCliModelInfo {
                slug: slug.to_string(),
                display_name: Some(display_name),
                ..Default::default()
            })
        })
        .collect();
    models.sort_by(|a, b| a.slug.cmp(&b.slug));
    models.dedup_by(|a, b| a.slug == b.slug);
    if models.is_empty() {
        return Err("Antigravity CLI returned no model identifiers".into());
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builds_stream_json_command_with_model_and_conversation() {
        let request: ExternalCliRunRequest = serde_json::from_value(json!({
            "message": "hello",
            "adapter": "antigravity",
            "operation": "chat",
            "params": {"conversationId": "conv_1"},
            "adapterConfig": {"model": "gemini-3.8-flash-medium", "reasoningEffort": "high"}
        }))
        .unwrap();
        let mut args = Vec::new();
        configure_args(&mut args, &request).unwrap();
        assert_eq!(
            args,
            vec![
                "--input-format".to_string(),
                "stream-json".to_string(),
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--model".to_string(),
                "gemini-3.8-flash-medium".to_string(),
                "--effort".to_string(),
                "high".to_string(),
                "--conversation".to_string(),
                "conv_1".to_string(),
            ]
        );
    }

    #[test]
    fn rejects_custom_args_and_foreign_permission_flags() {
        let mut request: ExternalCliRunRequest = serde_json::from_value(json!({
            "message": "hello", "adapter": "antigravity"
        }))
        .unwrap();
        request.adapter_config.args = vec!["--print".into()];
        let mut args = Vec::new();
        assert!(configure_args(&mut args, &request).is_err());

        request.adapter_config.args.clear();
        request.adapter_config.permission_mode = Some("plan".into());
        assert!(configure_args(&mut args, &request).is_err());
    }

    #[test]
    fn bypass_is_only_enabled_when_explicit() {
        let mut request: ExternalCliRunRequest = serde_json::from_value(json!({
            "message": "hello", "adapter": "antigravity"
        }))
        .unwrap();
        let mut args = Vec::new();
        configure_args(&mut args, &request).unwrap();
        assert!(!args
            .iter()
            .any(|arg| arg == "--dangerously-skip-permissions"));

        request.adapter_config.danger_full_access = Some(true);
        args.clear();
        configure_args(&mut args, &request).unwrap();
        assert!(args
            .iter()
            .any(|arg| arg == "--dangerously-skip-permissions"));
    }

    #[test]
    fn resumes_through_external_thread_id() {
        assert!(supports_external_cli_thread_resume(ANTIGRAVITY_ADAPTER));
        let request: ExternalCliRunRequest = serde_json::from_value(json!({
            "message": "next",
            "adapter": "antigravity",
            "params": {"threadId": "216cecfb-3322-4a8c-94f0-9ed0ce5994f0"}
        }))
        .unwrap();
        let mut args = Vec::new();
        configure_args(&mut args, &request).unwrap();
        assert!(args.windows(2).any(|pair| {
            pair[0] == "--conversation" && pair[1] == "216cecfb-3322-4a8c-94f0-9ed0ce5994f0"
        }));
    }

    #[test]
    fn antigravity_default_is_registered_without_changing_selected_runner() {
        let config = normalized_gateway_config(ExternalCliGatewayConfig::default());
        assert_eq!(
            config.runners[DEFAULT_ANTIGRAVITY_RUNNER_ID].adapter,
            ANTIGRAVITY_ADAPTER
        );
        assert_ne!(config.default_runner_id, DEFAULT_ANTIGRAVITY_RUNNER_ID);
        assert!(supports_external_cli_model_slash(ANTIGRAVITY_ADAPTER));
        assert_eq!(
            canonical_external_cli_runner_id(&config, "agy"),
            DEFAULT_ANTIGRAVITY_RUNNER_ID
        );
    }

    #[test]
    fn adapter_dispatch_parses_real_stream_shape() {
        let lines = [
            json!({"event":"init","conversation_id":"conv_1","init":{"cwd":"/tmp"}}),
            json!({"event":"step_update","step_update":{"conversation_id":"conv_1","step_index":0,"state":"DONE","step_type":"user_input"}}),
            json!({"event":"step_update","step_update":{"conversation_id":"conv_1","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"ok"}}),
            json!({"event":"step_update","step_update":{"conversation_id":"conv_1","step_index":1,"state":"DONE","step_type":"agent_response","text_delta":"\n"}}),
            json!({"event":"result","result":{"conversation_id":"conv_1","status":"SUCCESS","response":"ok\n"}}),
        ];
        let stdout = lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let events = parse_progress_events_for_adapter(&stdout, ANTIGRAVITY_ADAPTER);
        assert!(completed_successfully(&events));
        assert!(events.iter().any(|event| {
            event.event_type == ExternalCliProgressEventType::AssistantDelta
                && event.content == "ok"
        }));
        let response = terminal_response(&events, ExternalCliRunStatus::Succeeded, "");
        assert_eq!(response, "ok");
    }

    #[test]
    fn stdin_prompt_wraps_message_as_stream_json_user_event() {
        let bytes = stdin_prompt("line one\n\"quoted\"");
        let line = String::from_utf8(bytes).unwrap();
        assert!(line.ends_with('\n'));
        let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(value["event"], "user");
        assert_eq!(value["message"]["content"], "line one\n\"quoted\"");
    }

    #[test]
    fn parses_text_deltas_tools_and_result() {
        let mut state = ParseState::default();
        let init = parse_event(
            json!({"event":"init","conversation_id":"conv_1","init":{"cwd":"/tmp"}}),
            &mut state,
        )
        .unwrap();
        assert_eq!(init.event_type, ExternalCliProgressEventType::Status);
        assert_eq!(init.raw["session_id"], "conv_1");

        let active = parse_event(
            json!({"event":"step_update","step_update":{"conversation_id":"conv_1","step_index":2,"state":"ACTIVE","step_type":"agent_response","text_delta":"hello "}}),
            &mut state,
        )
        .unwrap();
        assert_eq!(
            active.event_type,
            ExternalCliProgressEventType::AssistantDelta
        );
        assert_eq!(active.content, "hello ");

        let tool = parse_event(
            json!({"event":"step_update","step_update":{"conversation_id":"conv_1","step_index":4,"state":"DONE","step_type":"tool","tool_name":"run_command","tool_info":{"output":"ok"}}}),
            &mut state,
        )
        .unwrap();
        assert_eq!(tool.event_type, ExternalCliProgressEventType::ToolFinished);
        assert_eq!(tool.content, "ok");

        let result = parse_event(
            json!({"event":"result","result":{"conversation_id":"conv_1","status":"SUCCESS","response":"hello world\n"}}),
            &mut state,
        )
        .unwrap();
        assert_eq!(result.event_type, ExternalCliProgressEventType::RunFinished);
        assert_eq!(result.content, "hello world");
    }

    #[test]
    fn ignores_child_conversations() {
        let mut state = ParseState::default();
        parse_event(
            json!({"event":"init","conversation_id":"root","init":{}}),
            &mut state,
        );
        let child = parse_event(
            json!({"event":"result","result":{"conversation_id":"child","status":"SUCCESS","response":"nested"}}),
            &mut state,
        );
        assert!(child.is_none());
    }

    #[test]
    fn error_status_and_empty_success_fail() {
        let mut state = ParseState::default();
        let error = parse_event(
            json!({"event":"result","result":{"conversation_id":"c","status":"ERROR","response":"","error":"quota denied"}}),
            &mut state,
        )
        .unwrap();
        assert_eq!(error.event_type, ExternalCliProgressEventType::RunFailed);
        assert_eq!(error.content, "quota denied");

        let empty = parse_event(
            json!({"event":"result","result":{"conversation_id":"c","status":"SUCCESS","response":""}}),
            &mut state,
        )
        .unwrap();
        assert_eq!(empty.event_type, ExternalCliProgressEventType::RunFailed);
        assert_eq!(empty.content, "Antigravity CLI returned an empty response.");

        let timeout = parse_event(
            json!({"event":"result","result":{"conversation_id":"c","status":"ERROR","finished_reason":"print_timeout"}}),
            &mut state,
        )
        .unwrap();
        assert_eq!(timeout.content, "Antigravity run ended: print_timeout");
    }

    #[test]
    fn terminal_response_prefers_failure_then_final() {
        let events = vec![ExternalCliProgressEvent {
            event_type: ExternalCliProgressEventType::RunFinished,
            content: "done".into(),
            title: None,
            raw: json!({}),
        }];
        assert_eq!(
            terminal_response(&events, ExternalCliRunStatus::Succeeded, ""),
            "done"
        );
        let events = vec![ExternalCliProgressEvent {
            event_type: ExternalCliProgressEventType::RunFailed,
            content: "denied".into(),
            title: None,
            raw: json!({}),
        }];
        assert_eq!(
            terminal_response(&events, ExternalCliRunStatus::Failed, ""),
            "denied"
        );
        assert_eq!(
            terminal_response(&[], ExternalCliRunStatus::Failed, ""),
            "Antigravity CLI exited without a completed response."
        );
    }

    #[test]
    fn parses_text_model_catalog() {
        let models = parse_model_catalog(
            "gemini-3.8-flash-medium   Gemini 3.8 Flash (Medium)\n\
             gemini-3.1-pro-high       Gemini 3.1 Pro (High)\n",
        )
        .unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].slug, "gemini-3.1-pro-high");
        assert_eq!(
            models[1].display_name.as_deref(),
            Some("Gemini 3.8 Flash (Medium)")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn exec_runtime_preserves_response_and_session() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("agy-mock");
        std::fs::write(
            &executable,
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo 1.4.12; exit 0; fi
cat >/dev/null
printf '%s\n' '{"event":"init","conversation_id":"conv_1","init":{}}' '{"event":"step_update","step_update":{"conversation_id":"conv_1","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"hello "}}' '{"event":"step_update","step_update":{"conversation_id":"conv_1","step_index":1,"state":"DONE","step_type":"agent_response","text_delta":"world"}}' '{"event":"result","result":{"conversation_id":"conv_1","status":"SUCCESS","response":"hello world\n"}}'
"#,
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let request: ExternalCliRunRequest = serde_json::from_value(json!({
            "message": "hello", "adapter": "antigravity", "workDir": temp.path(),
            "adapterConfig": {"executable": executable, "timeoutSecs": 5}
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
            Some("conv_1")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn antigravity_exec_runtime_sends_stream_json_and_resumes_conversation() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let stdin_copy = temp.path().join("stdin.json");
        let executable = temp.path().join("agy-mock");
        std::fs::write(&executable, format!(r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo 1.4.12; exit 0; fi
cat > '{}'
printf '%s\n' '{{"event":"init","conversation_id":"conv_1","init":{{}}}}' '{{"event":"step_update","step_update":{{"conversation_id":"conv_1","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"hello"}}}}' '{{"event":"result","result":{{"conversation_id":"conv_1","status":"SUCCESS","response":"hello"}}}}'
"#, stdin_copy.display())).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let request: ExternalCliRunRequest = serde_json::from_value(json!({
            "message":"hi", "adapter":"antigravity", "workDir":temp.path(),
            "adapterConfig":{"executable":executable, "timeoutSecs":5}
        }))
        .unwrap();
        let runtime = ExternalCliRuntime::new(temp.path().join("runs"));
        let result = runtime
            .run_in_current_process_with_progress(request, None)
            .await
            .unwrap();
        assert_eq!(result.status, ExternalCliRunStatus::Succeeded);
        assert_eq!(result.response, "hello");
        assert_eq!(
            result.metadata.get("threadId").map(String::as_str),
            Some("conv_1")
        );
        let sent: serde_json::Value =
            serde_json::from_str(std::fs::read_to_string(&stdin_copy).unwrap().trim()).unwrap();
        assert_eq!(sent["event"], "user");
        assert!(sent["message"]["content"].as_str().unwrap().contains("hi"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn antigravity_exec_runtime_fails_closed_on_zero_exit_without_result() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        for (name, body, response) in [
            (
                "error-result",
                r#"printf '%s\n' '{"event":"result","result":{"conversation_id":"c","status":"ERROR","error":"quota exhausted"}}'"#,
                "quota exhausted",
            ),
            (
                "no-result",
                r#"printf '%s\n' '{"event":"step_update","step_update":{"conversation_id":"c","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"partial"}}'"#,
                "Antigravity CLI exited without a completed response.",
            ),
        ] {
            let executable = temp.path().join(name);
            std::fs::write(&executable, format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo test; exit 0; fi\ncat >/dev/null\n{body}\n")).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let request: ExternalCliRunRequest = serde_json::from_value(json!({"message":"hello", "adapter":"antigravity", "operation":"chat", "workDir":temp.path(), "adapterConfig":{"executable":executable, "timeoutSecs":5}})).unwrap();
            let runtime = ExternalCliRuntime::new(temp.path().join(format!("runs-{name}")));
            let result = runtime
                .run_in_current_process_with_progress(request, None)
                .await
                .unwrap();
            assert_eq!(result.status, ExternalCliRunStatus::Failed, "{name}");
            assert_eq!(result.response, response, "{name}");
        }
    }
}
