//! Blue-essence-only purchases. Quotes bind account, connection and prices; POSTs are never retried.
use std::{collections::{HashMap, HashSet}, sync::{Arc, atomic::{AtomicU64, Ordering}}, time::Instant};
use anyhow::{ensure, Context};
use riftflow_ipc::{handler, IpcError, RiftFlowIpcMain};
use riftflow_lcu::{LeagueClientMain, http::{LcuHttp, RequestConfig}};
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all="camelCase")]
struct Hero { id:u64, name:String, owned:bool, free_to_play:bool, price:Option<u64> }
struct Snapshot { account:String, balance:Option<u64>, heroes:Vec<Hero>, warnings:Vec<String> }
struct Quote { renderer:u64, account:String, http:Arc<LcuHttp>, created:Instant, heroes:HashMap<u64,Hero> }
impl Quote {
    fn consume(&mut self, renderer:u64, id:u64, expected_id:u64, hero_id:u64, http:&Arc<LcuHttp>)->anyhow::Result<Hero> {
        ensure!(self.renderer==renderer && id==expected_id && self.created.elapsed().as_secs()<300 && Arc::ptr_eq(&self.http,http),"购买确认已过期或连接已变化，请刷新");
        self.heroes.remove(&hero_id).context("此英雄已提交或未确认")
    }
}
struct Shop { lc:Arc<LeagueClientMain>, quote:Mutex<Option<Quote>>, next:AtomicU64, quote_id:AtomicU64 }

async fn request(http:&LcuHttp, method:&str, path:&str, data:Option<Value>) -> anyhow::Result<Value> {
    let r=http.request(RequestConfig{method:method.into(),url:path.into(),data,..Default::default()}).await.map_err(|error|{
        let text=error.to_string();
        if let Some(status)=text.strip_prefix("LCU request failed with status ").and_then(|s|s.parse::<u16>().ok()) {
            anyhow::anyhow!("{} {} 返回 HTTP {}，请打开客户端商店后刷新",method,path,status)
        }else{anyhow::anyhow!("{} {} 连接失败，请确认客户端仍在线",method,path)}
    })?;
    ensure!((200..300).contains(&r.status), "{} {} 返回 HTTP {}，请打开客户端商店后刷新",method,path,r.status);
    ensure!(r.body.get("errorCode").is_none(), "{} {} 返回客户端错误，请打开客户端商店后刷新",method,path);
    Ok(r.body)
}
fn price(item:&Value)->Option<u64> {
    // Use the client-provided cost, including a sale quote. The purchase request carries
    // this exact amount so an expired discount is rejected rather than silently repriced.
    let base=item["prices"].as_array()?.iter().find(|p|p["currency"]=="IP")?["cost"].as_u64()?;
    Some(item.pointer("/sale/prices").and_then(Value::as_array).and_then(|v|v.iter().find(|p|p["currency"]=="IP")).and_then(|p|p["cost"].as_u64()).map_or(base,|p|p.min(base)))
}
fn catalog(inventory:&Value, store:&Value)->anyhow::Result<Vec<Hero>> {
    let inv=inventory.as_array().context("英雄清单格式变化，请更新软件")?;
    let items=store.as_array().context("英雄商店格式变化，请更新软件")?;
    ensure!(!inv.is_empty(),"英雄清单为空，请等待客户端加载完成");
    let mut seen=HashSet::new();
    let mut heroes=Vec::new();
    for h in inv {
        let Some(id)=h["id"].as_u64().filter(|id|*id>0) else {continue};
        ensure!(seen.insert(id),"英雄清单包含重复项目");
        let free_to_play=h["freeToPlay"].as_bool().context("客户端未提供完整周免状态，已停止购买")?;
        let owned=h.pointer("/ownership/owned").and_then(Value::as_bool).context("客户端未提供英雄拥有状态")?;
        let item=items.iter().find(|p|p["inventoryType"]=="CHAMPION" && p["itemId"].as_u64()==Some(id) && p["active"]==true);
        heroes.push(Hero{id,name:h["name"].as_str().unwrap_or("未知英雄").into(),owned,free_to_play,price:item.and_then(price)});
    }
    heroes.sort_by_key(|h|(h.owned,h.price.unwrap_or(u64::MAX),h.id));
    Ok(heroes)
}
fn currency_amount(value:&Value)->Option<u64> {
    const MAX_SAFE:u64=9_007_199_254_740_991;
    let amount=value.as_u64().or_else(||value.as_str().filter(|s|!s.is_empty() && s.len()<=16 && s.bytes().all(|b|b.is_ascii_digit())).and_then(|s|s.parse().ok()))
        .or_else(||value.as_f64().filter(|n|n.is_finite() && *n>=0.0 && n.fract()==0.0 && *n<=MAX_SAFE as f64).map(|n|n as u64))?;
    (amount<=MAX_SAFE).then_some(amount)
}
fn wallet_balance(value:&Value)->Option<u64> {
    // Inventory uses lol_blue_essence; store prices/purchase payloads still use IP.
    // Prefer the current inventory key over legacy aliases. Never pick the first currency value.
    for path in ["/lol_blue_essence","/balances/lol_blue_essence","/data/balances/lol_blue_essence","/data/lol_blue_essence","/IP","/ip","/balances/IP","/data/balances/IP"] {
        if let Some(amount)=value.pointer(path).and_then(currency_amount){return Some(amount)}
    }
    currency_amount(value)
}
fn wallet_shape(value:&Value)->String {
    match value {
        Value::Object(items) if items.is_empty()=>"空对象".into(),
        Value::Object(items)=>items.iter().take(8).map(|(key,value)|{
            let kind=match value{Value::Null=>"null",Value::Bool(_)=>"bool",Value::Number(_)=>"number",Value::String(_)=>"string",Value::Array(_)=>"array",Value::Object(_)=>"object"};
            format!("{}:{}",key.chars().take(48).collect::<String>(),kind)
        }).collect::<Vec<_>>().join(", "),
        Value::Array(items)=>format!("array({})",items.len()),
        Value::Null=>"null".into(),
        _=>"非余额值".into(),
    }
}
async fn balance(http:&LcuHttp)->anyhow::Result<u64> {
    let mut failures=Vec::new();
    for path in ["/lol-inventory/v1/wallet/lol_blue_essence","/lol-inventory/v1/wallet?currencyTypes=%5B%22lol_blue_essence%22%5D","/lol-inventory/v1/wallet/IP","/lol-inventory/v1/wallet?currencyTypes=%5B%22IP%22%5D"] {
        match request(http,"GET",path,None).await {
            Ok(value)=>if let Some(amount)=wallet_balance(&value){return Ok(amount)}else{failures.push(format!("{} 未提供余额（{}）",path,wallet_shape(&value)));},
            Err(error)=>failures.push(error.to_string()),
        }
    }
    anyhow::bail!("蓝色精萃余额读取失败：{}",failures.join("；"))
}

