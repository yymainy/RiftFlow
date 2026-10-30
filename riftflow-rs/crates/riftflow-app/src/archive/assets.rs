//! Read-only skin collection from the signed-in local League client.
use super::{controller::ArchiveController, store::now};
use anyhow::{ensure, Context};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

fn entries(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(items) => items.values().collect(),
        _ => Vec::new(),
    }
}

fn skin_id(skin: &Value) -> Option<u64> {
    skin["id"].as_u64().or_else(|| skin["skinId"].as_u64())
}

fn owned(skin: &Value) -> bool {
    skin["ownership"]["owned"] == true || skin["ownership"] == true || skin["owned"] == true
}

fn build_catalog(catalog: &Value, inventory: &Value) -> anyhow::Result<Value> {
    let all: Vec<(Option<u64>, &Value)> = match catalog {
        Value::Array(items) => items.iter().map(|skin| (None,skin)).collect(),
        Value::Object(items) => items.iter().map(|(key,skin)| (key.parse().ok(),skin)).collect(),
        _ => Vec::new(),
    };
    let account = entries(inventory);
    ensure!(!all.is_empty(), "客户端皮肤目录为空或格式已变化");
    ensure!(inventory.is_array() || inventory.is_object(), "账号皮肤清单格式已变化");
    let ownership: HashMap<u64, bool> = account.iter().filter_map(|skin| skin_id(skin).map(|id| (id, owned(skin)))).collect();
    let champions: Value = serde_json::from_str(include_str!("../../assets/champion-summary.json"))?;
    let names: HashMap<u64, &str> = champions.as_array().unwrap().iter()
        .filter_map(|champion| Some((champion["id"].as_u64()?, champion["name"].as_str()?))).collect();
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for (key, skin) in all {
        let Some(id) = skin_id(skin).or(key) else { continue };
        let champion_id = skin["championId"].as_u64().unwrap_or(id / 1000);
        if id >= 60_000_000 || id % 1000 == 0 || skin["isBase"] == true || skin["skinClassification"] == "kBase" || !names.contains_key(&champion_id) { continue; }
        let name = skin["name"].as_str().unwrap_or("").trim();
        if name.is_empty() { continue; }
        if !seen.insert((champion_id, name.to_lowercase())) { continue; }
        rows.push(json!({"id":id,"championId":champion_id,"championName":names[&champion_id],"name":name,
            "owned":ownership.get(&id).copied().unwrap_or(false)}));
    }
    ensure!(!rows.is_empty(), "客户端皮肤目录没有可识别的皮肤");
    rows.sort_by(|a,b| a["championId"].as_u64().cmp(&b["championId"].as_u64()).then_with(|| a["id"].as_u64().cmp(&b["id"].as_u64())));
    let owned_count = rows.iter().filter(|row| row["owned"] == true).count();
    Ok(json!({"total":rows.len(),"owned":owned_count,"missing":rows.len()-owned_count,"skins":rows}))
}

impl ArchiveController {
    pub async fn assets(&self, action: &str) -> anyhow::Result<Value> {
        if action == "cached" { return self.store.document("skin-assets"); }
        ensure!(action == "refresh", "未知资产操作");
        let http = self.client.http().context("请先启动并登录英雄联盟客户端")?;
        let auth = self.client.state.auth_public().context("尚未连接客户端")?;
        let region = format!("{}:{}", auth.region, auth.rso_platform_id);
        let summoner = super::controller::archive_get(&http,"/lol-summoner/v1/current-summoner","读取当前账号").await?;
        let summoner_id = summoner["summonerId"].as_u64().context("当前账号尚未加载完成")?;
        let owner = summoner["puuid"].as_str().filter(|s| !s.is_empty()).context("当前账号尚未登录")?;
        let catalog = super::controller::archive_get(&http,"/lol-game-data/assets/v1/skins.json","读取皮肤目录").await?;
        let inventory = super::controller::archive_get(&http,&format!("/lol-champions/v1/inventories/{summoner_id}/skins-minimal"),"读取账号皮肤清单").await?;
        let mut result = build_catalog(&catalog,&inventory)?;
        result["owner"] = json!(owner);
        result["region"] = json!(region);
        result["playerName"] = json!(format!("{}#{}",summoner["gameName"].as_str().unwrap_or(""),summoner["tagLine"].as_str().unwrap_or("")));
        result["updatedAt"] = json!(now());
        self.store.put_document("skin-assets",&result)?;
        Ok(result)
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn ownership_excludes_base_and_rentals() {
        let catalog=json!([{"id":1000,"championId":1,"name":"Base"},{"id":1001,"championId":1,"name":"A"},{"id":1002,"championId":1,"name":"B"}]);
        let inventory=json!([{"id":1001,"ownership":{"owned":true}},{"id":1002,"ownership":{"owned":false,"rental":{"rented":true}}}]);
        let result=build_catalog(&catalog,&inventory).unwrap();
        assert_eq!(result["total"],2);assert_eq!(result["owned"],1);assert_eq!(result["missing"],1);
    }
    #[test] fn keyed_catalog_and_boolean_ownership_are_supported() {
        let catalog=json!({"1001":{"championId":1,"name":"A"},"1002":{"championId":1,"name":"B"}});
        let inventory=json!([{"id":1001,"ownership":true}]);
        let result=build_catalog(&catalog,&inventory).unwrap();
        assert_eq!(result["total"],2);assert_eq!(result["owned"],1);
    }
}
