#!/usr/bin/env python3
"""Author-to-consumer acceptance: package, approve, upload, verify and publish a real Wasm extension."""
import argparse
import base64
import copy
import hashlib
import http.server
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
from urllib.parse import parse_qs, unquote, urlsplit
from project_config import load, save

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--keep',action='store_true')
args=parser.parse_args()
root=Path(__file__).resolve().parent.parent
binary=root/'target/debug/lenso'
scratch=Path(tempfile.mkdtemp(prefix='lenso-publish-'))
A='1'*40
B='2'*40
token='test-publish-token-34b71'
signed='test-publish-download-c11'
env=dict(os.environ,LENSO_CACHE_DIR=str(scratch/'cache'),PUBLISH_TEST_TOKEN=token,GITHUB_ACTIONS='false',NO_PROXY='127.0.0.1,localhost',no_proxy='127.0.0.1,localhost')
for name in ('GH_TOKEN','GITHUB_TOKEN'):env.pop(name,None)
requests=[]
mutations=[]
releases={}
assets={}
tags={}
fail_upload=None
redirect_upload=False
evil_upload=False
legacy_digest=False
annotated=False
next_release=100
next_asset=1000
leaks=[]
descriptor='schema_version = 1\n[[extensions]]\nid = "code"\npath = "extensions/code"\ntag = "code-v{version}"\nasset = "custom-code-{version}.tgz"\ninclude = ["LICENSE"]\n'

def blob(body):return hashlib.sha1(b'blob '+str(len(body)).encode()+b'\0'+body).hexdigest()
def document(repo,path):
    if repo=='acme/tools' and path=='lenso.registry.toml':return descriptor.encode()
    if repo=='acme/resources' and path=='README':return b'Code from the uploaded release\n'
    return None

def asset_record(asset):
    value={key:asset[key] for key in ['id','name','size','state']}
    value['digest']=None if legacy_digest else 'sha256:'+hashlib.sha256(asset['body']).hexdigest()
    return value

def release_record(release):
    value=copy.deepcopy(release)
    host='http://127.0.0.1:1' if evil_upload else api.rstrip('/')
    value['upload_url']=f'{host}/repos/acme/tools/releases/{release["id"]}/assets{{?name,label}}'
    value['html_url']=api+f'acme/tools/releases/tag/{release["tag_name"]}'
    value['assets']=[asset_record(asset) for asset in assets.values() if asset['release']==release['id']]
    return value

