import { resolveConfig } from 'electron-vite'
import { resolve } from 'node:path'
import { build } from 'vite'

// Build the original five renderer entries for the Rust/Tauri desktop host.
// This host does not use Electron main/preload or native-addon binaries.
process.env.NODE_ENV = 'production'
const { config } = await resolveConfig({}, 'build', 'production')
if (!config?.renderer) throw new Error('Missing renderer configuration')
const renderer = config.renderer
renderer.build = {
  ...renderer.build,
  outDir: resolve(process.argv[2] || '../riftflow-rs/renderer-build'),
  emptyOutDir: true
}
await build(renderer)
