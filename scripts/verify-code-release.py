#!/usr/bin/env python3
"""Verify a published Code package using the installed CLI and existing acceptance suites."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from author_acceptance import verify

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--repo', required=True)
parser.add_argument('--version', default='0.2.0')
parser.add_argument('--cli-version', default='1.0.0')
parser.add_argument('--ref', required=True)
parser.add_argument('--token-env', default='GITHUB_TOKEN')
parser.add_argument('--out', type=Path, required=True)
args = parser.parse_args()
if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', args.repo):
    parser.error('--repo must be owner/repo')
if not re.fullmatch(r'[a-fA-F0-9]{40}', args.ref):
    parser.error('--ref must be a full source commit SHA')
token = os.environ.get(args.token_env)
if not token:
    parser.error('the named credential environment variable must be set')
root = Path(__file__).resolve().parent.parent
binary = args.binary.resolve(strict=True)
assert subprocess.check_output([str(binary), '--version'], text=True).strip() == 'lenso ' + args.cli_version
scratch = args.out.resolve()
scratch.mkdir(parents=True, exist_ok=False)
source = 'github-release:' + args.repo + '/code'
env = dict(os.environ, LENSO_CACHE_DIR=str(scratch / 'cache'))
empty_path = scratch / 'empty-path'
empty_path.mkdir()
empty_env = dict(env, PATH=str(empty_path))
commands = []


def run(project, *arguments, expected=0, overrides=None):
    result = subprocess.run([str(binary), '--project', str(project), *arguments, '--json'],
                            env=env if overrides is None else overrides, capture_output=True, text=True, timeout=120)
    assert token not in result.stdout + result.stderr, 'credential leaked in CLI output'
    assert result.returncode == expected, (arguments, result.returncode, result.stdout[-5000:], result.stderr[-3000:])
    assert '\x1b' not in result.stdout, 'terminal formatting in JSON'
    commands.append({'args': list(arguments), 'exit_code': result.returncode})
    return json.loads(result.stdout)


def setup(project):
    run(project, 'init', '--preset', 'empty', '--yes')
    run(project, 'ext', 'add', source, '--id', 'code', '--constraint', '=' + args.version,
        '--token-env', args.token_env, '--input', json.dumps({'author_root': 'author', 'target': 'src'}), '--yes')
    lock = json.loads((project / 'lenso.lock').read_text())
    pin = next(item for item in lock['extensions'] if item['definition']['id'] == 'code')
    assert pin['definition']['version'] == args.version
    assert pin['github']['tag'] == 'code-v' + args.version
    (scratch / 'extension-pin.json').write_text(json.dumps(pin['github'], indent=2) + '\n')


def application(project):
    return {str(path.relative_to(project)): path.read_bytes() for path in (project / 'src').rglob('*') if path.is_file()}


subprocess.run([sys.executable, str(root / 'scripts/verify-github-release.py'), '--binary', str(binary),
                '--repo', args.repo, '--version', args.version, '--ref', args.ref,
                '--token-env', args.token_env, '--out', str(scratch / 'resources')], check=True)
verify(root, scratch, None, None, run, application, empty_env, setup=setup)
subprocess.run([sys.executable, str(root / 'scripts/test-native.py'), '--binary', str(binary),
                '--extension-source', source, '--extension-version', args.version,
                '--token-env', args.token_env, '--out', str(scratch / 'native')], check=True)
report = {'cli_version': args.cli_version, 'extension_version': args.version, 'source': source,
          'source_commit': args.ref, 'resource_download_and_cache_restore': True,
          'compiled_memory_disk_shared': True, 'adapter_cleanup': True, 'local_update_merge': True,
          'preview_and_authorization': True, 'locked_offline': True,
          'native_dependencies': json.loads((scratch / 'native/verification.json').read_text()), 'commands': commands}
(scratch / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
print('Published Code release acceptance passed: ' + str(scratch / 'verification.json'), flush=True)
