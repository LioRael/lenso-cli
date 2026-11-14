#!/usr/bin/env python3
"""Real Wasm installation and authenticated native streaming file acceptance."""
import socket
import argparse,functools,hashlib,http.client,http.server,json,os,re,shutil,signal,subprocess,threading,time
from pathlib import Path
from http.client import HTTPConnection
from urllib.parse import urlsplit
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary',type=Path,required=True)
p.add_argument('--modules',type=Path)
p.add_argument('--official',action='store_true')
p.add_argument('--token-env')
p.add_argument('--upgrade',action='store_true',help='Upgrade real official App 0.1.0 and Auth 0.3.0')
p.add_argument('--out',type=Path,required=True)
p.add_argument('--legacy-app',type=Path,help='Unpacked published App 0.1.0 for reviewed upgrade acceptance')
p.add_argument('--language',choices=['typescript','rust'])
p.add_argument('--storage',choices=['sqlite','postgres','both'],default='sqlite')
a=p.parse_args()
if a.official and a.modules:p.error('choose official sources or local modules')
if not a.official and not a.modules:p.error('--modules is required for local acceptance')
if a.upgrade and not a.official and not a.legacy_app:p.error('local upgrade needs --legacy-app')
legacy=bool(a.legacy_app or a.upgrade)
credentials=['--token-env',a.token_env] if a.token_env else []
root=Path(__file__).resolve().parent.parent;binary=a.binary.resolve(strict=True)
out=a.out.resolve();out.mkdir(parents=True,exist_ok=False)
server=None;preset='app'
if not a.official:
 served=out/'served';served.mkdir()
 for name in ['app','code']:
  shutil.copytree(root/'dist/mainline'/name,served/name)
  shutil.copyfile(root/'dist/author'/name/(name+'.wasm'),served/name/(name+'.wasm'))
 class Handler(http.server.SimpleHTTPRequestHandler):
  def log_message(self,*args):pass
 server=http.server.ThreadingHTTPServer(('127.0.0.1',0),functools.partial(Handler,directory=str(served)))
 threading.Thread(target=server.serve_forever,daemon=True).start();base=f'http://127.0.0.1:{server.server_port}'
 if legacy:
  shutil.copytree(a.legacy_app,served/'legacy')
  old=served/'legacy/extension.toml';old.write_text(old.read_text().replace('registry:@lenso/code',base+'/code/extension.toml'))
 manifest=served/'app/extension.toml';manifest.write_text(manifest.read_text().replace('registry:@lenso/code',base+'/code/extension.toml'))
 if legacy:shutil.copytree(served/'app',served/'app-v2')
 (served/'preset.json').write_text(json.dumps({'schema_version':1,'id':'@test/app','version':'0.2.0','create':'app/init','adopt':'app/init','extensions':[{'id':'app','source':base+'/app/extension.toml'},{'id':'code','source':base+'/code/extension.toml'}]}))
 preset=base+'/preset.json'
