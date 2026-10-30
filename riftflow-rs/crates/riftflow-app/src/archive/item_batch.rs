use std::{fs, path::{Path, PathBuf}};

use anyhow::{ensure, Context};
use serde_json::{json, Value};

use super::{controller::{archive_get, ArchiveController}, store::now};

const PREFIX: &str = "riftflow-batch-";

fn valid_template(value: &Value) -> bool {
    value["blocks"].as_array().map_or(false, |blocks| {
        !blocks.is_empty() && blocks.len() <= 20 && blocks.iter().all(|block| {
            block["items"].as_array().map_or(false, |items| {
                !items.is_empty() && items.len() <= 100 && items.iter().all(|item| {
                    item["id"].as_str().and_then(|id| id.parse::<u32>().ok()).is_some_and(|id| id > 0)
                })
            })
        })
    })
}

fn read_template(path: &Path) -> anyhow::Result<Value> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 128 * 1024, "装备方案文件格式或大小不受支持");
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(valid_template(&value), "装备方案缺少有效装备分组");
    Ok(value)
}

fn targets(input: &Value) -> anyhow::Result<Vec<u64>> {
    let list = input["targets"].as_array().context("请选择目标英雄")?;
    ensure!(!list.is_empty() && list.len() <= 30, "一次请选择 1 到 30 名英雄");
    let champions: Value = serde_json::from_str(include_str!("../../assets/champion-summary.json"))?;
    let known = champions.as_array().context("英雄目录异常")?;
    let mut ids = std::collections::BTreeSet::new();
    for value in list {
        let id = value.as_u64().context("无效英雄 ID")?;
        ensure!(known.iter().any(|champion| champion["id"].as_u64() == Some(id)), "目标包含未知英雄");
        ids.insert(id);
    }
    Ok(ids.into_iter().collect())
}

fn source_path(directory: &Path, input: &Value) -> anyhow::Result<PathBuf> {
    let name = input["source"].as_str().context("请选择装备方案模板")?;
    ensure!(name.ends_with(".json") && name.len() <= 100 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) && !name.starts_with('.'), "无效模板文件名");
    let path = directory.join(name);
    ensure!(path.is_file(), "装备方案模板不存在");
    Ok(path)
}

impl ArchiveController {
    pub async fn item_batch(&self, action: &str, input: &Value) -> anyhow::Result<Value> {
        ensure!(!matches!(self.phase().as_str(), "ChampSelect" | "ReadyCheck" | "InProgress" | "GameStart"), "选人或游戏中暂停修改装备方案");
        let http = self.client.http().context("请先连接并登录英雄联盟客户端")?;
        let auth = self.client.state.auth_public().context("请先连接客户端")?;
        let install = archive_get(&http, "/data-store/v1/install-dir", "读取游戏目录").await?;
        let install = install.as_str().context("客户端没有提供安装目录")?.trim_matches('"');
        let directory = riftflow_lcu::http::item_set_target_dir(install, &auth.region);
        if action == "list" {
            let mut options = Vec::new();
            if directory.is_dir() {
                for entry in fs::read_dir(&directory)? {
                    let entry = entry?;
                    if !entry.file_name().to_string_lossy().ends_with(".json") { continue; }
                    if let Ok(value) = read_template(&entry.path()) {
                        options.push(json!({"value":entry.file_name().to_string_lossy(),"label":value["title"].as_str().unwrap_or("未命名装备方案")}));
                    }
                }
            }
            options.sort_by(|a, b| a["label"].as_str().cmp(&b["label"].as_str()));
            return Ok(json!({"templates":options}));
        }
        if action == "undo" {
            let undo = self.store.document("item-batch-undo")?;
            ensure!(undo["directory"].as_str() == Some(&directory.to_string_lossy()), "装备方案目录已变化，未执行撤销");
            let files = undo["files"].as_array().context("没有可撤销的装备方案批量操作")?;
            for file in files {
                let name = file["name"].as_str().context("撤销记录损坏")?;
                ensure!(name.starts_with(PREFIX) && name.ends_with(".json") && !name.contains(['/', '\\']), "撤销记录文件名无效");
                ensure!(fs::read_to_string(directory.join(name))? == file["content"].as_str().context("撤销记录损坏")?, "装备方案已被修改，未执行撤销");
            }
            for file in files { fs::remove_file(directory.join(file["name"].as_str().unwrap()))?; }
            self.store.put_document("item-batch-undo", &Value::Null)?;
            return Ok(json!({"count":files.len()}));
        }
        ensure!(action == "preview" || action == "apply", "未知装备方案操作");
        let source = source_path(&directory, input)?;
        let template = read_template(&source)?;
        let ids = targets(input)?;
        let title = template["title"].as_str().unwrap_or("装备方案");
        let champions: Value = serde_json::from_str(include_str!("../../assets/champion-summary.json"))?;
        let label = |id: u64| champions.as_array().unwrap().iter()
            .find(|champion| champion["id"].as_u64() == Some(id))
            .and_then(|champion| champion["name"].as_str()).unwrap_or("未知英雄");
        let rows: Vec<Value> = ids.iter().map(|id| json!({"id":id,"title":format!("{title} · {}",label(*id)),"blocks":template["blocks"].as_array().unwrap().len()})).collect();
        if action == "preview" { return Ok(json!({"rows":rows})); }
        ensure!(input["expected"] == json!(rows), "模板或目标在预览后发生变化，请重新预览");
        fs::create_dir_all(&directory)?;
        let mut written = Vec::<Value>::new();
        let stamp = now();
        for (id, row) in ids.into_iter().zip(&rows) {
            let uid = format!("{PREFIX}{stamp}-{id}");
            let name = format!("{uid}.json");
            let mut value = template.clone();
            value["uid"] = json!(uid);
            value["title"] = row["title"].clone();
            value["type"] = json!("global");
            value["map"] = json!("any");
            value["mode"] = json!("any");
            value["associatedChampions"] = json!([id]);
            value["associatedMaps"] = json!([12]);
            let content = serde_json::to_string(&value)?;
            let path = directory.join(&name);
            let file = fs::OpenOptions::new().write(true).create_new(true).open(&path);
            let write_result = match file {
                Ok(mut file) => {
                    use std::io::Write;
                    let result = file.write_all(content.as_bytes()).and_then(|_| file.sync_all());
                    if result.is_err() { let _ = fs::remove_file(&path); }
                    result
                }
                Err(error) => Err(error),
            };
            if let Err(error) = write_result {
                for file in &written { let _ = fs::remove_file(directory.join(file["name"].as_str().unwrap())); }
                return Err(error.into());
            }
            written.push(json!({"name":name,"content":content}));
        }
        if let Err(error) = self.store.put_document("item-batch-undo", &json!({"directory":directory,"files":written})) {
            for file in &written { let _ = fs::remove_file(directory.join(file["name"].as_str().unwrap())); }
            return Err(error);
        }
        Ok(json!({"count":written.len()}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_champions_and_invalid_item_templates() {
        assert!(targets(&json!({"targets":[999999]})).is_err());
        assert!(!valid_template(&json!({"blocks":[{"items":[{"id":"not-an-item"}]}]})));
        assert!(valid_template(&json!({"blocks":[{"items":[{"id":"1001"}]}]})));
    }
}
