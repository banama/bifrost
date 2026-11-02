# OpenCode runner

## Goal

Let IM Gateway chats and schedules run on the OpenCode CLI as a native runner,
using `opencode run --format json` on the existing exec worker, without changing
existing runners and without ever enabling a permission bypass automatically. The
built-in runner follows OpenCode defaults; a deployment may select a model such as
`opencode/deepseek-v4.1-flash-free` explicitly.

## Native adapter contract

- Adapter and runner: `opencode` / `OpenCode`; CLI provider setup accepts case-insensitive `opencode`.
- Use exec transport, prompt on stdin, explicit `--session` from channel metadata. Never use global `--continue`.
- Pass an absolute `--dir` matching the process work directory; OpenCode 1.19.3 resolves relative values against inherited PWD. Native parsing is enabled only for the OpenCode adapter.
- JSON `sessionID` becomes Bifrost thread metadata. Child-session output is ignored. Completed text parts are streamed as deltas; only a terminal step produces the final response. Tool-call intermediate steps are not terminal.
- JSON errors and missing terminal text fail the run even when the CLI exits zero. Raw JSON is never used as a final reply.
- Models come from `opencode models`; `adapterConfig.model` becomes `--model`. Reasoning effort, if explicitly supplied through config, becomes `--variant`; supported variants are provider-specific and are not advertised as a universal list.
- Custom args and foreign permission/sandbox flags are rejected. Configure OpenCode permissions itself; no `--auto` is injected.
- First version does not implement live steering, live model changes, local session discovery, fork/checkpoint operations or native attachment transport. Those must not be advertised as verified capabilities.
- Success/failure and thread resume use the shared `fail_closed_*`,
  `adapter_reported_success` and `supports_external_cli_thread_resume` helpers
  (see `design/antigravity-runner.md`).

## Local development use

Keep the development executable separate from the installed official binary. Validate with an isolated BIFROST_DATA_DIR and non-production port (never 9900). Build web assets before CLI when required by build.rs. Do not run two services connected to the same production bot.

Production switch is a separate deployment step: back up configuration, gracefully stop the existing service, start the pinned development binary and verify status. Preserve the official executable for rollback. Official upgrades may replace an installed development binary and do not preserve local patches.

## Local validation commands

```sh
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
cargo build -p bifrost-cli --bin bifrost
DEV_DATA=$(mktemp -d "${TMPDIR:-/tmp}/bifrost-opencode.XXXXXX")
BIFROST_DISABLE_TRAY=1 BIFROST_SYNC_DISABLE_AUTO_LOGIN_PROMPT=1 BIFROST_DATA_DIR="$DEV_DATA" cargo run -p bifrost-cli --bin bifrost -- start -p 19901 --host 127.0.0.1 --access-mode local_only --skip-cert-check --no-system-proxy --no-tray -d
# In the development settings select OpenCode and set the executable/model.
BIFROST_DATA_DIR="$DEV_DATA" ./target/debug/bifrost -p 19901 agent run --runner OpenCode --new "Reply with OK"
BIFROST_DATA_DIR="$DEV_DATA" ./target/debug/bifrost -p 19901 stop
```

The development service must stay isolated until real model and channel acceptance succeeds. An official installed CLI does not automatically use changes in this source checkout.

## Latest validation status

| Check | Result |
| --- | --- |
| OpenCode focused backend tests | 13 PASS, including real exec runtime, timeout and generic-event regression |
| Proxy lint baseline regression | badge 28 PASS; process_info 30 PASS |
| Workspace Clippy, all targets/features | PASS after two semantics-preserving lint fixes in a separate commit |
| Frontend TypeScript/build and selection unit test | PASS |
| Real development API and Playwright UI | PASS with deterministic executable fixture; light/dark model round-trip and adapter switches preserve timeout and clear unsupported fields |
| Full workspace tests | BLOCKED: admin 3530 PASS, 2 FAIL, 1 ignored; same ASR retry wait and existing power fallback failures |
| ASR retry isolated reproduction | FAIL: existing two-second running-task assertion; no ASR production changes made |
| Power fallback failure attribution | Existing test hardcodes 8800; an unrelated pre-existing Node service listens there and returns 404 |
| Full changed-line coverage | BLOCKED: complete instrumented admin run ended with the same two unrelated test failures; make coverage-changed did not pass |
| P2 changed-line coverage measurement | 93.33% (28/30) versus fe182827; recovered LCOV from the complete instrumented run, then coverage-diff passed 90%. This does not turn the failed test/preflight command into a pass |
| Real installed OpenCode through development API | PASS: configured deepseek/deepseek-flash new session, memory continuation, explicit model, exported directory and cancellation/process cleanup |
| Real OpenCode free-model response/continuation | BLOCKED: latest bounded probe logged Free usage limit reached; no JSON before the 45-second deadline |
| Remote CI | NOT_VERIFIED: repository CI inspector requires GITHUB_TOKEN |

Notable fixes found during validation: native parsing is restricted to the
OpenCode adapter so custom runners' `sessionID`/error events keep the generic
parser; stderr-only and timeout diagnostics are preserved; unsupported settings
are hidden and cleared in the WebUI; `--dir` is passed as an absolute path because
OpenCode 1.19.3 resolves relative values against the inherited PWD.
