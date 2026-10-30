import http from 'node:http'
import { createHash, randomUUID, timingSafeEqual } from 'node:crypto'
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises'
import { resolve, dirname } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const languages = ['zh-CN', 'en']
const configKeys = ['app/feature-gates', 'sgp/league-servers', 'sgp/supported-queues', 'auto-select/groups']
const fail = (status, message) => { throw Object.assign(new Error(message), { status }) }
const check = (condition, message) => { if (!condition) fail(400, message) }
const string = value => typeof value === 'string'
const object = value => value && typeof value === 'object' && !Array.isArray(value)
const https = value => { try { return new URL(value).protocol === 'https:' } catch { return false } }
const iso = value => string(value) && /(?:Z|[+-]\d{2}:\d{2})$/.test(value) && Number.isFinite(Date.parse(value))

function validate(key, value) {
  check(object(value), '请求内容必须为 JSON 对象')
  if (key.startsWith('notice/')) {
    check(['low', 'medium', 'high'].includes(value.severity), '公告级别应为 low、medium 或 high')
    check(string(value.summary) && value.summary.trim() && value.summary.length <= 500, '请输入 1–500 字的公告摘要')
    check(string(value.content) && value.content.trim() && value.content.length <= 200000, '请输入公告正文，最多 200000 字')
    return { revision: randomUUID(), language: key.slice(7), severity: value.severity, summary: value.summary.trim(), contentType: 'text/markdown', content: value.content, updatedAt: new Date().toISOString() }
  }
  if (key === 'contacts') {
    check(Array.isArray(value.channels) && value.channels.length <= 30, '联系渠道最多 30 项')
    const ids = new Set()
    for (const channel of value.channels) {
      check(object(channel), '联系渠道格式错误')
      for (const [field, max] of [['id',64],['name',80],['identifier',120]]) check(string(channel[field]) && channel[field].trim() && channel[field].length <= max, `联系渠道 ${field} 无效`)
      check(!ids.has(channel.id), '联系渠道 ID 重复'); ids.add(channel.id)
      check(['qq','telegram','discord','email','website','other'].includes(channel.platform), '联系渠道平台无效')
      check(https(channel.url) || /^mailto:[^\s]+$/.test(channel.url), '联系地址必须使用 HTTPS 或 mailto')
      check(channel.password === null || (string(channel.password) && channel.password.trim() && channel.password.length <= 120), '群密码格式错误')
    }
  } else if (key.startsWith('release/')) {
    check(string(value.version) && /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(value.version), '版本号格式应为 1.0.0')
    check(iso(value.publishedAt) && string(value.description), '版本发布日期或说明无效')
    check(Array.isArray(value.artifacts) && value.artifacts.length <= 20, '安装包列表无效')
    for (const artifact of value.artifacts) {
      check(object(artifact), '安装包格式错误')
      for (const field of ['platform','arch','fileName','contentType']) check(string(artifact[field]) && artifact[field], `安装包 ${field} 无效`)
      check(Number.isSafeInteger(artifact.size) && artifact.size > 0 && https(artifact.downloadUrl), '安装包大小或 HTTPS 下载地址无效')
      check(string(artifact.sha256) && /^[a-f0-9]{64}$/i.test(artifact.sha256), '安装包必须提供 SHA256 校验值')
    }
  } else fail(404, '不支持修改此资源')
  return { ...value, ...(key === 'contacts' ? { updatedAt: new Date().toISOString() } : {}) }
}

async function body(req) {
  if (!/^application\/json(?:\s*;|$)/i.test(req.headers['content-type'] || '')) fail(415, '请使用 application/json')
  const chunks = []; let size = 0
  for await (const chunk of req) {
    size += chunk.length
    if (size > 1024 * 1024) fail(413, '请求内容超过 1 MB')
    chunks.push(chunk)
  }
  try { return JSON.parse(Buffer.concat(chunks).toString('utf8')) } catch { fail(400, 'JSON 格式错误') }
}

