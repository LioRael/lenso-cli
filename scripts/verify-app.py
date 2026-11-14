#!/usr/bin/env python3
"""Create native apps, install Auth, supervise HTTP hosts and verify real operations."""
import tomllib
import argparse,functools,http.server,json,os,re,shutil,signal,subprocess,tempfile,threading,time
from pathlib import Path
from urllib.request import Request,urlopen
from urllib.error import HTTPError
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary',type=Path,required=True)
p.add_argument('--modules',type=Path,help='Working-tree Auth source; omit for the live module directory')
p.add_argument('--official',action='store_true')
p.add_argument('--token-env')
p.add_argument('--out',type=Path,required=True)
p.add_argument('--language',choices=['typescript','rust'])
p.add_argument('--storage',choices=['sqlite','postgres','both'],default='sqlite')
a=p.parse_args();root=Path(__file__).resolve().parent.parent;binary=a.binary.resolve(strict=True)
out=a.out.resolve();out.mkdir(parents=True,exist_ok=False)
server=None
if not a.official:
 served=out/'served';served.mkdir()
 for name in ['code','app']:
  shutil.copytree(root/'dist/mainline'/name,served/name)
  shutil.copyfile(root/'dist/author'/name/(name+'.wasm'),served/name/(name+'.wasm'))
 class Handler(http.server.SimpleHTTPRequestHandler):
  def log_message(self,*args):pass
 server=http.server.ThreadingHTTPServer(('127.0.0.1',0),functools.partial(Handler,directory=str(served)))
 threading.Thread(target=server.serve_forever,daemon=True).start();base=f'http://127.0.0.1:{server.server_port}'
 manifest=served/'app/extension.toml';manifest.write_text(manifest.read_text().replace('registry:@lenso/code',base+'/code/extension.toml'))
 (served/'preset.json').write_text(json.dumps({'schema_version':1,'id':'@test/app','version':'0.1.0','create':'app/init','adopt':'app/init','extensions':[{'id':'app','source':base+'/app/extension.toml'},{'id':'code','source':base+'/code/extension.toml'}]}))
 preset=base+'/preset.json'
