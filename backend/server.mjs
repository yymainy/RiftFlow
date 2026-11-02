import http from 'node:http'
import { isIP } from 'node:net'
import { createHash, randomBytes, randomUUID, scryptSync, timingSafeEqual } from 'node:crypto'
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises'
import { resolve, dirname } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { encryptedAiConfig, validateAiConfig } from './ai-config.mjs'

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

export async function createService({ dataDir, publicBase, adminToken, adminUser='admin', adminPassword, seeds = {}, aiConfigFile = resolve(dataDir,'ai-provider.json') }) {
  const base = new URL(publicBase)
  if (!['https:','http:'].includes(base.protocol) || base.search || base.hash || base.username || base.password) throw new Error('PUBLIC_BASE_URL 无效')
  publicBase = base.href.replace(/\/$/, '')
  if (!adminToken || adminToken.length < 32) throw new Error('ADMIN_TOKEN 至少需要 32 个字符')
  await mkdir(dataDir, { recursive: true })
  const adminHtml = await readFile(new URL('./admin.html', import.meta.url), 'utf8')
  const allowed = new Set(['notice/zh-CN','notice/en','contacts','release/zh-CN','release/en', ...configKeys.map(key => `config/${key}`)])
  const file = key => resolve(dataDir, key.replaceAll('/', '--') + '.json')
  async function load(key) {
    try { return JSON.parse(await readFile(file(key), 'utf8')) }
    catch (error) { if (error.code === 'ENOENT') return seeds[key] ?? null; throw error }
  }
  async function readAiConfig() {
    try { return JSON.parse(await readFile(aiConfigFile, 'utf8')) }
    catch (error) { if (error.code === 'ENOENT') return null; throw error }
  }
  const publicAiConfig = value => ({url:value.url||'',model:value.model||'',timeout_seconds:value.timeout_seconds||90,max_tokens:value.max_tokens||1800,api_key_set:!!(value.api_key&&value.api_key.trim())})
  async function writeResource(key, value) {
    const temp = file(key) + '.' + randomUUID() + '.tmp'
    await writeFile(temp, JSON.stringify(value, null, 2), { mode:0o600 })
    await rename(temp, file(key))
    return value
  }
  function extractJson(text) {
    const stripped = String(text).trim().replace(/^```(?:json)?\s*/i, '').replace(/\s*```$/, '')
    const start = stripped.indexOf('{'), end = stripped.lastIndexOf('}')
    if (start < 0 || end <= start) throw new Error('翻译结果不是 JSON')
    return JSON.parse(stripped.slice(start, end + 1))
  }
  async function translateNotice(summary, content) {
    const config = await readAiConfig()
    if (!config?.url || !config?.api_key || !config?.model) throw new Error('未配置 AI 默认上游')
    if (content.length > 20000) throw new Error('公告正文过长，暂不自动翻译')
    const base = config.url.replace(/\/+$/, '')
    const endpoint = base.endsWith('/chat/completions') ? base : base + '/chat/completions'
    const controller = new AbortController()
    const timer = setTimeout(() => controller.abort(), Math.min(90, Number(config.timeout_seconds) || 90) * 1000)
    let response
    try {
      response = await fetch(endpoint, {
        method: 'POST',
        headers: { 'Content-Type':'application/json', Authorization: 'Bearer ' + config.api_key },
        body: JSON.stringify({
          model: config.model,
          temperature: 0.1,
          max_tokens: Math.min(8192, Math.max(1024, Number(config.max_tokens) || 1800)),
          messages: [
            { role:'system', content:'You are a precise Chinese-to-English translator for in-app notices of a Windows desktop application. Preserve Markdown structure, links, code, numbers and product names such as RiftFlow and League of Legends. Translate faithfully; do not add or remove facts. Return strictly one JSON object with string fields summary and content, without code fences or explanation.' },
            { role:'user', content: JSON.stringify({ summary, content }) }
          ]
        }),
        signal: controller.signal
      })
    } finally { clearTimeout(timer) }
    if (!response.ok) throw new Error('AI 翻译请求失败（HTTP ' + response.status + '）')
    const payload = await response.json()
    const message = payload?.choices?.[0]?.message?.content
    if (typeof message !== 'string') throw new Error('AI 翻译返回格式无效')
    const result = extractJson(message)
    if (!string(result.summary) || !string(result.content)) throw new Error('AI 翻译缺少摘要或正文')
    return { summary: result.summary.trim().slice(0,500), content: result.content.trim() }
  }
  const expectedToken = createHash('sha256').update(adminToken).digest()
  const loginUser = adminUser || 'admin'
  const loginPassword = adminPassword || adminToken
  const expectedPassword = createHash('sha256').update(loginPassword).digest()
  const authFile = resolve(dataDir, 'admin-auth.json')
  const sessions = new Map()
  async function readAuth() {
    try { return JSON.parse(await readFile(authFile, 'utf8')) }
    catch (error) { if (error.code === 'ENOENT') return null; throw error }
  }
  function verifyScrypt(password, auth) {
    const salt = Buffer.from(auth?.salt || '', 'base64')
    const expected = Buffer.from(auth?.hash || '', 'base64')
    if (salt.length !== 16 || expected.length !== 64) return false
    return timingSafeEqual(scryptSync(password, salt, 64), expected)
  }
  async function verifyLogin(username, password) {
    const auth = await readAuth()
    const expectedUser = createHash('sha256').update(auth?.username || loginUser).digest()
    const userOk = matches(username, expectedUser)
    const passwordOk = auth ? verifyScrypt(password, auth) : matches(password, expectedPassword)
    return Boolean(userOk & passwordOk)
  }
  function matches(value, expected) {
    if (typeof value !== 'string') return false
    return timingSafeEqual(createHash('sha256').update(value).digest(), expected)
  }
  function cookieToken(req) {
    const entry = (req.headers.cookie || '').split(';').map(value => value.trim()).find(value => value.startsWith('rf_admin_session='))
    return entry ? decodeURIComponent(entry.slice('rf_admin_session='.length)) : null
  }
  function sessionFor(req) {
    const token = cookieToken(req)
    if (!token) return null
    const session = sessions.get(token)
    if (!session) return null
    if (session.expires <= Date.now()) { sessions.delete(token); return null }
    return session
  }
  function bearerAuthorized(req) {
    if (!req.headers.authorization?.startsWith('Bearer ')) return false
    return timingSafeEqual(createHash('sha256').update(req.headers.authorization.slice(7)).digest(), expectedToken)
  }
  function authorized(req) {
    if (bearerAuthorized(req) || sessionFor(req)) return
    fail(401, '需要管理员身份')
  }
  function sessionCookie(req, token, maxAge) {
    const secure = req.headers['x-forwarded-proto'] === 'https' ? '; Secure' : ''
    return `rf_admin_session=${encodeURIComponent(token)}; HttpOnly; Path=/admin; SameSite=Strict; Max-Age=${maxAge}${secure}`
  }
  function clientIp(req) {
    const forwarded = req.headers['x-real-ip']
    return typeof forwarded === 'string' && isIP(forwarded) ? forwarded : (req.socket.remoteAddress || 'unknown')
  }
  function send(req, res, status, value, headers = {}) {
    const text = JSON.stringify(value)
    res.writeHead(status, { 'Content-Type':'application/json; charset=utf-8', 'Cache-Control':'no-store', 'X-Content-Type-Options':'nosniff', 'Content-Length':Buffer.byteLength(text), ...headers })
    res.end(req.method === 'HEAD' ? undefined : text)
  }
  function sendHtml(req, res, status, text) {
    res.writeHead(status, { 'Content-Type':'text/html; charset=utf-8', 'Cache-Control':'no-store', 'X-Content-Type-Options':'nosniff', 'X-Frame-Options':'DENY', 'Content-Security-Policy':"default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'", 'Content-Length':Buffer.byteLength(text) })
    res.end(req.method === 'HEAD' ? undefined : text)
  }
  const aiLimits = new Map()
  const adminLimits = new Map()
  const loginLimits = new Map()
  const server = http.createServer(async (req, res) => {
    try {
      const url = new URL(req.url, 'http://localhost')
      const path = url.pathname
      if(path==='/ai/v1/config'){
        if(req.method!=='POST')fail(405,'仅支持加密配置请求')
        const forwarded=req.headers['x-real-ip']
        const now=Date.now(),ip=typeof forwarded==='string'&&isIP(forwarded)?forwarded:(req.socket.remoteAddress||'unknown')
        for(const [key,bucket] of aiLimits)if(now-bucket.at>60000)aiLimits.delete(key)
        if(!aiLimits.has(ip)&&aiLimits.size>=10000)fail(429,'请求过于频繁')
        const bucket=aiLimits.get(ip)||{at:now,count:0}
        if(now-bucket.at>=60000){bucket.at=now;bucket.count=0}
        if(++bucket.count>30)fail(429,'请求过于频繁，请稍后重试')
        aiLimits.set(ip,bucket)
        return send(req,res,200,await encryptedAiConfig(aiConfigFile,await body(req)))
      }
      if (path === '/admin/login') {
        if (req.method !== 'POST') fail(405, '仅支持 POST')
        const now = Date.now(), ip = clientIp(req)
        for (const [key, bucket] of loginLimits) if (now - bucket.at > 60000) loginLimits.delete(key)
        const bucket = loginLimits.get(ip) || { at: now, count: 0 }
        if (now - bucket.at >= 60000) { bucket.at = now; bucket.count = 0 }
        if (++bucket.count > 10) fail(429, '登录尝试过于频繁，请稍后再试')
        loginLimits.set(ip, bucket)
        const input = await body(req)
        check(object(input), '请求内容必须为 JSON 对象')
        const ok = await verifyLogin(input.username, input.password)
        if (!ok) fail(401, '账号或密码错误')
        for (const [key, session] of sessions) if (session.expires <= now) sessions.delete(key)
        const token = randomBytes(32).toString('hex')
        sessions.set(token, { user: loginUser, expires: Date.now() + 7 * 24 * 60 * 60 * 1000 })
        loginLimits.delete(ip)
        return send(req, res, 200, { ok: true, user: loginUser }, { 'Set-Cookie': sessionCookie(req, token, 604800) })
      }
      if (path === '/admin/logout') {
        if (req.method !== 'POST') fail(405, '仅支持 POST')
        const token = cookieToken(req)
        if (token) sessions.delete(token)
        return send(req, res, 200, { ok: true }, { 'Set-Cookie': sessionCookie(req, '', 0) })
      }
      if (path === '/admin/session') {
        if (req.method !== 'GET') fail(405, '仅支持 GET')
        const session = sessionFor(req)
        if (session) return send(req, res, 200, { user: session.user })
        if (bearerAuthorized(req)) return send(req, res, 200, { user: loginUser })
        fail(401, '需要管理员身份')
      }
      if (path === '/admin/password') {
        if (req.method !== 'POST') fail(405, '仅支持 POST')
        if (!sessionFor(req) && !bearerAuthorized(req)) fail(401, '需要管理员身份')
        const input = await body(req)
        check(object(input), '请求内容必须为 JSON 对象')
        const current = string(input.current_password) ? input.current_password : ''
        const next = string(input.new_password) ? input.new_password : ''
        check(await verifyLogin(loginUser, current), '当前密码错误')
        check(next.length >= 10 && next.length <= 200, '新密码长度需为 10–200 位')
        check(next !== current, '新密码不能与当前密码相同')
        const salt = randomBytes(16)
        const hash = scryptSync(next, salt, 64)
        const auth = { version: 1, username: loginUser, salt: salt.toString('base64'), hash: hash.toString('base64'), updatedAt: new Date().toISOString() }
        await mkdir(dirname(authFile), { recursive: true })
        const temp = authFile + '.' + randomUUID() + '.tmp'
        await writeFile(temp, JSON.stringify(auth, null, 2), { mode:0o600 })
        await rename(temp, authFile)
        sessions.clear()
        const token = randomBytes(32).toString('hex')
        sessions.set(token, { user: loginUser, expires: Date.now() + 7 * 24 * 60 * 60 * 1000 })
        return send(req,res,200,{ok:true},{'Set-Cookie':sessionCookie(req, token, 604800)})
      }
      if (path==='/admin/ui') {
        if (req.method==='GET'||req.method==='HEAD') return sendHtml(req,res,200,adminHtml)
        fail(405, '仅支持 GET')
      }
      if (path.startsWith('/admin')) {
        const now=Date.now(),ip=clientIp(req)
        for(const [key,bucket] of adminLimits)if(now-bucket.at>60000)adminLimits.delete(key)
        const bucket=adminLimits.get(ip)||{at:now,count:0}
        if(now-bucket.at>=60000){bucket.at=now;bucket.count=0}
        if(++bucket.count>120)fail(429,'管理请求过于频繁，请稍后重试')
        adminLimits.set(ip,bucket)
      }
      if (path === '/admin/ai-config') {
        authorized(req)
        if (req.method === 'GET') return send(req,res,200,publicAiConfig((await readAiConfig())||{}))
        if (req.method !== 'PUT') fail(405, '仅支持 GET 和 PUT')
        const input = await body(req)
        check(object(input), '请求内容必须为 JSON 对象')
        const current = await readAiConfig()
        const apiKey = string(input.api_key) && input.api_key.trim() ? input.api_key : current?.api_key
        if (!apiKey) fail(400, '请输入 API Key')
        let value
        try { value = validateAiConfig({ ...input, api_key: apiKey }) }
        catch (error) { fail(400, 'AI 配置无效：' + error.message) }
        await mkdir(dirname(aiConfigFile), { recursive: true })
        const temp = aiConfigFile + '.' + randomUUID() + '.tmp'
        await writeFile(temp, JSON.stringify(value, null, 2), { mode:0o600 })
        await rename(temp, aiConfigFile)
        return send(req,res,200,{ok:true,config:publicAiConfig(value)})
      }
      if (path.startsWith('/admin/')) {
        authorized(req)
        const key = path.slice(7)
        if (!allowed.has(key) || key.startsWith('config/')) fail(404, '此管理资源不存在或暂为只读')
        if (req.method === 'GET') {
          const value = await load(key)
          if (value === null) fail(404, '尚未发布内容')
          return send(req,res,200,value)
        }
        if (req.method !== 'PUT') fail(405, '仅支持 GET 和 PUT')
        const input = await body(req)
        const value = validate(key, input)
        const saved = await writeResource(key, value)
        let translation = null
        if (key === 'notice/zh-CN' && input.sync_english === true) {
          try {
            const translated = await translateNotice(value.summary, value.content)
            const english = validate('notice/en', { severity: value.severity, summary: translated.summary, content: translated.content })
            await writeResource('notice/en', english)
            translation = { ok: true, revision: english.revision, updatedAt: english.updatedAt }
          } catch (error) {
            translation = { ok: false, error: error.message }
            console.error('[admin] notice translation failed:', error.message)
          }
        }
        return send(req,res,200, translation ? { ...saved, translation } : saved)
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
  const server = await createService({dataDir:process.env.DATA_DIR || resolve(dirname(fileURLToPath(import.meta.url)),'data'),publicBase:process.env.PUBLIC_BASE_URL,adminToken:process.env.ADMIN_TOKEN,adminUser:process.env.ADMIN_USER,adminPassword:process.env.ADMIN_PASSWORD,seeds})
  server.listen(Number(process.env.PORT || 8080),process.env.HOST || '127.0.0.1',() => console.log('RiftFlow backend ready'))
  for (const signal of ['SIGTERM','SIGINT']) process.on(signal,() => server.close(() => process.exit(0)))
}
