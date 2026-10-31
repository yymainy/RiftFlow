import fs from 'node:fs'
import path from 'node:path'
import https from 'node:https'
import assert from 'node:assert/strict'
import {spawn, spawnSync} from 'node:child_process'
import {fileURLToPath} from 'node:url'
import {WebSocketServer} from '../frontend/node_modules/ws/wrapper.mjs'
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
process.chdir(path.join(root,'riftflow-rs'))
const dir=path.resolve('tmp/hero-shop-smoke-'+Date.now())
fs.mkdirSync(dir,{recursive:true})
const gitOpenSSL='C:/Program Files/Git/usr/bin/openssl.exe'
const openssl=fs.existsSync(gitOpenSSL)?gitOpenSSL:'openssl'
const cert=spawnSync(openssl,['req','-x509','-newkey','rsa:2048','-keyout',path.join(dir,'test.key'),'-out',path.join(dir,'test.pem'),'-days','1','-nodes','-subj','/CN=localhost'],{windowsHide:true,stdio:'ignore'})
assert.equal(cert.status,0,'OpenSSL is required to create the isolated test certificate')
const heroes=[{id:1,name:'周免测试英雄',freeToPlay:true,ownership:{owned:false}},{id:2,name:'普通测试英雄',freeToPlay:false,ownership:{owned:false}},{id:3,name:'已拥有测试英雄',freeToPlay:true,ownership:{owned:true}}]
let balance=1000,account='test-account',price=450,fail=false,postDelay=0,storeUnavailable=false,walletMode='modern',inventoryUnavailable=false
const posts=[]
const server=https.createServer({key:fs.readFileSync(path.join(dir,'test.key')),cert:fs.readFileSync(path.join(dir,'test.pem'))},async(req,res)=>{
 const url=new URL(req.url,'https://localhost');let data='';for await(const chunk of req)data+=chunk
 res.setHeader('Content-Type','application/json')
 const send=x=>res.end(JSON.stringify(x))
 if(url.pathname==='/riotclient/auth-token')return send('test')
 if(url.pathname==='/lol-summoner/v1/current-summoner')return send({summonerId:123,puuid:account,gameName:'接口测试',tagLine:'TEST',summonerLevel:30})
 if(url.pathname==='/lol-champions/v1/inventories/123/champions-minimal'){if(inventoryUnavailable){res.statusCode=503;return send({errorCode:'TEST_INVENTORY_UNAVAILABLE'})}return send(heroes)}
 if(url.pathname==='/lol-store/v1/catalog' && (storeUnavailable || url.searchParams.has('inventoryType'))){res.statusCode=400;return send({errorCode:'TEST_QUERY_UNSUPPORTED'})}
 if(url.pathname==='/lol-store/v1/catalog')return send(heroes.map(h=>({itemId:h.id,inventoryType:'CHAMPION',active:true,prices:[{currency:'IP',cost:price}]})))
 if(url.pathname.startsWith('/lol-inventory/v1/wallet') || url.pathname==='/lol-login/v1/wallet'){
  const requested=url.pathname.endsWith('/lol_blue_essence') || (url.searchParams.get('currencyTypes')||'').includes('lol_blue_essence')
  if(walletMode==='modern' || walletMode==='modern-string')return send(requested?{lol_blue_essence:walletMode==='modern-string'?String(balance):balance,RP:99999}:{})
  if(walletMode==='unavailable' || (walletMode==='legacy' && url.pathname!=='/lol-inventory/v1/wallet/IP')){res.statusCode=503;return send({errorCode:'TEST_WALLET_UNAVAILABLE'})}
  return send(walletMode==='lower' || walletMode==='legacy'?{ip:balance,rp:99999}:{IP:balance})
 }
 if(url.pathname==='/lol-purchase-widget/v2/purchaseItems'){
  const body=JSON.parse(data);posts.push(body);if(postDelay)await new Promise(r=>setTimeout(r,postDelay));if(fail){res.statusCode=500;return send({errorCode:'TEST'})}
  const item=body.items[0];assert.equal(item.purchaseCurrencyInfo.currencyType,'IP');assert.equal(item.quantity,1);assert.equal(item.purchaseCurrencyInfo.price,price)
  const hero=heroes.find(h=>h.id===item.itemKey.itemId);assert(hero&&!hero.ownership.owned);balance-=price;hero.ownership.owned=true;return send({})
 }
 if(url.pathname==='/lol-gameflow/v1/gameflow-phase')return send('None')
 if(url.pathname==='/lol-login/v1/session')return send({state:'SUCCEEDED',puuid:account})
 res.statusCode=404;send({errorCode:'TEST_NOT_FOUND'})
})
const wss=new WebSocketServer({server})
await new Promise(r=>server.listen(0,'127.0.0.1',r))
const child=spawn(path.resolve('target/release/riftflow.exe'),[],{windowsHide:true,env:{...process.env,APPDATA:path.join(dir,'profile-'+Date.now()),RIFTFLOW_AUTO_CONNECT:'0',WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=8963'},stdio:'ignore'})
const delay=ms=>new Promise(r=>setTimeout(r,ms));let ws
try {
 let page;for(let i=0;i<100;i++){try{page=(await(await fetch('http://127.0.0.1:8963/json/list')).json()).find(p=>p.url.includes('main-window'));if(page)break}catch{}await delay(200)}
 assert(page,'App debugger unavailable');ws=new WebSocket(page.webSocketDebuggerUrl);await new Promise((r,j)=>{ws.onopen=r;ws.onerror=j})
 let id=0;const pending=new Map();ws.onmessage=({data})=>{const m=JSON.parse(data),p=pending.get(m.id);if(p){pending.delete(m.id);m.error?p.reject(Error('CDP error')):p.resolve(m.result)}}
 const evaluate=async expression=>{const result=await new Promise((resolve,reject)=>{const n=++id;pending.set(n,{resolve,reject});ws.send(JSON.stringify({id:n,method:'Runtime.evaluate',params:{expression,returnByValue:true,awaitPromise:true}}));setTimeout(()=>{if(pending.delete(n))reject(Error('timeout'))},30000).unref()});assert(!result.exceptionDetails,JSON.stringify(result.exceptionDetails));return result.result.value}
 const wait=async expression=>{for(let i=0;i<100;i++){if(await evaluate(expression))return;await delay(150)}throw Error('UI wait: '+expression)}
 const raw=async(ns,method,...args)=>evaluate(`window.electron.ipcRenderer.invoke('riftflowCall',${JSON.stringify(ns)},${JSON.stringify(method)},...${JSON.stringify(args)})`)
 const call=async(method,...args)=>{const r=await raw('hero-shop-main',method,...args);assert(r.success,JSON.stringify(r));return r.data}
 const click=async text=>{const ok=await evaluate(`(()=>{const e=[...document.querySelectorAll('button')].find(e=>e.innerText.trim()===${JSON.stringify(text)});if(!e)return false;e.click();return true})()`);assert(ok,'button missing: '+text);await delay(200)}
 await wait(`typeof window.electron?.ipcRenderer?.invoke==='function'`)
 await raw('league-client-main','connect',{port:server.address().port,authToken:'test-fixture',pid:process.pid,force:true})
 for(let i=0;i<60;i++){const r=await raw('hero-shop-main','list');if(r.success)break;await delay(250)}
 let list=await call('list');assert.equal(list.balance,1000);assert.equal(list.heroes.filter(h=>h.freeToPlay).length,2)
 await evaluate(`location.hash='/toolkit/hero-shop'`);await wait(`document.body.innerText.includes('全选可购英雄')`);await click('刷新');await wait(`document.body.innerText.includes('普通测试英雄')`)
 await click('全选可购英雄');await wait(`document.body.innerText.includes('已勾选 1 位当前周免英雄')`)
 await click('一键排除周免');assert(!(await evaluate('document.body.innerText')).includes('已勾选 1 位当前周免英雄'))
 await click('核对并购买');await wait(`document.body.innerText.includes('确认使用蓝色精萃购买')`)
 assert((await evaluate('document.body.innerText')).includes('购买 1 位英雄'))
 await click('取消');assert.equal(posts.length,0)
 await click('全选可购英雄');await click('核对并购买');await wait(`document.body.innerText.includes('包含当前周免英雄')`)
 await click('确认使用蓝色精萃购买');await wait(`document.body.innerText.includes('已确认购买 2 位')`)
 assert.equal(posts.length,2);assert.equal(balance,100)
 console.log('PASS: isolated native/LCU fixture and real UI: weekly listing, exclude weekly, selection warning, modal warning, cancel sends no POST, sequential purchase verifies ownership.')
 // Native gates: insufficient balance, stale price/account, and no POST retry on server failure.
 heroes[0].ownership.owned=false;heroes[1].ownership.owned=false;balance=1000
 let quote=await call('prepare',{ids:[1]});price=451
 assert(!(await raw('hero-shop-main','buy',{id:quote.id,heroId:1})).success);assert.equal(posts.length,2)
 price=450;quote=await call('prepare',{ids:[1]});account='changed-account'
 assert(!(await raw('hero-shop-main','buy',{id:quote.id,heroId:1})).success);assert.equal(posts.length,2)
 account='test-account';balance=1;assert(!(await raw('hero-shop-main','prepare',{ids:[1]})).success)
 balance=1000;quote=await call('prepare',{ids:[1,2]});fail=true
 assert(!(await raw('hero-shop-main','buy',{id:quote.id,heroId:1})).success);assert.equal(posts.length,3)
 assert(!(await raw('hero-shop-main','buy',{id:quote.id,heroId:2})).success);assert.equal(posts.length,3)
 fail=false;postDelay=1200;balance=1000;heroes[0].ownership.owned=false;heroes[1].ownership.owned=false
 await click('刷新');await wait(`document.body.innerText.includes('普通测试英雄')`);await click('全选可购英雄');await click('核对并购买');await wait(`document.body.innerText.includes('确认使用蓝色精萃购买')`)
 const beforeStop=posts.length
 await click('确认使用蓝色精萃购买');await wait(`document.body.innerText.includes('停止后续购买')`);await click('停止后续购买')
 await wait(`document.body.innerText.includes('已停止后续购买，已完成的购买保留。')`)
 assert.equal(posts.length,beforeStop+1,'Stop must prevent the second purchase')
 assert.equal(balance,550)
 console.log('PASS: stopping a pending transaction preserves that purchase and never submits the next hero.')
 console.log('PASS: changed price/account and low balance block spending; POST failure is not retried and invalidates remaining quote. No real account purchase performed.')
 fail=false;postDelay=0;balance=1000;heroes[0].ownership.owned=false;heroes[1].ownership.owned=false
 const beforeReadTests=posts.length
 walletMode='modern';assert.equal((await call('list')).balance,1000)
 walletMode='modern-string';assert.equal((await call('list')).balance,1000)
 walletMode='lower';assert.equal((await call('list')).balance,1000)
 walletMode='legacy';assert.equal((await call('list')).balance,1000)
 walletMode='unavailable';let partial=await call('list');assert.equal(partial.balance,null);assert.equal(partial.heroes.length,3);assert(partial.warnings.length);assert(!(await raw('hero-shop-main','prepare',{ids:[1]})).success)
 await click('刷新');await wait(`document.body.innerText.includes('蓝色精萃余额读取失败')`)
 await click('全选可购英雄');assert(await evaluate(`[...document.querySelectorAll('button')].find(b=>b.innerText.trim()==='核对并购买').disabled`))
 walletMode='modern';storeUnavailable=true;partial=await call('list');assert.equal(partial.heroes.length,3);assert.equal(partial.heroes.filter(h=>h.freeToPlay).length,2);assert(partial.heroes.every(h=>h.price===null));assert(partial.warnings.some(w=>w.includes('HTTP 400')))
 await click('刷新');await wait(`document.body.innerText.includes('英雄价格读取失败') && document.body.innerText.includes('当前周免（2 位）')`)
 storeUnavailable=false;inventoryUnavailable=true
 await click('刷新');await wait(`document.body.innerText.includes('HTTP 503')`)
 assert(!(await evaluate('document.body.innerText')).includes('[object Object]'))
 assert.equal(posts.length,beforeReadTests)
 console.log('PASS: no vector catalog query, lowercase/legacy wallet compatibility, missing prices/balance preserve rotation and block purchases, IPC object errors show endpoint and HTTP status. Read-only failures send no purchase POST.')

} finally {ws?.close();child.kill();for(const c of wss.clients)c.terminate();wss.close();server.close()}
