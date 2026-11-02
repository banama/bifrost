# Antigravity runner real CLI acceptance

Run against the installed `agy` in a temporary directory. Use an isolated Bifrost
data directory and a port other than 9900 for API checks; never point two running
services at the same production bot.

| Case | Steps | Expected |
| --- | --- | --- |
| AG-01 | Run `agy --version` | Version is printed |
| AG-02 | Pipe one `{"event":"user","message":{"content":"Reply with exactly: OK"}}` line into `agy --input-format stream-json --output-format stream-json` | `init`, `step_update` text delta and `result` with status `SUCCESS`; one `conversation_id` throughout |
| AG-03 | Repeat with `--conversation <AG-02 id>` asking for the previous reply | Same `conversation_id`; answer is the earlier reply |
| AG-04 | Run the adapter exec tests (`cargo test -p bifrost-admin antigravity`) | All pass, including mock-`agy` runtime and zero-exit failure cases |
| AG-05 | Bind a Feishu bot to the `Antigravity` runner on the development binary and chat several turns | Replies arrive with a stable thread id; an empty `SUCCESS` result is reported as a failure, not a blank reply |

## Current evidence

Executed on 2026-11-02 with installed `agy` 1.4.12 in a throwaway directory:

```json
{
  "AG-01": { "version": "1.4.12" },
  "AG-02": { "status": "SUCCESS", "response": "OK", "events": ["init", "step_update", "step_update", "step_update", "result"] },
  "AG-03": { "status": "SUCCESS", "same_conversation": true, "response": "OK" },
  "AG-04": "PASS: 15 tests",
  "AG-05": { "date": "2026-11-01", "runs": 5, "succeeded": 4, "thread_id_on_all": true,
             "failed": "1: agy ended after read_file/run_command tool steps with status SUCCESS and an empty response; surfaced as 'Antigravity CLI returned an empty response.'" }
}
```

The stream shape matches `adapter_dispatch_parses_real_stream_shape`. AG-05 ran
through a real Feishu bot (`agy`) on the development binary; message contents are
not recorded here. Schedule runs and WebUI runner selection remain to be recorded.

## Cleanup

Probe directories are temporary. The probes create ordinary Antigravity
conversations in the existing profile and change no configuration.
