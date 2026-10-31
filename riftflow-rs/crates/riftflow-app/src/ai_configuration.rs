//! Default provider is fetched over HTTPS and decrypted only in this process.
use std::{path::{Path,PathBuf},time::{Duration,Instant,SystemTime,UNIX_EPOCH}};
use anyhow::{ensure,Context};
use base64::{Engine,engine::general_purpose::STANDARD};
use ring::{aead,agreement,hkdf,rand::{SecureRandom,SystemRandom}};
use serde_json::{Value,json};
use tokio::sync::Mutex;
use super::ai_assistant::{Config,endpoint};

pub struct Configuration {
    pub path:PathBuf,
    remote_url:String,
    cached:Mutex<Option<(Instant,Config)>>,
    file_lock:std::sync::Mutex<()>,
}
impl Configuration {
    pub fn new(directory:&Path)->Self{
        let remote_url=std::env::var("RIFTFLOW_AI_CONFIG_URL").ok().filter(|value|{
            reqwest::Url::parse(value).ok().is_some_and(|url|matches!(url.host_str(),Some("127.0.0.1"|"localhost"))&&std::env::var("RIFTFLOW_AUTO_CONNECT").as_deref()==Ok("0"))
        }).unwrap_or_else(||"https://riftflow-api.yymainy.online/ai/v1/config".into());
        Self{path:directory.join("ai-custom.dpapi"),remote_url,cached:Mutex::new(None),file_lock:std::sync::Mutex::new(())}
    }
    fn custom(&self)->anyhow::Result<Option<Config>>{
        let _guard=self.file_lock.lock().unwrap();
        if !self.path.exists(){return Ok(None)}
        let bytes=std::fs::read(&self.path).context("无法读取自定义 AI 配置")?;
        let plaintext=protect(&bytes,false)?;
        let config:Config=serde_json::from_slice(&plaintext).map_err(|_|anyhow::anyhow!("自定义 AI 配置损坏，请恢复默认或重新保存"))?;
        endpoint(&config)?;
        Ok(Some(config))
    }
    pub async fn get(&self)->anyhow::Result<Config>{
        if let Some(config)=self.custom()?{return Ok(config)}
        let mut cache=self.cached.lock().await;
        if let Some((at,config))=&*cache{if at.elapsed()<Duration::from_secs(300){return Ok(config.clone())}}
        let config=fetch(&self.remote_url).await?;
        *cache=Some((Instant::now(),config.clone()));
        Ok(config)
    }
    pub async fn status(&self)->Value{
        let custom=self.path.exists();
        match self.get().await{
            Ok(config)=>json!({"configured":true,"path":self.path,"source":if custom{"custom"}else{"default"},"model":if custom{config.model.as_str()}else{"默认服务"},"error":null}),
            Err(error)=>json!({"configured":false,"path":self.path,"source":if custom{"custom"}else{"default"},"model":"","error":error.to_string()}),
        }
    }
    pub fn view(&self)->anyhow::Result<Value>{
        Ok(match self.custom()?{
            Some(config)=>json!({"source":"custom","url":config.url,"model":config.model,"hasKey":true}),
            None=>json!({"source":"default","url":"","model":"","hasKey":false}),
        })
    }
    pub fn save(&self,value:&Value)->anyhow::Result<Value>{
        let mut config:Config=serde_json::from_value(value.clone()).map_err(|_|anyhow::anyhow!("配置格式无效"))?;
        ensure!(config.url.len()<=8192&&config.api_key.len()<=16000&&config.model.len()<=256,"配置内容过长");
        if config.api_key.trim().is_empty(){if let Some(old)=self.custom()?{config.api_key=old.api_key}}
        endpoint(&config)?;
        let bytes=protect(&serde_json::to_vec(&config)?,true)?;
        let _guard=self.file_lock.lock().unwrap();
        let temporary=self.path.with_extension("tmp");
        std::fs::write(&temporary,bytes).context("无法保存 AI 配置")?;
        std::fs::rename(temporary,&self.path).context("无法替换 AI 配置")?;
        drop(_guard);
        self.view()
    }
    pub async fn reset(&self)->anyhow::Result<Value>{
        {
            let _guard=self.file_lock.lock().unwrap();
            if self.path.exists(){std::fs::remove_file(&self.path).context("无法清除自定义配置")?}
        }
        *self.cached.lock().await=None;
        Ok(self.view()?)
    }
}

