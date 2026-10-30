use super::{controller::ArchiveController, store::now};
use anyhow::{ensure, Context};
use serde_json::{json,Value};

impl ArchiveController {
    pub fn personal(&self, action: &str, input: &Value) -> anyhow::Result<Value> {
        let mut data=self.store.document("personal")?;
        if !data.is_object() {data=json!({"notes":[],"goal":null});}
        match action {
            "get" => return Ok(data),
            "deleteNote" => {
                data["notes"].as_array_mut().unwrap().retain(|n|n["id"]!=input["id"]);
            },
            "saveNote" => {
                let source=self.store.get(input["matchKey"].as_str().context("请选择来源对局")?)?;
                let fact=input["fact"].as_str().filter(|s|!s.trim().is_empty() && s.len()<=2000).context("请填写观察事实（最多 2000 字节）")?;
                let action=input["action"].as_str().filter(|s|!s.trim().is_empty() && s.len()<=2000).context("请填写下次行动")?;
                let opponent=input["opponent"].as_i64().filter(|id|*id>0).context("请选择对手英雄")?;
                let role=input["role"].as_str().unwrap_or("unknown");
                ensure!(["top","jungle","middle","bottom","utility","unknown"].contains(&role),"无效位置");
                let status=input["status"].as_str().unwrap_or("unreviewed");
                ensure!(["unreviewed","effective","ineffective","outdated"].contains(&status),"无效状态");
                let id=input["id"].as_str().filter(|s|!s.is_empty()).map(str::to_string).unwrap_or_else(||now().to_string());
                let note=json!({"id":id,"matchKey":source["key"],"gameId":source["gameId"],"owner":source["owner"],"region":source["region"],"championId":source["championId"],"championName":source["championName"],"gameMode":source["gameMode"],"version":source["version"],"opponent":opponent,"role":role,"fact":fact,"action":action,"status":status,"at":now()});
                let notes=data["notes"].as_array_mut().unwrap();
                if let Some(old)=notes.iter_mut().find(|n|n["id"]==id) {*old=note;} else {ensure!(notes.len()<2000,"记忆卡已达上限");notes.push(note);}
            },
            "saveGoal" => {
                let source=self.store.get(input["matchKey"].as_str().context("请选择来源对局")?)?;
                let metric=input["metric"].as_str().context("请选择目标")?;
                ensure!(["csPerMinute","csAt10","firstDeathMinute","deaths","controlWards"].contains(&metric),"未知目标");
                let target=input["target"].as_f64().filter(|n|n.is_finite() && *n>=0.0 && *n<=100.0).context("目标数值应在 0 到 100 之间")?;
                data["goal"]=json!({"matchKey":source["key"],"owner":source["owner"],"region":source["region"],"championId":source["championId"],"championName":source["championName"],"queueId":source["queueId"],"gameMode":source["gameMode"],"version":source["version"],"role":record_role(&source),"metric":metric,"target":target,"at":now(),"review":""});
            },
            "reviewGoal" => {ensure!(input["review"].as_str().map_or(false,|s|s.len()<=4000),"复核内容过长");data["goal"]["review"]=input["review"].clone();},
            "clearGoal" => data["goal"]=Value::Null,
            "progress" => {
                let goal=&data["goal"];
                if goal.is_null() {return Ok(json!({"before":[],"after":[]}));}
                let all=self.store.records()?;
                let metric=goal["metric"].as_str().unwrap();
                let mut before=Vec::new();let mut after=Vec::new();
                for record in &all {
                    if ["owner","region","championId","queueId","gameMode","version"].iter().any(|key|record[*key]!=goal[*key]) || record_role(record)!=goal["role"] {continue;}
                    let Some(value)=metric_value(record,metric) else {continue};
                    let entry=json!({"key":record["key"],"gameId":record["gameId"],"createdAt":record["createdAt"],"duration":record["duration"],"value":value,"met":if metric=="deaths" {value<=goal["target"].as_f64().unwrap()} else {value>=goal["target"].as_f64().unwrap()}});
                    if record["createdAt"].as_u64().unwrap_or(0)>goal["at"].as_u64().unwrap_or(u64::MAX) {after.push(entry);} else {before.push(entry);}
                }
                before.sort_by_key(|r|std::cmp::Reverse(r["createdAt"].as_u64().unwrap_or(0)));before.truncate(5);
                after.sort_by_key(|r|r["createdAt"].as_u64().unwrap_or(0));after.truncate(5);
                return Ok(json!({"goal":goal,"before":before,"after":after}));
            },
            _ => anyhow::bail!("未知个人工具操作"),
        }
        self.store.put_document("personal",&data)?;Ok(data)
    }
}
fn own_participant(record: &Value) -> Option<&Value> {
    let identities=record["game"]["participantIdentities"].as_array()?;
    let id=&identities.iter().find(|i|i["player"]["puuid"]==record["owner"])?["participantId"];
    record["game"]["participants"].as_array()?.iter().find(|p|p["participantId"]==*id)
}
fn record_role(record: &Value) -> Value {
    let Some(participant)=own_participant(record) else {return json!("UNKNOWN")};
    let lane=participant.pointer("/timeline/lane").and_then(Value::as_str).filter(|s|!s.is_empty()).unwrap_or("UNKNOWN");
    let role=participant.pointer("/timeline/role").and_then(Value::as_str).filter(|s|!s.is_empty()).unwrap_or("UNKNOWN");
    if lane=="UNKNOWN" && role=="UNKNOWN" {json!("UNKNOWN")} else {json!(format!("{lane}:{role}"))}
}
fn metric_value(record: &Value, metric: &str) -> Option<f64> {
    match metric {
        "csPerMinute" => {let duration=record["duration"].as_f64()?;if duration<300.0 {None} else {Some(record["cs"].as_f64()?*60.0/duration)}},
        "csAt10" => record["csAt10"].as_f64(),
        "firstDeathMinute" => record["firstDeathMinute"].as_f64(),
        "deaths" => record["deaths"].as_f64(),
        "controlWards" => own_participant(record)?["stats"]["visionWardsBoughtInGame"].as_f64(),
        _ => None,
    }
}
