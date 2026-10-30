"""Move this release into the existing Filebrowser public source."""
import hashlib, json, os, pathlib, shutil, subprocess, time

name = 'RiftFlow-1.5.1-20261029.exe'
source = pathlib.Path('/www/wwwroot/files.yymainy.online/downloads/riftflow')/name
folder = pathlib.Path('/www/filebrowser/public/RiftFlow')
target = folder/name
expected = '67c58aec157d8301a48711a41d7d3e81cae724942b2eee892f740cb16f9b9189'
def digest(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
folder.mkdir(exist_ok=True)
os.chown(folder,1000,1000)
if not target.exists():
    if digest(source) != expected:raise RuntimeError('Original file checksum mismatch')
    shutil.copy2(source,target)
if digest(target) != expected:raise RuntimeError('Filebrowser copy checksum mismatch')
os.chown(target,1000,1000)
target.chmod(0o644)

config = pathlib.Path('/www/server/panel/vhost/nginx/files.yymainy.online.conf')
text = config.read_text()
start = text.find('    # RiftFlow release downloads\n')
end = text.find('    # filebrowser (docker, 127.0.0.1:8090)',start)
if start >= 0:
    if end <= start:raise RuntimeError('Unexpected Nginx structure')
    backup = config.with_name(config.name+'.filebrowser-migration-'+str(time.time_ns()))
    shutil.copy2(config,backup)
    config.write_text(text[:start]+text[end:])
    if subprocess.run(['/www/server/nginx/sbin/nginx','-t']).returncode:
        shutil.copy2(backup,config)
        raise RuntimeError('Nginx validation failed; configuration restored')
    subprocess.run(['/www/server/nginx/sbin/nginx','-s','reload'],check=True)
if source.exists():source.unlink()
for old in [source.parent,source.parent.parent]:
    if old.exists() and not any(old.iterdir()):old.rmdir()
print(json.dumps({'filebrowserPath':'公开下载 / RiftFlow / '+name,'hostPath':str(target),'sha256':digest(target),'oldFileRemoved':not source.exists()},ensure_ascii=False))