class Handler(http.server.BaseHTTPRequestHandler):
    def authorized(self):
        if self.headers.get('Authorization')!='Bearer '+token:
            self.reply(403,b'');return False
        return True
    def do_GET(self):
        url=urlsplit(self.path);path=unquote(url.path);requests.append(('GET',path))
        if path.startswith('/download/'):
            if self.headers.get('Authorization'):leaks.append('credential reached redirect')
            if parse_qs(url.query).get('signed')!=[signed]:return self.reply(403,b'')
            return self.reply(200,assets[int(path.split('/')[-1])]['body'])
        if not self.authorized():return
        pieces=path.strip('/').split('/')
        if len(pieces)<4 or pieces[0]!='repos':return self.reply(404,b'')
        repo='/'.join(pieces[1:3]);route=pieces[3:]
        if route[:1]==['commits']:
            ref='/'.join(route[1:])
            if ref in ('HEAD','main',A):return self.reply(200,{'sha':A})
            if ref==B:return self.reply(200,{'sha':B})
            return self.reply(404,b'')
        if route[:3]==['git','ref','tags']:
            tag='/'.join(route[3:])
            if tag not in tags:return self.reply(404,b'')
            return self.reply(200,{'object':{'type':'tag' if annotated else 'commit','sha':B if annotated else tags[tag]}})
        if route[:2]==['git','tags']:return self.reply(200,{'object':{'type':'commit','sha':A}})
        if route[:2]==['git','trees']:
            name='lenso.registry.toml' if repo=='acme/tools' else 'README'
            body=document(repo,name)
            return self.reply(200,{'truncated':False,'tree':[{'path':name,'mode':'100644','type':'blob','sha':blob(body)}]})
        if route[:1]==['contents']:
            body=document(repo,'/'.join(route[1:]))
            if body is None:return self.reply(404,b'')
            return self.reply(200,{'type':'file','encoding':'base64','sha':blob(body),'content':base64.b64encode(body).decode()})
        if route==['releases']:
            page=int(parse_qs(url.query).get('page',['1'])[0])
            values=[release_record(release) for release in releases.values()]
            return self.reply(200,values[(page-1)*30:page*30])
        if route[:2]==['releases','assets']:
            asset=assets.get(int(route[2]))
            if asset is None:return self.reply(404,b'')
            self.send_response(302);self.send_header('Location',f'/download/{asset["id"]}?signed={signed}');self.end_headers();return
        if len(route)==2 and route[0]=='releases':
            release=releases.get(int(route[1]));return self.reply(200 if release else 404,release_record(release) if release else b'')
        self.reply(404,b'')
    def do_POST(self):
        global next_release,next_asset,fail_upload
        url=urlsplit(self.path);path=unquote(url.path);requests.append(('POST',path))
        if not self.authorized():return
        body=self.rfile.read(int(self.headers.get('Content-Length','0')))
        if path=='/repos/acme/tools/releases':
            data=json.loads(body)
            if not data['draft']:return self.reply(400,b'')
            if any(r['tag_name']==data['tag_name'] for r in releases.values()):return self.reply(422,b'')
            next_release+=1
            release={'id':next_release,'tag_name':data['tag_name'],'target_commitish':data['target_commitish'],'draft':True,'prerelease':data['prerelease'],'immutable':False}
            releases[release['id']]=release;mutations.append(('create',release['id']))
            return self.reply(201,release_record(release))
        pieces=path.strip('/').split('/')
        if len(pieces)==6 and pieces[:4]==['repos','acme','tools','releases'] and pieces[5]=='assets':
            release=releases[int(pieces[4])];name=parse_qs(url.query)['name'][0]
            assert release['draft'],'upload reached an already published release'
            if redirect_upload:
                self.send_response(307);self.send_header('Location','/unexpected-upload');self.end_headers();return
            if name==fail_upload:
                fail_upload=None;return self.reply(502,b'')
            if any(a['release']==release['id'] and a['name']==name for a in assets.values()):return self.reply(422,b'')
            next_asset+=1
            asset={'id':next_asset,'release':release['id'],'name':name,'body':body,'size':len(body),'state':'uploaded'}
            assets[asset['id']]=asset;mutations.append(('upload',name))
            return self.reply(201,asset_record(asset))
        self.reply(404,b'')
    def do_PATCH(self):
        path=unquote(urlsplit(self.path).path);requests.append(('PATCH',path))
        if not self.authorized():return
        data=json.loads(self.rfile.read(int(self.headers.get('Content-Length','0'))))
        release=releases[int(path.rsplit('/',1)[1])]
        owned=[a for a in assets.values() if a['release']==release['id']]
        assert len(owned)==3,'publication reached an incomplete release'
        assert data=={'draft':False,'target_commitish':A}
        release['target_commitish']=data['target_commitish']
        release['draft']=False;tags[release['tag_name']]=release['target_commitish'];mutations.append(('publish',release['id']))
        self.reply(200,release_record(release))
    def reply(self,status,body):
        body=json.dumps(body).encode() if not isinstance(body,bytes) else body
        self.send_response(status);self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    def log_message(self,*args):pass
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
api=f'http://127.0.0.1:{server.server_port}/'

def run(project,*arguments,expected=0,overrides=None):
    result=subprocess.run([str(binary),'--project',str(project),*arguments,'--json'],capture_output=True,text=True,env=env if overrides is None else overrides,timeout=120)
    assert token not in result.stdout+result.stderr and signed not in result.stdout+result.stderr
    assert result.returncode==expected,(arguments,result.returncode,result.stdout[-5000:],result.stderr[-1000:])
    return json.loads(result.stdout)