else:preset='app'
reports=[]
languages=[a.language] if a.language else ['typescript','rust']
storages=['sqlite','postgres'] if a.storage=='both' else [a.storage]
try:
 for language in languages:
  for storage in storages:
   case=out/(language+'-'+storage);case.mkdir();project=case/'app'
   env=dict(os.environ,LENSO_CACHE_DIR=str(case/'cache'))
   credentials=['--token-env',a.token_env] if a.token_env else []
   settings={'language':language,'port':0}
   if a.modules:settings.update(author_root='author',format='lenso-modules-v1',catalog='lenso.modules.toml')
   intent=json.dumps(settings);commands=[]
   def call(directory,*args,expected=0,empty=False):
    r=subprocess.run([str(binary),'--project',str(directory),*args,'--json'],env=dict(env,PATH='') if empty else env,capture_output=True,text=True,timeout=600)
    assert r.returncode==expected,(args,r.returncode,r.stdout[-4500:],r.stderr[-4500:])
    assert '\x1b' not in r.stdout
    if a.token_env and env.get(a.token_env):assert env[a.token_env] not in r.stdout+r.stderr
    commands.append({'args':list(args),'exit_code':r.returncode});return json.loads(r.stdout)
   choice=call(case,'new','app','--preset',preset,'--yes',*credentials,expected=2,empty=True)
   assert choice['question']['id']=='app:language' and (not project.exists() or not any(project.iterdir())),choice
   # Separate metadata-only target checks plan/denial without native tools or file mutation.
   preview=case/'preview';preview.mkdir()
   call(preview,'init','--preset','empty','--yes',empty=True)
   source='app' if a.official else base+'/app/extension.toml'
   call(preview,'ext','add',source,'--id','app','--input',intent,'--yes',*credentials,empty=True)
   call(preview,'app','preview',empty=True)
   pending=call(preview,'app','init',expected=2,empty=True)
   answers=case/'answers.json';answers.write_text(json.dumps({pending['question']['id']:False}))
   call(preview,'app','init','--answers',str(answers),expected=130,empty=True)
   assert not (preview/'src').exists() and not (preview/'lenso.code.toml').exists()
   answers.write_text(json.dumps({pending['question']['id']:True}))
   permission=call(preview,'app','init','--answers',str(answers),expected=2,empty=True)
   answers.write_text(json.dumps({pending['question']['id']:True,permission['question']['id']:False}))
   call(preview,'app','init','--answers',str(answers),expected=130,empty=True)
   assert not (preview/'src').exists() and not (preview/'lenso/app.lock').exists()
   call(preview,'app','init','--yes',empty=True,expected=1)
   assert 'state="pending"' in (preview/'lenso/app.lock').read_text()
   call(preview,'app','prepare','--yes')
   assert 'state="ready"' in (preview/'lenso/app.lock').read_text()
   call(case,'new','app','--preset',preset,'--input',intent,'--yes',*credentials)
   assert (project/'lenso/app.lock').is_file()
   config=call(project,'code','config')['outputs']['config']['config']
   app_version=next(item['definition']['version'] for item in json.loads((project/'lenso.lock').read_text())['extensions'] if item['id']=='app')
   host_v2=tuple(map(int,app_version.split('.'))) >= (0,2,0)
   assert config['project']['language']==language and config['conventions']['app_host']==('lenso-http-v2' if host_v2 else 'lenso-http-v1')
   call(project,'app','check','--yes')
   pending_run=call(project,'app','run',expected=2,empty=True)
   assert pending_run['question']['kind']['type']=='confirm'
   answers.write_text(json.dumps({pending_run['question']['id']:False}))
   call(project,'app','run','--answers',str(answers),expected=1,empty=True)
   def start(migrate=False):
    if migrate:call(project,'app','migrate','--yes')
    log=case/f'server-{time.time_ns()}.log';stream=log.open('wb')
    command=[str(binary),'--project',str(project),'app','run','--yes','--json']
    process=subprocess.Popen(command,env=env,stdout=subprocess.PIPE,stderr=stream)
    deadline=time.monotonic()+600
    while time.monotonic()<deadline:
     text=log.read_text(errors='replace');match=re.search(r'LISTENING (\d+)',text)
     if match:return process,stream,'http://127.0.0.1:'+match[1]
     if process.poll() is not None:
      stream.close();raise AssertionError(('server failed',process.returncode,process.stdout.read().decode()[-2000:],text[-7000:]))
     time.sleep(.1)
    process.kill();process.wait();stream.close();raise AssertionError(('startup timed out',log))
   def stop(process,stream,url):
    process.send_signal(signal.SIGINT)
    try:process.communicate(timeout=20)
    except subprocess.TimeoutExpired:process.kill();process.communicate();raise
    finally:stream.close()
    assert process.returncode in (0,130,-signal.SIGINT),process.returncode
    try:urlopen(url+'/health',timeout=1)
    except OSError:return
    raise AssertionError('supervised server survived cancellation')
   def http(url,path,method='GET',body=None,token=None):
    headers={};data=None
    if body is not None:data=json.dumps(body).encode();headers['Content-Type']='application/json'
    if token:headers['Authorization']='Bearer '+token
    request=Request(url+path,data=data,method=method,headers=headers)
    try:
     with urlopen(request,timeout=30) as response:return response.status,json.load(response)
    except HTTPError as error:
     content=error.read();return error.code,json.loads(content) if content else None
   if language=='typescript':
    stale=project/'dist/modules/removed/assembly/host.js';stale.parent.mkdir(parents=True)
    stale.write_text('throw new Error("stale module must not execute");')
   process,stream,url=start()
   if language=='typescript':assert not stale.exists()
   try:
    assert http(url,'/health')[0]==200
    assert http(url,'/auth/session')[0]==404
   finally:stop(process,stream,url)
   if a.modules:
    for directory in ['templates','typescript/src/modules','rust/src/modules']:
     shutil.copytree(a.modules/directory,project/'author'/directory)
    for path in a.modules.glob('*.toml'):
     shutil.copyfile(path,project/'author'/path.name)
    shutil.copytree(project/'author',preview/'author')
    call(preview,'ext','add','code' if a.official else base+'/code/extension.toml','--id','code','--input',intent,'--yes',*credentials,empty=True)
    for suffix in ['', '.app']:
     call(preview,'ext','configure','code','--input',json.dumps({'author_root':'author','format':'lenso-compose-v1','catalog':f'auth.{language}{suffix}.toml'}),'--yes',empty=True)
     for sample in (['default','postgres'] if suffix else ['default','multiple','postgres']):
      validated=call(preview,'code','validate','--sample',sample,empty=True)['outputs']['validate']
      assert validated['validated'] and len(validated['samples'])==1
   call(project,'code','config','--provider','lenso.auth-store='+storage,'--yes')
   plan=call(project,'code','preview','auth')['outputs']['resolve']
   expected_version=tomllib.loads((a.modules/'lenso.modules.toml').read_text())['modules']['auth']['releases'][-1]['version'] if a.modules else ('0.4.0' if host_v2 else '0.3.0')
   assert plan['module']['version']==expected_version and not (project/'src/modules/auth').exists()
   call(project,'code','add','auth','--yes')
   assert (project/('src/modules/auth/assembly/host.ts' if language=='typescript' else 'src/modules/auth/assembly/host.rs')).is_file()
   if storage=='postgres':
    assert env.get('DATABASE_URL'),'DATABASE_URL is required for PostgreSQL'
    (project/'.env').write_text('DATABASE_URL='+env['DATABASE_URL']+'\n')
   call(project,'app','check','--yes')
   process,stream,url=start(True)
   username='u_'+language+'_'+str(time.time_ns());password='correct horse battery'
   try:
    assert http(url,'/health')[0]==200
    assert http(url,'/auth/session')[0]==401
    assert http(url,'/auth/register','POST',{'name':username,'password':'short'})[0]==400
    status,user=http(url,'/auth/register','POST',{'name':username,'password':password});assert status==200,(status,user)
    status,session=http(url,'/auth/login','POST',{'name':username,'password':password});assert status==200,(status,session)
    token=session['token'];status,current=http(url,'/auth/session',token=token);assert status==200 and current['id']==user['id'],(status,current)
    assert http(url,'/auth/logout','POST',token=token)[0]==200
    assert http(url,'/auth/session',token=token)[0]==401
   finally:stop(process,stream,url)
   entry=project/('src/main.ts' if language=='typescript' else 'src/main.rs')
   entry.write_text('// user host customization\n'+entry.read_text())
   manifest=project/('package.json' if language=='typescript' else 'Cargo.toml');before=manifest.read_bytes()
   call(project,'app','init','--yes');assert manifest.read_bytes()==before and 'user host customization' in entry.read_text()
   core=project/('src/modules/auth/modules/auth/core.ts' if language=='typescript' else 'src/modules/auth/modules/auth/mod.rs')
   core.write_text('// user auth customization\n'+core.read_text())
   call(project,'code','update','auth','--yes');assert 'user auth customization' in core.read_text()
   process,stream,url=start()
   try:assert http(url,'/auth/login','POST',{'name':username,'password':password})[0]==200
   finally:stop(process,stream,url)
   assert call(project,'code','preview','auth','--locked','--offline')['outputs']['resolve']['files']==[]
   for name in ['lenso.toml','lenso.lock','lenso.code.toml','lenso/app.lock']:
    assert env.get('DATABASE_URL','UNSET_DATABASE_URL') not in (project/name).read_text()
   report={'language':language,'storage':storage,'official':a.official,'native_create_and_health':True,'auth_http_and_persistence':True,'preview_and_two_phase_denial':True,'pending_preparation_retry':True,'run_denial':True,'user_changes_preserved':True,'supervised_stop':True,'offline_replay':True,'commands':commands}
   (case/'verification.json').write_text(json.dumps(report,indent=2)+'\n');reports.append(report)
   print(language+'/'+storage+' App/Auth HTTP lifecycle passed',flush=True)
finally:
 if server:server.shutdown();server.server_close()
(out/'verification.json').write_text(json.dumps({'cases':reports},indent=2)+'\n')
