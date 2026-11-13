#!/usr/bin/env python3
"""Real Wasm project conventions, provider defaults and safe convention migration."""
import json,os,shutil,subprocess,tempfile
from pathlib import Path
root=Path(__file__).resolve().parent.parent
project=Path(tempfile.mkdtemp(prefix='lenso-config-'))
code=project/'lenso/extensions/code';code.mkdir(parents=True)
for name in ['extension.toml','code.wasm']:shutil.copyfile(root/'dist/author/code'/name,code/name)
(project/'preset.toml').write_text('schema_version=1\nid="@test/code"\nversion="1.0.0"\nadopt="code/ready"\n[[extensions]]\nid="code"\nsource="lenso/extensions/code/extension.toml"\n')
author=project/'author';author.mkdir()
(author/'lenso.modules.toml').write_text('''schema_version=1
namespace="test"
[modules.feature]
root="feature"
description="Convention-selected templates with flexible storage"
[[modules.feature.releases]]
version="1.0.0"
reference="working-tree"
[[modules.feature.releases.targets]]
language="typescript"
runtime="node26"
catalog="plain.toml"
when={styles="plain",rsc=true}
[[modules.feature.releases.targets]]
language="typescript"
runtime="node26"
catalog="fancy.toml"
when={styles="fancy",rsc=true}
''')
for style in ['plain','fancy']:
 (author/(style+'.toml')).write_text('''schema_version=1
[modules.feature]
version="1.0.0"
supports={language=["typescript"],runtime=["node26"],styles=["'''+style+'''"]}
[modules.feature.options.label]
prompt="Label"
kind={type="text"}
default="author"
[modules.feature.slots.store]
contract="demo.store"
version="^1"
[modules.sqlite]
version="1.0.0"
provides={"demo.store"={version="1.0.0"}}
[modules.postgres]
version="1.0.0"
provides={"demo.store"={version="1.0.0"}}
[generator]
schema_version=1
directory="generated/feature"
files=[{scope="composition",template="module.j2",target="index.ts"}]
''')
(author/'module.j2').write_text('export const variant = {{ resolution.context.styles|tojson }};\nexport const alias = {{ resolution.context.import_alias|tojson }};\nexport const provider = {{ resolution.instances[resolution.instances.feature.bindings.store].module|tojson }};\nexport const label = {{ resolution.instances.feature.configuration.label|tojson }};\n')
env=dict(os.environ,LENSO_CACHE_DIR=str(project/'cache'))
def call(*args,expected=0):
 r=subprocess.run([str(root/'target/debug/lenso'),'--project',str(project),*args,'--json'],env=env,text=True,capture_output=True,timeout=120)
 assert r.returncode==expected,(args,r.stdout[-4000:],r.stderr[-2000:]);return json.loads(r.stdout)
call('init','--preset','preset.toml','--input',json.dumps({'author_root':'author','format':'lenso-modules-v1','catalog':'lenso.modules.toml'}),'--yes')
settings={'project':{'language':'typescript','runtime':'node26'},'conventions':{'styles':'plain','rsc':True},'paths':{'modules':'src/features','import_alias':'@/features'},'defaults':{'providers':{'demo.store':'sqlite'}},'modules':{'feature':{'options':{'label':'project'}}}}
initial={**settings,'conventions':{}}
call('code','config','--input',json.dumps({'settings':initial}),'--yes')
configfile=project/'lenso.code.toml';before=configfile.read_text()
answers={};answerfile=project/'answers.json'
for value in ['true','plain']:
 answerfile.write_text(json.dumps(answers))
 question=call('code','preview','feature','--answers',str(answerfile),'--yes',expected=2)['question']
 assert question['id'].startswith('code:project-convention:')
 answers[question['id']]=value
answerfile.write_text(json.dumps(answers))
call('code','preview','feature','--answers',str(answerfile),'--yes')
assert configfile.read_text()==before and not (project/'src/features').exists()
pending=call('code','add','feature','--answers',str(answerfile),expected=2)
assert configfile.read_text()==before
answerfile.write_text(json.dumps({**answers,pending['question']['id']:False}))
call('code','add','feature','--answers',str(answerfile),expected=130)
assert configfile.read_text()==before and not (project/'src/features').exists()
answerfile.write_text(json.dumps(answers))
call('code','add','feature','--answers',str(answerfile),'--yes')
assert 'rsc = true' in configfile.read_text() and 'styles = "plain"' in configfile.read_text()
# Reset the fixture so explicit configuration is independently checked below.
configfile.unlink()
pending=call('code','config','--input',json.dumps({'settings':settings}),expected=2)
assert not (project/'lenso.code.toml').exists()
call('code','config','--input',json.dumps({'settings':settings}),'--yes')
configfile=project/'lenso.code.toml';configfile.write_text('# user note\n'+configfile.read_text())
call('code','config','--convention','rsc=true','--yes')
assert configfile.read_text().startswith('# user note')
assert call('code','config')['outputs']['config']['config']['project']['language']=='typescript'
existing=(project/'src/features/feature/assembly/index.ts').read_text()
call('code','preview','feature','--yes');assert (project/'src/features/feature/assembly/index.ts').read_text()==existing
call('code','add','feature','--yes')
receipt=project/'src/code/modules/test/feature.json'
group=json.loads(receipt.read_text())['native_catalog']
output=project/'src/features/feature/assembly/index.ts';text=output.read_text();assert '"plain"' in text and '"sqlite"' in text and '"project"' in text
settings['defaults']['providers']['demo.store']='postgres'
call('code','config','--input',json.dumps({'settings':settings}),'--yes')
assert call('code','preview','feature')['outputs']['resolve']['files']==[]
settings['conventions']['styles']='fancy'
call('code','config','--input',json.dumps({'settings':settings}),'--yes')
call('code','preview','feature',expected=1)
call('code','preview','feature','--reconfigure');assert output.read_text()==text
call('code','add','feature','--reconfigure','--yes')
assert json.loads(receipt.read_text())['native_catalog']==group
assert '"fancy"' in output.read_text() and '"sqlite"' in output.read_text()
assert call('code','preview','feature','--locked','--offline')['outputs']['resolve']['files']==[]
call('code','add','feature','--input',json.dumps({'bindings':{'feature.store':{'type':'module','module':'postgres'}}}),'--yes')
assert '"postgres"' in output.read_text()
settings['project']['language']='rust'
call('code','config','--input',json.dumps({'settings':settings}),'--yes',expected=1)
print('Project config passed: approved config writes, fixed variants, aliases/paths, defaults, retained choices, explicit convention migration and offline replay')
print(project)
