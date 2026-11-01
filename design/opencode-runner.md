# OpenCode runner plan

## P2 correction verification plan

Work starts from a clean `feat/opencode-runner` at `fe182827`; the current worktree is reused because it has no unrelated changes. Scope is Rust runner parsing/error propagation and frontend runner settings.

- [x] Preserve custom-runner errors carrying `sessionID`; enable native parsing only for OpenCode.
- [x] Preserve stderr-only failures and timeout diagnostics without exposing raw protocol JSON.
- [x] Hide unsupported OpenCode settings, clear stale form values and verify adapter switches in light/dark UI.
- [ ] Run focused backend/frontend tests, API/UI E2E, fmt, workspace Clippy, workspace tests and changed-line coverage (90% gate).
- [x] Reattempt installed OpenCode new-session/continuation/cancellation acceptance using isolated Bifrost data/work and recorded process groups; configured model passed, free model quota is exhausted.
- [x] Complete two review/test rounds and update human-test evidence; final independent Standards/Spec reviews found no additional P0/P1/P2.
- [ ] Publish to the existing draft PR and inspect CI through the PAT skill; the CI inspector remains blocked by missing GITHUB_TOKEN.

Use native `opencode run --format json` with the existing exec worker. Preserve existing runners and channel isolation. Never automatically enable permission bypass. The built-in runner follows OpenCode defaults; deployment may select `opencode/deepseek-v4.1-flash-free` explicitly.

## Verification tasks

- [x] Startup: clean main at ebadd7f6; task branch feat/opencode-runner.
- [x] Backend/UI route: command construction, event parsing, session metadata, models, built-in runner and settings choice.
- [x] Required: new/continued sessions, explicit model, text/tool/error events, stop, timeout and process cleanup.
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
- Pass an absolute `--dir` matching the process work directory; OpenCode 1.19.3 resolves relative values against inherited PWD. Native parsing is enabled only for the OpenCode adapter.
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

Original round-2 boundaries (superseded by the latest status below): OpenCode 1.19.3 listed the free model, but three probes timed out without JSON; no real reply was claimed at that stage. Clippy then reported two proxy baseline warnings, later fixed separately. Frontend formatting already failed on all four original modified files; only the touched serializer/form region is formatted in this correction. GitHub publication uses the connected GitHub app; the required CI inspector still needs the unavailable GITHUB_TOKEN.

Round 3: rechecked latest diff and untracked files after the second-round fixes. The isolated real API E2E passed again. Playwright light/dark default-model save/reopen passed and verified preservation of existing timeout configuration. Fixed test-only accessible-button locators and ensured the UI E2E builds fresh frontend assets before embedding them. External real-model acceptance remains blocked; it is reported separately from fixture-backed API acceptance.

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

Round 4: narrowed the OpenCode event discriminator to its known event names so custom CLI `assistant_final` events carrying sessionID still use the existing generic parser. Added and passed the compatibility regression. Rechecked complete diff/status and all untracked additions. The final backend Clippy gate passes; remaining real-model, workspace-environment and coverage blockers are explicit rather than hidden or weakened assertions.

P2 round 1: reproduced the custom error regression before fixing it. Restricted both streaming and fallback native parsers to OpenCode, preserved stderr/timeout diagnostics and removed unsupported settings from OpenCode forms. Focused tests and light/dark UI passed. Independent Standards/Spec review found no further code defects, but real acceptance exposed a work-directory mismatch even with relative `--dir .`.

P2 round 2: resolved the directory to an absolute path before launching OpenCode. The installed CLI now passes real new-session, memory continuation, directory-export and cancellation acceptance through development Bifrost. The directory handling follows the [OpenCode 1.19.3 run command](https://github.com/anomalyco/opencode/blob/v1.19.3/packages/opencode/src/cli/cmd/run.ts#L323). Regression API tests preserve both custom and stderr-only errors. Workspace Clippy passes; full tests and the unfiltered coverage preflight still fail on unchanged ASR/power tests. No production bot was enabled or service replaced.
