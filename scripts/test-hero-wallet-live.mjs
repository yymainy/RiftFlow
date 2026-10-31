// Read-only live wallet smoke test. The proxy rejects every non-GET operation.
import fs from 'node:fs'
import path from 'node:path'
import https from 'node:https'
import assert from 'node:assert/strict'
import {spawn, spawnSync, execFileSync} from 'node:child_process'
import {fileURLToPath} from 'node:url'
import {WebSocketServer} from '../frontend/node_modules/ws/wrapper.mjs'
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const dir=path.join(root,'riftflow-rs/tmp/live-wallet-'+Date.now());fs.mkdirSync(dir,{recursive:true})
// Credentials stay in process memory, are never printed, and are not passed to the test app.
const credentials=JSON.parse(execFileSync('python',['-c',`import sqlite3,os,json,subprocess
c=sqlite3.connect('file:'+os.path.join(os.environ['APPDATA'],'riftflow','RiftFlow.db')+'?mode=ro',uri=True)
v=json.loads(c.execute("select value from Settings where key='league-client-main/lastConnectedClient'").fetchone()[0])
p=subprocess.run(['powershell','-NoProfile','-Command','Get-Process -Name LeagueClient -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id'],capture_output=True,text=True)
assert v['pid'] in {int(s) for s in p.stdout.split() if s.isdigit()},'Current connected client required'
print(json.dumps({'port':v['port'],'token':v['authToken']}))`],{encoding:'utf8'}))
const auth='Basic '+Buffer.from('riot:'+credentials.token).toString('base64')
function getActual(url){return new Promise((resolve,reject)=>{
 const req=https.get({hostname:'127.0.0.1',port:credentials.port,path:url,rejectUnauthorized:false,headers:{Authorization:auth}},res=>{const chunks=[];res.on('data',c=>chunks.push(c));res.on('end',()=>resolve({status:res.statusCode,data:Buffer.concat(chunks)}))});req.on('error',()=>reject(Error('Local client unavailable')));req.setTimeout(15000,()=>req.destroy())
})}
const original=await getActual('/lol-inventory/v1/wallet/lol_blue_essence');assert.equal(original.status,200)
const expected=JSON.parse(original.data).lol_blue_essence;assert(Number.isSafeInteger(expected)&&expected>=0,'Unexpected live wallet format')
const openssl=fs.existsSync('C:/Program Files/Git/usr/bin/openssl.exe')?'C:/Program Files/Git/usr/bin/openssl.exe':'openssl'
assert.equal(spawnSync(openssl,['req','-x509','-newkey','rsa:2048','-keyout',path.join(dir,'test.key'),'-out',path.join(dir,'test.pem'),'-days','1','-nodes','-subj','/CN=localhost'],{windowsHide:true,stdio:'ignore'}).status,0)
let blockedWrites=0,forwardedReads=0
const server=https.createServer({key:fs.readFileSync(path.join(dir,'test.key')),cert:fs.readFileSync(path.join(dir,'test.pem'))},async(req,res)=>{
 res.setHeader('Content-Type','application/json')
 if(req.method!=='GET'){blockedWrites++;res.writeHead(403);res.end('{}');return}
 const url=new URL(req.url,'https://localhost')
 if(url.pathname==='/riotclient/auth-token'){res.end('"read-only-fixture"');return}
 if(url.pathname==='/lol-gameflow/v1/gameflow-phase'){res.end('"None"');return}
 if(url.pathname==='/lol-summoner/v1/current-summoner' || /^\/lol-champions\/v1\/inventories\/\d+\/champions-minimal$/.test(url.pathname) || url.pathname==='/lol-store/v1/catalog' || url.pathname.startsWith('/lol-inventory/v1/wallet')){
  try{const r=await getActual(req.url);forwardedReads++;res.writeHead(r.status);res.end(r.data)}catch{res.writeHead(502);res.end('{}')};return
 }
 res.writeHead(404);res.end('{}')
})
const wss=new WebSocketServer({server});await new Promise(r=>server.listen(0,'127.0.0.1',r))
const child=spawn(path.join(root,'riftflow-rs/target/release/riftflow.exe'),[],{windowsHide:true,env:{...process.env,APPDATA:path.join(dir,'profile'),RIFTFLOW_AUTO_CONNECT:'0',WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=8967'},stdio:'ignore'})
const delay=ms=>new Promise(r=>setTimeout(r,ms));let ws
try{
 let page;for(let i=0;i<100;i++){try{page=(await(await fetch('http://127.0.0.1:8967/json/list')).json()).find(p=>p.url.includes('main-window'));if(page)break}catch{}await delay(200)}
 assert(page,'Test window unavailable');ws=new WebSocket(page.webSocketDebuggerUrl);await new Promise((r,j)=>{ws.onopen=r;ws.onerror=j})
 let id=0;const pending=new Map();ws.onmessage=({data})=>{const m=JSON.parse(data),p=pending.get(m.id);if(p){pending.delete(m.id);m.error?p.reject(Error('CDP error')):p.resolve(m.result)}}
 const evaluate=async expression=>{const result=await new Promise((resolve,reject)=>{const n=++id;pending.set(n,{resolve,reject});ws.send(JSON.stringify({id:n,method:'Runtime.evaluate',params:{expression,returnByValue:true,awaitPromise:true}}));setTimeout(()=>{if(pending.delete(n))reject(Error('timeout'))},45000).unref()});assert(!result.exceptionDetails,'Test evaluation failed');return result.result.value}
 const raw=async(ns,method,...args)=>evaluate(`window.electron.ipcRenderer.invoke('riftflowCall',${JSON.stringify(ns)},${JSON.stringify(method)},...${JSON.stringify(args)})`)
 for(let i=0;i<100;i++){if(await evaluate(`typeof window.electron?.ipcRenderer?.invoke==='function'`))break;await delay(100)}
 await raw('league-client-main','connect',{port:server.address().port,authToken:'read-only-fixture',pid:process.pid,force:true})
 let result;for(let i=0;i<60;i++){result=await raw('hero-shop-main','list');if(result.success)break;await delay(300)}
 assert(result?.success,'Live hero shop read failed');assert.equal(result.data.balance,expected);assert(result.data.heroes.length>0)
 assert(!result.data.warnings.some(w=>w.includes('余额读取失败')))
 await evaluate(`location.hash='/toolkit/hero-shop'`)
 let visible=false;for(let i=0;i<100;i++){visible=await evaluate(`document.body.innerText.includes(${JSON.stringify('蓝色精萃：'+expected.toLocaleString())})`);if(visible)break;await delay(150)}
 assert(visible,'Wallet balance not displayed in UI');assert.equal(blockedWrites,0)
 console.log('PASS: shipped EXE and real client GET responses agree on Blue Essence balance; hero list and wallet render in real UI; no non-GET operations were forwarded or requested.')
}finally{ws?.close();child.kill();for(const c of wss.clients)c.terminate();wss.close();server.close()}
