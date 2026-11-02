#!/usr/bin/env bash
set -euo pipefail
export BIFROST_DISABLE_TRAY=1 BIFROST_SYNC_DISABLE_AUTO_LOGIN_PROMPT=1
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_DIR"
if [[ "${OSTYPE:-}" == msys* || "${OSTYPE:-}" == cygwin* ]]; then
  echo "SKIP: OpenCode executable fixture requires POSIX process groups"
  exit 0
fi
# Build the current checkout; never launch the installed production executable.
if [[ "${BIFROST_OPENCODE_UI_TEST:-}" == "1" ]]; then
  pnpm --dir web build
fi
SKIP_FRONTEND_BUILD=1 cargo build -p bifrost-cli --bin bifrost
python3 - "$REPO_DIR" <<'PY'
import json, os, pathlib, signal, socket, subprocess, sys, tempfile, time, urllib.request
root = pathlib.Path(sys.argv[1])
with tempfile.TemporaryDirectory(prefix='bifrost-opencode-e2e-') as directory:
    data = pathlib.Path(directory)
    fixture = data / 'opencode-fixture'
    fixture.write_text('#!' + sys.executable + '\n' + '''import json,sys,pathlib
args=sys.argv[1:]
if args == ['--version']: print('fixture'); sys.exit(0)
if args == ['models']: print('opencode/test'); sys.exit(0)
prompt=sys.stdin.read()
if 'custom_error' in prompt:
 print(json.dumps({'type':'error','sessionID':'custom','content':'quota denied'}));sys.exit(1)
if 'startup_error' in prompt:
 print('ModelNotFound: provider/example is unavailable',file=sys.stderr);sys.exit(1)
assert args[:3] == ['run','--format','json'],args
assert args[args.index('--model')+1] == 'opencode/test',args
directory=pathlib.Path(args[args.index('--dir')+1])
assert directory.is_absolute() and directory.resolve() == pathlib.Path.cwd(),args
if 'force_error' in prompt:
 print(json.dumps({'type':'error','sessionID':'ses_e2e','error':{'message':'fixture quota denied'}}));sys.exit(0)
resumed='--session' in args
if resumed: assert args[args.index('--session')+1] == 'ses_e2e',args
for event in [
 {'type':'step_start','part':{}},
 {'type':'text','part':{'id':'part1','text':'RESUMED' if resumed else 'NEW'}},
 {'type':'step_finish','part':{'reason':'stop'}}]:
 event['sessionID']='ses_e2e';print(json.dumps(event),flush=True)
''')
    fixture.chmod(0o700)
    with socket.socket() as listener:
        listener.bind(('127.0.0.1',0));port=listener.getsockname()[1]
    assert port != 9900
    env = dict(os.environ, BIFROST_DATA_DIR=str(data), BIFROST_DISABLE_TRAY='1', BIFROST_SYNC_DISABLE_AUTO_LOGIN_PROMPT='1')
    log = (data/'server.log').open('w')
    process = subprocess.Popen([str(root/'target/debug/bifrost'),'start','--host','127.0.0.1','-p',str(port),'--skip-cert-check','--no-system-proxy','--no-tray'],env=env,stdout=log,stderr=log,start_new_session=True)
    base=f'http://127.0.0.1:{port}/_bifrost/api'
    client=urllib.request.build_opener(urllib.request.ProxyHandler({}))
    def request(path, body=None, method=None, stream=False):
        req=urllib.request.Request(base+path, data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json'},method=method)
        with client.open(req,timeout=25) as response:
            return [json.loads(line) for line in response if line.strip()] if stream else json.load(response)
    try:
        for _ in range(100):
            try: request('/proxy/address');break
            except Exception:
                if process.poll() is not None: raise RuntimeError('development server exited')
                time.sleep(.1)
        else: raise RuntimeError('development server not ready')
        config=request('/im-gateway/chat/config')
        assert config['runners']['OpenCode']['adapter']=='opencode',config.keys()
        config['runners']['OpenCode']['adapterConfig']={'executable':str(fixture),'model':'opencode/test','timeoutSecs':10}
        config['runners']['CustomReview']={'enabled':True,'adapter':'custom','adapterConfig':{'executable':str(fixture),'timeoutSecs':10}}
        request('/im-gateway/chat/config',config,'PATCH')
        if os.environ.get('BIFROST_OPENCODE_UI_TEST') == '1':
            subprocess.run(['node',str(root/'e2e-tests/test_data/opencode/runner-ui.cjs'),str(root),f'http://127.0.0.1:{port}'],check=True,timeout=90)
        def chat(message, session='opencode-e2e', runner='OpenCode'):
            events=request('/im-gateway/chat/stream',{'message':message,'sessionKey':session,'runnerId':runner,'runtime':'external_cli'},'POST',True)
            finished=[e for e in events if e.get('eventType')=='run_finished']
            assert len(finished)==1,[(e.get('eventType'),e.get('status')) for e in events]
            return finished[0]
        first=chat('first');assert first['status']=='succeeded' and first['response']=='NEW',first
        second=chat('second');assert second['status']=='succeeded' and second['response']=='RESUMED',second
        failure=chat('force_error','opencode-failure');assert failure['status']=='failed' and 'quota denied' in failure['response'],failure
        startup=chat('startup_error','opencode-startup');assert startup['status']=='failed' and startup['response']=='ModelNotFound: provider/example is unavailable',startup
        custom=chat('custom_error','custom-failure','CustomReview');assert custom['status']=='failed' and custom['response']=='quota denied',custom
        print('PASS: OpenCode registration, model flag, JSON stream, session continuation and error-with-zero-exit through real Bifrost API')
    finally:
        if process.poll() is None:
            os.killpg(process.pid,signal.SIGTERM)
            try: process.wait(timeout=10)
            except subprocess.TimeoutExpired: os.killpg(process.pid,signal.SIGKILL);process.wait()
        log.close()
PY
