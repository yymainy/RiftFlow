//! Account/install-scoped settings snapshots. Restore only explicitly selected existing fields.
use super::controller::ArchiveController;
use super::store::now;
use anyhow::{ensure, Context};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

fn replace_file(path: &std::path::Path, contents: &str) -> anyhow::Result<()> {
    ensure!(!std::fs::metadata(path)?.permissions().readonly(),"配置文件为只读，请先解除设置文件锁定");
    let temporary=path.with_extension(format!("riftflow-{}-{}.tmp",std::process::id(),now()));
    let result=(|| -> anyhow::Result<()> {
        use std::io::Write;
        let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(contents.as_bytes())?;file.sync_all()?;drop(file);
        #[cfg(windows)] {
            use std::os::windows::ffi::OsStrExt;
            #[link(name="kernel32")] unsafe extern "system" {fn MoveFileExW(old:*const u16,new:*const u16,flags:u32)->i32;}
            let old=temporary.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
            let new=path.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
            if unsafe {MoveFileExW(old.as_ptr(),new.as_ptr(),9)}==0 {return Err(std::io::Error::last_os_error().into());}
        }
        #[cfg(not(windows))] std::fs::rename(&temporary,path)?;
        Ok(())
    })();
    if temporary.exists() {let _=std::fs::remove_file(temporary);}
    result
}

fn fields(text: &str, file: &str) -> anyhow::Result<Map<String, Value>> {
    let mut output = Map::new();
    if file.ends_with(".json") {
        fn visit(value: &Value, path: &str, output: &mut Map<String, Value>) {
            match value {
                Value::Object(map) => for (key, value) in map { visit(value, &format!("{path}/{}", key.replace('~',"~0").replace('/',"~1")), output); },
                Value::Array(items) => for (i,value) in items.iter().enumerate() { visit(value, &format!("{path}/{i}"), output); },
                _ => { output.insert(path.into(),value.clone()); }
            }
        }
        visit(&serde_json::from_str(text.trim_start_matches('\u{feff}'))?, "", &mut output);
    } else {
        let mut section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') { section = line.into(); }
            else if !line.starts_with([';','#']) {
                if let Some((key,value)) = line.split_once('=') { output.insert(format!("{section}/{}",key.trim()),json!(value.trim())); }
            }
        }
    }
    Ok(output)
}

fn merge(current: &str, file: &str, changes: &Map<String,Value>) -> anyhow::Result<String> {
    if file.ends_with(".json") {
        let mut document: Value = serde_json::from_str(current.trim_start_matches('\u{feff}'))?;
        for (path,value) in changes {
            let target = document.pointer_mut(path).context("字段结构已变化，请重新预览")?;
            ensure!(std::mem::discriminant(target)==std::mem::discriminant(value), "字段类型已变化，请重新检查");
            *target=value.clone();
        }
        return Ok(serde_json::to_string_pretty(&document)?);
    }
    let mut section=String::new();
    let ending=if current.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines=Vec::new();
    for line in current.lines() {
        let trimmed=line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') { section=trimmed.into(); }
        if let Some((key,_))=line.split_once('=') {
            if let Some(value)=changes.get(&format!("{section}/{}",key.trim())) {
                lines.push(format!("{key}={}",value.as_str().context("无效的 INI 值")?));continue;
            }
        }
        lines.push(line.into());
    }
    Ok(lines.join(ending)+if current.ends_with('\n') {ending} else {""})
}

impl ArchiveController {
    async fn vault_scope(&self) -> anyhow::Result<(String,PathBuf)> {
        ensure!(!matches!(self.phase().as_str(),"InProgress"|"GameStart"|"Reconnect"|"ChampSelect"|"ReadyCheck"),"选人或对局中暂停操作游戏配置");
        ensure!(riftflow_native::pids_by_name(riftflow_native::GAME_CLIENT_PROCESS_NAME).is_empty(),"请先退出正在运行的游戏进程");
        let http=self.client.http().context("请先连接并登录客户端")?;
        let install=http.get("/data-store/v1/install-dir").await?;
        let base=PathBuf::from(install.as_str().filter(|s| !s.is_empty()).context("客户端未提供安装目录")?);
        let dir=if self.client.state.region()=="TENCENT" {base.parent().context("无效安装目录")?.join("Game/Config")} else {base.join("Config")}.canonicalize()?;
        let summoner=http.get("/lol-summoner/v1/current-summoner").await?;
        let owner=summoner["puuid"].as_str().filter(|s| !s.is_empty()).context("等待账号登录")?;
        Ok((serde_json::to_string(&(self.client.state.region(),owner,dir.to_string_lossy()))?,dir))
    }

