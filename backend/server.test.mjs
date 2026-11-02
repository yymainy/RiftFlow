import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { once } from 'node:events'
import { createService } from './server.mjs'

test('publication authentication, persistence, languages and desktop-compatible API', async () => {
  const dataDir = await mkdtemp(join(tmpdir(),'riftflow-backend-'))
  const token = 'test-only-token-with-at-least-32-characters'
  const options = {dataDir,publicBase:'https://files.example.com/riftflow',adminToken:token,seeds:{'config/sgp/supported-queues':{updatedAt:'2026-10-29T06:00:00.000Z',queues:[420]},contacts:{channels:[],updatedAt:'2026-10-29T06:00:00.000Z'}}}
  let server
  async function start() {
    server = await createService(options)
    server.listen(0,'127.0.0.1'); await once(server,'listening')
    return `http://127.0.0.1:${server.address().port}`
  }
  try {
    let base = await start()
    const value = {severity:'medium',summary:'新版本公告',content:'## 修复\n公告正常显示'}
    const put = (data,authorization=token,path='/admin/notice/zh-CN') => fetch(base+path,{method:'PUT',headers:{authorization:'Bearer '+authorization,'content-type':'application/json'},body:JSON.stringify(data)})
    assert.equal((await put(value,'wrong')).status,401)
    assert.equal((await put({...value,severity:'critical'})).status,400)
    assert.equal((await put({...value,summary:''})).status,400)
    const published = await (await put(value)).json()
    assert.equal(published.language,'zh-CN'); assert.equal(published.contentType,'text/markdown')
    assert.ok(published.revision); assert.ok(published.updatedAt)
    const publicNotice = await (await fetch(base+'/notice/v1/latest?lang=zh-CN')).json()
    assert.deepEqual(publicNotice,published)
    assert.equal((await fetch(base+'/notice/v1/latest?lang=en')).status,404)
    assert.equal((await fetch(base+'/notice/v1/latest?lang=invalid')).status,400)
    assert.equal((await fetch(base+'/statistics/v1/records',{method:'POST'})).status,405)
    assert.deepEqual(await (await fetch(base+'/config/v1/sgp/supported-queues')).json(),options.seeds['config/sgp/supported-queues'])
    assert.equal((await fetch(base+'/config/v1/unknown')).status,404)
    assert.equal((await put(value,token,'/admin/../anything')).status,405)
    const bootstrap = await (await fetch(base+'/bootstrap.json')).json()
    assert.equal(bootstrap.riftflowBootstrap.baseUrls.api,options.publicBase)
    const second = await (await put(value)).json(); assert.notEqual(second.revision,published.revision)
    assert.equal((await put({channels:[{id:'bad',platform:'website',name:'x',identifier:'x',url:'javascript:alert(1)',password:null}]},token,'/admin/contacts')).status,400)
    await new Promise(resolve=>server.close(resolve))
    base = await start()
    assert.deepEqual(await (await fetch(base+'/notice/v1/latest')).json(),second)
  } finally {
    if (server?.listening) await new Promise(resolve=>server.close(resolve))
    await rm(dataDir,{recursive:true,force:true})
  }
})

