"""Run via the authorized SSH connection; provider JSON is read from stdin, never logged."""
import json,os,pathlib,shutil,subprocess,sys,time

base=pathlib.Path('/opt/riftflow-backend')
source=pathlib.Path(__file__).resolve().parent
provider=json.load(sys.stdin)
if not all(isinstance(provider.get(k),str) and provider[k].strip() for k in ['url','api_key','model']):
    raise SystemExit('Provider configuration is incomplete')
stamp=str(int(time.time()))
backup=base.parent/'riftflow-backups'/('secure-ai-'+stamp)
backup.mkdir(mode=0o700,parents=True)
files=['server.mjs','ai-config.mjs','package.json','Dockerfile']
for name in files:
    if (base/name).exists():shutil.copy2(base/name,backup/name)
    shutil.copy2(source/name,base/name)
private=base/'data'/'ai-provider.json'
if private.exists():shutil.copy2(private,backup/'ai-provider.json')
temporary=private.with_suffix('.tmp')
temporary.write_text(json.dumps(provider,ensure_ascii=False),encoding='utf-8')
temporary.chmod(0o600);os.chown(temporary,1000,1000);temporary.replace(private)
nginx=pathlib.Path('/www/server/panel/vhost/nginx/riftflow-api.yymainy.online.conf')
if nginx.exists():
    text=nginx.read_text()
    if 'proxy_set_header X-Real-IP' not in text:
        shutil.copy2(nginx,backup/'nginx.conf')
        text=text.replace('proxy_set_header Host $host;','proxy_set_header Host $host;\n        proxy_set_header X-Real-IP $remote_addr;')
        nginx.write_text(text)
        result=subprocess.run(['/www/server/nginx/sbin/nginx','-t'],capture_output=True)
        if result.returncode:
            shutil.copy2(backup/'nginx.conf',nginx)
            raise SystemExit('Nginx validation failed; previous config restored')
        subprocess.run(['/www/server/nginx/sbin/nginx','-s','reload'],check=True)
old=subprocess.check_output(['docker','inspect','--format','{{.Image}}','riftflow-backend'],text=True).strip()
tag='riftflow-backend:secure-ai-'+stamp
def run(image):
    subprocess.run(['docker','run','-d','--name','riftflow-backend','--restart','unless-stopped','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges','--memory','256m','--cpus','1','--env-file',str(base/'service.env'),'-p','127.0.0.1:8091:8080','-v',str(base/'data')+':/data',image],check=True,stdout=subprocess.DEVNULL)
try:
    subprocess.run(['docker','build','-t',tag,str(base)],check=True,stdout=subprocess.DEVNULL)
    subprocess.run(['docker','rm','-f','riftflow-backend'],check=True,stdout=subprocess.DEVNULL)
    run(tag)
    for attempt in range(25):
        if subprocess.run(['curl','-fsS','http://127.0.0.1:8091/healthz'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0:break
        time.sleep(1)
    else:raise RuntimeError('New service failed health check')
except Exception:
    subprocess.run(['docker','rm','-f','riftflow-backend'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    run(old)
    for name in files:
        if (backup/name).exists():shutil.copy2(backup/name,base/name)
    raise
print('Encrypted AI configuration service deployed; health check passed. Backup:',backup.name)