    pub async fn vault(&self, action: &str, input: &Value) -> anyhow::Result<Value> {
        let _serial=self.vault_lock.lock().await;
        let mut snapshots=self.store.document("vault")?.as_array().cloned().unwrap_or_default();
        if action=="list" { return Ok(json!(snapshots.iter().rev().map(|v|json!({"id":v["id"],"at":v["at"],"label":v["label"],"scope":v["scope"]})).collect::<Vec<_>>())); }
        if action=="delete" {snapshots.retain(|s|s["id"]!=input["id"]);self.store.put_document("vault",&json!(snapshots))?;return Ok(json!({"deleted":true}));}
        let (scope,dir)=self.vault_scope().await?;
        let read = || -> anyhow::Result<Map<String,Value>> {
            let mut files=Map::new();
            for file in ["game.cfg","PersistedSettings.json"] {
                let path=dir.join(file);
                if path.exists() {
                    ensure!(path.canonicalize()?.parent()==Some(dir.as_path()),"配置路径不属于当前安装目录");
                    ensure!(std::fs::metadata(&path)?.len()<=4*1024*1024,"配置文件过大");
                    files.insert(file.into(),json!(std::fs::read_to_string(path)?));
                }
            }
            ensure!(!files.is_empty(),"未找到游戏配置文件");Ok(files)
        };
        let current=read()?;
        if action=="snapshot" || action=="autoSnapshot" {
            if action=="autoSnapshot" && snapshots.iter().rev().find(|s|s["scope"]==scope).map_or(false,|s|s["files"]==json!(current)) {return Ok(json!({"unchanged":true}));}
            let id=now().to_string();
            snapshots.push(json!({"id":id,"at":now(),"label":input["label"].as_str().unwrap_or("手动快照"),"scope":scope,"files":current}));
            ensure!(snapshots.len()<=200,"快照已达 200 份，请先备份并清理");
            self.store.put_document("vault",&json!(snapshots))?;return Ok(json!({"id":id}));
        }
        let selected=snapshots.iter().find(|s|s["id"]==input["id"]).context("快照不存在")?;
        ensure!(selected["scope"]==scope,"此快照属于其他账号或安装目录，请切换到原账号和目录");
        let mut differences=Vec::new();
        for (file,text) in &current {
            let Some(old)=selected["files"][file].as_str() else {continue};
            let old=fields(old,file)?;let present=fields(text.as_str().unwrap(),file)?;
            if file.ends_with(".json") {
                for (path,value) in &old {
                    if path.to_lowercase().ends_with("/name") {
                        ensure!(present.get(path)==Some(value),"配置分组或字段顺序已变化，无法安全对应旧快照，请新建快照");
                    }
                }
            }
            for (path,value) in old {
                if let Some(new)=present.get(&path) {if *new!=value {differences.push(json!({"key":serde_json::to_string(&(file,&path))?,"file":file,"path":path,"before":new,"after":value}));}}
            }
        }
        if action=="preview" {return Ok(json!({"changes":differences,"expected":current}));}
        ensure!(action=="restore","未知保险箱操作");
        ensure!(input["expected"]==json!(current),"配置在预览后发生变化，请重新预览");
        let keys=input["keys"].as_array().context("请选择恢复字段")?;
        ensure!(!keys.is_empty(),"请选择恢复字段");
        // Prepare and validate every replacement before writing either file.
        let mut pending=Vec::new();
        for (file,text) in &current {
            let changes=differences.iter().filter(|d|d["file"]==file.as_str() && keys.contains(&d["key"]))
                .map(|d|(d["path"].as_str().unwrap().to_string(),d["after"].clone())).collect::<Map<_,_>>();
            if !changes.is_empty() {pending.push((file.clone(),merge(text.as_str().unwrap(),file,&changes)?));}
        }
        ensure!(!pending.is_empty(),"没有可恢复的字段");
        ensure!(snapshots.len()<200,"快照空间已满，无法保存恢复前备份");
        snapshots.push(json!({"id":now().to_string(),"at":now(),"label":"恢复前自动备份","scope":scope,"files":current}));
        self.store.put_document("vault",&json!(snapshots))?;
        let mut written:Vec<String>=Vec::new();
        for (file,contents) in &pending {
            // Recheck immediately before each write; read-only locks return an explicit error.
            self.vault_scope().await?;
            ensure!(std::fs::read_to_string(dir.join(file))?==current[file].as_str().unwrap(),"配置已变化，恢复中止，备份已保留");
            if let Err(error)=replace_file(&dir.join(file),contents) {
                for previous in written {let _=replace_file(&dir.join(&previous),current[&previous].as_str().unwrap());}
                return Err(error);
            }
            written.push(file.clone());
        }
        Ok(json!({"restored":keys.len()}))
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn selective_restore_preserves_new_fields_and_other_values() {
        let text="[General]\r\nMouseSpeed=15\r\nNewSetting=1\r\n[Other]\r\nMouseSpeed=7\r\n";
        let result=merge(text,"game.cfg",&serde_json::from_value(json!({"[General]/MouseSpeed":"10"})).unwrap()).unwrap();
        assert!(result.contains("MouseSpeed=10\r\nNewSetting=1"));assert!(result.contains("[Other]\r\nMouseSpeed=7"));
        let result=merge(r#"{"files":[{"name":"x","value":5}],"added":true}"#,"PersistedSettings.json",&serde_json::from_value(json!({"/files/0/value":3})).unwrap()).unwrap();
        let value:Value=serde_json::from_str(&result).unwrap();assert_eq!(value["added"],true);assert_eq!(value["files"][0]["value"],3);
        assert!(merge("{}","PersistedSettings.json",&serde_json::from_value(json!({"/missing":3})).unwrap()).is_err());
    }
}
