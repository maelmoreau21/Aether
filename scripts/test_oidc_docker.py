"""Disposable OIDC protocol + browser test for both built Docker modes.
Synthetic provider only, not an Authentik deployment. No existing data is touched.
Requires cryptography, Docker Desktop Windows named pipe, Chrome and Playwright.
"""
import base64
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
import os
from pathlib import Path
import secrets
import subprocess
import threading
import time
import uuid
from urllib.parse import urlsplit, parse_qs, urlencode
from cryptography.hazmat.primitives import serialization, hashes
from cryptography.hazmat.primitives.asymmetric import padding

ROOT = Path(__file__).resolve().parents[1]
PREFIX = 'oidcqa-' + uuid.uuid4().hex[:10]
WORK = ROOT / 'target' / PREFIX
WORK.mkdir(parents=True)
REPORT = {'run': PREFIX, 'provider': 'synthetic OIDC RS256, actual HTTP discovery/token/JWKS', 'modes': []}
ISSUER = 'http://localhost:48888/application/o/media/'
SECRET = secrets.token_hex(32)
CODES = {}
KEY = serialization.load_pem_private_key((ROOT/'crates/core/tests/fixtures/oidc/key1.pem').read_bytes(),None)
JWKS = json.loads((ROOT/'crates/core/tests/fixtures/oidc/jwks1.json').read_text())
MODE = {}
CONTAINERS = []

def enc(b): return base64.urlsafe_b64encode(b).decode().rstrip('=')
def token(claims):
    raw = enc(json.dumps({'alg':'RS256','kid':'test-1'}).encode()) + '.' + enc(json.dumps(claims).encode())
    return raw + '.' + enc(KEY.sign(raw.encode(),padding.PKCS1v15(),hashes.SHA256()))

class Provider(BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def reply(self,status,body):
        payload=json.dumps(body).encode();self.send_response(status);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(payload)));self.end_headers();self.wfile.write(payload)
    def do_GET(self):
        u=urlsplit(self.path)
        if u.path.endswith('/.well-known/openid-configuration'):
            self.reply(200,{'issuer':ISSUER,'authorization_endpoint':'http://localhost:48888/authorize','token_endpoint':'http://localhost:48888/token','jwks_uri':'http://localhost:48888/jwks','id_token_signing_alg_values_supported':['RS256']})
        elif u.path=='/jwks': self.reply(200,JWKS)
        elif u.path=='/authorize':
            q={k:v[0] for k,v in parse_qs(u.query).items()}
            assert q['client_id']=='docker-client' and q['code_challenge_method']=='S256'
            code=secrets.token_hex(24);CODES[code]=q
            self.send_response(302);self.send_header('Location',q['redirect_uri']+'?'+urlencode({'code':code,'state':q['state']}));self.send_header('Content-Length','0');self.end_headers()
        else: self.reply(404,{})
    def do_POST(self):
        assert self.path=='/token'
        q={k:v[0] for k,v in parse_qs(self.rfile.read(int(self.headers['Content-Length'])).decode()).items()}
        assert self.headers['Authorization']=='Basic '+base64.b64encode(('docker-client:'+SECRET).encode()).decode()
        original=CODES.pop(q['code'])
        assert q['redirect_uri']==original['redirect_uri'] and enc(hashlib.sha256(q['code_verifier'].encode()).digest())==original['code_challenge']
        now=int(time.time())
        self.reply(200,{'id_token':token({'iss':ISSUER,'sub':'docker-user','aud':'docker-client','iat':now,'exp':now+300,'nonce':original['nonce'],'groups':['media-users'],'email':'qa-admin@example.test'}),'access_token':'synthetic-never-exposed','token_type':'Bearer'})

