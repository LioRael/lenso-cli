#!/usr/bin/env python3
"""Actual CLI/Wasm acceptance for Release packages and immutable repository resources."""
import argparse
import base64
import collections
import copy
import hashlib
import http.server
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
from urllib.parse import urlsplit, parse_qs, unquote
from project_config import load, save
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--keep',action='store_true')
args=parser.parse_args()
root=Path(__file__).resolve().parent.parent
binary=root/'target/debug/lenso'
scratch=Path(tempfile.mkdtemp(prefix='lenso-github-'))
cache=scratch/'cache'
token='test-github-private-token-83b01'
signed='test-ephemeral-download-19f8a'
env=dict(os.environ,LENSO_CACHE_DIR=str(cache),LENSO_TEST_GH_TOKEN=token)
requests=collections.Counter()
assets={}
releases=[]
A='1'*40
B='2'*40
revision=A
files={}
trees={}
leaks=[]

def snapshot(repo,sha,data,modes=None):
    files[(repo,sha)]=dict(data)
    dirs={''}
    for path in data:
        parts=path.split('/')
        dirs.update('/'.join(parts[:i]) for i in range(1,len(parts)))
    ids={directory:hashlib.sha1((repo+sha+directory).encode()).hexdigest() for directory in dirs}
    ids['']=sha
    for directory in dirs:
        entries=[]
        prefix=directory+'/' if directory else ''
        children=set()
        for path,body in data.items():
            if not path.startswith(prefix):continue
            rest=path[len(prefix):]
            name=rest.split('/')[0]
            if name in children:continue
            children.add(name)
            if '/' in rest:
                entries.append({'path':name,'mode':'040000','type':'tree','sha':ids[prefix+name]})
            else:
                entries.append({'path':name,'mode':(modes or {}).get(path,'100644'),'type':'blob','sha':hashlib.sha1(b'blob '+str(len(body)).encode()+b'\0'+body).hexdigest()})
        trees[(repo,ids[directory])]={'tree':entries,'truncated':False}

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        url=urlsplit(self.path)
        path=unquote(url.path)
        requests[path]+=1
        if path=='/index/code.json':
            if self.headers.get('Authorization'):leaks.append('API credential reached public Registry')
            return self.reply(200,{'schema_version':1,'name':'@test/code','releases':[{'version':'0.1.0','source':'github-release:acme/tools/code','sha256':hashlib.sha256(assets[101]).hexdigest()}]})
        if path.startswith('/download/'):
            if self.headers.get('Authorization'):leaks.append('API credential reached download redirect')
            if parse_qs(url.query).get('temporary') != [signed]:return self.reply(403,b'')
            return self.reply(200,assets[int(path.rsplit('/',1)[1])])
        if self.headers.get('Authorization') != 'Bearer '+token:return self.reply(403,b'')
        pieces=path.strip('/').split('/')
        if len(pieces)<4 or pieces[0]!='repos':return self.reply(404,b'')
        repo='/'.join(pieces[1:3]);route=pieces[3:]
        if route[:1]==['commits']:
            return self.reply(200,{'sha':A if repo=='acme/tools' else revision})
        if route==['releases']:
            page=int(parse_qs(url.query).get('page',['1'])[0])
            return self.reply(200,releases[(page-1)*30:page*30])
        if route[:2]==['releases','assets']:
            asset=int(route[2])
            if asset not in assets:return self.reply(404,b'')
            self.send_response(302)
            self.send_header('Location',f'/download/{asset}?temporary={signed}')
            self.end_headers();return
        if route[:2]==['git','trees']:
            item=trees.get((repo,route[2]))
            return self.reply(200 if item else 404,item or b'')
        if route[:1]==['contents']:
            ref=parse_qs(url.query).get('ref',[''])[0]
            name='/'.join(route[1:]);body=files.get((repo,ref),{}).get(name)
            if body is None:return self.reply(404,b'')
            return self.reply(200,{'type':'file','encoding':'base64','size':len(body),'sha':hashlib.sha1(b'blob '+str(len(body)).encode()+b'\0'+body).hexdigest(),'content':base64.b64encode(body).decode()})
        self.reply(404,b'')
    def reply(self,status,body):
        body=json.dumps(body).encode() if not isinstance(body,bytes) else body
        self.send_response(status);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    def log_message(self,*args):pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