try:
    seed=scratch/'seed';seed.mkdir()
    subprocess.run(['python3',str(root/'scripts/configure-code.py'),str(seed),'--repo','acme/resources','--file','README','--item','readme','--target','docs','--github-api',api,'--token-env','PUBLISH_TEST_TOKEN'],check=True,env=env)
    policy=load(seed/'lenso.toml');module=seed/Path(policy['cli']['extensions'][0]['source']).parent/'code.wasm'
    author=scratch/'author';package=author/'extensions/code';package.mkdir(parents=True)
    shutil.copyfile(module,package/'code.wasm');shutil.copyfile(root/'presets/code/extension.toml',package/'extension.toml');(package/'LICENSE').write_text('Acceptance fixture\n')
    (author/'lenso.registry.toml').write_text(descriptor)
    base=['publish','--github','--id','code','--api',api,'--token-env','PUBLISH_TEST_TOKEN']
    common=[*base,'--repo','acme/tools','--target',A]
    no_token=dict(env);no_token.pop('PUBLISH_TEST_TOKEN')
    dry=run(author,*common,'--dry-run','--offline',overrides=no_token)
    assert dry['outputs']['main']['state']=='prepared' and not requests and not mutations
    prepared={p.name:p.read_bytes() for p in (author/'dist/lenso').iterdir()}
    run(author,*common,'--dry-run',overrides=no_token)
    assert prepared=={p.name:p.read_bytes() for p in (author/'dist/lenso').iterdir()} and not requests
    local_descriptor=author/'lenso.registry.toml'
    local_descriptor.write_text(descriptor.replace('custom-code-{version}.tgz','extension.toml'))
    run(author,*common,'--dry-run',expected=1,overrides=no_token)
    assert not requests;local_descriptor.write_text(descriptor)
    pending=run(author,*common,expected=2)
    assert pending['question']['id'].startswith('publish:') and 'sha256' in pending['question']['message']
    assert not mutations
    answers=scratch/'answers.json';answers.write_text(json.dumps({pending['question']['id']:False}))
    run(author,*common,'--answers',str(answers),expected=130)
    assert not mutations
    manifest=package/'extension.toml';original=manifest.read_text()
    answers.write_text(json.dumps({pending['question']['id']:True}))
    manifest.write_text(original.replace('version = "0.1.0"','version = "0.1.1"'))
    changed=run(author,*common,'--answers',str(answers),expected=2)
    assert changed['question']['id']!=pending['question']['id'] and not mutations
    manifest.write_text(original)
    # Invalid prebuilt modules fail local validation before a GitHub request.
    count=len(requests);manifest.write_text(original.replace('sha256 = "auto"','sha256 = "'+'0'*64+'"'))
    run(author,*common,'--yes',expected=1);assert len(requests)==count and not mutations;manifest.write_text(original)
    for invalid in [original.replace('schema_version = 1','schema_version = 999'),original.replace('handler = "code/resolve"','handler = "code/missing"'),original.replace('provider = "code"','provider = "foreign"')]:
        manifest.write_text(invalid);run(author,*common,'--yes',expected=1)
        assert len(requests)==count and not mutations
    manifest.write_text(original)
    tags['code-v0.1.0']=B
    mismatch=run(author,*common,'--yes',expected=1);assert 'different commit' in mismatch['message'] and not mutations
    tags.clear()
    fail_upload='extension.toml'
    partial=run(author,*common,'--yes',expected=1)
    assert 'preserved' in partial['message'] and 'verified uploads in this attempt: 1' in partial['message']
    release=next(iter(releases.values()));assert release['draft'] and len(assets)==1
    uploaded_name=next(iter(assets.values()))['name'];asset_before=copy.deepcopy(next(iter(assets.values())))
    # A draft created for another commit cannot be silently promoted for this target.
    before=list(mutations);release['target_commitish']=B
    mismatch=run(author,*common,'--yes',expected=1)
    assert 'target differs' in mismatch['message'] and mutations==before
    release['target_commitish']=A
    release['prerelease']=True
    mismatch=run(author,*common,'--yes',expected=1)
    assert 'prerelease flag' in mismatch['message'] and mutations==before
    release['prerelease']=False
    # Unexpected draft contents are never deleted/replaced.
    foreign={'id':999,'release':release['id'],'name':'unrelated.txt','body':b'keep','size':4,'state':'uploaded'};assets[999]=foreign
    before=list(mutations);run(author,*common,'--yes',expected=1);assert mutations==before and assets[999]==foreign;del assets[999]
    # Resume with byte verification when the server has no digest metadata.
    legacy_digest=True
    published=run(author,*common,'--yes')['outputs']['main']
    assert published['state']=='published' and published['uploaded']==2 and not release['draft']
    assert [op for op in mutations if op==('upload',uploaded_name)]==[('upload',uploaded_name)]
    assert next(a for a in assets.values() if a['name']==uploaded_name)==asset_before
    before=list(mutations)
    annotated=True
    repeated=run(author,*common)['outputs']['main']
    assert repeated['uploaded']==0 and mutations==before
    release['immutable']=True
    run(author,*common);assert mutations==before
    run(author,*common,'--draft','--yes',expected=1);assert mutations==before
    damaged=next(iter(assets.values()));original_body=damaged['body'];damaged['body']=b'x'*len(original_body)
    run(author,*common,'--yes',expected=1);assert mutations==before;damaged['body']=original_body
    # Actions defaults, event tag validation, and native step output support.
    output=scratch/'github-output';output.write_text('')
    actions=dict(env,GITHUB_ACTIONS='true',GITHUB_REPOSITORY='acme/tools',GITHUB_SHA=A,GITHUB_REF_TYPE='tag',GITHUB_REF_NAME='code-v0.1.0',GITHUB_OUTPUT=str(output))
    actions.pop('PUBLISH_TEST_TOKEN');actions['GITHUB_TOKEN']=token
    action_args=['publish','--github','--id','code','--api',api,'--yes']
    result=run(author,*action_args,overrides=actions)['outputs']['main']
    assert result['release_id']==release['id'] and 'state=published\n' in output.read_text() and 'release_id=' in output.read_text()
    wrong=dict(actions,GITHUB_REF_NAME='code-v9.9.9')
    run(author,*action_args,overrides=wrong,expected=1);assert mutations==before
    # Upload URL authorization is checked before any credential/body can leave the API host.
    annotated=False;evil_upload=True
    manifest.write_text(original.replace('version = "0.1.0"','version = "0.1.2"'))
    unsafe=run(author,*common,'--yes',expected=1)
    assert 'outside the approved' in unsafe['message']
    draft=[r for r in releases.values() if r['tag_name']=='code-v0.1.2'][0]
    assert draft['draft'] and not [a for a in assets.values() if a['release']==draft['id']]
    evil_upload=False
    redirect_upload=True
    redirected=run(author,*common,'--yes',expected=1)
    assert '307' in redirected['message'] and all(path!='/unexpected-upload' for _,path in requests)
    assert not [a for a in assets.values() if a['release']==draft['id']]
    redirect_upload=False
    ready=run(author,*common,'--draft','--yes')['outputs']['main']
    assert ready['state']=='draft' and ready['uploaded']==3 and draft['draft']
    # Publisher and consumer both work without Node, Git or gh in PATH.
    empty=scratch/'empty-path';empty.mkdir()
    final=run(author,*common,'--yes',overrides=dict(env,PATH=str(empty)))['outputs']['main']
    assert final['state']=='published' and final['uploaded']==0
    consumer=scratch/'consumer';consumer.mkdir()
    policy['cli']['extensions'][0]['source']='github-release:acme/tools/code';policy['cli']['extensions'][0]['version']='=0.1.2';save(consumer/'lenso.toml',policy)
    run(consumer,'code','preview','readme',overrides=dict(env,PATH=str(empty)))
    run(consumer,'code','add','readme','--yes',overrides=dict(env,PATH=str(empty)))
    assert (consumer/'docs/README').read_text()=='Code from the uploaded release\n'
    run(consumer,'code','diff','readme','--offline','--locked',overrides=dict(env,PATH=str(empty)))
    assert not leaks,leaks
    print('Publish acceptance passed: deterministic package, consent/revision, draft uploads, checksums, partial resume, immutable idempotence, tag checks, Actions outputs, origin guards, and published Wasm consumption with empty PATH',flush=True)
    print(f'Projects: {scratch}',flush=True)
except BaseException:
    print(f'Failed projects retained: {scratch}',flush=True);raise
else:
    if not args.keep:shutil.rmtree(scratch)
finally:
    server.shutdown();server.server_close();thread.join()