async fn snapshot(http:&LcuHttp)->anyhow::Result<Snapshot> {
    let me=request(http,"GET","/lol-summoner/v1/current-summoner",None).await?;
    let id=me["summonerId"].as_u64().context("请先登录客户端")?;
    let account=me["puuid"].as_str().filter(|s|!s.is_empty()).context("账号标识未就绪")?.to_owned();
    let inventory=request(http,"GET",&format!("/lol-champions/v1/inventories/{id}/champions-minimal"),None).await?;
    let mut warnings=Vec::new();
    // No vector query parameter: client builds disagree on its serialization.
    // Filter CHAMPION items locally, leaving ownership/rotation available even if the store is down.
    let store=match request(http,"GET","/lol-store/v1/catalog",None).await {
        Ok(value) if value.is_array()=>value,
        Ok(_)=>{warnings.push("英雄商店价格格式无法识别，暂时禁止购买".into());json!([])},
        Err(error)=>{warnings.push(format!("英雄价格读取失败：{}",error));json!([])},
    };
    let balance=match balance(http).await {
        Ok(amount)=>Some(amount),
        Err(error)=>{warnings.push(error.to_string());None},
    };
    let heroes=catalog(&inventory,&store)?;
    if heroes.iter().any(|h|!h.owned) && heroes.iter().all(|h|h.owned || h.price.is_none()) && warnings.is_empty(){
        warnings.push("客户端商店尚未提供可用的蓝色精萃价格，请打开客户端商店后刷新。周免和拥有状态仍可查看。".into());
    }
    let current=request(http,"GET","/lol-summoner/v1/current-summoner",None).await?;
    ensure!(current["puuid"].as_str()==Some(&account),"账号已切换，请刷新");
    Ok(Snapshot{account,balance,heroes,warnings})
}

