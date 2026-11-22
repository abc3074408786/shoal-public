"""Native JSONL search fixtures, offline and read-only; no model/auth access."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    cli = args.cli.resolve(strict=True)
    checks = []
    with tempfile.TemporaryDirectory(prefix='vl-native-search-integration-') as temp:
        root = Path(temp)
        work, other = root / '工作 folder', root / 'other'
        work.mkdir()
        other.mkdir()
        codex, claude = root / 'native-codex', root / 'native-claude'
        sessions, projects = codex / 'sessions' / '2026' / '11' / '01', claude / 'projects' / 'encoded-project'
        sessions.mkdir(parents=True)
        projects.mkdir(parents=True)
        ids = [str(uuid.uuid4()) for _ in range(3)]
        rows = [{'type': 'session_meta', 'payload': {'id': ids[0], 'cwd': str(work)}},
                {'type': 'response_item', 'payload': {'type': 'message', 'role': 'user', 'content': [{'type': 'input_text', 'text': 'native 搜索中文 user'}]}},
                {'type': 'response_item', 'payload': {'type': 'message', 'role': 'assistant', 'content': [{'type': 'output_text', 'text': 'native 搜索中文 assistant'}]}},
                {'type': 'response_item', 'payload': {'type': 'function_call_output', 'output': 'tool-only-marker'}},
                {'type': 'response_item', 'payload': {'type': 'reasoning', 'summary': [{'text': 'private-reasoning-marker'}]}},
                {'type': 'response_item', 'payload': {'type': 'message', 'role': 'system', 'content': [{'type': 'input_text', 'text': 'private-system-marker'}]}}]
        codex_file = sessions / ('rollout-fixture-' + ids[0] + '.jsonl')
        codex_file.write_text(''.join(json.dumps(row, ensure_ascii=False) + '\n' for row in rows), encoding='utf-8')
        (sessions / ('rollout-other-' + ids[1] + '.jsonl')).write_text(
            json.dumps({'type': 'session_meta', 'payload': {'id': ids[1], 'cwd': str(other)}}) + '\n' +
            json.dumps(rows[1]) + '\n', encoding='utf-8')
        claude_file = projects / (ids[2] + '.jsonl')
        claude_file.write_text(json.dumps({'type': 'assistant', 'sessionId': ids[2], 'cwd': str(work),
                                         'message': {'content': [{'type': 'text', 'text': 'native 搜索中文 Claude'},
                                                                 {'type': 'thinking', 'thinking': 'private-reasoning-marker'}]}}) + '\n', encoding='utf-8')
        env = {**os.environ, 'USERPROFILE': str(root), 'HOME': str(root), 'CODEX_HOME': str(codex), 'CLAUDE_CONFIG_DIR': str(claude)}
        profile = root / 'never-created-profile'

        def invoke(query, *extra, ok=True):
            run = subprocess.run([str(cli), '--data-dir', str(profile), '--json', 'search', query, '--path', str(work), *map(str, extra)],
                                 env=env, capture_output=True, timeout=10,
                                 creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
            reply = json.loads(run.stdout)
            assert (run.returncode == 0) == ok and reply['ok'] == ok, reply
            return reply['result'] if ok else reply

        before = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob('*.jsonl')}
        hits = invoke('搜索中文')
        assert hits['complete'] is True and len(hits['matches']) == 3, hits
        assert {h['native_session_id'] for h in hits['matches']} == {ids[0], ids[2]}
        assert not profile.exists()
        checks.append('offline Unicode conversation search scopes exact cwd and native identity across Codex/Claude')
        assert invoke('tool-only-marker')['matches'] == []
        assert len(invoke('tool-only-marker', '--scope', 'all')['matches']) == 1
        assert invoke('private-reasoning-marker', '--scope', 'all')['matches'] == []
        assert invoke('private-system-marker', '--scope', 'all')['matches'] == []
        checks.append('explicit tool scope excludes reasoning and system text')
        exact = invoke('搜索中文', '--session', ids[0])
        assert exact['complete'] is True and len(exact['matches']) == 2
        for hit in exact['matches']:
            record = json.loads(codex_file.read_bytes()[hit['byte_start']:hit['byte_end']])
            assert record['payload']['role'] == hit['role']
        checks.append('native UUID filter and source byte-range provenance')
        assert invoke('搜索中文', '--max-matches', 1)['complete'] is False
        assert invoke('搜索中文', '--max-files', 1)['omitted_files'] > 0
        assert invoke('搜索中文', '--max-bytes', 10)['complete'] is False
        checks.append('hit, file and byte limits report incomplete results')
        with claude_file.open('ab') as stream:
            stream.write(b'{"unfinished":')
        assert invoke('搜索中文')['complete'] is False
        checks.append('in-progress JSONL tail cannot be reported complete')
        unchanged = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob('*.jsonl') if p != claude_file}
        assert unchanged == {p: h for p, h in before.items() if p != str(claude_file)}
        assert not profile.exists()
        rejected = invoke('x', '--ssh-profile', 'missing', '--server-id', str(uuid.uuid4()), '--remote-data-dir', '/unused', ok=False)
        assert rejected['error']['code'] == 'remote_not_supported'
        checks.append('read-only files/profile and SSH rejection before target lookup')
    args.receipt.write_text(json.dumps({'cli_sha256': hashlib.sha256(cli.read_bytes()).hexdigest(),
                                       'real_model_requests': False, 'checks': checks}, indent=2), encoding='utf-8')
    print(json.dumps({'passed': len(checks), 'real_model_requests': False}))


if __name__ == '__main__':
    main()
