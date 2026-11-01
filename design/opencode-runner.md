# OpenCode runner plan

Use native `opencode run --format json` with the existing exec worker. Preserve existing runners and channel isolation. Never automatically enable permission bypass. The built-in runner follows OpenCode defaults; deployment may select `opencode/deepseek-v4.1-flash-free` explicitly.

## Verification tasks

- [x] Startup: clean main at ebadd7f6; task branch feat/opencode-runner.
- [x] Backend/UI route: command construction, event parsing, session metadata, models, built-in runner and settings choice.
- [ ] Required: new/continued sessions, explicit model, text/tool/error events, stop, timeout and process cleanup.
- [x] Unit tests: command flags; sessionID metadata; multiple text parts; intermediate tool steps; error/unknown events; model catalog; existing-runner regression.
- [ ] Real CLI checks: isolated temporary directory, free-model new session and continuation; cancellation. Record human_tests/opencode-runner.md results immediately after defining cases.
- [ ] Rust checks: fmt, workspace clippy, affected unit tests, changed-line coverage and workspace tests.
- [x] UI: TypeScript, runner selection test, real Playwright light/dark model round-trip. Formatting baseline failures documented below.
- [x] Review/Fix/Test round 1: complete diff/status, correct gaps, run focused tests.
- [x] Review/Fix/Test round 2: latest diff/status, failure paths and docs, rerun focused tests.
- [ ] Commit, fork/push, draft PR, PAT skill fail-fast CI. Fork created at https://github.com/banama/bifrost; GITHUB_TOKEN is still unavailable for the required CI inspector.
- [ ] Final self-check: implementation, two reviews, test evidence and remaining blockers.

## Local development use

Keep the development executable separate from the installed official binary. Validate with an isolated BIFROST_DATA_DIR and non-production port (never 9900). Build web assets before CLI when required by build.rs. Do not run two services connected to the same production bot.

Production switch is a separate deployment step: back up configuration, gracefully stop the existing service, start the pinned development binary and verify status. Preserve the official executable for rollback. Official upgrades may replace an installed development binary and do not preserve local patches.

## Native adapter contract

- Adapter and runner: `opencode` / `OpenCode`; CLI provider setup accepts case-insensitive `opencode`.
- Use exec transport, prompt on stdin, explicit `--session` from channel metadata. Never use global `--continue`.
- JSON `sessionID` becomes Bifrost thread metadata. Child-session output is ignored. Completed text parts are streamed as deltas; only a terminal step produces the final response. Tool-call intermediate steps are not terminal.
- JSON errors and missing terminal text fail the run even when the CLI exits zero. Raw JSON is never used as a final reply.
- Models come from `opencode models`; `adapterConfig.model` becomes `--model`. Reasoning effort, if explicitly supplied through config, becomes `--variant`; supported variants are provider-specific and are not advertised as a universal list.
- Custom args and foreign permission/sandbox flags are rejected. Configure OpenCode permissions itself; no `--auto` is injected.
- First version does not implement live steering, live model changes, local session discovery, fork/checkpoint operations or native attachment transport. Those must not be advertised as verified capabilities.

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

## Review and execution record

Round 1: reviewed command construction, JSON normalization, default registration, metadata and UI selection. Corrected the supported operation list to include the real chat gateway's `chat` operation. Focused tests: 10 passed; TypeScript and runner selection unit test passed. Real Bifrost API E2E passed with an isolated protocol fixture (new session, continuation, model argument, JSON error with zero exit).

Round 2: reviewed status preservation, terminal responses, permission boundaries and configuration round-trip. Corrected timeout classification so a timeout is not rewritten as a protocol failure; added timeout, zero-exit-error and incomplete-stream runtime tests. Preserved existing non-form adapter settings when saving the new default-model field. A further review is required after these corrections.

Validation boundaries: installed OpenCode 1.19.3 lists the requested free model, but three no-tools free-model probes timed out before producing JSON. No real model response or real continuation is claimed. Workspace clippy currently reports pre-existing `manual_find` in proxy transform/badge.rs and `collapsible_else_if` in proxy utils/process_info.rs. Frontend formatter also reports failures on all four modified frontend files in the untouched HEAD baseline; broad reformatting is intentionally excluded. GitHub fork/push requires GITHUB_TOKEN, which is unavailable in this environment.

Round 3: rechecked latest diff and untracked files after the second-round fixes. The isolated real API E2E passed again. Playwright light/dark default-model save/reopen passed and verified preservation of existing timeout configuration. Fixed test-only accessible-button locators and ensured the UI E2E builds fresh frontend assets before embedding them. External real-model acceptance remains blocked; it is reported separately from fixture-backed API acceptance.

## Latest validation status

| Check | Result |
| --- | --- |
| OpenCode focused backend tests | 13 PASS, including real exec runtime, timeout and generic-event regression |
| Proxy lint baseline regression | badge 28 PASS; process_info 30 PASS |
| Workspace Clippy, all targets/features | PASS after two semantics-preserving lint fixes in a separate commit |
| Frontend TypeScript/build and selection unit test | PASS |
| Real development API and Playwright UI | PASS with deterministic executable fixture; light/dark default-model round-trip preserves timeout |
| Full workspace tests | BLOCKED: admin 3529 PASS, 2 FAIL, 1 ignored; failed ASR retry wait and existing power fallback test |
| ASR retry isolated reproduction | FAIL: existing two-second running-task assertion; no ASR production changes made |
| Power fallback failure attribution | Existing test hardcodes 8800; an unrelated pre-existing Node service listens there and returns 404 |
| Full changed-line coverage | INCOMPLETE: attempted with instrumentation; existing voice-wake tests remained waiting over 60 seconds and output stalled; terminated only this owned coverage process tree after more than ten minutes. No 90% claim |
| Real OpenCode free-model response/continuation | BLOCKED: no JSON before 55-second probe deadlines |
| Remote CI | NOT_VERIFIED: repository CI inspector requires GITHUB_TOKEN |

Round 4: narrowed the OpenCode event discriminator to its known event names so custom CLI `assistant_final` events carrying sessionID still use the existing generic parser. Added and passed the compatibility regression. Rechecked complete diff/status and all untracked additions. The final backend Clippy gate passes; remaining real-model, workspace-environment and coverage blockers are explicit rather than hidden or weakened assertions.