api=f'http://127.0.0.1:{server.server_port}/'
env.update(NO_PROXY='127.0.0.1,localhost',no_proxy='127.0.0.1,localhost')

def run(project,*arguments,expected=0,overrides=None):
    result=subprocess.run([str(binary),'--project',str(project),*arguments,'--json'],capture_output=True,text=True,env=env if overrides is None else overrides,timeout=90)
    assert result.returncode==expected,(arguments,result.returncode,result.stdout[-4000:],result.stderr[-1500:])
    assert token not in result.stdout+result.stderr and signed not in result.stdout+result.stderr
    return json.loads(result.stdout)

def application(project):
    return {str(path.relative_to(project)):path.read_bytes() for path in (project/'src').rglob('*') if path.is_file()} if (project/'src').exists() else {}

try:
    first=scratch/'first';first.mkdir()
    subprocess.run(['python3',str(root/'scripts/configure-code.py'),str(first),'--repo','acme/resources','--ref','main','--github-api',api,'--token-env','LENSO_TEST_GH_TOKEN'],env=env,check=True)
    config=load(first/'lenso.toml')
    local=first/Path(config['cli']['extensions'][0]['source']).parent
    author=scratch/'author';package=author/'extensions/code';package.mkdir(parents=True)
    shutil.copyfile(local/'code.wasm',package/'code.wasm')
    shutil.copyfile(root/'presets/code/extension.toml',package/'extension.toml')
    (package/'LICENSE').write_text('Acceptance fixture license\n')
    descriptor='schema_version = 1\n[[extensions]]\nid = "code"\npath = "extensions/code"\ntag = "code-v{version}"\nasset = "custom-code-{version}.tgz"\ninclude = ["LICENSE"]\n'
    (author/'lenso.registry.toml').write_text(descriptor)
    snapshot('acme/tools',A,{'lenso.registry.toml':descriptor.encode()})
    def publish(version,number):
        manifest=re.sub(r'(?m)^version = "[^"]+"$', f'version = "{version}"', (root/'presets/code/extension.toml').read_text(), count=1)
        (package/'extension.toml').write_text(manifest)
        result=run(author,'package','--id','code','--out',f'dist/{version}')
        assert result['outputs']['main']['tag']=='code-v'+version
        output=author/'dist'/version
        before=(output/f'custom-code-{version}.tgz').read_bytes()
        run(author,'package','--id','code','--out',f'dist/{version}')
        assert (output/f'custom-code-{version}.tgz').read_bytes()==before,'package must be deterministic'
        names=['lenso-release.json','extension.toml',f'custom-code-{version}.tgz']
        for index,name in enumerate(names):assets[number+index]=(output/name).read_bytes()
        return {'id':number,'tag_name':'code-v'+version,'draft':False,'prerelease':False,'assets':[{'id':number+i,'name':name} for i,name in enumerate(names)]}
    releases.append(publish('0.1.0',100))
    config['cli']['extensions'][0]['source']='github-release:acme/tools/code'
    config['cli']['extensions'][0]['version']='^0.1'
    save(first/'lenso.toml',config)
    catalog=b'schema_version = 1\nname = "acceptance"\n[[items]]\nname = "button"\ndescription = "Repository button"\nfiles = ["components/button.tsx"]\n'
    snapshot('acme/resources',A,{'catalog.toml':catalog,'components/button.tsx':b'export function RepoButton() { return <button>Version one</button> }\n'})
    snapshot('acme/resources',B,{'catalog.toml':catalog,'components/button.tsx':b'export function RepoButton() { return <button>Version two</button> }\n','only-new.txt':b'new snapshot only'})
    run(first,'code','preview','button','--locked',expected=1)
    assert not (first/'lenso.lock').exists()
    help_result=subprocess.run([str(binary),'--project',str(first),'--help'],env=env,capture_output=True,text=True,timeout=45)
    assert help_result.returncode==0 and 'code' in help_result.stdout
    assert requests['/repos/acme/tools/releases/assets/102']==0,'help downloaded the executable package'
    assert not (first/'lenso.lock').exists(),'help wrote a lock'
    preview=run(first,'code','preview','button')
    assert preview['outputs']['resolve']['commit']==A
    assert preview['outputs']['resolve']['files'][0]['path']=='src/components/button.tsx'
    assert not application(first)
    assert requests['/repos/acme/tools/releases/assets/102']==1
    lock=json.loads((first/'lenso.lock').read_text())
    assert lock['extensions'][0]['github']['tag']=='code-v0.1.0' and lock['resources']
    run(first,'code','search','--query','button')
    pending=run(first,'code','add','button',expected=2)
    answers=scratch/'answers.json';answers.write_text(json.dumps({pending['question']['id']:False}))
    run(first,'code','add','button','--answers',str(answers),expected=130)
    assert not application(first)
    installed=run(first,'code','add','button','--yes')
    assert installed['outputs']['verify']['verified']=='button'
    target=first/'src/components/button.tsx'
    pinned=(first/'lenso.lock').read_bytes()
    revision=B
    assert run(first,'code','diff','button')['outputs']['resolve']['summary']['changed']==0
    assert (first/'lenso.lock').read_bytes()==pinned,'floating ref advanced without update'
    offline=run(first,'code','diff','button','--locked','--offline')
    assert offline['outputs']['resolve']['commit']==A
    target.write_text(target.read_text()+'// local modification\n')
    run(first,'code','update','button','--locked',expected=1)
    conflict=run(first,'code','update','button','--yes',expected=2)
    assert conflict['question']['id']=='conflict:src/components/button.tsx'
    answers.write_text(json.dumps({conflict['question']['id']:'keep'}))
    kept=run(first,'code','update','button','--answers',str(answers),'--yes')
    assert kept['outputs']['resolve']['summary']['kept']==1 and 'local modification' in target.read_text()
    difference=run(first,'code','diff','button')
    assert 'Version two' in difference['outputs']['plan']['diff']
    answers.write_text(json.dumps({conflict['question']['id']:'overwrite'}))
    run(first,'code','update','button','--answers',str(answers),'--yes')
    assert 'Version two' in target.read_text() and 'local modification' not in target.read_text()
    policy_path=first/'lenso.toml'
    original_policy=policy_path.read_text()
    custom=load(policy_path)
    custom['cli']['extensions'][0]['options']['registries']['main'].update({'format':'inline','items':[{'name':'missing','files':['only-new.txt']}]})
    # A different ref already has this file, but a full pinned commit must never fall back.
    custom['cli']['extensions'][0]['options']['registries']['main']['ref']=A
    save(policy_path,custom)
    absent=run(first,'code','preview','missing',expected=1)
    assert absent['code']=='invalid_input' and absent['details']['reason']=='not_found_in_snapshot'
    C='3'*40
    snapshot('acme/resources',C,{'link.txt':b'dereferenced contents'},modes={'link.txt':'120000'})
    custom['cli']['extensions'][0]['options']['registries']['main'].update({'ref':C,'items':[{'name':'link','files':['link.txt']}]})
    save(policy_path,custom)
    link=run(first,'code','preview','link',expected=1)
    assert link['code']=='permission_denied' and link['details']['restriction']=='regular_source_file'
    policy_path.write_text(original_policy)
    # The same release/module and source snapshot reuse immutable cached bytes in another project.
    second=scratch/'second';second.mkdir();save(second/'lenso.toml',config)
    before=requests['/repos/acme/tools/releases/assets/102']
    run(second,'code','preview','button')
    assert requests['/repos/acme/tools/releases/assets/102']==before
    # All repository capability and file grants remain independent of a package's metadata.
    original=(first/'lenso.toml').read_text()
    policy=load(first/'lenso.toml');policy['cli']['extensions'][0]['sources']=[];save(first/'lenso.toml',policy)
    denied=run(first,'code','preview','button',expected=1);assert denied['code']=='permission_denied' and denied['details']['capability']=='host.sources'
    (first/'lenso.toml').write_text(original)
    no_node=dict(env,PATH=str(scratch/'empty-path'))
    run(first,'code','diff','button','--offline','--locked',overrides=no_node)
    # Explicit extension update chooses compatible releases, and preserves resource pins.
    releases.extend([publish('0.2.0',300),publish('0.1.1',200),publish('0.1.0+unlisted',400)])
    releases[:0]=[{'id':1000+i,'tag_name':f'other-v0.0.{i}','draft':False,'prerelease':False,'assets':[]} for i in range(30)]
    run(first,'code','diff','button')
    assert json.loads((first/'lenso.lock').read_text())['extensions'][0]['definition']['version']=='0.1.0'
    run(first,'update')
    updated=json.loads((first/'lenso.lock').read_text())
    assert updated['extensions'][0]['definition']['version']=='0.1.1' and updated['resources']
    run(first,'code','diff','button')
    # Curated Registry versions bind Release selection even when newer unlisted versions exist.
    curated=scratch/'curated';curated.mkdir()
    run(curated,'init','--preset','empty','--api',api,'--token-env','LENSO_TEST_GH_TOKEN','--yes')
    curated_config=load(curated/'lenso.toml')
    assert curated_config['cli']['github']['token_env']=='LENSO_TEST_GH_TOKEN'
    curated_config['cli']['registries']={'@test':api+'index/'}
    save(curated/'lenso.toml',curated_config)
    run(curated,'ext','add','@test/code','--input',json.dumps({'repo':'acme/resources'}),'--yes')
    curated_lock=json.loads((curated/'lenso.lock').read_text())
    curated_code=next(item for item in curated_lock['extensions'] if item['id']=='code')
    assert curated_code['definition']['version']=='0.1.0'
    assert curated_code['github']['tag']=='code-v0.1.0'
    assert curated_code['source']=='registry:@test/code'
    from composition_acceptance import verify as verify_composition
    verify_composition(root, scratch, config, snapshot, run, application, no_node)
    from author_acceptance import verify as verify_author
    verify_author(root, scratch, config, snapshot, run, application, no_node)
    # No ref/release discovery is needed to restore a locked package after cache deletion.
    pinned=(first/'lenso.lock').read_bytes();ref_queries=requests['/repos/acme/tools/commits/HEAD'];release_queries=requests['/repos/acme/tools/releases']
    shutil.rmtree(cache)
    run(first,'code','diff','button','--locked','--offline',expected=1)
    run(first,'code','diff','button','--locked')
    assert requests['/repos/acme/tools/commits/HEAD']==ref_queries and requests['/repos/acme/tools/releases']==release_queries
    assert (first/'lenso.lock').read_bytes()==pinned
    # A changed package fails integrity verification without applying project files.
    assets[202]=b'corrupt archive'
    shutil.rmtree(cache)
    app_before=application(first)
    failed=run(first,'code','diff','button','--locked',expected=1)
    assert failed['code']=='integrity_mismatch' and application(first)==app_before and (first/'lenso.lock').read_bytes()==pinned
    assert not leaks,leaks
    for path in [first/'lenso.lock',*cache.rglob('*')]:
        if path.is_file():assert token.encode() not in path.read_bytes() and signed.encode() not in path.read_bytes(),'credentials or signed URLs persisted'
    print('GitHub acceptance passed: real Wasm Release package, custom author config, code catalog/source files, source pins, cache reuse/restore, approvals, conflicts, JSON, no Node/Git, checksums and credential isolation',flush=True)
    print(f'Projects: {scratch}',flush=True)
except BaseException:
    print(f'Failed projects retained: {scratch}',flush=True)
    raise
else:
    if not args.keep:shutil.rmtree(scratch)
finally:
    server.shutdown();server.server_close();thread.join()
