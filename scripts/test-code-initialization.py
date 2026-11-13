#!/usr/bin/env python3
"""Exercise Code defaults and reviewed legacy-profile migration with actual Wasm."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from project_config import save

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, default=root / 'target/debug/lenso')
parser.add_argument('--official', action='store_true')
parser.add_argument('--live', action='store_true', help='Also read the live official module directory')
parser.add_argument('--token-env')
parser.add_argument('--out', type=Path)
args = parser.parse_args()
binary = args.binary.resolve(strict=True)
out = args.out.resolve() if args.out else Path(tempfile.mkdtemp(prefix='lenso-code-initialization-'))
if args.out:
    out.mkdir(parents=True, exist_ok=False)
reports = []
for name, settings in [('default', {}), ('explicit-official', {'repo': 'LioRael/lenso-modules'})]:
    project = out / name
    project.mkdir()
    env = dict(os.environ, PATH='', LENSO_CACHE_DIR=str(project / 'cache'))
    credentials = ['--token-env', args.token_env] if args.token_env else []
    source = 'code'
    if not args.official:
        extension = project / 'lenso/extensions/code'
        extension.mkdir(parents=True)
        for file in ['extension.toml', 'code.wasm']:
            shutil.copyfile(root / 'dist/author/code' / file, extension / file)
        source = 'lenso/extensions/code/extension.toml'
    commands = []

    def call(*arguments, expected=0):
        r = subprocess.run([str(binary), '--project', str(project), *arguments, '--json'], env=env,
                           capture_output=True, text=True, timeout=180)
        assert r.returncode == expected, (arguments, r.stdout[-4000:], r.stderr[-2000:])
        assert '\x1b' not in r.stdout
        if args.token_env and env.get(args.token_env):
            assert env[args.token_env] not in r.stdout + r.stderr
        commands.append({'args': list(arguments), 'exit_code': r.returncode})
        return json.loads(r.stdout)

    def config():
        return call('config')['outputs']['main']['config']

    def code(config):
        return next(e for e in config['cli']['extensions'] if e['id'] == 'code')

    call('init', '--preset', 'empty', '--yes')
    call('ext', 'add', source, '--id', 'code', '--input', json.dumps(settings), '--yes', *credentials)
    installed = code(config())
    pin = next(e for e in json.loads((project / 'lenso.lock').read_text())['extensions'] if e['id'] == 'code')
    assert pin['definition']['version'] == '0.3.6'
    fresh = json.loads(json.dumps(installed))
    profile = installed['options']['registries']['main']
    assert profile['format'] == 'lenso-modules-v1' and profile['path'] == 'lenso.modules.toml'
    assert 'lenso.code.toml' in installed['reads'] and installed['options']['native']
    if args.live or args.official:
        assert any(i['name'] == 'auth' for i in call('code', 'search', 'auth')['outputs']['search']['results'])
        language = call('code', 'preview', 'auth', '--refresh', expected=2)['question']
        assert language['id'].startswith('code:module-language:')
    cfg = config()
    installed = code(cfg)
    installed['options']['registries'] = {
        'official': {'source': 'github-repo:LioRael/lenso-modules', 'format': 'lenso-code-v1',
                     'path': 'catalog.toml', 'ref': 'HEAD', 'target': 'src/custom'},
        'custom': {'source': 'github-repo:acme/resources', 'format': 'lenso-code-v1',
                   'path': 'custom.toml', 'ref': 'pinned-ref', 'target': 'src/other'}}
    if name == 'default':
        installed['options'].pop('native')
        for field in ['reads', 'files', 'processes']:
            installed[field] = []
    installed['options']['unrelated'] = 'retained'
    installed['options']['selection'] = {'language': 'typescript', 'runtime': 'node26'}
    installed['sources'].append('github:acme/resources')
    installed['reads'].append('custom.toml')
    installed['files'].append('custom.toml')
    installed['processes'].append({'program': 'custom-native-helper', 'args': ['check']})
    save(project / 'lenso.toml', cfg)
    before = (project / 'lenso.toml').read_bytes()
    lock_before = (project / 'lenso.lock').read_bytes()
    diagnostic = call('code', 'preview', 'auth', '--registry', 'official', '--locked', '--offline', expected=1)['message']
    assert 'lenso ext configure code' in diagnostic and 'backtrace' not in diagnostic
    assert (project / 'lenso.toml').read_bytes() == before and (project / 'lenso.lock').read_bytes() == lock_before
    pending = call('ext', 'configure', 'code', expected=2)
    assert (project / 'lenso.toml').read_bytes() == before
    answers = project / 'decline.json'
    answers.write_text(json.dumps({pending['question']['id']: False}))
    call('ext', 'configure', 'code', '--answers', str(answers), expected=130)
    assert (project / 'lenso.toml').read_bytes() == before and (project / 'lenso.lock').read_bytes() == lock_before
    call('ext', 'configure', 'code', '--yes')
    migrated = code(config())
    expected = json.loads(json.dumps(installed))
    expected['options']['registries']['official'].update(format='lenso-modules-v1', path='lenso.modules.toml')
    if name == 'default':
        expected['options']['native'] = fresh['options']['native']
        for field in ['reads', 'files', 'processes']:
            expected[field] = fresh[field] + installed[field]
    assert migrated['options'] == expected['options']
    for field in ['sources', 'read_roots', 'write_roots', 'reads', 'files', 'processes']:
        assert migrated[field] == expected[field], (field, migrated[field], expected[field])
    stable = (project / 'lenso.toml').read_bytes()
    call('ext', 'configure', 'code', '--yes')
    assert (project / 'lenso.toml').read_bytes() == stable
    if args.live or args.official:
        language = call('code', 'preview', 'auth', '--registry', 'official', '--refresh', expected=2)['question']
        assert language['id'].startswith('code:module-language:')
    # Existing Host versions preserve setup on package refresh; migration remains reviewed.
    save(project / 'lenso.toml', cfg)
    call('ext', 'add', source, '--id', 'code', '--yes', *credentials)
    upgraded = code(config())
    assert upgraded['options'] == installed['options']
    diagnostic = call('code', 'preview', 'auth', '--registry', 'official', '--locked', '--offline', expected=1)['message']
    assert 'lenso ext configure code' in diagnostic
    call('ext', 'configure', 'code', '--yes')
    assert code(config())['options'] == expected['options']
    assert upgraded['sources'] == expected['sources']
    assert not (project / 'src').exists() and not (project / 'lenso.code.toml').exists()
    reports.append({'case': name, 'extension_version': pin['definition']['version'], 'default_directory': True, 'legacy_diagnostic': True,
                    'denial_without_writes': True, 'reviewed_migration': True, 'idempotent': True,
                    'custom_profiles_and_grants_preserved': True, 'upgrade_then_review_migration': True,
                    'live_directory': args.live or args.official, 'commands': commands})
(out / 'verification.json').write_text(json.dumps({'platform': sys.platform, 'official': args.official, 'cases': reports}, indent=2) + '\n')
print('Code initialization/migration passed: ' + str(out / 'verification.json'), flush=True)