reports=[]
try:
 for language in ([a.language] if a.language else ['typescript','rust']):
  for storage in (['sqlite','postgres'] if a.storage=='both' else [a.storage]):
   if legacy and not a.official:shutil.copytree(served/'legacy',served/'app',dirs_exist_ok=True)
   case=out/(language+'-'+storage);case.mkdir();project=case/'app';commands=[]
   env=dict(os.environ,LENSO_CACHE_DIR=str(case/'cache'))
   def call(*args,expected=0,empty=False):
    result=subprocess.run([str(binary),'--project',str(project),*args,'--json'],env=dict(env,PATH='') if empty else env,capture_output=True,text=True,timeout=600)
    assert result.returncode==expected,(args,result.returncode,result.stdout[-5000:],result.stderr[-6000:])
    assert '\x1b' not in result.stdout
    if a.token_env and env.get(a.token_env):assert env[a.token_env] not in result.stdout+result.stderr
    commands.append({'args':list(args),'exit_code':result.returncode});return json.loads(result.stdout)
   def start():
    log=case/f'server-{time.time_ns()}.log';stream=log.open('wb')
    process=subprocess.Popen([str(binary),'--project',str(project),'app','run','--yes','--json'],env=env,stdout=subprocess.PIPE,stderr=stream)
    deadline=time.monotonic()+600
    while time.monotonic()<deadline:
     content=log.read_text(errors='replace');match=re.search(r'LISTENING (\d+)',content)
     if match:return process,stream,int(match[1])
     if process.poll() is not None:
      stream.close();raise AssertionError((process.returncode,process.stdout.read().decode()[-2500:],content[-8000:]))
     time.sleep(.1)
    process.send_signal(signal.SIGINT);process.wait(timeout=20);stream.close();raise AssertionError(('startup timeout',log))
   def stop(process,stream):
    process.send_signal(signal.SIGINT)
    try:process.communicate(timeout=20)
    finally:stream.close()
    assert process.returncode in [0,130,-signal.SIGINT],process.returncode
   def http(port,path,method='GET',body=None,token=None,chunked=False):
    headers={};data=body
    if isinstance(body,dict):data=json.dumps(body).encode();headers['Content-Type']='application/json'
    if isinstance(body,bytes):headers['Content-Type']='application/octet-stream'
    if token:headers['Authorization']='Bearer '+token
    if chunked:
     headers['Transfer-Encoding']='chunked';data=(body[i:i+16384] for i in range(0,len(body),16384))
    conn=HTTPConnection('127.0.0.1',port,timeout=60)
    try:
     conn.request(method,path,body=data,headers=headers,encode_chunked=chunked);response=conn.getresponse();content=response.read();status=response.status
     return status,json.loads(content) if response.getheader('Content-Type','').startswith('application/json') and content else content
    finally:conn.close()
   settings={'language':language,'port':0}
   if a.modules:settings.update(author_root='author',format='lenso-modules-v1',catalog='lenso.modules.toml')
   if a.official and legacy:
    project.mkdir();call('init','--preset','empty','--yes',*credentials)
    call('ext','add','code','--constraint','=0.3.6','--input',json.dumps(settings),'--yes',*credentials)
    call('ext','add','app','--constraint','=0.1.0','--input',json.dumps(settings),'--yes',*credentials)
    call('app','init','--yes')
    call('ext','configure','code','--input','{"ref":"v0.3.0"}','--yes',*credentials)
   else:
    result=subprocess.run([str(binary),'--project',str(case),'new','app','--preset',preset,'--input',json.dumps(settings),'--yes',*credentials,'--json'],env=env,capture_output=True,text=True,timeout=600)
    assert result.returncode==0,(result.stdout[-4000:],result.stderr[-6000:])
   if a.modules:
    author=project/'author';author.mkdir()
    for directory in ['templates','typescript/src/modules','rust/src/modules']:shutil.copytree(a.modules/directory,author/directory)
    for path in a.modules.glob('*.toml'):shutil.copyfile(path,author/path.name)
   config=call('code','config')['outputs']['config']['config']
   config.setdefault('defaults',{}).setdefault('providers',{}).update({'lenso.files-metadata':storage,'lenso.auth-store':storage})
   config.setdefault('modules',{}).setdefault('files',{}).setdefault('options',{})['max_bytes']=2*1024*1024
   call('code','config','--input',json.dumps({'settings':config}),'--yes')
   if storage=='postgres':
    assert env.get('DATABASE_URL');(project/'.env').write_text('DATABASE_URL='+env['DATABASE_URL']+'\n')
   if legacy:
    call('code','add','auth','--yes');call('app','migrate','--yes')
    if a.official:
     old_index=json.loads((project/'src/code.modules.json').read_text())
     assert old_index['modules']['@lenso/auth']['version']=='0.3.0'
     old_lock=json.loads((project/'lenso.lock').read_text())
     assert next(x['definition']['version'] for x in old_lock['extensions'] if x['id']=='app')=='0.1.0'
    process,stream,port=start()
    legacy_credentials={'name':f'legacy_{time.time_ns()}','password':'correct horse battery'}
    try:
     assert http(port,'/auth/register','POST',legacy_credentials)[0]==200
     status,session=http(port,'/auth/login','POST',legacy_credentials);assert status==200;legacy_token=session['token']
    finally:stop(process,stream)
    entry=project/('src/main.ts' if language=='typescript' else 'src/main.rs')
    entry.write_text('// user host customization\n'+entry.read_text())
    if not a.official:shutil.copytree(served/'app-v2',served/'app',dirs_exist_ok=True)
    call('ext','add','app' if a.official else base+'/app/extension.toml','--id','app','--constraint','^0.2.0','--input',json.dumps(settings),'--yes',*credentials)
    if a.official:call('preset','apply','app','--input',json.dumps(settings),'--yes',*credentials)
    before={str(path):path.read_bytes() for path in [entry,project/'lenso.code.toml',project/'lenso/app.lock']}
    call('app','upgrade-preview',empty=True)
    assert all(Path(path).read_bytes()==content for path,content in before.items())
    question=call('app','upgrade',expected=2,empty=True);answer_file=case/'upgrade-answers.json'
    answer_file.write_text(json.dumps({question['question']['id']:False}))
    call('app','upgrade','--answers',str(answer_file),expected=130,empty=True)
    assert all(Path(path).read_bytes()==content for path,content in before.items())
    call('app','upgrade','--yes',empty=True);assert 'user host customization' in entry.read_text()
    if a.official:call('ext','configure','code','--input','{"ref":"HEAD"}','--yes',*credentials)
    call('code','update','auth','--input','{"reconfigure":true}','--yes')
    call('app','check','--yes')
    process,stream,port=start()
    try:assert http(port,'/auth/session',token=legacy_token)[0]==200
    finally:stop(process,stream)
   # Verify that Files can be generated independently, but cannot run without its declared identity.
   plan=call('code','preview','files',empty=True)['outputs']['resolve'];assert plan['module']['version']=='0.1.0'
   assert not (project/'src/modules/files').exists() and not (project/'data').exists()
   pending=call('code','add','files',expected=2,empty=True);answers=case/'answers.json'
   answers.write_text(json.dumps({pending['question']['id']:False}));call('code','add','files','--answers',str(answers),expected=130,empty=True)
   assert not (project/'src/modules/files').exists()
   call('code','add','files','--yes')
   call('app','check','--yes')
   if not legacy:
    call('app','migrate','--yes',expected=1)
    assert not (project/'data').exists()
   call('code','add','auth','--yes')
   if storage=='postgres':
    assert env.get('DATABASE_URL');(project/'.env').write_text('DATABASE_URL='+env['DATABASE_URL']+'\n')
   call('app','check','--yes');call('app','migrate','--yes')
   public_pins={}
   if a.official:
    lock=json.loads((project/'lenso.lock').read_text())
    for name,version in [('app','0.2.0'),('code','0.3.6')]:
     item=next(x for x in lock['extensions'] if x['id']==name)
     assert item['source']=='registry:@lenso/'+name and item['definition']['version']==version
     assert item['github']['tag']==name+'-v'+version
     public_pins[name]=item['github']
    assert lock['presets']['@lenso/app']['version']=='0.2.0'
    installed=json.loads((project/'src/code.modules.json').read_text())['modules']
    for name,version in [('auth','0.4.0'),('files','0.1.0')]:
     item=installed['@lenso/'+name]
     assert item['version']==version and item['profile']['source']=='github-repo:LioRael/lenso-modules' and item['profile']['ref']=='v0.4.0'
     public_pins[name]=item
    assert not (project/'author').exists()
   payload=bytes(range(256))*4096+os.urandom(711);process,stream,port=start()
   try:
    assert http(port,'/files')[0]==401
    users=[];tokens=[]
    for index in range(2):
     credentials={'name':f'f_{language}_{time.time_ns()}_{index}','password':'correct horse battery'}
     status,user=http(port,'/auth/register','POST',credentials);assert status==200,(status,user);users.append(user)
     status,session=http(port,'/auth/login','POST',credentials);assert status==200,(status,session);tokens.append(session['token'])
    status,file=http(port,'/files?name=sample.bin&owner=forged','POST',payload,tokens[0],True);assert status==201,(status,file)
    assert file['owner']==users[0]['id'] and file['size']==len(payload) and file['sha256']==hashlib.sha256(payload).hexdigest()
    file_id=file['id'];assert http(port,'/files/'+file_id+'/content',token=tokens[0])==(200,payload)
    assert http(port,'/files/'+file_id+'/content',token=tokens[1])[0]==404
    assert http(port,'/files/'+file_id,'DELETE',token=tokens[1])[0]==404
    assert http(port,'/files',token=tokens[1])==(200,{'files':[]})
    assert len(http(port,'/files?limit=1',token=tokens[0])[1]['files'])==1
    assert http(port,'/files?name=../escape','POST',b'bad',tokens[0])[0]==400
    status,oversize=http(port,'/files?name=large.bin','POST',b'0'*(2*1024*1024+1),tokens[0]);assert status==400,(status,oversize)
    assert len(http(port,'/files',token=tokens[0])[1]['files'])==1
    sock=socket.create_connection(('127.0.0.1',port),timeout=10)
    try:
     sock.sendall((f'POST /files?name=interrupted.bin HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {tokens[0]}\r\nContent-Length: 2097152\r\nContent-Type: application/octet-stream\r\n\r\n').encode()+b'x'*65536)
     for _ in range(100):
      if len(list((project/'data/files/files').iterdir()))>1:break
      time.sleep(.01)
     assert len(list((project/'data/files/files').iterdir()))>1
     sock.shutdown(socket.SHUT_RDWR)
    finally:sock.close()
    for _ in range(100):
     if len(list((project/'data/files/files').iterdir()))==1:break
     time.sleep(.05)
    assert len(list((project/'data/files/files').iterdir()))==1,'interrupted upload left content'
    assert len(http(port,'/files',token=tokens[0])[1]['files'])==1
    assert http(port,'/auth/logout' ,'POST',token=tokens[0])[0]==200
    assert http(port,'/files/'+file_id+'/content',token=tokens[0])[0]==401
    status,session=http(port,'/auth/login','POST',credentials);assert status==200
   finally:stop(process,stream)
   # Independent updates retain both module families and customized source.
   core=project/('src/modules/files/modules/files/core.ts' if language=='typescript' else 'src/modules/files/modules/files/mod.rs')
   core.write_text('// user Files customization\n'+core.read_text())
   before_auth=(project/('src/modules/auth/assembly/index.ts' if language=='typescript' else 'src/modules/auth/assembly/mod.rs')).read_bytes()
   call('code','update','files','--yes');assert 'user Files customization' in core.read_text()
   assert (project/('src/modules/auth/assembly/index.ts' if language=='typescript' else 'src/modules/auth/assembly/mod.rs')).read_bytes()==before_auth
   assert call('code','preview','files','--locked','--offline',empty=True)['outputs']['resolve']['files']==[]
   process,stream,port=start()
   try:
    # Log in as the owner again using the first user's name.
    status,session=http(port,'/auth/login','POST',{'name':users[0]['name'],'password':'correct horse battery'});assert status==200
    token=session['token'];assert http(port,'/files/'+file_id+'/content',token=token)==(200,payload)
    assert http(port,'/files/'+file_id,'DELETE',token=token)[0]==200
    assert http(port,'/files/'+file_id+'/content',token=token)[0]==404
    assert http(port,'/files',token=token)==(200,{'files':[]})
    assert not (project/'data/files/files'/file_id).exists()
   finally:stop(process,stream)
   for path in ['lenso.toml','lenso.lock','lenso.code.toml']:
    if env.get('DATABASE_URL'):assert env['DATABASE_URL'] not in (project/path).read_text()
   report={'language':language,'storage':storage,'legacy_app_upgrade':legacy,'official':a.official,'public_pins':public_pins,'streamed_bytes':len(payload),'shared_identity':True,'cross_user_denied':True,'revoked_session_denied':True,'restart_persistence':True,'delete_content':True,'oversize_cleanup':True,'interrupted_upload_cleanup':True,'preview_denial_update_offline':True,'commands':commands}
   (case/'verification.json').write_text(json.dumps(report,indent=2)+'\n');reports.append(report)
   print(f'{language}/{storage} authenticated streaming Files passed',flush=True)
finally:
 if server:server.shutdown();server.server_close()
(out/'verification.json').write_text(json.dumps({'cases':reports},indent=2)+'\n')