impl Shop {
    async fn call(&self,renderer:u64,method:&str,arg:&Value)->anyhow::Result<Value> {
        // The same lock covers quote preparation and each full transaction.
        let mut guard=self.quote.try_lock().context("购买操作进行中，请稍候")?;
        let http=self.lc.http().context("请先连接英雄联盟客户端（客户端以管理员运行时，软件也需相同权限）")?;
        if method=="list" {
            *guard=None;
            let s=snapshot(&http).await?;
            return Ok(json!({"balance":s.balance,"heroes":s.heroes,"warnings":s.warnings}));
        }
        if method=="prepare" {
            *guard=None;
            let ids=arg["ids"].as_array().context("请选择英雄")?;
            ensure!(!ids.is_empty() && ids.len()<=300,"请选择 1–300 个英雄");
            let s=snapshot(&http).await?;
            let mut selected=HashMap::new();let mut total=0u64;
            for id in ids {
                let id=id.as_u64().context("英雄编号无效")?;
                ensure!(!selected.contains_key(&id),"英雄选择重复");
                let hero=s.heroes.iter().find(|h|h.id==id && !h.owned).context("英雄已拥有或不可购买，请刷新")?.clone();
                total=total.checked_add(hero.price.context("所选英雄暂不支持蓝色精萃购买")?).context("总价无效")?;
                selected.insert(id,hero);
            }
            let balance=s.balance.context("未能确认蓝色精萃余额，已阻止购买。请打开客户端商店后刷新")?;
            ensure!(total<=balance,"蓝色精萃不足：需要 {}，当前 {}",total,balance);
            let id=self.next.fetch_add(1,Ordering::Relaxed)+1;
            self.quote_id.store(id,Ordering::Relaxed);
            let result=json!({"id":id,"total":total,"balance":s.balance,"heroes":selected.values().collect::<Vec<_>>()});
            *guard=Some(Quote{renderer,account:s.account,http,created:Instant::now(),heroes:selected});
            return Ok(result);
        }
        ensure!(method=="buy","未知操作");
        let id=arg["id"].as_u64().context("确认已失效")?;
        let hero_id=arg["heroId"].as_u64().context("英雄无效")?;
        let q=guard.as_mut().context("请重新确认购买")?;
        
        // Consume before any network call: errors and unknown outcomes cannot be retried.
        let hero=q.consume(renderer,id,self.quote_id.load(Ordering::Relaxed),hero_id,&http)?;
        let result=async {
            let s=snapshot(&http).await?;
            ensure!(s.account==q.account,"账号已切换，请重新确认");
            let now=s.heroes.iter().find(|h|h.id==hero_id).context("英雄不可购买")?;
            ensure!(now==&hero,"英雄价格、拥有状态或周免状态变化，请重新确认");
            let cost=hero.price.context("价格无效")?;
            ensure!(s.balance.context("未能确认蓝色精萃余额，已停止购买")?>=cost,"蓝色精萃不足，已停止后续购买");
            request(&http,"POST","/lol-purchase-widget/v2/purchaseItems",Some(purchase_body(hero_id,cost))).await.context("购买结果尚未确认，请先查看客户端英雄资产，勿立即重复购买")?;
            // A successful HTTP status alone is not a fulfilled purchase.
            for _ in 0..5 {
                tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                let me=request(&http,"GET","/lol-summoner/v1/current-summoner",None).await?;
                ensure!(me["puuid"].as_str()==Some(&q.account),"账号已切换，请查看客户端核实购买结果");
                let inv=request(&http,"GET",&format!("/lol-champions/v1/inventories/{}/champions-minimal",me["summonerId"].as_u64().context("账号未就绪")?),None).await?;
                if inv.as_array().is_some_and(|v|v.iter().any(|h|h["id"].as_u64()==Some(hero_id) && h["ownership"]["owned"]==true)) {
                    return Ok(json!({"name":hero.name,"price":cost}));
                }
            }
            anyhow::bail!("客户端尚未确认到账，已停止后续购买。请先查看英雄资产，勿立即重复购买")
        }.await;
        if result.is_err(){*guard=None;}
        result
    }
}
fn purchase_body(id:u64,cost:u64)->Value {json!({"items":[{"itemKey":{"inventoryType":"CHAMPION","itemId":id},"purchaseCurrencyInfo":{"currencyType":"IP","price":cost,"purchasable":true},"quantity":1,"source":"champion"}]})}
pub fn register(ipc:&Arc<RiftFlowIpcMain>,lc:&Arc<LeagueClientMain>)->anyhow::Result<()> {
    let shop=Arc::new(Shop{lc:lc.clone(),quote:Mutex::new(None),next:AtomicU64::new(0),quote_id:AtomicU64::new(0)});
    for method in ["list","prepare","buy"] {
        let shop=shop.clone();
        ipc.on_call("hero-shop-main",method,handler(move |ctx,args|{let shop=shop.clone();async move {
            shop.call(ctx.renderer,method,args.first().unwrap_or(&Value::Null)).await.map_err(IpcError::plain)
        }}))?;
    }
    Ok(())
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn separates_owned_and_weekly_free(){
        let inv=json!([{"id":1,"name":"A","ownership":{"owned":false},"freeToPlay":true},{"id":2,"name":"B","ownership":{"owned":true},"freeToPlay":false}]);
        let store=json!([{"itemId":1,"inventoryType":"CHAMPION","active":true,"prices":[{"currency":"RP","cost":100},{"currency":"IP","cost":450}]}]);
        let rows=catalog(&inv,&store).unwrap();assert!(!rows[0].owned);assert!(rows[0].free_to_play);assert_eq!(rows[0].price,Some(450));assert!(rows[1].owned);
        assert!(catalog(&json!([{"id":1,"ownership":{"owned":false}}]),&store).is_err());
    }
    #[test] fn quotes_are_bound_and_single_use() {
        let http=Arc::new(LcuHttp::new(1,String::new()));
        let other=Arc::new(LcuHttp::new(1,String::new()));
        let hero=Hero{id:1,name:"A".into(),owned:false,free_to_play:true,price:Some(450)};
        let mut q=Quote{renderer:7,account:"test".into(),http:http.clone(),created:Instant::now(),heroes:HashMap::from([(1,hero)])};
        assert!(q.consume(8,1,1,1,&http).is_err());
        assert!(q.consume(7,2,1,1,&http).is_err());
        assert!(q.consume(7,1,1,1,&other).is_err());
        q.created=Instant::now()-std::time::Duration::from_secs(301);
        assert!(q.consume(7,1,1,1,&http).is_err());
        q.created=Instant::now();
        assert!(q.consume(7,1,1,1,&http).is_ok());
        assert!(q.consume(7,1,1,1,&http).is_err());
    }
    #[test] fn wallet_fields_never_confuse_rp_with_blue_essence(){
        assert_eq!(wallet_balance(&json!({"lol_blue_essence":12,"IP":0,"RP":99})),Some(12));
        assert_eq!(wallet_balance(&json!({"lol_blue_essence":"1200"})),Some(1200));
        assert_eq!(wallet_balance(&json!({"lol_blue_essence":1200.0})),Some(1200));
        assert_eq!(wallet_balance(&json!({"data":{"balances":{"lol_blue_essence":7}}})),Some(7));
        assert_eq!(wallet_balance(&json!({"lol_blue_essence":"12.5"})),None);
        assert_eq!(wallet_balance(&json!({"lol_blue_essence":12.5})),None);
        assert_eq!(wallet_balance(&json!({"lol_blue_essence":9007199254740992u64})),None);
        assert_eq!(wallet_balance(&json!({"lol_blue_essence":null,"RP":99})),None);
        assert_eq!(wallet_balance(&json!({})),None);
        assert_eq!(wallet_balance(&json!({"IP":12,"RP":99})),Some(12));
        assert_eq!(wallet_balance(&json!({"ip":0,"rp":99})),Some(0));
        assert_eq!(wallet_balance(&json!({"data":{"balances":{"IP":7}}})),Some(7));
        assert_eq!(wallet_balance(&json!({"RP":99})),None);
        assert_eq!(wallet_balance(&json!({"IP":-1})),None);
    }
    #[test] fn unavailable_store_preserves_rotation_and_ownership(){
        let heroes=catalog(&json!([{"id":1,"name":"test","freeToPlay":true,"ownership":{"owned":false}}]),&json!([])).unwrap();
        assert!(heroes[0].free_to_play);assert!(!heroes[0].owned);assert_eq!(heroes[0].price,None);
    }
    #[test] fn only_exact_blue_essence_price(){
        let body=purchase_body(22,450);assert_eq!(body["items"][0]["purchaseCurrencyInfo"]["currencyType"],"IP");assert_eq!(body["items"][0]["quantity"],1);
        assert_eq!(price(&json!({"prices":[{"currency":"RP","cost":1}]})),None);
        assert_eq!(price(&json!({"prices":[{"currency":"IP","cost":450}],"sale":{"prices":[{"currency":"IP","cost":225}]}})),Some(225));
    }
}
