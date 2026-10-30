"""Rasterize the approved vector and pack a multi-resolution Windows icon.
Requires the local Python Playwright installation and Microsoft Edge.
"""
from pathlib import Path
import struct
from playwright.sync_api import sync_playwright

project = Path(__file__).resolve().parents[1]
workspace = project.parent
brand = project / 'src/renderer-shared/assets/brand'
svg = (brand / 'riftflow-icon.svg').read_text(encoding='utf-8')
mark = (brand / 'riftflow-mark.svg').read_text(encoding='utf-8')
sizes = [16, 20, 24, 32, 48, 64, 128, 256]
images = []
with sync_playwright() as p:
    browser = p.chromium.launch(channel='msedge', headless=True)
    page = browser.new_page(device_scale_factor=1)
    for size in sizes:
        page.set_viewport_size({'width': size, 'height': size})
        page.set_content('<style>html,body{margin:0;background:transparent}svg{display:block;width:100vw;height:100vh}</style>' + svg)
        data = page.screenshot(omit_background=True)
        (brand / f'riftflow-{size}.png').write_bytes(data)
        images.append(data)
    page.set_viewport_size({'width': 32, 'height': 32})
    page.set_content('<style>html,body{margin:0;background:transparent}svg{width:32px;height:32px}</style>' + mark.replace('#acc9b6','#ffffff').replace('#e3e8e0','#ffffff'))
    (project / 'resources/iconTemplate.png').write_bytes(page.screenshot(omit_background=True))
    browser.close()
header = struct.pack('<HHH', 0, 1, len(images))
offset = 6 + len(images) * 16
entries = []
for size, data in zip(sizes, images):
    entries.append(struct.pack('<BBBBHHII', size if size < 256 else 0, size if size < 256 else 0, 0, 0, 1, 32, len(data), offset))
    offset += len(data)
ico = header + b''.join(entries) + b''.join(images)
for destination in [project/'resources/RIFTFLOW_ICON.ico', workspace/'riftflow-rs/crates/riftflow-app/assets/RIFTFLOW_ICON.ico', workspace/'riftflow-rs/crates/riftflow-app/icons/icon.ico', brand/'riftflow.ico']:
    destination.write_bytes(ico)
for destination in [project/'build/icon.png', workspace/'riftflow-rs/crates/riftflow-app/icons/icon.png']:
    destination.write_bytes(images[-1])
print('Generated 8 PNG sizes, ICO resources, application icon and transparent tray template')
