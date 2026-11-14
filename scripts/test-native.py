#!/usr/bin/env python3
"""Actual CLI/Wasm and Cargo/npm installation acceptance using offline local packages."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from project_config import load, save

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--keep', action='store_true')
parser.add_argument('--binary', type=Path, help='Use an installed CLI instead of the workspace build')
parser.add_argument('--extension-source', help='Install a published extension instead of building local Wasm')
parser.add_argument('--extension-version', default='0.2.0')
parser.add_argument('--token-env', default='GITHUB_TOKEN')
parser.add_argument('--out', type=Path, help='New directory for projects, cache and verification report')
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
binary = args.binary.resolve(strict=True) if args.binary else root / 'target/debug/lenso'
scratch = args.out.resolve() if args.out else Path(tempfile.mkdtemp(prefix='lenso-native-'))
if args.out:
    scratch.mkdir(parents=True, exist_ok=False)
project = scratch / 'project'
project.mkdir()
env = dict(os.environ, LENSO_CACHE_DIR=str(scratch / 'cache'))
answers = scratch / 'answers.json'

def run(*arguments, expected=0):
    if arguments[:2] in [('code', 'compose'), ('code', 'compose-preview')] and '--registry' not in arguments:
        arguments = (*arguments, '--registry', 'main')
    result = subprocess.run([str(binary), '--project', str(project), *arguments, '--json'], capture_output=True, text=True, env=env, timeout=120)
    assert result.returncode == expected, (arguments, result.returncode, result.stdout[-5000:], result.stderr[-3000:])
    return json.loads(result.stdout)

def write(path, content):
    path = project / path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)

def catalog(version=1, owners=('app', 'other')):
    body = '''schema_version = 1
[modules.app]
version = "1.0.0"
[modules.other]
version = "1.0.0"
[modules.empty]
version = "1.0.0"
[generator]
schema_version = 1
directory = "generated"
files = [{ scope = "composition", template = "output.j2", target = "graph.txt" }]
[samples.on]
instances = { app = "app" }
[samples.both]
instances = { app = "app", other = "other" }
[samples.other]
instances = { other = "other" }
[samples.none]
instances = { empty = "empty" }
'''
    for owner in owners:
        body += f'''\n[[generator.dependencies]]
ecosystem = "cargo"
package = "native-fixture"
version = "{version}"
module = "{owner}"
options = {{ path = "vendor/rust-{version}" }}
[[generator.dependencies]]
ecosystem = "npm"
package = "native-js"
version = "file:vendor/js-{version}"
module = "{owner}"
'''
    write('author/catalog.toml', body)

def setup(native):
    run('ext', 'configure', 'code', '--input', json.dumps({'native': native}), '--yes')

def manifest_state():
    return ((project / 'Cargo.toml').read_bytes(), (project / 'package.json').read_bytes(), (project / 'lenso/native.lock').read_bytes() if (project / 'lenso/native.lock').exists() else None)

def js_value():
    result = subprocess.run(['node', '-e', 'process.stdout.write(require("native-js"))'], cwd=project, capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    return result.stdout

try:
    write('src/lib.rs', 'pub fn answer() -> u32 { 42 }\n')
    write('Cargo.toml', '# preserve this comment\n[package]\nname="native-acceptance"\nversion="0.1.0"\nedition="2024"\n[workspace]\n')
    initial_package = {'name': 'native-acceptance', 'private': True, 'scripts': {'postinstall': 'node -e "require(\'fs\').writeFileSync(\'unexpected-hook\', \'ran\')"'}, 'dependencies': {'native-js': 'file:vendor/js-user', 'user-js': 'file:vendor/user-js'}}
    write('package.json', json.dumps(initial_package, indent=2) + '\n')
    for version in (1, 2):
        write(f'vendor/rust-{version}/Cargo.toml', f'[package]\nname="native-fixture"\nversion="{version}.0.0"\nedition="2024"\n')
        write(f'vendor/rust-{version}/src/lib.rs', f'pub fn version() -> u32 {{ {version} }}\n')
        write(f'vendor/js-{version}/package.json', json.dumps({'name': 'native-js', 'version': f'{version}.0.0', 'main': 'index.js'}))
        write(f'vendor/js-{version}/index.js', f'module.exports = "{version}";\n')
    for path, name, value in [('js-user', 'native-js', 'user'), ('user-js', 'user-js', 'unrelated')]:
        write(f'vendor/{path}/package.json', json.dumps({'name': name, 'version': '0.1.0', 'main': 'index.js'}))
        write(f'vendor/{path}/index.js', f'module.exports = "{value}";\n')
    write('author/output.j2', '{% for id in resolution.order %}{{ id }}\n{% endfor %}')
    catalog()
    if args.extension_source:
        run('init', '--preset', 'empty', '--yes')
        run('ext', 'add', args.extension_source, '--id', 'code', '--constraint', '=' + args.extension_version,
            '--token-env', args.token_env, '--input', json.dumps({'author_root': 'author', 'target': 'src'}), '--yes')
        pin = next(item for item in json.loads((project / 'lenso.lock').read_text())['extensions'] if item['definition']['id'] == 'code')
        assert pin['definition']['version'] == args.extension_version
        assert pin['github']['tag'] == 'code-v' + args.extension_version
    else:
        subprocess.run(['python3', str(root / 'scripts/configure-code.py'), str(project), '--author-root', 'author'], env=env, check=True)
    native = {'cargo': {'manifest': 'Cargo.toml', 'offline': True}, 'npm': {'manifest': 'package.json', 'offline': True}}
    configured_before = (project / 'lenso.toml').read_bytes()
    grant = run('ext', 'configure', 'code', '--input', json.dumps({'native': native}), expected=2)
    assert grant['question']['id'].startswith('grants:')
    assert 'Cargo.toml' in grant['question']['message'] and 'npm' in grant['question']['message']
    answers.write_text(json.dumps({grant['question']['id']: False}))
    run('ext', 'configure', 'code', '--input', json.dumps({'native': native}), '--answers', str(answers), expected=130)
    assert (project / 'lenso.toml').read_bytes() == configured_before
    setup(native)
    before = manifest_state()
    pending = run('code', 'compose-preview', '--sample', 'on', '--yes', expected=2)
    assert pending['question']['id'].startswith('native:')
    answers.write_text(json.dumps({pending['question']['id']: 'keep'}))
    kept = run('code', 'compose-preview', '--sample', 'on', '--answers', str(answers))
    assert kept['outputs']['compose']['native']['kept'] == 1
    assert manifest_state() == before
    choices = {pending['question']['id']: 'replace'}
    answers.write_text(json.dumps(choices))
    pending = run('code', 'compose', '--sample', 'on', '--answers', str(answers), expected=2)
    assert pending['question']['id'].startswith('apply:')
    choices[pending['question']['id']] = True
    answers.write_text(json.dumps(choices))
    pending = run('code', 'compose', '--sample', 'on', '--answers', str(answers), expected=2)
    assert pending['question']['id'].startswith('commands:')
    choices[pending['question']['id']] = False
    answers.write_text(json.dumps(choices))
    run('code', 'compose', '--sample', 'on', '--answers', str(answers), expected=130)
    assert manifest_state() == before, 'command denial changed a manifest or ownership state'
    choices[pending['question']['id']] = True
    answers.write_text(json.dumps(choices))
    installed = run('code', 'compose', '--sample', 'on', '--answers', str(answers))
    assert installed['outputs']['apply']['finalized'] == ['lenso/native.lock']
    assert js_value() == '1'
    assert (project / 'Cargo.lock').exists() and (project / 'package-lock.json').exists()
    assert not (project / 'unexpected-hook').exists()
    assert '# preserve this comment' in (project / 'Cargo.toml').read_text()
    replay = run('code', 'compose-preview')['outputs']['compose']
    assert replay['files'] == [] and replay['commands'] == [] and replay['finalizations'] == []
    run('code', 'compose', '--sample', 'both', '--yes')
    run('code', 'compose', '--sample', 'other', '--yes')
    assert js_value() == '1', 'remaining module owner lost its dependency'
    # Another composition target shares the project-level native ownership record.
    config = load(project / 'lenso.toml')
    extension = config['cli']['extensions'][0]
    extension['options']['registries']['secondary'] = {**extension['options']['registries']['main'], 'target': 'secondary'}
    extension['read_roots'].append('secondary')
    extension['write_roots'].append('secondary')
    save(project / 'lenso.toml', config)
    run('code', 'compose', '--registry', 'secondary', '--sample', 'other', '--yes')
    run('code', 'compose', '--registry', 'main', '--sample', 'none', '--yes')
    assert js_value() == '1'
    # Restore one profile before failure/update checks.
    run('code', 'compose', '--registry', 'main', '--sample', 'on', '--yes')
    run('code', 'compose', '--registry', 'secondary', '--sample', 'none', '--yes')
    # Force installation checks exact process grants before any mutation.
    original_config = (project / 'lenso.toml').read_text()
    denied_config = load(project / 'lenso.toml')
    denied_config['cli']['extensions'][0]['processes'] = []
    save(project / 'lenso.toml', denied_config)
    before = manifest_state()
    denied = run('code', 'compose', '--registry', 'main', '--install', '--yes', expected=1)
    assert denied['code'] == 'permission_denied' and denied['details']['capability'] == 'host.processes' and manifest_state() == before
    (project / 'lenso.toml').write_text(original_config)
    # Failure persists pending original baselines; retry prepares the same requirements.
    fail = scratch / 'fail-cargo'
    fail.write_text('#!/bin/sh\nexit 7\n')
    fail.chmod(0o755)
    setup({**native, 'cargo': {**native['cargo'], 'program': str(fail)}})
    failed = run('code', 'compose', '--sample', 'on', '--install', '--yes', expected=1)
    assert failed['code'] == 'partial_failure' and failed['details']['completed_files']
    assert 'installed =' not in (project / 'lenso/native.lock').read_text()
    setup(native)
    retried = run('code', 'compose', '--sample', 'on', '--yes')
    assert len(retried['outputs']['compose']['commands']) == 2
    assert js_value() == '1'
    catalog(2)
    before = manifest_state()
    updated = run('code', 'compose-preview')['outputs']['compose']
    assert updated['native']['changed'] == 2 and manifest_state() == before
    run('code', 'compose', '--yes')
    assert js_value() == '2'
    assert 'version = "2.0.0"' in (project / 'Cargo.lock').read_text()
    # A failed last-owner retirement must keep a retry marker, even though no
    # dependency requests remain in the new composition.
    setup({**native, 'cargo': {**native['cargo'], 'program': str(fail)}})
    run('code', 'compose', '--sample', 'none', '--yes', expected=1)
    setup(native)
    retired = run('code', 'compose', '--sample', 'none', '--yes')
    assert len(retired['outputs']['compose']['commands']) == 2
    assert js_value() == 'user', 'retirement did not restore the initial user declaration'
    pending = run('code', 'compose', '--sample', 'on', '--yes', expected=2)
    answers.write_text(json.dumps({pending['question']['id']: 'replace'}))
    run('code', 'compose', '--sample', 'on', '--answers', str(answers), '--yes')
    # User changes are never silently erased by dependency retirement.
    package = json.loads((project / 'package.json').read_text())
    package['dependencies']['native-js'] = 'file:vendor/js-1'
    write('package.json', json.dumps(package, indent=2) + '\n')
    pending = run('code', 'compose', '--sample', 'none', '--yes', expected=2)
    assert pending['question']['id'].startswith('native:')
    answers.write_text(json.dumps({pending['question']['id']: 'keep'}))
    run('code', 'compose', '--sample', 'none', '--answers', str(answers), '--yes')
    assert json.loads((project / 'package.json').read_text())['dependencies']['native-js'] == 'file:vendor/js-1'
    assert 'native-fixture' not in (project / 'Cargo.toml').read_text()
    # A fresh ownership cycle restores the user declaration captured at adoption.
    pending = run('code', 'compose', '--sample', 'on', '--yes', expected=2)
    answers.write_text(json.dumps({pending['question']['id']: 'replace'}))
    run('code', 'compose', '--sample', 'on', '--answers', str(answers), '--yes')
    run('code', 'compose', '--sample', 'none', '--yes')
    assert js_value() == '1'
    assert 'user-js' in json.loads((project / 'package.json').read_text())['dependencies']
    assert run('code', 'compose-preview')['outputs']['compose']['commands'] == []
    print('Native dependency acceptance passed: actual offline Cargo/npm installation, manifest preservation, two-phase authorization, updates, shared module/composition owners, failure/retry, user conflicts, restoration and safe removal', flush=True)
    (scratch / 'verification.json').write_text(json.dumps({
        'extension_source': args.extension_source, 'version': args.extension_version if args.extension_source else None,
        'cli_version': subprocess.check_output([str(binary), '--version'], text=True).strip(),
        'cargo_npm_install': True, 'updates': True, 'shared_ownership': True,
        'conflict_preservation': True, 'failure_retry': True, 'safe_removal': True,
        'github': pin['github'] if args.extension_source else None,
    }, indent=2) + '\n')
except BaseException:
    print(f'Failed project retained: {project}', flush=True)
    raise
else:
    if not args.keep and not args.out:
        shutil.rmtree(scratch)
