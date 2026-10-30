use super::{controller::ArchiveController,store::now};
use riftflow_store::settings::SettingFactory;
use anyhow::{ensure,Context};
use serde_json::{json,Value};

impl ArchiveController {
    pub async fn batch(&self, settings: &SettingFactory, action:&str, input:&Value) -> anyhow::Result<Value> {
        ensure!(!matches!(self.phase().as_str(),"ChampSelect"|"ReadyCheck"|"InProgress"|"GameStart"),"选人或游戏中暂停批量修改");
        let undo=if action=="undo" {self.store.document("batch-undo")?} else {Value::Null};
        let kind=if action=="undo" {undo["kind"].as_str().unwrap_or("runes")} else {input["kind"].as_str().unwrap_or("runes")};
        ensure!(kind=="runes" || kind=="spells","未知配置类型");
        let setting_key=if kind=="spells" {"summonerSpells"} else {"runesV2"};
        let mut all=settings.get_setting("auto-champ-config-main",setting_key)?;
        ensure!(all.is_object(),"符文配置格式异常");
        if action=="undo" {
            let rows=undo["rows"].as_array().context("没有可撤销的批量操作")?;
            for row in rows {ensure!(all[ row["id"].as_str().unwrap() ]["aram"]==row["after"],"英雄配置在批量保存后又被修改，请手动检查，未覆盖新修改");}
            for row in rows {
                let champion=all.get_mut(row["id"].as_str().unwrap()).and_then(Value::as_object_mut).context("英雄配置已变化")?;
                if row["existed"]==true {champion.insert("aram".into(),row["before"].clone());} else {champion.remove("aram");}
            }
            settings.set_setting("auto-champ-config-main",setting_key,all).await?;
            self.store.put_document("batch-undo",&Value::Null)?;return Ok(json!({"count":rows.len()}));
        }
        let source=input["source"].as_str().context("请选择已有配置模板")?;
        let (champion,mode)=source.split_once(':').context("无效符文模板")?;
        if kind=="spells" {ensure!(mode=="aram","召唤师技能模板必须来自已保存的大乱斗方案");}
        let template=all[champion][mode].clone();
        if kind=="runes" {
            ensure!(template["selectedPerkIds"].as_array().map_or(false,|ids|ids.len()==9),"模板缺少完整符文，请先在自动操作中保存符文");
        } else {
            ensure!(template["spell1Id"].as_u64().filter(|v|*v>0).is_some() && template["spell2Id"].as_u64().filter(|v|*v>0).is_some() && template["spell1Id"]!=template["spell2Id"],"模板缺少两项不同的召唤师技能");
        }
        let targets=input["targets"].as_array().context("请选择目标英雄")?;
        ensure!(!targets.is_empty() && targets.len()<=200,"请选择 1 到 200 名英雄");
        let champions:Value=serde_json::from_str(include_str!("../../assets/champion-summary.json"))?;
        let mut rows=Vec::new();let mut seen=std::collections::BTreeSet::new();
        for target in targets {
            let id=target.as_u64().context("无效英雄 ID")?;
            ensure!(champions.as_array().unwrap().iter().any(|c|c["id"].as_u64()==Some(id)),"目标包含未知英雄");
            if !seen.insert(id) {continue;}
            let id=id.to_string();let current=&all[&id];
            rows.push(json!({"id":id,"existed":current.get("aram").is_some(),"before":current["aram"],"after":template}));
        }
        if action=="preview" {return Ok(json!({"rows":rows}));}
        ensure!(action=="apply","未知批量操作");
        ensure!(input["expected"]==json!(rows),"配置在预览后发生变化，请重新预览");
        self.store.put_document("batch-undo",&json!({"at":now(),"kind":kind,"rows":rows}))?;
        for row in &rows {
            let id=row["id"].as_str().unwrap();
            if !all[id].is_object() {all[id]=json!({});}
            all[id]["aram"]=template.clone();
        }
        settings.set_setting("auto-champ-config-main",setting_key,all).await?;
        Ok(json!({"count":rows.len()}))
    }
}