test('admin UI and AI default configuration editing', async () => {
  const dataDir = await mkdtemp(join(tmpdir(),'riftflow-admin-'))
  const token = 'test-only-admin-token-with-at-least-32-chars'
  const server = await createService({dataDir,publicBase:'https://files.example.com/riftflow',adminToken:token})
  server.listen(0,'127.0.0.1'); await once(server,'listening')
  const base = `http://127.0.0.1:${server.address().port}`
  const auth = {authorization:'Bearer '+token}
  const jsonHeaders = {...auth,'content-type':'application/json'}
  const aiFile = join(dataDir,'ai-provider.json')
  const putAi = data => fetch(base+'/admin/ai-config',{method:'PUT',headers:jsonHeaders,body:JSON.stringify(data)})
  try {
    const page = await fetch(base+'/admin/ui')
    assert.equal(page.status,200)
    assert.match(page.headers.get('content-security-policy')||'',/frame-ancestors 'none'/)
    assert.match(await page.text(),/RiftFlow 管理/)
    assert.equal((await fetch(base+'/admin/ai-config')).status,401)
    assert.equal((await fetch(base+'/admin/ai-config',{headers:{authorization:'Bearer wrong-token'}})).status,401)
    assert.equal((await fetch(base+'/admin/release/zh-CN',{headers:auth})).status,404)
    const initial = await (await fetch(base+'/admin/ai-config',{headers:auth})).json()
    assert.equal(initial.api_key_set,false); assert.equal(initial.url,''); assert.equal(initial.api_key,undefined)
    const saved = await putAi({url:'http://127.0.0.1:8963/v1',model:'dfmodel',api_key:'secret-key',timeout_seconds:90,max_tokens:1800})
    assert.equal(saved.status,200)
    const savedBody = await saved.json()
    assert.equal(savedBody.config.api_key_set,true); assert.equal(savedBody.config.api_key,undefined)
    const stored = JSON.parse(await readFile(aiFile,'utf8'))
    assert.equal(stored.api_key,'secret-key'); assert.equal(stored.model,'dfmodel')
    const readBack = await (await fetch(base+'/admin/ai-config',{headers:auth})).json()
    assert.equal(readBack.api_key_set,true); assert.equal(readBack.api_key,undefined); assert.equal(readBack.model,'dfmodel')
    assert.equal((await putAi({url:'http://127.0.0.1:8963/v1',model:'next-model',api_key:''})).status,200)
    const kept = JSON.parse(await readFile(aiFile,'utf8'))
    assert.equal(kept.api_key,'secret-key'); assert.equal(kept.model,'next-model')
    assert.equal((await putAi({url:'ftp://bad',model:'x',api_key:'k'})).status,400)
    assert.equal((await putAi({url:'https://example.com/v1',model:'',api_key:'k'})).status,400)
    assert.equal((await fetch(base+'/admin/ui',{method:'POST'})).status,405)
  } finally {
    if (server.listening) await new Promise(resolve=>server.close(resolve))
    await rm(dataDir,{recursive:true,force:true})
  }
})

test('page login issues and clears an HttpOnly admin session', async () => {
  const dataDir = await mkdtemp(join(tmpdir(),'riftflow-login-'))
  const token = 'test-only-token-with-at-least-32-characters'
  const server = await createService({dataDir,publicBase:'https://files.example.com/riftflow',adminToken:token,adminUser:'admin',adminPassword:'test-password-123'})
  server.listen(0,'127.0.0.1'); await once(server,'listening')
  const base = `http://127.0.0.1:${server.address().port}`
  const login = (username,password) => fetch(base+'/admin/login',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({username,password})})
  try {
    assert.equal((await login('admin','wrong-password')).status,401)
    const response = await login('admin','test-password-123')
    assert.equal(response.status,200)
    const setCookie = response.headers.get('set-cookie')
    assert.match(setCookie,/rf_admin_session=/)
    assert.match(setCookie,/HttpOnly/)
    assert.match(setCookie,/SameSite=Strict/)
    const cookie = setCookie.split(';')[0]
    const session = await (await fetch(base+'/admin/session',{headers:{cookie}})).json()
    assert.equal(session.user,'admin')
    assert.equal((await fetch(base+'/admin/ai-config',{headers:{cookie}})).status,200)
    assert.equal((await fetch(base+'/admin/session',{headers:{authorization:'Bearer '+token}})).status,200)
    const logout = await fetch(base+'/admin/logout',{method:'POST',headers:{cookie}})
    assert.equal(logout.status,200)
    assert.match(logout.headers.get('set-cookie'),/Max-Age=0/)
    assert.equal((await fetch(base+'/admin/session',{headers:{cookie}})).status,401)
    assert.equal((await fetch(base+'/admin/ai-config')).status,401)
  } finally {
    if (server.listening) await new Promise(resolve=>server.close(resolve))
    await rm(dataDir,{recursive:true,force:true})
  }
})
