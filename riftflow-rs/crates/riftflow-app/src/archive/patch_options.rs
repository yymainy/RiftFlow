use anyhow::{ensure, Context};
use serde_json::{json, Value};
use super::{controller::ArchiveController, store::now};

pub(super) fn published_versions(html: &str) -> Vec<String> {
    let mut found = std::collections::BTreeSet::new();
    for tail in html.split("league-of-legends-patch-").skip(1) {
        let Some((version, _)) = tail.split_once("-notes") else { continue };
        let Some((year, patch)) = version.split_once('-') else { continue };
        let (Ok(year), Ok(patch)) = (year.parse::<u16>(), patch.parse::<u16>()) else { continue };
        if (26..=99).contains(&year) && (1..=30).contains(&patch) { found.insert((year, patch)); }
    }
    found.into_iter().rev().take(30).map(|(year, patch)| format!("{year}.{patch}")).collect()
}

async fn fetch(client: &reqwest::Client, url: &str) -> anyhow::Result<String> {
    let response = client.get(url).send().await?.error_for_status()?;
    ensure!(response.content_length().unwrap_or(0) <= 8 * 1024 * 1024, "官方资料响应过大");
    let body = response.text().await?;
    ensure!(body.len() <= 8 * 1024 * 1024, "官方资料响应过大");
    Ok(body)
}

fn item_options(zh: &Value, en: &Value) -> Vec<Value> {
    let mut items = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    if let Some(all) = zh["data"].as_object() {
        for (id, item) in all {
            if item["inStore"] == false || item["gold"]["purchasable"] == false { continue; }
            let Some(name) = item["name"].as_str().filter(|name| !name.is_empty()) else { continue };
            let Some(english) = en["data"][id]["name"].as_str().filter(|name| !name.is_empty()) else { continue };
            if id.parse::<u32>().is_err() { continue; }
            if !names.insert(english.to_lowercase()) { continue; }
            items.push(json!({"value":id,"label":name,"english":english}));
        }
    }
    items.sort_by(|a,b| a["label"].as_str().cmp(&b["label"].as_str()));
    items
}

impl ArchiveController {
    pub(super) async fn patch_options(&self, force: bool, region: &str) -> anyhow::Result<Value> {
        ensure!(region=="cn" || region=="international","请选择国服或国际服");
        let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build()?;
        let paused = matches!(self.phase().as_str(), "InProgress" | "ChampSelect" | "ReadyCheck" | "GameStart");
        let catalog_key=if region=="cn" {"patch-catalog-cn"} else {"patch-catalog"};
        let mut catalog = self.store.document(catalog_key)?;
        let mut items = self.store.document("patch-items")?;
        let mut warnings = Vec::new();
        if paused { warnings.push("正在选人或对局，暂不联网刷新；有缓存时继续使用。".to_string()); }
        if !paused && (force || now().saturating_sub(catalog["at"].as_u64().unwrap_or(0)) > 6*60*60*1000) {
            let fetched = async {
                if region=="cn" {return super::patch_china::fetch_catalog(&client).await;}
                let html = fetch(&client, "https://www.leagueoflegends.com/en-us/news/game-updates/").await?;
                let versions = published_versions(&html);
                ensure!(!versions.is_empty(), "官方公告目录暂时无法识别");
                Ok::<_,anyhow::Error>(json!({"at":now(),"versions":versions}))
            }.await;
            match fetched { Ok(value) => { self.store.put_document(catalog_key, &value)?; catalog=value; }, Err(_) => warnings.push(format!("暂时无法获取{}公告目录，请检查网络后刷新；仅显示该来源已有缓存，不会切换成其他服区的公告。",if region=="cn" {"国服"} else {"国际服"})) }
        }
        if !paused && (force || now().saturating_sub(items["at"].as_u64().unwrap_or(0)) > 24*60*60*1000) {
            let fetched = async {
                let versions: Value = serde_json::from_str(&fetch(&client, "https://ddragon.leagueoflegends.com/api/versions.json").await?)?;
                let version = versions[0].as_str().filter(|v| v.bytes().all(|b| b.is_ascii_digit() || b == b'.')).context("装备资料版本不可用")?;
                let zh: Value = serde_json::from_str(&fetch(&client, &format!("https://ddragon.leagueoflegends.com/cdn/{version}/data/zh_CN/item.json")).await?)?;
                let en: Value = serde_json::from_str(&fetch(&client, &format!("https://ddragon.leagueoflegends.com/cdn/{version}/data/en_US/item.json")).await?)?;
                let options = item_options(&zh, &en);
                ensure!(!options.is_empty(), "装备资料暂时不可用");
                Ok::<_,anyhow::Error>(json!({"at":now(),"items":options,"version":version}))
            }.await;
            match fetched { Ok(value) => { self.store.put_document("patch-items", &value)?; items=value; }, Err(_) => warnings.push("暂时无法加载中文装备目录；仍可先选择英雄生成简报，或稍后刷新。".into()) }
        }
        Ok(json!({"region":region,"versions":catalog["versions"].as_array().cloned().unwrap_or_default(),"items":items["items"].as_array().cloned().unwrap_or_default(),"at":catalog["at"],"warnings":warnings,"itemVersion":items["version"]}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn uses_only_published_links_and_numeric_version_order() {
        assert_eq!(published_versions("/league-of-legends-patch-26-9-notes/ /league-of-legends-patch-26-21-notes/ /league-of-legends-patch-26-21-notes/ /league-of-legends-patch-bogus-notes/"), vec!["26.21", "26.9"]);
    }
    #[test] fn maps_chinese_items_to_the_same_english_item_id() {
        let zh=json!({"data":{"3031":{"name":"无尽之刃"},"1":{"name":"测试","inStore":false}}});
        let en=json!({"data":{"3031":{"name":"Infinity Edge"},"1":{"name":"Test"}}});
        let rows=item_options(&zh,&en);
        assert_eq!(rows.len(),1);
        assert_eq!(rows[0]["english"],"Infinity Edge");
        assert_eq!(rows[0]["label"],"无尽之刃");
    }
}
