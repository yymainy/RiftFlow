"""Cache public game icons for standalone desktop builds (no client required)."""
import concurrent.futures
import json
import pathlib
import time
import urllib.request

root = pathlib.Path(__file__).resolve().parents[2]
cache = root / 'frontend/resources/game-data'
cache.mkdir(parents=True, exist_ok=True)

def read_json(url):
    with urllib.request.urlopen(url, timeout=30) as response:
        return json.load(response)

version = read_json('https://ddragon.leagueoflegends.com/api/versions.json')[0]
cdn = f'https://ddragon.leagueoflegends.com/cdn/{version}'
champions = json.loads((root / 'riftflow-rs/crates/riftflow-app/assets/champion-summary.json').read_text(encoding='utf-8'))
jobs = {}
mapping = {}
for champion in champions:
    if champion['id'] <= 0:
        continue
    local = f"champion-icons/{champion['id']}.png"
    jobs[local] = f"https://game.gtimg.cn/images/lol/act/img/champion/{champion['alias']}.png"
    mapping[local] = local
for item_id, item in read_json(f'{cdn}/data/en_US/item.json')['data'].items():
    local = f'item-icons/{item_id}.png'
    jobs[local] = f"{cdn}/img/item/{item['image']['full']}"
    mapping[f'item-icons/{item_id}.png'] = local
for spell in read_json(f'{cdn}/data/en_US/summoner.json')['data'].values():
    name = pathlib.PurePosixPath(spell['image']['full']).stem.lower()
    local = f'spell-icons/{name}.png'
    jobs[local] = f"{cdn}/img/spell/{spell['image']['full']}"
    mapping[f'spell-names/{name}'] = local
for style in read_json(f'{cdn}/data/en_US/runesReforged.json'):
    for entry in [style] + [rune for slot in style['slots'] for rune in slot['runes']]:
        local = entry['icon'].lower()
        jobs[local] = 'https://ddragon.leagueoflegends.com/cdn/img/' + entry['icon']
        mapping[local] = local

def valid(data):
    return len(data) > 100 and (data.startswith(b'\x89PNG\r\n\x1a\n') or data.startswith(b'\xff\xd8') or data.startswith(b'RIFF'))

def download(entry):
    local, url = entry
    path = cache / local
    if path.is_file() and valid(path.read_bytes()):
        return
    for attempt in range(3):
        try:
            with urllib.request.urlopen(url, timeout=25) as response:
                data = response.read()
            if not valid(data):
                raise ValueError('Not an image: ' + local)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            return
        except Exception:
            if attempt == 2:
                raise
            time.sleep(1)

print(f'Preparing {len(jobs)} icons, Data Dragon {version}', flush=True)
with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
    for index, _ in enumerate(pool.map(download, jobs.items()), 1):
        if index % 100 == 0:
            print(f'{index}/{len(jobs)} verified', flush=True)
manifest = json.dumps({'version': version, 'files': mapping}, ensure_ascii=False, indent=2) + '\n'
(cache / 'manifest.json').write_text(manifest, encoding='utf-8')
(root / 'riftflow-rs/crates/riftflow-app/assets/game-icon-map.json').write_text(manifest, encoding='utf-8')
print(f'All {len(jobs)} icons ready', flush=True)
