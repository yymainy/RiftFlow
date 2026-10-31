"""Prepare HTTP challenge hosting, then activate HTTPS after acme.sh installs the certificate."""
import pathlib, shutil, subprocess, sys, time

domain = 'riftflow-api.yymainy.online'
nginx = '/www/server/nginx/sbin/nginx'
config = pathlib.Path('/www/server/panel/vhost/nginx/'+domain+'.conf')
root = pathlib.Path('/www/wwwroot/'+domain)
root.mkdir(exist_ok=True)
challenge = root/'.well-known/acme-challenge'
challenge.mkdir(parents=True,exist_ok=True)
subprocess.run(['chown','-R','ubuntu:ubuntu',str(root)],check=True)
activate = '--activate' in sys.argv
certdir = pathlib.Path('/etc/nginx/ssl/'+domain)
if activate:
    certdir.mkdir(parents=True,exist_ok=True)
    subprocess.run(['/home/ubuntu/.acme.sh/acme.sh','--home','/home/ubuntu/.acme.sh','--install-cert','-d',domain,'--ecc','--key-file',str(certdir/'key.pem'),'--fullchain-file',str(certdir/'fullchain.pem'),'--reloadcmd',nginx+' -s reload'],check=True)
if activate and not all((certdir/name).is_file() for name in ['fullchain.pem','key.pem']):raise RuntimeError('Certificate not installed')
content = f'''server {{
    listen 80;
    server_name {domain};
    location /.well-known/acme-challenge/ {{ root {root}; }}
    location / {{ {'return 301 https://$host$request_uri;' if activate else 'return 404;'} }}
}}
'''
if activate:
    content += f'''server {{
    listen 443 ssl;
    http2 on;
    server_name {domain};
    ssl_certificate {certdir}/fullchain.pem;
    ssl_certificate_key {certdir}/key.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    location ^~ /admin/ {{ return 404; }}
    location / {{
        proxy_pass http://127.0.0.1:8091;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Real-IP $remote_addr;
        client_max_body_size 1m;
        proxy_read_timeout 20s;
    }}
}}
'''
old = config.read_text() if config.exists() else None
config.write_text(content)
if subprocess.run([nginx,'-t']).returncode:
    if old is None:config.unlink()
    else:config.write_text(old)
    raise RuntimeError('Nginx configuration rejected and restored')
subprocess.run([nginx,'-s','reload'],check=True)
if activate:
    env = pathlib.Path('/opt/riftflow-backend/service.env')
    lines = env.read_text().splitlines()
    env.write_text('\n'.join('PUBLIC_BASE_URL=https://'+domain if line.startswith('PUBLIC_BASE_URL=') else line for line in lines)+'\n')
    env.chmod(0o600)
    subprocess.run(['docker','rm','-f','riftflow-backend'],check=True)
    subprocess.run(['docker','run','-d','--name','riftflow-backend','--restart','unless-stopped','--read-only','--cap-drop','ALL','--security-opt','no-new-privileges','--memory','256m','--cpus','1','--env-file',str(env),'-p','127.0.0.1:8091:8080','-v','/opt/riftflow-backend/data:/data','riftflow-backend:20261029'],check=True)
    for _ in range(30):
        if subprocess.run(['curl','-fsS','http://127.0.0.1:8091/healthz'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode == 0:break
        time.sleep(1)
    else:raise RuntimeError('New backend container unhealthy')
    subprocess.run(['curl','-fsS','https://'+domain+'/healthz'],check=True)
    # Remove only the temporary API block we added, retaining the file site.
    files = pathlib.Path('/www/server/panel/vhost/nginx/files.yymainy.online.conf')
    text = files.read_text()
    start = text.find('    # RiftFlow independent backend\n')
    end = text.find('    # filebrowser (docker, 127.0.0.1:8090)',start)
    if start >= 0 and end > start:
        backup = files.with_name(files.name+'.riftflow-domain-backup-'+str(time.time_ns()))
        shutil.copy2(files,backup)
        files.write_text(text[:start]+text[end:])
        if subprocess.run([nginx,'-t']).returncode:
            shutil.copy2(backup,files)
            raise RuntimeError('File site configuration restored after validation failure')
        subprocess.run([nginx,'-s','reload'],check=True)
print('HTTPS active' if activate else 'HTTP challenge host prepared')