async fn fetch(url:&str)->anyhow::Result<Config>{
    let rng=SystemRandom::new();
    let private=agreement::EphemeralPrivateKey::generate(&agreement::X25519,&rng).map_err(|_|anyhow::anyhow!("无法生成配置会话密钥"))?;
    let public=STANDARD.encode(private.compute_public_key().map_err(|_|anyhow::anyhow!("无法生成公钥"))?.as_ref());
    let mut random=[0;16];rng.fill(&mut random).map_err(|_|anyhow::anyhow!("无法生成随机数"))?;
    let id=random.iter().map(|byte|format!("{byte:02x}")).collect::<String>();
    let client=reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(8)).build()?;
    let mut response=client.post(url).json(&json!({"version":1,"requestId":id,"clientPublicKey":public})).send().await.map_err(|_|anyhow::anyhow!("无法获取默认 AI 配置，请检查网络或使用自定义配置"))?;
    ensure!(response.status().is_success(),"默认 AI 配置服务暂不可用（HTTP {}）",response.status().as_u16());
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|_|anyhow::anyhow!("配置响应读取失败"))?{ensure!(bytes.len()+chunk.len()<=65536,"配置响应过大");bytes.extend_from_slice(&chunk)}
    let envelope:Value=serde_json::from_slice(&bytes).map_err(|_|anyhow::anyhow!("配置响应格式无效"))?;
    decrypt(private,&public,&id,&envelope)
}
fn decrypt(private:agreement::EphemeralPrivateKey,public:&str,id:&str,value:&Value)->anyhow::Result<Config>{
    ensure!(value["version"]==1&&value["algorithm"]=="X25519-HKDF-SHA256-AES256GCM"&&value["requestId"]==id,"配置响应不匹配");
    let issued=value["issuedAt"].as_u64().context("配置时间无效")?;
    let expires=value["expiresAt"].as_u64().context("配置过期时间无效")?;
    let now=SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    ensure!(issued<=now+30000&&now<=expires&&expires>issued&&expires-issued<=120000,"配置响应已过期或时间无效");
    let decode=|name:&str|->anyhow::Result<Vec<u8>>{STANDARD.decode(value[name].as_str().context("配置字段缺失")?).map_err(|_|anyhow::anyhow!("配置编码无效"))};
    let peer=decode("serverPublicKey")?;let salt=decode("salt")?;let iv=decode("iv")?;let tag=decode("tag")?;let mut ciphertext=decode("ciphertext")?;
    ensure!(peer.len()==32&&salt.len()==16&&iv.len()==12&&tag.len()==16,"配置加密参数无效");
    let key=agreement::agree_ephemeral(private,&agreement::UnparsedPublicKey::new(&agreement::X25519,&peer),|shared|{
        let salt=hkdf::Salt::new(hkdf::HKDF_SHA256,&salt);
        let prk=salt.extract(shared);let info=[b"riftflow-ai-config-v1".as_slice()];
        let okm=prk.expand(&info,&aead::AES_256_GCM).map_err(|_|anyhow::anyhow!("配置密钥派生失败"))?;
        let mut key=[0;32];okm.fill(&mut key).map_err(|_|anyhow::anyhow!("配置密钥派生失败"))?;Ok::<_,anyhow::Error>(key)
    }).map_err(|_|anyhow::anyhow!("配置密钥交换失败"))??;
    let mut key=key;
    let cipher=aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM,&key).map_err(|_|anyhow::anyhow!("配置密钥无效"))?);key.fill(0);
    let aad=format!("riftflow-ai-config-v1\n{id}\n{issued}\n{expires}\n{public}\n{}",value["serverPublicKey"].as_str().unwrap_or(""));
    ciphertext.extend_from_slice(&tag);
    let nonce:[u8;12]=iv.try_into().map_err(|_|anyhow::anyhow!("配置 nonce 无效"))?;
    let plaintext=cipher.open_in_place(aead::Nonce::assume_unique_for_key(nonce),aead::Aad::from(aad.as_bytes()),&mut ciphertext).map_err(|_|anyhow::anyhow!("配置完整性校验失败"))?;
    let config:Config=serde_json::from_slice(plaintext).map_err(|_|anyhow::anyhow!("默认配置格式无效"))?;
    endpoint(&config)?;Ok(config)
}

