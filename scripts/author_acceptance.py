"""Actual Wasm author validation plus compiler acceptance for generated adapter assembly."""
import copy
import json
import os
import shutil
import subprocess
from project_config import load, save


def verify(root, scratch, config, snapshot, run, application, empty_path_env, setup=None):
    project = scratch / 'author-demo'
    shutil.copytree(root / 'examples/codegen-demo', project)
    if setup:
        setup(project)
    else:
        subprocess.run(['python3', str(root / 'scripts/configure-code.py'), str(project), '--author-root', 'author'], check=True)
        declaration = load(project / 'lenso.toml')['cli']['extensions'][0]['source']
        run(project, 'ext', 'remove', 'code', '--yes')
        run(project, 'ext', 'add', declaration, '--id', 'code', '--input', json.dumps({'author_root': 'author', 'target': 'src'}), '--yes')
    authored = {path: path.read_bytes() for path in [project / 'src/main.rs', project / 'src/business.rs']}
    checked = run(project, 'code', 'validate', overrides=empty_path_env)
    assert checked['outputs']['validate']['validated']
    assert len(checked['outputs']['validate']['samples']) == 3
    assert checked['outputs']['validate']['uncovered_modules'] == []
    assert not (project / 'src/generated').exists()
    assert not (project / 'state').exists(), 'author validation executed application code'
    before = application(project)
    preview = run(project, 'code', 'compose-preview', '--sample', 'memory')
    assert application(project) == before
    files = preview['outputs']['compose']['files']
    assert all(file['path'].startswith('src/generated/') or file['path'] == 'src/composition.json' for file in files)
    pending = run(project, 'code', 'compose', '--sample', 'memory', expected=2)
    answers = scratch / 'author-answers.json'
    answers.write_text(json.dumps({pending['question']['id']: False}))
    run(project, 'code', 'compose', '--sample', 'memory', '--answers', str(answers), expected=130)
    assert application(project) == before
    run(project, 'code', 'compose', '--sample', 'memory', '--yes')

    def compile_run(expected):
        result = subprocess.run([os.environ.get('CARGO', 'cargo'), 'run', '--offline', '--quiet'], cwd=project, capture_output=True, text=True, timeout=90)
        assert result.returncode == 0, (result.stdout, result.stderr)
        assert result.stdout == expected, result.stdout
        for path, content in authored.items():
            assert path.read_bytes() == content, 'generator changed a user-owned entry or contract'

    compile_run('primary: Hello Lenso\n')
    assert not (project / 'state').exists()
    assert run(project, 'code', 'compose-preview')['outputs']['compose']['files'] == []
    before = application(project)
    switch = run(project, 'code', 'compose-preview', '--sample', 'disk')
    assert application(project) == before
    assert 'src/generated/modules/memory/adapter.rs' in switch['outputs']['compose']['deletions']
    assert 'src/generated/instances/primary.store/factory.rs' not in switch['outputs']['compose']['deletions'], 'same instance path should be updated, not deleted twice'
    run(project, 'code', 'compose', '--sample', 'disk', '--yes')
    assert not (project / 'src/generated/modules/memory/adapter.rs').exists()
    compile_run('primary: Welcome Lenso\n')
    assert (project / 'state/6e616d65').read_text() == 'Lenso'
    run(project, 'code', 'compose', '--sample', 'shared', '--yes')
    compile_run('primary: Hello Lenso\nsecondary: Hi Lenso\n')
    assert not (project / 'src/generated/modules/disk/adapter.rs').exists()
    receipt = json.loads((project / 'src/composition.json').read_text())['generation']['files']
    assert receipt['src/generated/modules/memory/adapter.rs']['owners'] == ['shared']
    # Template changes use the same three-way update path as copied source files.
    factory = project / 'src/generated/instances/shared/factory.rs'
    factory.write_text(factory.read_text() + '// local factory customization\n')
    memory_template = project / 'author/templates/memory.rs.j2'
    memory_template.write_text('// upstream generator revision\n' + memory_template.read_text())
    before = application(project)
    updated = run(project, 'code', 'compose-preview')
    assert updated['outputs']['compose']['summary']['merged'] == 1
    assert application(project) == before
    run(project, 'code', 'compose', '--yes')
    assert 'local factory customization' in factory.read_text() and 'upstream generator revision' in factory.read_text()
    compile_run('primary: Hello Lenso\nsecondary: Hi Lenso\n')
    # All sources are checked, including the unselected disk template.
    template = project / 'author/templates/disk.rs.j2'
    original_template = template.read_bytes()
    template.unlink()
    failed = run(project, 'code', 'validate', expected=1)
    assert failed['code'] == 'wasm_failed' and failed['recovery']['kind'] == 'inspect_extension'
    template.write_bytes(original_template)
    template.write_text('{% invalid %}')
    run(project, 'code', 'validate', expected=1)
    template.write_bytes(original_template)
    # Output declarations must stay below a generated directory.
    catalog = project / 'author/catalog.toml'
    original_catalog = catalog.read_text()
    catalog.write_text(original_catalog.replace('target = "app.rs"', 'target = "../business.rs"'))
    run(project, 'code', 'validate', expected=1)
    catalog.write_text(original_catalog)
    before = application(project)
    policy = load(project / 'lenso.toml')
    original_policy = copy.deepcopy(policy)
    policy['cli']['extensions'][0]['read_roots'] = ['src']
    save(project / 'lenso.toml', policy)
    denied = run(project, 'code', 'validate', expected=1)
    assert denied['code'] == 'permission_denied' and denied['details']['capability'] == 'host.reads'
    assert application(project) == before
    if snapshot is None:
        save(project / 'lenso.toml', original_policy)
        assert run(project, 'code', 'compose-preview', '--locked', '--offline', overrides=empty_path_env)['outputs']['compose']['files'] == []
        print('Published author acceptance passed: validation, compiled memory/disk/shared samples, adapter cleanup, local update merge, denial and locked/offline replay', flush=True)
        return
    # The exact author catalog works through pinned GitHub sources as well.
    resources = {str(path.relative_to(project / 'author')): path.read_bytes() for path in (project / 'author').rglob('*') if path.is_file()}
    snapshot('acme/author-demo', '1' * 40, resources)
    remote = scratch / 'author-remote'
    remote.mkdir()
    remote_policy = copy.deepcopy(config)
    extension = remote_policy['cli']['extensions'][0]
    extension['sources'] = ['github:acme/author-demo']
    extension['options']['registries']['main'].update({'source': 'github-repo:acme/author-demo', 'ref': '1' * 40, 'format': 'lenso-compose-v1'})
    save(remote / 'lenso.toml', remote_policy)
    run(remote, 'code', 'validate')
    run(remote, 'code', 'compose', '--sample', 'memory', '--yes')
    assert run(remote, 'code', 'compose-preview', '--locked', '--offline', overrides=empty_path_env)['outputs']['compose']['files'] == []
    print('Author generation acceptance passed: strict validation, isolated output, real memory/disk Rust compilation and execution, explicit shared instance, adapter cleanup, no-op replay, denial, local grants and pinned remote templates', flush=True)
