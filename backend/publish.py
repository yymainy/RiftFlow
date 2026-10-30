"""Publish from an SSH session; the administrator secret never leaves the server."""
import json, pathlib, sys, urllib.request

if len(sys.argv) != 3:
    raise SystemExit('Usage: sudo python3 publish.py notice/zh-CN announcement.json')
key, filename = sys.argv[1:]
if key not in ['notice/zh-CN','notice/en','contacts','release/zh-CN','release/en']:
    raise SystemExit('Unknown publication key')
env = dict(line.split('=',1) for line in pathlib.Path('/opt/riftflow-backend/service.env').read_text().splitlines() if '=' in line)
payload = json.loads(pathlib.Path(filename).read_text(encoding='utf-8-sig'))
request = urllib.request.Request('http://127.0.0.1:8091/admin/'+key,data=json.dumps(payload,ensure_ascii=False).encode(),method='PUT',headers={'Authorization':'Bearer '+env['ADMIN_TOKEN'],'Content-Type':'application/json'})
with urllib.request.urlopen(request,timeout=15) as response:
    value = json.load(response)
print(json.dumps({'published':True,'resource':key,'revision':value.get('revision'),'version':value.get('version')},ensure_ascii=False))
