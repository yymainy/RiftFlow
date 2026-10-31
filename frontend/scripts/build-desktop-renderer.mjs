import { resolveConfig } from 'electron-vite'
import { cp, readFile, stat } from 'node:fs/promises'
import { resolve } from 'node:path'
import { build } from 'vite'

import { verifyDesktopAssets } from './verify-desktop-assets.mjs'

const iconDirectory = resolve(import.meta.dirname, '../resources/game-data')
const iconManifest = JSON.parse(await readFile(resolve(iconDirectory, 'manifest.json'), 'utf8'))
for (const path of new Set(Object.values(iconManifest.files))) {
  const file = await stat(resolve(iconDirectory, path))
  if (!file.isFile() || file.size < 100) throw new Error(`Missing game icon: ${path}`)
}

// Build the renderer entries for the Rust/Tauri desktop host.
// This host does not use Electron main/preload or native-addon binaries.
process.env.NODE_ENV = 'production'
const { config } = await resolveConfig({}, 'build', 'production')
if (!config?.renderer) throw new Error('Missing renderer configuration')
const renderer = config.renderer
renderer.build = {
  ...renderer.build,
  outDir: resolve(process.argv[2] || '../riftflow-rs/renderer-dist'),
  emptyOutDir: true
}
await build(renderer)
await cp(iconDirectory, resolve(renderer.build.outDir, 'game-data'), { recursive: true })
console.log(`Bundled ${Object.keys(iconManifest.files).length} game icons`)
const report = await verifyDesktopAssets({ rendererDir: renderer.build.outDir })
console.log(
  `Verified ${report.entries.length} windows, ${report.iconCount} icons; renderer SHA256 ${report.rendererSha256}`
)