export async function createService({ dataDir, publicBase, adminToken, seeds = {} }) {
  const base = new URL(publicBase)
  if (!['https:','http:'].includes(base.protocol) || base.search || base.hash || base.username || base.password) throw new Error('PUBLIC_BASE_URL 无效')
  publicBase = base.href.replace(/\/$/, '')
  if (!adminToken || adminToken.length < 32) throw new Error('ADMIN_TOKEN 至少需要 32 个字符')
  await mkdir(dataDir, { recursive: true })
  const allowed = new Set(['notice/zh-CN','notice/en','contacts','release/zh-CN','release/en', ...configKeys.map(key => `config/${key}`)])
  const file = key => resolve(dataDir, key.replaceAll('/', '--') + '.json')
  async function load(key) {
    try { return JSON.parse(await readFile(file(key), 'utf8')) }
    catch (error) { if (error.code === 'ENOENT') return seeds[key] ?? null; throw error }
  }
  const expectedToken = createHash('sha256').update(adminToken).digest()
  function authorized(req) {
    if (!req.headers.authorization?.startsWith('Bearer ')) fail(401, '需要管理员身份')
    const actual = createHash('sha256').update(req.headers.authorization.slice(7)).digest()
    if (!timingSafeEqual(actual, expectedToken)) fail(401, '需要管理员身份')
  }
  function send(req, res, status, value) {
    const text = JSON.stringify(value)
    res.writeHead(status, { 'Content-Type':'application/json; charset=utf-8', 'Cache-Control':'no-store', 'X-Content-Type-Options':'nosniff', 'Content-Length':Buffer.byteLength(text) })
    res.end(req.method === 'HEAD' ? undefined : text)
  }
  const server = http.createServer(async (req, res) => {
    try {
      const url = new URL(req.url, 'http://localhost')
      const path = url.pathname
      if (path.startsWith('/admin/')) {
        authorized(req)
        const key = path.slice(7)
        if (!allowed.has(key) || key.startsWith('config/')) fail(404, '此管理资源不存在或暂为只读')
        if (req.method === 'GET') return send(req,res,200,await load(key))
        if (req.method !== 'PUT') fail(405, '仅支持 GET 和 PUT')
        const value = validate(key, await body(req))
        const temp = file(key) + '.' + randomUUID() + '.tmp'
        await writeFile(temp, JSON.stringify(value, null, 2), { mode:0o600 })
        await rename(temp, file(key))
        return send(req,res,200,value)
      }
      if (!['GET','HEAD'].includes(req.method)) fail(405, '仅支持读取')
      if (path === '/healthz') return send(req,res,200,{status:'ok',service:'riftflow-backend'})
      if (path === '/bootstrap.json') return send(req,res,200,{riftflowBootstrap:{schemaVersion:1,generation:2026102901,baseUrls:{api:publicBase,static:publicBase+'/static'}}})
      const language = url.searchParams.get('lang') || 'zh-CN'
      if (!languages.includes(language)) fail(400, '仅支持 zh-CN 或 en')
      let key
      if (path === '/notice/v1/latest') key = `notice/${language}`
      else if (path === '/website/v1/contact-channels') key = 'contacts'
      else if (path === '/releases/v1/latest') key = `release/${language}`
      else if (path.startsWith('/config/v1/') && configKeys.includes(path.slice(11))) key = `config/${path.slice(11)}`
      else fail(404, '接口不存在')
      const value = await load(key)
      if (value === null) fail(404, '尚未发布内容')
      send(req,res,200,value)
    } catch (error) {
      if (res.headersSent) return res.destroy()
      send(req,res,error.status || 500,{error:{code:error.status ? `HTTP_${error.status}` : 'INTERNAL_ERROR',message:error.status ? error.message : '服务暂时不可用'}})
      if (!error.status) console.error('Request failed:',error.code || error.name)
    }
  })
  server.requestTimeout = 15000
  server.headersTimeout = 10000
  server.keepAliveTimeout = 5000
  return server
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const seeds = JSON.parse(await readFile(new URL('./seed.json',import.meta.url),'utf8'))
  const server = await createService({dataDir:process.env.DATA_DIR || resolve(dirname(fileURLToPath(import.meta.url)),'data'),publicBase:process.env.PUBLIC_BASE_URL,adminToken:process.env.ADMIN_TOKEN,seeds})
  server.listen(Number(process.env.PORT || 8080),process.env.HOST || '127.0.0.1',() => console.log('RiftFlow backend ready'))
  for (const signal of ['SIGTERM','SIGINT']) process.on(signal,() => server.close(() => process.exit(0)))
}
