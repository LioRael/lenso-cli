"""Verify terminal reminders and detached checks using a stalled local HTTPS proxy."""
import argparse
import errno
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time

if os.name != 'posix':
    print('PTY notification verification requires a Unix host')
    sys.exit(0)
import pty

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, default=Path('target/debug/lenso'))
binary = parser.parse_args().binary.resolve(strict=True)
base=dict(os.environ)
for key in ['CI','GITHUB_ACTIONS','GITLAB_CI','BUILDKITE','CIRCLECI','TF_BUILD','LENSO_NO_UPDATE_CHECK','HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy']:
    base.pop(key,None)
base.update(TERM='xterm',CI='false',NO_PROXY='',no_proxy='')
server=socket.socket();server.bind(('127.0.0.1',0));server.listen();server.settimeout(.1)
connected=threading.Event();stop=threading.Event();requests=[]
def hold(connection):
    with connection:
        connection.recv(8192)
        requests.append(time.monotonic())
        connection.sendall(b'HTTP/1.1 200 Connection established\r\n\r\n')
        connected.set()
        stop.wait(6)
def accept():
    while not stop.is_set():
        try: connection,_=server.accept()
        except socket.timeout:continue
        except OSError:
            if stop.is_set(): break
            raise
        threading.Thread(target=hold,args=(connection,),daemon=True).start()
threading.Thread(target=accept,daemon=True).start()
base['HTTPS_PROXY']=base['https_proxy']='http://127.0.0.1:'+str(server.getsockname()[1])
with tempfile.TemporaryDirectory(prefix='lenso-notice-qa-') as tmp:
    root=Path(tmp);cache=root/'cache';env=dict(base,LENSO_CACHE_DIR=str(cache))
    result=cache/'v1/update-notice/result.json'
    def run(args,extra=None,terminal=True):
        start=time.monotonic();selected=dict(env,**(extra or {}))
        if terminal:
            master,slave=pty.openpty()
            p=subprocess.Popen([str(binary),*args],cwd=root,env=selected,stdin=slave,stderr=slave,stdout=subprocess.PIPE)
            out,_=p.communicate(timeout=2)
            os.set_blocking(master,False)
            chunks=[]
            while True:
                try:part=os.read(master,65536)
                except OSError as error:
                    if error.errno in (errno.EIO, errno.EAGAIN):break
                    raise
                if not part:break
                chunks.append(part)
            os.close(slave);os.close(master);err=b''.join(chunks)
        else:
            p=subprocess.run([str(binary),*args],cwd=root,env=selected,stdin=subprocess.DEVNULL,capture_output=True,timeout=2)
            out,err=p.stdout,p.stderr
        assert p.returncode==0,(out,err)
        return out,err,time.monotonic()-start
    out,err,elapsed=run(['cache','path'])
    assert b'New Lenso version' not in err
    assert connected.wait(2),'detached worker did not contact the controlled stalled proxy'
    assert elapsed<2,elapsed
    reservation=result.read_bytes()
    out,err,warm=run(['cache','path'])
    assert result.read_bytes()==reservation
    assert len(requests)==1,requests
    time.sleep(3.5)
    assert len(requests)==1
    def seed(fresh):
        result.write_text(json.dumps({'schema_version':1,'checked_at':int(time.time()) if fresh else 0,'latest':'9999.0.0'}))
    seed(True)
    out,err,_=run(['cache','path'])
    assert b'New Lenso version' in err and b'9999.0.0' in err,(out,err)
    for args,extra in [(['cache','path','--json'],{}),(['cache','path','--offline'],{}),(['cache','path','--no-interactive'],{}),(['cache','path'],{'CI':'true'}),(['cache','path'],{'GITHUB_ACTIONS':'true'}),(['cache','path'],{'LENSO_NO_UPDATE_CHECK':'1'}),(['--version'],{}),(['self','update','--help'],{})]:
        seed(True);out,err,_=run(args,extra)
        assert b'New Lenso version' not in err,(args,err)
        if '--json' in args:json.loads(out)
        seed(False);before=result.read_bytes()
        run(args,extra)
        time.sleep(.05)
        assert result.read_bytes()==before,args
    seed(True)
    out,err,_=run(['cache','path'],terminal=False)
    assert b'New Lenso version' not in err
    assert len(requests)==1,requests
    print('PASS: real terminal reminder; CI/offline/JSON/non-interactive/opt-out suppression; cached failure backoff')
    print('PASS: stalled network check did not block parent; cold %.1f ms, cached %.1f ms; one worker request'%(elapsed*1000,warm*1000))
stop.set();server.close()
