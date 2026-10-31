import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createDecipheriv,createPublicKey,diffieHellman,generateKeyPairSync,hkdfSync } from 'node:crypto'
import { mkdtemp,writeFile,rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { once } from 'node:events'
import { createService } from './server.mjs'
test('AI config is encrypted per request, authenticated, private, and rate limited',async()=>{
 const dataDir=await mkdtemp(join(tmpdir(),'riftflow-ai-'))
 let server
 try{
  await writeFile(join(dataDir,'ai-provider.json'),JSON.stringify({url:'https://private-provider.example/v1',api_key:'secret-provider-key',model:'private-model'}))
  server=await createService({dataDir,publicBase:'https://example.com',adminToken:'test-admin-token-at-least-32-characters'})
  server.listen(0,'127.0.0.1');await once(server,'listening')
  const base=`http://127.0.0.1:${server.address().port}`
  assert.equal((await fetch(base+'/ai/v1/config')).status,405)
  assert.equal((await fetch(base+'/config/v1/ai')).status,404)
  assert.equal((await fetch(base+'/ai/v1/config',{method:'POST',headers:{'content-type':'application/json'},body:'{}'})).status,400)
  const pair=generateKeyPairSync('x25519'),clientPublicKey=pair.publicKey.export({format:'der',type:'spki'}).subarray(-32).toString('base64')
  const request={version:1,requestId:'ab'.repeat(16),clientPublicKey}
  const post=()=>fetch(base+'/ai/v1/config',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(request)})
  const response=await post(),envelope=await response.json()
  assert.equal(response.status,200);assert.equal(response.headers.get('cache-control'),'no-store')
  for(const secret of ['secret-provider-key','private-provider.example','private-model'])assert(!JSON.stringify(envelope).includes(secret))
  const peer=createPublicKey({key:Buffer.concat([Buffer.from('302a300506032b656e032100','hex'),Buffer.from(envelope.serverPublicKey,'base64')]),format:'der',type:'spki'})
  const shared=diffieHellman({privateKey:pair.privateKey,publicKey:peer})
  const key=hkdfSync('sha256',shared,Buffer.from(envelope.salt,'base64'),Buffer.from('riftflow-ai-config-v1'),32)
  const aad=Buffer.from(`riftflow-ai-config-v1\n${request.requestId}\n${envelope.issuedAt}\n${envelope.expiresAt}\n${clientPublicKey}\n${envelope.serverPublicKey}`)
  const decrypt=(ciphertext)=>{const cipher=createDecipheriv('aes-256-gcm',key,Buffer.from(envelope.iv,'base64'));cipher.setAAD(aad);cipher.setAuthTag(Buffer.from(envelope.tag,'base64'));return Buffer.concat([cipher.update(ciphertext),cipher.final()])}
  const ciphertext=Buffer.from(envelope.ciphertext,'base64')
  assert.equal(JSON.parse(decrypt(ciphertext)).api_key,'secret-provider-key')
  ciphertext[0]^=1;assert.throws(()=>decrypt(ciphertext))
  const second=await(await post()).json();assert.notEqual(second.serverPublicKey,envelope.serverPublicKey);assert.notEqual(second.iv,envelope.iv)
  let limited=false;for(let i=0;i<31;i++){if((await post()).status===429){limited=true;break}}assert(limited)
 }finally{if(server)await new Promise(r=>server.close(r));await rm(dataDir,{recursive:true,force:true})}
})