# A loopback-only proxy inside Core's network namespace forwards to the host fixture.
PROXY = '''from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
import http.client
class Proxy(BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def handle_request(self):
  c=http.client.HTTPConnection('host.docker.internal',48889,timeout=15)
  b=self.rfile.read(int(self.headers.get('Content-Length',0)))
  c.request(self.command,self.path,b,dict(self.headers));r=c.getresponse();data=r.read()
  self.send_response(r.status)
  for k,v in r.getheaders():
   if k.lower() not in ['transfer-encoding','connection','server','date']:self.send_header(k,v)
  self.end_headers();self.wfile.write(data);c.close()
 do_GET=handle_request
 do_POST=handle_request
ThreadingHTTPServer(('0.0.0.0',48888),Proxy).serve_forever()
'''

def docker(*args):
    r=subprocess.run(['docker',*map(str,args)],capture_output=True,text=True,timeout=600)
    if r.returncode: raise RuntimeError('docker '+args[0]+' failed (output suppressed)')
    return r.stdout.strip()

# Reuse the repository's real TTY bootstrap, supplying only disposable names/secrets.
spec=importlib.util.spec_from_file_location('p2_fixture',ROOT/'scripts/test_p2_docker.py')
p2=importlib.util.module_from_spec(spec);spec.loader.exec_module(p2)
server=ThreadingHTTPServer(('0.0.0.0',48889),Provider)
threading.Thread(target=server.serve_forever,daemon=True).start()
(WORK/'proxy.py').write_text(PROXY)
try:
    for mode,image in [('standalone',os.environ.get('MEDIA_TEST_STANDALONE_IMAGE','aether:oidc-qa')),('distributed',os.environ.get('MEDIA_TEST_CORE_IMAGE','aether-core:oidc-qa'))]:
        name=PREFIX+'-'+mode
        env={'MEDIA_PUBLIC_URL':'http://127.0.0.1:48887','MEDIA_PORT':'3000','MEDIA_BIND_ADDR':'0.0.0.0','MEDIA_DB_PATH':'/data/core.sqlite3','MEDIA_OIDC_ENABLED':'true','MEDIA_OIDC_ISSUER':ISSUER,'MEDIA_OIDC_CLIENT_ID':'docker-client','MEDIA_OIDC_CLIENT_SECRET':SECRET,'MEDIA_OIDC_USER_GROUPS':'media-users','MEDIA_MODE':mode}
        envfile=WORK/(mode+'.env');envfile.write_text('\n'.join(k+'='+v for k,v in env.items()))
        docker('run','-d','--name',name,'--cap-drop','ALL','--security-opt','no-new-privileges:true','--env-file',envfile,'-p','127.0.0.1:48887:3000','-p','127.0.0.1:48888:48888',image);CONTAINERS.append(name)
        proxy=name+'-provider';docker('run','-d','--name',proxy,'--network','container:'+name,'--cap-drop','ALL','--security-opt','no-new-privileges:true','-v',str(WORK/'proxy.py')+':/proxy.py:ro','python:3.13-alpine','python','/proxy.py');CONTAINERS.append(proxy)
        origin='http://127.0.0.1:48887';p2.wait(origin);p2.bootstrap(name)
        # Browser assertions include full document redirect across different sites (localhost/127.0.0.1).
        browser=subprocess.run(['node',str(ROOT/'scripts/browser-oidc.cjs'),origin,p2.PASSWORD,str(WORK/(mode+'-browser.json'))],capture_output=True,text=True,timeout=120)
        if browser.returncode:
            (WORK/(mode+'-browser-error.txt')).write_text(browser.stderr)
            raise AssertionError(mode+' browser failed; inspect target report')
        report=json.loads((WORK/(mode+'-browser.json')).read_text());REPORT['modes'].append({'mode':mode,**report})
        logs=docker('logs',name)
        assert SECRET not in logs and p2.PASSWORD not in logs and 'synthetic-never-exposed' not in logs
        docker('rm','-f','-v',proxy);CONTAINERS.remove(proxy);docker('rm','-f','-v',name);CONTAINERS.remove(name)
    REPORT['success']=True
finally:
    server.shutdown()
    for name in reversed(CONTAINERS): docker('rm','-f','-v',name)
    (WORK/'report.json').write_text(json.dumps(REPORT,indent=2))
    print(json.dumps({'report':str(WORK/'report.json'),'success':REPORT.get('success',False)}))
