import fs from 'node:fs'
import path from 'node:path'
import assert from 'node:assert/strict'
import {createHash} from 'node:crypto'
import {spawn} from 'node:child_process'
import http from 'node:http'
import {fileURLToPath} from 'node:url'
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const version=JSON.parse(fs.readFileSync(path.join(root,'frontend/package.json'),'utf8')).version
const dir=path.join(root,'riftflow-rs/tmp/update-smoke-'+Date.now());fs.mkdirSync(dir,{recursive:true})
const release=path.join(root,'riftflow-rs/target/release/riftflow.exe')
const exe=path.join(dir,'Test-RiftFlow.exe');fs.copyFileSync(release,exe)
const delay=ms=>new Promise(r=>setTimeout(r,ms));const hash=b=>createHash('sha256').update(b).digest('hex')
const env=(port,profile)=>({...process.env,APPDATA:path.join(dir,profile),RIFTFLOW_AUTO_CONNECT:'0',WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`})
async function connect(port){
 let page;for(let i=0;i<150;i++){try{page=(await(await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(p=>p.url.includes('main-window'));if(page)break}catch{}await delay(200)}
 assert(page,'Window unavailable');const ws=new WebSocket(page.webSocketDebuggerUrl);await new Promise((r,j)=>{ws.onopen=r;ws.onerror=j})
 let id=0;const pending=new Map();ws.onmessage=({data})=>{const m=JSON.parse(data),p=pending.get(m.id);if(p){pending.delete(m.id);m.error?p.reject(Error('CDP failed')):p.resolve(m.result)}}
 const evaluate=async expression=>{const result=await new Promise((resolve,reject)=>{const n=++id;pending.set(n,{resolve,reject});ws.send(JSON.stringify({id:n,method:'Runtime.evaluate',params:{expression,returnByValue:true,awaitPromise:true}}));setTimeout(()=>{if(pending.delete(n))reject(Error('CDP timeout'))},30000).unref()});assert(!result.exceptionDetails,JSON.stringify(result.exceptionDetails));return result.result.value}
 const wait=async (expression,tries=150)=>{for(let i=0;i<tries;i++){if(await evaluate(expression))return;await delay(200)}throw Error('Wait failed: '+expression)}
 return {ws,evaluate,wait}
}
const child=spawn(exe,[],{windowsHide:true,env:env(8965,'profile'),stdio:'ignore'})
let ui
try{
 ui=await connect(8965)
 await ui.wait(`!!document.querySelector('#app')?.__vue_app__`)
 await ui.evaluate(`(()=>{const app=document.querySelector('#app').__vue_app__;const p=Reflect.ownKeys(app._context.provides).map(k=>app._context.provides[k]).find(v=>v?._s);window.updateTest=p._s.get('shard:self-update-renderer');p._s.get('shard:app-common-renderer').settings.locale='zh-CN'})()`)
 await ui.wait(`updateTest.releaseInfo !== null`)
 assert(await ui.evaluate('updateTest.isUpdateSupportedOnCurrentPlatform'),'Platform update support disabled')
 const current=await ui.evaluate('updateTest.releaseInfo.currentVersion')
 assert.equal(current,version)
 assert(await ui.evaluate('!updateTest.releaseInfo.isNew'),'Published older build must not cause a downgrade prompt')
 console.log('PASS: real startup automatically checks the release service; older online release is not offered as an update.')
 await ui.evaluate(`(()=>{window.fixture={version:'99.0.0',currentVersion:'${version}',publishedAt:new Date().toISOString(),description:'## 测试更新\\n\\n更新说明',isNew:true,isUpdateSupported:true,artifact:{downloadUrl:'https://example.invalid/RiftFlow.exe'}};updateTest.settings.ignoreVersion=null;updateTest.releaseInfo={...fixture}})()`)
 await ui.wait(`document.body.innerText.includes('99.0.0')`)
 await ui.wait(`!!document.querySelector('[aria-label="软件更新"]')`)
 const count=await ui.evaluate(`document.querySelectorAll('.n-notification').length`)
 await ui.evaluate(`updateTest.releaseInfo={...fixture}`);await delay(200)
 assert.equal(await ui.evaluate(`document.querySelectorAll('.n-notification').length`),count,'Same version notified twice')
 await ui.evaluate(`document.querySelector('[aria-label="软件更新"]').click()`)
 await ui.wait(`!!document.querySelector('.n-modal')`)
 assert((await ui.evaluate('document.body.innerText')).includes('99.0.0'))
 await ui.evaluate(`updateTest.updateProgressInfo={phase:'waiting-for-restart',downloadingProgress:1,averageDownloadSpeed:0,downloadTimeLeft:0,fileSize:123}`)
 await ui.wait(`document.body.innerText.includes('重启完成更新')`)
 await ui.evaluate(`updateTest.settings.ignoreVersion='100.0.0';updateTest.releaseInfo={...fixture,version:'100.0.0'};updateTest.updateProgressInfo=null`)
 await delay(200)
 assert(!(await ui.evaluate(`[...document.querySelectorAll('.n-notification')].map(n=>n.innerText).join(' ')`)).includes('100.0.0'),'Ignored version prompted')
 console.log('PASS: real UI shows update icon, notification, release dialog and ready/restart state; repeated and ignored versions do not create new prompts.')
} finally {ui?.ws.close();child.kill()}
await new Promise(r=>child.exitCode!==null?r():child.once('exit',r))
await delay(700)
const target=path.join(dir,'Replacement-RiftFlow.exe'),staged=path.join(dir,'new.exe'),worker=path.join(dir,'worker.exe')
fs.writeFileSync(target,'old-test-program');fs.copyFileSync(release,staged);fs.copyFileSync(release,worker)
const bytes=fs.readFileSync(staged)
const parent=spawn(process.execPath,['-e','setTimeout(()=>{},60000)'],{windowsHide:true,stdio:'ignore'})
const job=path.join(dir,'job.json');fs.writeFileSync(job,JSON.stringify({parent_pid:parent.pid,target,staged,sha256:hash(bytes),size:bytes.length}))
const helper=spawn(worker,['--riftflow-apply-update',job],{windowsHide:true,env:env(8966,'replacement-profile'),stdio:'ignore'})
let replacement
try{
 await delay(700);assert.equal(fs.readFileSync(target,'utf8'),'old-test-program','Worker replaced target before parent exited')
 parent.kill()
 replacement=await connect(8966)
 await replacement.wait(`typeof window.electron?.ipcRenderer?.invoke==='function'`)
 assert.equal(hash(fs.readFileSync(target)),hash(bytes))
 const backups=fs.readdirSync(dir).filter(n=>n.startsWith('Replacement-RiftFlow.backup-'))
 assert(backups.length===1);assert.equal(fs.readFileSync(path.join(dir,backups[0]),'utf8'),'old-test-program')
 console.log('PASS: shipped updater worker waits for parent exit, replaces only the isolated target, retains old backup, and launches the new executable.')
} finally {
 if(replacement){await Promise.race([replacement.evaluate(`void window.electron.ipcRenderer.invoke('riftflowCall','app-common-main','exit')`).catch(()=>{}),delay(1000)]);replacement.ws.close()}
 parent.kill();helper.kill()
}

// The release modal's own restart button must apply a staged update too. It exits
// through app-common, which bypasses Tauri's RunEvent::Exit; this test boots a
// marked copy against a local bootstrap that advertises a fake newer version and
// downloads the real latest EXE.
const live=await (await fetch('https://riftflow-api.yymainy.online/releases/v1/latest?lang=en')).json()
const artifact=live.artifacts.find(a=>a.platform==='win32'&&a.arch==='x64')
assert(artifact,'No win32/x64 artifact in the live release')
const modalExe=path.join(dir,'Modal-RiftFlow.exe')
fs.copyFileSync(release,modalExe);fs.appendFileSync(modalExe,'MODAL-MARKER')
const markedHash=hash(fs.readFileSync(modalExe))
const serverPort=await new Promise(r=>{const s=http.createServer();s.listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>r(p))})})
const server=http.createServer((req,res)=>{
 res.setHeader('Content-Type','application/json')
 if(req.url.startsWith('/npm.json')){res.end(JSON.stringify({riftflowBootstrap:{schemaVersion:1,generation:1,baseUrls:{api:`http://127.0.0.1:${serverPort}`,static:`http://127.0.0.1:${serverPort}/static`}}}));return}
 if(req.url.startsWith('/releases/v1/latest')){res.end(JSON.stringify({version:'99.0.0',publishedAt:new Date().toISOString(),description:'test update',artifacts:[artifact]}));return}
 if(req.url.startsWith('/website/v1/contact-channels')){res.end(JSON.stringify({channels:[]}));return}
 res.writeHead(404);res.end('{}')
})
await new Promise(r=>server.listen(serverPort,'127.0.0.1',r))
const modalEnv={...env(8967,'modal-profile'),RIFTFLOW_BOOTSTRAP_URL:`http://127.0.0.1:${serverPort}/npm.json`}
const modalChild=spawn(modalExe,[],{windowsHide:true,env:modalEnv,stdio:'ignore'})
let modalExited=false;modalChild.on('exit',()=>{modalExited=true})
let modalUi,modalUi2
try{
 modalUi=await connect(8967)
 await modalUi.wait(`!!document.querySelector('#app')?.__vue_app__`)
 await modalUi.evaluate(`(()=>{const app=document.querySelector('#app').__vue_app__;const p=Reflect.ownKeys(app._context.provides).map(k=>app._context.provides[k]).find(v=>v?._s);window.updateTest=p._s.get('shard:self-update-renderer');p._s.get('shard:app-common-renderer').settings.locale='zh-CN'})()`)
 await modalUi.evaluate(`window.electron.ipcRenderer.invoke('riftflowCall','self-update-main','checkUpdates')`)
 await modalUi.wait(`updateTest?.releaseInfo?.version === '99.0.0'`,300)
 await modalUi.evaluate(`document.querySelector('[aria-label="软件更新"]').click()`)
 await modalUi.wait(`!!document.querySelector('.n-modal')`)
 await modalUi.evaluate(`[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='立即更新').click()`)
 await modalUi.wait(`[...document.querySelectorAll('button')].some(b=>b.innerText.trim()==='重启完成更新')`,600)
 await modalUi.evaluate(`[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='重启完成更新').click()`).catch(()=>{})
 for(let i=0;i<200&&!modalExited;i++)await delay(100)
 assert(modalExited,'Modal restart did not exit the app')
 const backups=()=>fs.readdirSync(dir).filter(n=>n.startsWith('Modal-RiftFlow.backup-'))
 for(let i=0;i<300;i++){if(hash(fs.readFileSync(modalExe))===artifact.sha256&&backups().length===1)break;await delay(150)}
 assert.equal(hash(fs.readFileSync(modalExe)),artifact.sha256,'Modal restart did not replace the EXE')
 assert.equal(backups().length,1,'Modal restart did not keep the old EXE')
 assert.equal(hash(fs.readFileSync(path.join(dir,backups()[0]))),markedHash)
 for(let i=0;i<240&&!modalUi2;i++){try{modalUi2=await connect(8967)}catch{await delay(250)}}
 assert(modalUi2,'Modal restart did not relaunch the app')
 await modalUi2.wait(`!!document.querySelector('#app')?.__vue_app__`)
 await modalUi2.evaluate(`(()=>{const app=document.querySelector('#app').__vue_app__;const p=Reflect.ownKeys(app._context.provides).map(k=>app._context.provides[k]).find(v=>v?._s);window.updateTest=p._s.get('shard:self-update-renderer')})()`)
 await modalUi2.wait(`updateTest?.releaseInfo?.currentVersion === ${JSON.stringify(live.version)}`,300)
 console.log('PASS: the release modal restart applies the staged update, keeps the old EXE backup, and relaunches the released version.')
} finally {
 if(modalUi2){await Promise.race([modalUi2.evaluate(`void window.electron.ipcRenderer.invoke('riftflowCall','app-common-main','exit')`).catch(()=>{}),delay(1000)]);modalUi2.ws.close()}
 modalUi?.ws.close();if(!modalExited)modalChild.kill();server.close()
}
