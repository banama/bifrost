# OpenCode runner real CLI acceptance

Use the development binary with an isolated data directory and a port other than 9900. Never reuse production bot credentials in two running services.

| Case | Steps | Expected |
| --- | --- | --- |
| OC-01 | Run installed `opencode --version` and `opencode models` | Version and provider/model identifiers are returned |
| OC-02 | In a fresh temporary directory, pipe a short no-tools prompt to `opencode run --pure --format json --model opencode/deepseek-v4.1-flash-free` | JSON text, terminal step and sessionID; response marker present |
| OC-03 | On timeout, terminate only the probe's process group | Probe exits; production Bifrost is untouched |
| OC-04 | With OC-02's sessionID, invoke `--session` with a follow-up prompt | Correct session continuation; requires OC-02 to succeed |
| OC-05 | Select OpenCode in development settings, set default model, save and reopen | Selection and model retained; existing runners preserved |

## Current evidence

Executed against an isolated development service; results below. Protocol fixture tests do not replace OC-02/04. No production service replacement or Feishu authorization is part of this acceptance run.

```json
{
  "OC-01": {
    "version": "1.19.3",
    "exit": 0,
    "model_available": true
  },
  "OC-04": "BLOCKED until OC-02 produces a session",
  "OC-05": "PASS: subsequent Playwright light/dark run",
  "OC-02": {
    "status": "BLOCKED",
    "reason": "55-second timeout before any JSON response",
    "stdout_bytes": 0,
    "stderr_bytes": 0
  },
  "OC-03": {
    "process_reaped": true
  }
}
```

OC-05 PASS: Playwright opened the real development Agent Runners page in light and dark themes, saved a changed default model, reopened the form and restored it. API read-back verified that timeoutSecs remained 10. The API E2E uses a deterministic CLI fixture; it does not prove real external model availability.

## Cleanup

All probes use temporary work/data directories. Real OpenCode probes are terminated by their recorded process group on timeout. The API E2E always stops its recorded development server and closes its Playwright browser. Production data, provider credentials and installed Bifrost binary are unchanged.

Reproduce API acceptance with `bash e2e-tests/tests/test_im_gateway_opencode_runner.sh`; add `BIFROST_OPENCODE_UI_TEST=1` to exercise the real WebUI in both themes. This builds the current checkout and uses a fresh temporary service, never port 9900.
