# OpenCode runner real CLI acceptance

Use the development binary with an isolated data directory and a port other than 9900. Never reuse production bot credentials in two running services.

| Case | Steps | Expected |
| --- | --- | --- |
| OC-01 | Run installed `opencode --version` and `opencode models` | Version and provider/model identifiers are returned |
| OC-02 | In a fresh temporary directory, use the installed OpenCode through the development Bifrost API with an explicitly selected available model | JSON text, terminal step and sessionID; response marker present |
| OC-03 | On timeout, terminate only the probe's process group | Probe exits; production Bifrost is untouched |
| OC-04 | With OC-02's sessionID, invoke `--session` with a follow-up prompt | Correct session continuation; requires OC-02 to succeed |
| OC-05 | Select OpenCode in development settings, set default model, save and reopen | Selection and model retained; existing runners preserved |
| OC-06 | Start a real OpenCode request, observe its owned native process, then send `/stop` through the chat stream API | Run status is stopped and the recorded native process exits |
| OC-07 | Set `workDir` to an absolute temporary directory while the parent PWD points elsewhere; export the returned OpenCode session | Exported session directory matches workDir |
| OC-08 | In light/dark UI switch OpenCode to Codex, enter arguments and permission mode, then switch back and save | Unsupported fields disappear and are absent from saved OpenCode settings |
| OC-09 | Run deterministic CLI fixtures for a custom error with sessionID and an OpenCode stderr-only startup error | Original error text is preserved in both API responses |

## Current evidence

Executed on 2026-11-01 against the current development binary with an isolated Bifrost service. OC-02/04/06/07 use installed OpenCode 1.19.3 and the already configured `deepseek/deepseek-flash` provider, without a CLI fixture. No production service replacement or Feishu authorization is part of this acceptance run.

```json
{
  "OC-01": {
    "version": "1.19.3",
    "exit": 0,
    "model_available": true
  },
  "OC-02": {
    "status": "PASS",
    "model": "deepseek/deepseek-flash",
    "response": "P2_API_NEW"
  },
  "OC-03": { "process_reaped": true },
  "OC-04": { "status": "PASS", "same_session": true, "response": "P2_MEMORY_7319" },
  "OC-05": "PASS: Playwright light/dark model save/reopen; timeout preserved",
  "OC-06": { "status": "PASS", "run_status": "stopped", "observed_native_processes": 1, "remaining_native_processes": 0 },
  "OC-07": "PASS: opencode export session directory matches temporary workDir",
  "OC-08": "PASS: light/dark adapter switching clears unsupported settings",
  "OC-09": "PASS: quota denied and ModelNotFound messages preserved"
}
```

The free `opencode/deepseek-v4.1-flash-free` model is still blocked: the latest bounded probe logged `Free usage limit reached` and produced no JSON before the 45-second deadline. Its success or continuation is not claimed; it is distinct from the successful configured-model acceptance above.

OC-07 first reproduced a real directory mismatch. OpenCode 1.19.3 resolves relative `--dir` against inherited PWD, so `--dir .` was insufficient. The adapter now supplies an absolute directory; the final exported native session confirms the correction.

OC-05/08 use Playwright against the real development Agent Runners page in both themes. API read-back verifies timeoutSecs remains 10 and unsupported arguments/permission mode do not survive an adapter switch. OC-09 is protocol-fixture acceptance; it is separate from real model acceptance.

For OC-02/04, POST a new message to `/_bifrost/api/im-gateway/chat` with runnerId `OpenCode`, a fresh sessionKey and temporary workDir. Ask it to remember a marker, then ask for the marker using the same sessionKey; verify metadata.threadId is unchanged. For OC-07, run `opencode export <threadId>` and compare info.directory with workDir. For OC-06, POST `/stop` with the active sessionKey to `/_bifrost/api/im-gateway/chat/stream` and verify both the stopped result and process exit.

## Cleanup

Bifrost test data and project work directories are temporary. Real acceptance uses the existing OpenCode profile and creates ordinary test-session records, without changing its configuration. Probe processes are reaped by recorded process group on timeout; cancellation acceptance also checks the observed native PID has exited. The API E2E stops its recorded development server and closes Playwright. Production Bifrost data, provider credentials and installed Bifrost binary are unchanged.

Reproduce API acceptance with `bash e2e-tests/tests/test_im_gateway_opencode_runner.sh`; add `BIFROST_OPENCODE_UI_TEST=1` to exercise the real WebUI in both themes. This builds the current checkout and uses a fresh temporary service, never port 9900.
