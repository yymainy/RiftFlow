import { createCipheriv, createPublicKey, diffieHellman, generateKeyPairSync, hkdfSync, randomBytes } from 'node:crypto'
import { readFile } from 'node:fs/promises'

export const AI_ALGORITHM = 'X25519-HKDF-SHA256-AES256GCM'
const spkiPrefix = Buffer.from('302a300506032b656e032100','hex')
export function validateAiConfig(value) {
  if (!value || typeof value !== 'object') throw new Error('Invalid AI configuration')
  const {url,api_key,model}=value
  if (![url,api_key,model].every(v=>typeof v==='string'&&v.trim()&&v.length<=16000)) throw new Error('Invalid AI configuration')
  const address=new URL(url)
  if(!['https:','http:'].includes(address.protocol)||address.username||address.password||address.search||address.hash)throw new Error('Invalid AI URL')
  return {url:url.trim(),api_key:api_key.trim(),model:model.trim(),timeout_seconds:Math.min(180,Math.max(10,Number(value.timeout_seconds)||90)),max_tokens:Math.min(8192,Math.max(256,Number(value.max_tokens)||1800))}
}
export async function encryptedAiConfig(filename, request) {
  if(request?.version!==1||!/^[a-f0-9]{32}$/.test(request.requestId||'')||typeof request.clientPublicKey!=='string')throw Object.assign(new Error('Invalid encrypted configuration request'),{status:400})
  const raw=Buffer.from(request.clientPublicKey,'base64')
  if(raw.length!==32||raw.toString('base64')!==request.clientPublicKey)throw Object.assign(new Error('Invalid public key'),{status:400})
  let shared,pair
  try{
    pair=generateKeyPairSync('x25519')
    shared=diffieHellman({privateKey:pair.privateKey,publicKey:createPublicKey({key:Buffer.concat([spkiPrefix,raw]),format:'der',type:'spki'})})
  }catch{throw Object.assign(new Error('Invalid public key'),{status:400})}
  const config=validateAiConfig(JSON.parse(await readFile(filename,'utf8')))
  const serverPublicKey=pair.publicKey.export({format:'der',type:'spki'}).subarray(-32).toString('base64')
  const salt=randomBytes(16),iv=randomBytes(12),issuedAt=Date.now(),expiresAt=issuedAt+60000
  const aad=Buffer.from(`riftflow-ai-config-v1\n${request.requestId}\n${issuedAt}\n${expiresAt}\n${request.clientPublicKey}\n${serverPublicKey}`)
  const key=Buffer.from(hkdfSync('sha256',shared,salt,Buffer.from('riftflow-ai-config-v1'),32))
  const cipher=createCipheriv('aes-256-gcm',key,iv)
  cipher.setAAD(aad)
  const ciphertext=Buffer.concat([cipher.update(JSON.stringify(config),'utf8'),cipher.final()])
  const tag=cipher.getAuthTag()
  shared.fill(0);key.fill(0)
  return {version:1,algorithm:AI_ALGORITHM,requestId:request.requestId,issuedAt,expiresAt,serverPublicKey,salt:salt.toString('base64'),iv:iv.toString('base64'),ciphertext:ciphertext.toString('base64'),tag:tag.toString('base64')}
}
