"""Run with sudo on the authorized Ubuntu host after uploading the release bundle."""
import os, pathlib, secrets, subprocess, time

base = pathlib.Path('/opt/riftflow-backend')
base.mkdir(exist_ok=True)
subprocess.run(['tar','-xzf','/home/ubuntu/riftflow-backend.tar.gz','-C',str(base)],check=True)
data = base/'data'
data.mkdir(exist_ok=True)
os.chown(data,1000,1000)
env = base/'service.env'
if not env.exists():
    env.write_text('PUBLIC_BASE_URL=https://riftflow-api.yymainy.online\nADMIN_TOKEN='+secrets.token_hex(32)+'\n')
    env.chmod(0o600)
subprocess.run(['docker','build','-t','riftflow-backend:20261029',str(base)],check=True)
# Only replace this application's container on redeploy.
existing = subprocess.run(['docker','ps','-a','--filter','name=^/riftflow-backend$','--format','{{.Names}}'],capture_output=True,text=True,check=True).stdout.strip()
if existing == 'riftflow-backend':
    subprocess.run(['docker','rm','-f','riftflow-backend'],check=True)
subprocess.run(['docker','run','-d','--name','riftflow-backend','--restart','unless-stopped','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges','--memory','256m','--cpus','1','--env-file',str(env),'-p','127.0.0.1:8091:8080','-v',str(data)+':/data','riftflow-backend:20261029'],check=True)
for attempt in range(30):
    result = subprocess.run(['curl','-fsS','http://127.0.0.1:8091/healthz'],capture_output=True,text=True)
    if result.returncode == 0:break
    time.sleep(1)
else:raise RuntimeError('Backend failed its startup health check')

print('Deployment healthy; run configure_domain.py --activate for the configured HTTPS domain')
