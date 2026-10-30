import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
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
