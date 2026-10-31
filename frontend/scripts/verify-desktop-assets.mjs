import { createHash } from 'node:crypto'
import { readFile, readdir, stat } from 'node:fs/promises'
import { dirname, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const fail = (message) => {
  throw new Error(message)
}
const json = async (path) => JSON.parse(await readFile(path, 'utf8'))
function inside(base, name) {
  const path = resolve(base, name)
  if (!path.startsWith(resolve(base) + sep)) fail(`Asset path escapes directory: ${name}`)
  return path
}
async function files(directory) {
  const result = []
  for (const item of await readdir(directory, { withFileTypes: true })) {
    const path = resolve(directory, item.name)
    if (item.isSymbolicLink()) fail(`Symlink is not permitted in renderer assets: ${path}`)
    if (item.isDirectory()) result.push(...(await files(path)))
    else if (item.isFile()) result.push(path)
  }
  return result.sort()
}
export async function verifyDesktopAssets({
  rendererDir = resolve(root, 'riftflow-rs/renderer-dist')
} = {}) {
  const iconDir = resolve(root, 'frontend/resources/game-data')
  const manifest = await json(resolve(iconDir, 'manifest.json'))
  const native = await json(
    resolve(root, 'riftflow-rs/crates/riftflow-app/assets/game-icon-map.json')
  )
  if (JSON.stringify(manifest) !== JSON.stringify(native))
    fail('Icon manifest differs from native game-icon-map.json; run prepare-game-icons.py')
  const icons = [...new Set(Object.values(manifest.files || {}))]
  if (!icons.length) fail('Game icon manifest is empty')
  for (const family of ['champion-icons/', 'item-icons/', 'spell-icons/', 'perk-images/']) {
    if (!icons.some((path) => path.includes(family))) fail(`Missing icon family: ${family}`)
  }
  for (const name of icons) {
    const source = await readFile(inside(iconDir, name))
    const png = source.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))
    const jpeg = source[0] === 255 && source[1] === 216
    const webp =
      source.toString('ascii', 0, 4) === 'RIFF' && source.toString('ascii', 8, 12) === 'WEBP'
    if (source.length < 100 || !(png || jpeg || webp)) fail(`Invalid cached game image: ${name}`)
    const bundled = await readFile(inside(resolve(rendererDir, 'game-data'), name))
    if (!source.equals(bundled)) fail(`Bundled game image missing or stale: ${name}`)
  }
  const bundledManifest = await json(resolve(rendererDir, 'game-data/manifest.json'))
  if (JSON.stringify(manifest) !== JSON.stringify(bundledManifest))
    fail('Bundled icon manifest is stale')
  const vite = await readFile(resolve(root, 'frontend/electron.vite.config.ts'), 'utf8')
  const entries = [
    ...new Set([...vite.matchAll(/src\/renderer\/([^'"\s]+\.html)/g)].map((match) => match[1]))
  ]
  if (!entries.length) fail('No renderer entries found in Vite configuration')
  const nativeSources = (await files(resolve(root, 'riftflow-rs/crates/riftflow-app/src'))).filter(
    (path) => path.endsWith('.rs')
  )
  for (const path of nativeSources) {
    const source = await readFile(path, 'utf8')
    for (const match of source.matchAll(/"([a-z0-9-]+-window\.html)"/g)) {
      if (!entries.includes(match[1])) fail(`Native window has no Vite entry: ${match[1]}`)
    }
  }
  for (const name of entries) {
    const path = inside(rendererDir, name)
    const html = await readFile(path, 'utf8')
    if (!html.includes('type="module"')) fail(`Renderer entry has no module script: ${name}`)
    for (const match of html.matchAll(/(?:src|href)=["']([^"']+)["']/g)) {
      const link = match[1].split(/[?#]/)[0]
      if (!link || /^(?:[a-z]+:|\/\/|#)/i.test(link)) continue
      const target = inside(rendererDir, link.replace(/^\//, ''))
      if (!(await stat(target)).isFile()) fail(`Missing HTML asset: ${name} -> ${link}`)
    }
  }
  const frontend = await json(resolve(root, 'frontend/package.json'))
  const tauri = await json(resolve(root, 'riftflow-rs/crates/riftflow-app/tauri.conf.json'))
  const cargo = await readFile(resolve(root, 'riftflow-rs/Cargo.toml'), 'utf8')
  const version = cargo.match(/\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1]
  if (!version || version !== frontend.version || version !== tauri.version)
    fail('Frontend, Cargo workspace, and Tauri versions must match')
  const all = await files(rendererDir)
  for (const path of all.filter((path) => /\.(?:js|css)$/.test(path))) {
    const source = await readFile(path, 'utf8')
    const references = [
      ...source.matchAll(
        /["']((?:\.{1,2}\/|assets\/)[^"'\n]+?\.(?:js|css|png|jpe?g|webp|ico|woff2?)(?:\?[^"'\n]*)?)["']/g
      )
    ].map((match) => match[1])
    if (path.endsWith('.css')) {
      references.push(
        ...[...source.matchAll(/url\(\s*["']?([^\s"')]+)["']?\s*\)/g)].map((match) => match[1])
      )
    }
    for (const link of references) {
      if (/^(?:[a-z]+:|\/\/|#)/i.test(link)) continue
      const clean = link.split(/[?#]/)[0]
      const target = clean.startsWith('assets/')
        ? inside(rendererDir, clean)
        : inside(rendererDir, relative(rendererDir, resolve(dirname(path), clean)))
      if (!(await stat(target)).isFile())
        fail(`Missing chunk/style asset: ${relative(rendererDir, path)} -> ${clean}`)
    }
  }
  const fingerprint = createHash('sha256')
  let totalBytes = 0
  for (const path of all) {
    const name = relative(rendererDir, path).replaceAll('\\', '/')
    if (
      /(?:^|\/)(?:ai-config\.json|ai-provider\.json|service\.env)|\.(?:dpapi|db|log|map)$/i.test(
        name
      )
    )
      fail(`Private or debug file found in renderer: ${name}`)
    const data = await readFile(path)
    fingerprint.update(name + '\0').update(createHash('sha256').update(data).digest())
    totalBytes += data.length
  }
  if (
    resolve(root, 'riftflow-rs/crates/riftflow-app', tauri.build.frontendDist) !==
    resolve(rendererDir)
  ) {
    fail('Renderer output is not the frontendDist directory embedded by Tauri')
  }
  return {
    version,
    entries,
    iconVersion: manifest.version,
    iconCount: icons.length,
    rendererFiles: all.length,
    rendererBytes: totalBytes,
    rendererSha256: fingerprint.digest('hex')
  }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const report = await verifyDesktopAssets()
    console.log(JSON.stringify(report, null, 2))
  } catch (error) {
    console.error('Desktop asset verification failed:', error.message)
    process.exitCode = 1
  }
}
