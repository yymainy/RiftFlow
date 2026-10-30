"""Keep the managed Filebrowser copy; publish verified GitHub Release links."""
import argparse, datetime, hashlib, json, os, pathlib, re, shutil, urllib.request

parser = argparse.ArgumentParser()
parser.add_argument('source')
parser.add_argument('--version', default='1.0.0')
parser.add_argument('--repository', default='yymainy/RiftFlow')
args = parser.parse_args()
if not re.fullmatch(r'\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?',args.version):raise SystemExit('Invalid version')
if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+',args.repository):raise SystemExit('Invalid GitHub repository')
source = pathlib.Path(args.source).resolve()
if not source.is_file() or source.suffix.lower() != '.exe':raise SystemExit('Expected an EXE file')
filename = source.name
directory = pathlib.Path('/www/filebrowser/public/RiftFlow')
directory.mkdir(parents=True,exist_ok=True)
os.chown(directory,1000,1000)
destination = directory/filename
if source != destination.resolve():shutil.copyfile(source,destination)
os.chown(destination,1000,1000)
destination.chmod(0o644)
with destination.open('rb') as stream:digest = hashlib.file_digest(stream,'sha256').hexdigest()
env = dict(line.split('=',1) for line in pathlib.Path('/opt/riftflow-backend/service.env').read_text().splitlines() if '=' in line)
download = f'https://github.com/{args.repository}/releases/download/v{args.version}/RiftFlow.exe'
# Never advertise an unuploaded or mismatched release asset.
remote_digest = hashlib.sha256()
remote_size = 0
with urllib.request.urlopen(download,timeout=60) as response:
    if response.status != 200:raise RuntimeError('GitHub release asset is unavailable')
    while chunk := response.read(1024*1024):
        remote_digest.update(chunk)
        remote_size += len(chunk)
if remote_size != destination.stat().st_size or remote_digest.hexdigest() != digest:
    raise RuntimeError('GitHub release asset differs from the local EXE')
for lang,description in [('zh-CN','接入 RiftFlow 独立公告与配置服务，修复公告符号，默认主题跟随系统。'),('en','Connect to the independent RiftFlow service; fix patch note entities and follow the system theme by default.')]:
    release = {'version':args.version,'publishedAt':datetime.datetime.now(datetime.timezone.utc).isoformat(),'description':description,'artifacts':[{'platform':'win32','arch':'x64','fileName':'RiftFlow.exe','size':destination.stat().st_size,'contentType':'application/octet-stream','sha256':digest,'downloadUrl':download}]}
    request = urllib.request.Request('http://127.0.0.1:8091/admin/release/'+lang,data=json.dumps(release).encode(),method='PUT',headers={'Authorization':'Bearer '+env['ADMIN_TOKEN'],'Content-Type':'application/json'})
    with urllib.request.urlopen(request,timeout=15) as response:
        assert response.status == 200
print(json.dumps({'downloadUrl':download,'sha256':digest,'size':destination.stat().st_size}))
