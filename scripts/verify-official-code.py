#!/usr/bin/env python3
"""Consume the live official Code alias and preset with the published CLI and cold caches."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--token-env', help='Explicit GitHub credential binding for API quota')
parser.add_argument('--out', type=Path, required=True)
parser.add_argument('--code-version', default='0.3.3')
parser.add_argument('--preset-version', default='0.3.3')
args = parser.parse_args()
binary = args.binary.resolve(strict=True)
assert subprocess.check_output([str(binary), '--version'], text=True).strip() == 'lenso 1.0.0'
root = Path(__file__).resolve().parent.parent
output = args.out.resolve()
output.mkdir(parents=True, exist_ok=False)
settings = json.dumps({'author_root': 'author', 'target': 'src'})
credentials = ('--token-env', args.token_env) if args.token_env else ()
token = os.environ.get(args.token_env) if args.token_env else None
reports = []

for scenario in ('ext-add', 'new-preset', 'init-preset', 'apply-preset'):
    case = output / scenario
    case.mkdir()
    project = case / 'app'
    env = dict(os.environ, PATH='', LENSO_CACHE_DIR=str(case / 'cache'))
    for name in ('GH_TOKEN', 'GITHUB_TOKEN', 'LENSO_PUBLISH_TOKEN'):
        if name != args.token_env:
            env.pop(name, None)
    commands = []

    def run(directory, *arguments, expected=0, offline=False):
        environment = dict(env)
        if offline:
            environment.update(HTTP_PROXY='http://127.0.0.1:9', HTTPS_PROXY='http://127.0.0.1:9',
                               ALL_PROXY='http://127.0.0.1:9', NO_PROXY='')
        result = subprocess.run([str(binary), '--project', str(directory), *arguments, '--json'],
                                env=environment, capture_output=True, text=True, timeout=180)
        if token:
            assert token not in result.stdout + result.stderr, 'credential leaked in CLI output'
        assert result.returncode == expected, (scenario, arguments, result.returncode, result.stdout[-5000:], result.stderr[-3000:])
        assert '\x1b' not in result.stdout
        value = json.loads(result.stdout)
        commands.append({'args': list(arguments), 'exit_code': result.returncode})
        return value

    if scenario == 'new-preset':
        run(case, 'new', 'app', '--preset', 'code', '--input', settings, '--yes', *credentials)
    shutil.copytree(root / 'examples/codegen-demo', project, dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns('target', 'state', 'lenso.toml', 'lenso.lock'))
    if scenario == 'init-preset':
        run(project, 'init', '--preset', 'code', '--input', settings, '--yes', *credentials)
    elif scenario in ('ext-add', 'apply-preset'):
        run(project, 'init', '--preset', 'empty', '--yes')
        if scenario == 'ext-add':
            arguments = ('ext', 'add', 'code', '--input', settings, *credentials)
            before = (project / 'lenso.toml').read_bytes()
            pending = run(project, *arguments, expected=2)
            answers = case / 'decline.json'
            answers.write_text(json.dumps({pending['question']['id']: False}))
            run(project, *arguments, '--answers', str(answers), expected=130)
            assert (project / 'lenso.toml').read_bytes() == before
            run(project, *arguments, '--yes')
        else:
            run(project, 'preset', 'apply', 'code', '--input', settings, '--yes', *credentials)

    config = run(project, 'config')['outputs']['main']['config']['cli']
    selected = next(item for item in config['extensions'] if item['id'] == 'code')
    assert selected['source'] == 'registry:@lenso/code'
    if scenario != 'ext-add':
        preset = config['presets']['@lenso/code']
        assert preset['version'] == args.preset_version
        assert preset['extensions']['code']['version'] == '^' + args.code_version
    lock = json.loads((project / 'lenso.lock').read_text())
    pin = next(item for item in lock['extensions'] if item['id'] == 'code')
    assert pin['definition']['version'] == args.code_version
    assert pin['github']['tag'] == 'code-v' + args.code_version
    tree = run(project, 'ext', 'tree', '--locked', '--offline', offline=True)['outputs']['main']['extensions']
    assert {item['id'] for item in tree} == {'code', 'project'}
    checked = run(project, 'code', 'validate')['outputs']['validate']
    assert checked['validated'] and len(checked['samples']) == 3
    assert not (project / 'src/generated').exists()
    run(project, 'code', 'compose-preview', '--sample', 'memory')
    assert not (project / 'src/generated').exists()
    pending = run(project, 'code', 'compose', '--sample', 'memory', expected=2)
    answers = case / 'apply.json'
    answers.write_text(json.dumps({pending['question']['id']: False}))
    run(project, 'code', 'compose', '--sample', 'memory', '--answers', str(answers), expected=130)
    assert not (project / 'src/generated').exists()
    run(project, 'code', 'compose', '--sample', 'memory', '--yes')
    assert (project / 'src/generated/assembly/app.rs').exists()
    if scenario == 'apply-preset':
        run(project, 'preset', 'apply', 'code', '--input', settings, '--yes', *credentials)
    before = (project / 'lenso.lock').read_bytes()
    replay = run(project, 'code', 'compose-preview', '--locked', '--offline', offline=True)
    assert replay['outputs']['compose']['files'] == []
    assert (project / 'lenso.lock').read_bytes() == before
    assert not (project / '.lenso').exists() and not (project / 'node_modules').exists()
    reports.append({'scenario': scenario, 'extension_version': args.code_version,
                    'preset_version': None if scenario == 'ext-add' else args.preset_version,
                    'source': selected['source'], 'release_id': pin['github']['release_id'],
                    'manifest_sha256': pin['github']['metadata']['manifest']['sha256'],
                    'fresh_cache': True, 'empty_path': True, 'validation_and_generation': True,
                    'preview_and_consent': True, 'locked_offline': True, 'commands': commands})
    print('Official Code onboarding passed: ' + scenario, flush=True)

report = {'cli_version': '1.0.0', 'platform': sys.platform, 'cases': reports}
(output / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
print('All official Code alias/preset cases passed: ' + str(output / 'verification.json'), flush=True)
