# Antigravity runner

## Goal

Let IM Gateway chats and schedules run on the Antigravity CLI (`agy`) as a native
runner, alongside Codex, Traex, Claude Code and OpenCode, without changing the
behaviour of existing runners.

## Native adapter contract

- Adapter / runner: `antigravity` / `Antigravity`; the default executable is `agy`.
  CLI provider setup accepts `antigravity`, `antigravity-cli` and `agy`.
- Exec transport with `--input-format stream-json --output-format stream-json`.
  The prompt is written to stdin as one `{"event":"user","message":{"content":…}}`
  line, not as raw bytes.
- Output events: `init`, `step_update` (`agent_response` text deltas, tool steps)
  and `result`. The `conversation_id` becomes Bifrost thread metadata and is passed
  back with `--conversation` for continuation; child conversations are ignored.
- Fail closed: success requires a `result` with status `SUCCESS` and a non-empty
  response. An error/other status or a stream without `result` fails the run even
  on exit code 0. The failure text, stderr or timeout diagnostics are kept; raw
  JSON is never a reply. Shared with OpenCode via `fail_closed_completed` and
  `fail_closed_terminal_response`.
- `adapterConfig.model` → `--model`; reasoning effort → `--effort` (`low`,
  `medium`, `high`). Models come from `agy models`.
- Permissions: Antigravity has native flags, so an explicit `sandbox` opt-in maps to
  `--sandbox` and an explicit `dangerFullAccess: true` maps to
  `--dangerously-skip-permissions`. Nothing is injected by default. Other
  runners' `permissionMode`/`approvalPolicy` and custom `args` are rejected.
  (OpenCode has no equivalent flags, so it rejects all of these.)
- Not implemented in the first version: live steering, live model changes, local
  session discovery and native attachments.

## Shared runner capabilities

`supports_external_cli_thread_resume` lists the adapters whose thread id is resumed
by chat and schedule paths; `adapter_reported_success` lists the fail-closed JSON
adapters. New runners register there instead of editing each call site.

## Verification

- Unit tests: command flags, permission opt-ins, rejected foreign flags, stdin
  envelope, stream parsing (deltas, tools, result, child conversations), default
  runner registration and model catalog.
- Exec runtime tests with a mock `agy`: stream-json stdin, response and
  conversation metadata; zero-exit failures without a successful `result`.
- Real CLI (agy 1.4.12, 2026-11-02): see `human_tests/antigravity-runner.md`.
