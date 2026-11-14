"""Composition checks using the GitHub acceptance server and the actual CLI/Wasm."""
import copy
import json
from project_config import save


def verify(root, scratch, config, snapshot, run, application, empty_path_env):
    fixture = root / 'fixtures/composition'
    sources = {
        str(path.relative_to(fixture)): path.read_bytes()
        for path in fixture.rglob('*') if path.is_file()
    }
    snapshot('acme/composition', '1' * 40, sources)
    project = scratch / 'composition'
    project.mkdir()
    policy = copy.deepcopy(config)
    extension = policy['cli']['extensions'][0]
    extension['sources'] = ['github:acme/composition']
    extension['options']['registries']['main'].update({
        'source': 'github-repo:acme/composition', 'ref': '1' * 40,
        'format': 'lenso-compose-v1', 'target': 'src'
    })
    save(project / 'lenso.toml', policy)
    intent = {'instances': {'auth': 'auth'}, 'context': {'language': 'typescript'}}
    pending = run(project, 'code', 'compose-preview', '--input', json.dumps(intent), '--yes', expected=2)
    assert 'auth.store' in pending['question']['message'], pending
    assert not application(project)
    answers = scratch / 'composition-answers.json'
    answers.write_text(json.dumps({pending['question']['id']: 'module:sqlite'}))
    preview = run(project, 'code', 'compose-preview', '--input', json.dumps(intent), '--answers', str(answers))
    composition = preview['outputs']['compose']['composition']
    assert composition['resolution']['instances']['auth.store']['module'] == 'sqlite'
    assert 'auth.cache' not in composition['resolution']['instances']
    assert not application(project)
    ready = composition['request']
    pending = run(project, 'code', 'compose', '--input', json.dumps(ready), expected=2)
    answers.write_text(json.dumps({pending['question']['id']: False}))
    run(project, 'code', 'compose', '--input', json.dumps(ready), '--answers', str(answers), expected=130)
    assert not application(project)
    run(project, 'code', 'compose', '--input', json.dumps(ready), '--yes')
    artifact = project / 'src/composition.json'
    assert json.loads(artifact.read_text())['request'] == ready
    replay = run(project, 'code', 'compose-preview', '--offline', '--locked', overrides=empty_path_env)
    assert replay['outputs']['compose']['composition_id'] == composition['digest']
    intent = {
        'instances': {'auth': 'auth', 'staff': 'auth', 'hash': 'argon2'},
        'context': {'language': 'typescript'},
        'configuration': {
            'auth': {'cache-enabled': True},
            'auth.store': {'connection': {'env': 'DATABASE_URL'}},
            'auth.cache': {'connection': {'env': 'REDIS_URL'}}
        },
        'bindings': {
            'auth.store': {'type': 'module', 'module': 'pg'},
            'staff.store': {'type': 'module', 'module': 'sqlite'},
            'auth.cache': {'type': 'module', 'module': 'redis'},
            'auth.hasher': {'type': 'instance', 'instance': 'hash'},
            'staff.hasher': {'type': 'instance', 'instance': 'hash'}
        }
    }
    before = application(project)
    preview = run(project, 'code', 'compose-preview', '--input', json.dumps(intent))
    assert application(project) == before
    output = preview['outputs']['compose']
    assert len(output['composition']['resolution']['instances']) == 6
    assert len(output['composition']['generation']['files']['src/modules/argon2/index.ts']['owners']) == 1
    assert not any(file['path'] == 'src/modules/argon2/index.ts' for file in output['files']), 'unchanged shared source must not be rewritten'
    assert output['composition']['resolution']['instances']['auth.store']['configuration']['connection'] == {'env': 'DATABASE_URL'}
    run(project, 'code', 'compose', '--input', json.dumps(intent), '--yes')
    restored = json.loads(artifact.read_text())
    assert restored['resolution']['instances']['staff']['bindings']['hasher'] == 'hash'
    intent = {'configuration': dict(restored['request']['configuration'])}
    intent['configuration']['auth']['cache-enabled'] = False
    disabled = run(project, 'code', 'compose', '--input', json.dumps(intent), '--yes')
    assert 'auth.cache' not in disabled['outputs']['compose']['composition']['resolution']['instances']
    assert not (project / 'src/modules/redis/index.ts').exists()
    assert disabled['outputs']['compose']['summary']['deleted'] == 1
    intent['configuration']['auth']['cache-enabled'] = True
    enabled = run(project, 'code', 'compose-preview', '--input', json.dumps(intent), '--offline', '--locked')
    assert enabled['outputs']['compose']['composition_id'] == restored['digest']
    # Re-enable, then protect a locally edited obsolete adapter and release its ownership.
    run(project, 'code', 'compose', '--input', json.dumps(intent), '--yes')
    redis = project / 'src/modules/redis/index.ts'
    redis.write_text(redis.read_text() + '// user customization\n')
    intent['configuration']['auth']['cache-enabled'] = False
    pending = run(project, 'code', 'compose', '--input', json.dumps(intent), '--yes', expected=2)
    assert pending['question']['id'].startswith('generation:')
    answers.write_text(json.dumps({pending['question']['id']: 'keep'}))
    run(project, 'code', 'compose', '--input', json.dumps(intent), '--answers', str(answers), '--yes')
    assert 'user customization' in redis.read_text()
    assert 'src/modules/redis/index.ts' not in json.loads(artifact.read_text())['generation']['files']
    assert run(project, 'code', 'compose-preview')['outputs']['compose']['files'] == []
    # A new revision changes one end of the source while a local edit changes the other.
    sqlite = project / 'src/modules/sqlite/index.ts'
    original = sqlite.read_text()
    sqlite.write_text('// user header\n' + original)
    updated = dict(sources)
    updated['adapters/sqlite.ts'] = sources['adapters/sqlite.ts'] + b'export const revision = 2;\n'
    snapshot('acme/composition', '2' * 40, updated)
    extension['options']['registries']['main']['ref'] = '2' * 40
    save(project / 'lenso.toml', policy)
    before = application(project)
    proposed = run(project, 'code', 'compose-preview')
    assert proposed['outputs']['compose']['summary']['merged'] == 1
    assert application(project) == before
    run(project, 'code', 'compose', '--yes')
    assert sqlite.read_text().startswith('// user header\n') and 'revision = 2' in sqlite.read_text()
    assert run(project, 'code', 'compose-preview')['outputs']['compose']['files'] == []
    # Overlapping edits ask a domain question, even with --yes. The old answer is
    # invalid when a different current preimage is observed.
    pg = project / 'src/modules/pg/index.ts'
    pg.write_text('user replacement\n')
    newer = dict(updated)
    newer['adapters/pg.ts'] = b'upstream replacement\n'
    snapshot('acme/composition', '3' * 40, newer)
    extension['options']['registries']['main']['ref'] = '3' * 40
    save(project / 'lenso.toml', policy)
    pending = run(project, 'code', 'compose', '--yes', expected=2)
    answers.write_text(json.dumps({pending['question']['id']: 'overwrite'}))
    pg.write_text('changed after choice\n')
    stale = run(project, 'code', 'compose', '--answers', str(answers), '--yes', expected=2)
    assert stale['question']['id'] != pending['question']['id']
    answers.write_text(json.dumps({stale['question']['id']: 'overwrite'}))
    run(project, 'code', 'compose', '--answers', str(answers), '--yes')
    assert pg.read_text() == 'upstream replacement\n'
    # Keep a remaining owner: the SQLite source survives until staff switches away.
    final = {'instances': {'auth': 'auth'}, 'context': {'language': 'typescript'},
             'configuration': {'auth.store': {'connection': {'env': 'DATABASE_URL'}}},
             'bindings': {'auth.store': {'type': 'module', 'module': 'pg'}}}
    pending = run(project, 'code', 'compose', '--input', json.dumps(final), '--yes', expected=2)
    assert 'sqlite' in pending['question']['message']
    answers.write_text(json.dumps({pending['question']['id']: 'overwrite'}))
    run(project, 'code', 'compose', '--input', json.dumps(final), '--answers', str(answers), '--yes')
    assert not sqlite.exists()
    assert redis.exists(), 'ownership-released local files must never be collected later'
    assert run(project, 'code', 'compose-preview', '--locked', '--offline')['outputs']['compose']['files'] == []
    save(project / 'lenso.toml', {**policy, 'cli': {**policy['cli'], 'extensions': [{**extension, 'sources': []}]}})
    denied = run(project, 'code', 'compose-preview', expected=1)
    assert denied['code'] == 'permission_denied' and denied['details']['capability'] == 'host.sources'
    print('Composition acceptance passed: actual Wasm, scoped choices, SQLite/PG, optional Redis, shared instances, persisted intent, preview/denial/apply, locked offline replay without Node/Git and repository grants', flush=True)