#[cfg(windows)]
pub fn protect(bytes:&[u8],encrypt:bool)->anyhow::Result<Vec<u8>>{
    #[repr(C)]struct Blob{len:u32,data:*mut u8}
    #[link(name="Crypt32")]unsafe extern "system"{
        fn CryptProtectData(input:*const Blob,description:*const u16,entropy:*const Blob,reserved:*mut std::ffi::c_void,prompt:*mut std::ffi::c_void,flags:u32,output:*mut Blob)->i32;
        fn CryptUnprotectData(input:*const Blob,description:*mut *mut u16,entropy:*const Blob,reserved:*mut std::ffi::c_void,prompt:*mut std::ffi::c_void,flags:u32,output:*mut Blob)->i32;
    }
    #[link(name="Kernel32")]unsafe extern "system"{fn LocalFree(memory:*mut std::ffi::c_void)->*mut std::ffi::c_void;}
    ensure!(bytes.len()<=65536,"配置文件过大");
    let input=Blob{len:bytes.len() as u32,data:bytes.as_ptr() as *mut u8};let mut output=Blob{len:0,data:std::ptr::null_mut()};
    unsafe{
        let success=if encrypt{CryptProtectData(&input,std::ptr::null(),std::ptr::null(),std::ptr::null_mut(),std::ptr::null_mut(),1,&mut output)}else{CryptUnprotectData(&input,std::ptr::null_mut(),std::ptr::null(),std::ptr::null_mut(),std::ptr::null_mut(),1,&mut output)};
        ensure!(success!=0,"无法处理受保护配置，请使用保存该配置的 Windows 用户，或恢复默认");
        let result=std::slice::from_raw_parts(output.data,output.len as usize).to_vec();LocalFree(output.data.cast());Ok(result)
    }
}
#[cfg(not(windows))]
pub fn protect(_bytes:&[u8],_encrypt:bool)->anyhow::Result<Vec<u8>>{anyhow::bail!("当前平台不支持 Windows 配置保护")}


#[cfg(all(test,windows))]
mod tests{
    use super::*;
    #[tokio::test]
    async fn custom_credentials_are_protected_not_returned_and_reset_is_complete(){
        let directory=std::env::temp_dir().join(format!("riftflow-config-test-{}-{}",std::process::id(),SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&directory).unwrap();let store=Configuration::new(&directory);
        let view=store.save(&json!({"url":"https://example.com/v1","api_key":"private-test-key","model":"test"})).unwrap();
        assert_eq!(view["hasKey"],true);assert!(!view.to_string().contains("private-test-key"));
        assert!(!String::from_utf8_lossy(&std::fs::read(&store.path).unwrap()).contains("private-test-key"));
        store.save(&json!({"url":"https://example.com/v1","api_key":"","model":"changed"})).unwrap();
        assert_eq!(store.custom().unwrap().unwrap().api_key,"private-test-key");
        let mut bytes=std::fs::read(&store.path).unwrap();let index=bytes.len()-1;bytes[index]^=1;std::fs::write(&store.path,bytes).unwrap();assert!(store.custom().is_err());
        assert_eq!(store.reset().await.unwrap()["source"],"default");assert!(!store.path.exists());
        std::fs::remove_dir(directory).unwrap();
    }
}
