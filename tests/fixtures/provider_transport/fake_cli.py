#!/usr/bin/python3
"""Synthetic subprocess transport fixture. Never invokes a model or network."""
import json, os, pathlib, subprocess, sys, time
cfg = json.loads(pathlib.Path(__file__).with_suffix('.json').read_text())
args = sys.argv[1:]
provider = cfg.get('provider', 'codex')
mode = cfg.get('mode', 'good')
def emit(value):
    print(json.dumps(value, ensure_ascii=False), flush=True)
if '--version' in args:
    print(cfg.get('version', 'codex-cli 0.159.2' if provider == 'codex' else '2.1.286 (Claude Code)'))
    sys.exit(0)
if '--help' in args:
    if mode == 'slow_probe': time.sleep(0.12)
    if mode == 'failed_probe': sys.exit(7)
    if mode == 'bounded_probe': print('x' * 100000); sys.exit(0)
    print('' if mode == 'missing_flag' else '--json --output-schema --output-last-message --ephemeral --ignore-user-config --ignore-rules --no-daemon --strict-config --config --disable --cd --permission-profile --print --output-format --json-schema --tools --disallowedTools --strict-mcp-config --mcp-config --setting-sources --settings --restricted --safe-mode --no-chrome --no-session-persistence --permission-prompts --session-id --max-turns')
    sys.exit(0)
if args[:2] == ['login', 'status'] or args[:2] == ['auth', 'status']:
    if mode == 'failed_auth_probe': print('unexpected'); sys.exit(7)
    if provider == 'codex': print('Not logged in' if mode == 'auth_required' else 'Logged in using ChatGPT')
    else: emit({'loggedIn': mode != 'auth_required', 'authMethod': 'claude.ai', 'subscriptionType': 'pro'})
    sys.exit(1 if mode == 'auth_required' else 0)
if args[:2] == ['features', 'list']:
    print('synthetic fixture features'); sys.exit(0)
# The fixture's sandbox probe is also fake. Its provenance never becomes live.
if 'sandbox' in args:
    print('gitmanager-isolation-fixture'); sys.exit(0)
if mode == 'blocked_stdin': time.sleep(30); sys.exit(0)
if mode == 'early_exit':
    prefix = os.read(0, 4096).decode()
    start = prefix.index('"correlation":') + len('"correlation":')
    correlation, _ = json.JSONDecoder().raw_decode(prefix[start:])
    wire = {'correlation': correlation, 'prompt': 'only a prefix was read'}
else:
    wire = json.load(sys.stdin)
pathlib.Path('fixture-invocation.json').write_text(json.dumps({'argv': args, 'stdin': wire, 'environment_keys': sorted(os.environ)}))
if mode in ['slow', 'child', 'orphan_child']:
    if mode != 'slow':
        child = subprocess.Popen(['/usr/bin/python3','-c',"import time,pathlib; time.sleep(2); pathlib.Path('child-survived').write_text('unexpected')"])
        pathlib.Path('child.pid').write_text(str(child.pid))
    if mode != 'orphan_child': time.sleep(30)
if mode == 'stdout_overflow': print('x' * 2500000, flush=True); time.sleep(30)
if mode == 'stderr_overflow': print('x' * 100000, file=sys.stderr, flush=True); time.sleep(30)
if mode == 'nonzero': print('provider failed',file=sys.stderr); sys.exit(9)
if mode in ['quota','network']:
    emit({'type':'error','error':{'code':'rate_limit_exceeded' if mode=='quota' else 'network_error'}}); sys.exit(1)
envelope = dict(wire['correlation'])
envelope['payload'] = {'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}
if mode == 'wrong_digest': envelope['request_digest'] = '0' * 64
if mode == 'wrong_provider': envelope['provider'] = 'claude' if provider == 'codex' else 'codex'
if mode == 'wrong_nonce': envelope['nonce'] = '0' * 32
if mode == 'wrong_source': envelope['source_digest'] = '0' * 64
body = json.dumps(envelope, ensure_ascii=False)
if mode == 'duplicate_json': body = body[:-1] + ',"request_digest":"' + wire['correlation']['request_digest'] + '"}'
if provider == 'claude':
    result = {'type':'result','subtype':'success','is_error':False,'session_id':args[args.index('--session-id')+1], 'structured_output':envelope,'usage':{'input_tokens':1,'output_tokens':1}}
    if mode == 'wrong_session': result['session_id']='different'
    if mode == 'text_only': del result['structured_output']; result['result']=body
    if mode == 'claude_error': result['is_error']=True; result['subtype']='error_during_execution'
    if mode == 'partial': print('{"type":"result"',flush=True)
    else: emit(result)
    sys.exit(0)
output = pathlib.Path(args[args.index('--output-last-message')+1])
if mode == 'symlink_output': output.symlink_to(pathlib.Path(__file__).with_suffix('.outside'))
elif mode == 'oversize_file': output.write_text('x' * 1100000)
elif mode == 'partial': output.write_text(body[:len(body)//2])
elif mode == 'wrong_route': pathlib.Path('not-the-result.json').write_text(body)
else: output.write_text(body)
started = {'type':'thread.started','thread_id':'fixture-thread'}
turn = {'type':'turn.started'}
message = {'type':'item.completed','item':{'id':'fixture-item','type':'agent_message','text':body}}
complete = {'type':'turn.completed','usage':{'input_tokens':1,'output_tokens':1}}
if mode == 'out_of_order': emit(complete); emit(started); emit(turn); emit(message)
else:
    emit(started); emit(turn)
    if mode == 'tool_call': emit({'type':'item.started','item':{'id':'tool','type':'command_execution','command':'not executed by fixture'}})
    if mode == 'malformed': print('{invalid}',flush=True)
    emit(message)
    if mode == 'duplicate_id': emit(message)
    if mode != 'missing_terminal': emit(complete)
    if mode == 'after_terminal': emit(message)
if mode == 'truncated_line': sys.stdout.write('{"type":'); sys.stdout.flush()
