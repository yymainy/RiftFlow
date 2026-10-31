//! Portable EXE updates: verified staging, an independent worker, and rollback on launch failure.
use std::{path::{Path, PathBuf}, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, Ordering}}, time::{SystemTime, UNIX_EPOCH}};
use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use riftflow_state::StateSync;

#[derive(Serialize, Deserialize)]
struct Job { parent_pid:u32, target:PathBuf, staged:PathBuf, sha256:String, size:u64 }
struct Pending { worker:PathBuf, job:PathBuf, directory:PathBuf, version:String }
static PENDING:OnceLock<Mutex<Option<Pending>>>=OnceLock::new();
static BUSY:AtomicBool=AtomicBool::new(false);
fn pending()->&'static Mutex<Option<Pending>> {PENDING.get_or_init(||Mutex::new(None))}
struct Busy;
impl Drop for Busy {fn drop(&mut self){BUSY.store(false,Ordering::SeqCst);}}
pub fn valid_artifact(a:&Value)->bool {
    a["platform"]=="win32" && a["arch"]=="x64"
        && a["fileName"].as_str().is_some_and(|s|s.to_ascii_lowercase().ends_with(".exe"))
        && a["size"].as_u64().is_some_and(|n|n>0 && n<=200*1024*1024)
        && a["sha256"].as_str().is_some_and(|s|s.len()==64 && s.bytes().all(|b|b.is_ascii_hexdigit()))
        && a["downloadUrl"].as_str().and_then(|s|reqwest::Url::parse(s).ok()).is_some_and(|u|u.scheme()=="https" && u.username().is_empty() && u.password().is_none())
}
fn digest(bytes:&[u8])->String {ring::digest::digest(&ring::digest::SHA256,bytes).as_ref().iter().map(|b|format!("{b:02x}")).collect()}
fn verify(path:&Path,size:u64,hash:&str)->anyhow::Result<()> {
    ensure!(std::fs::metadata(path)?.len()==size,"更新文件大小不匹配");
    let bytes=std::fs::read(path)?;
    ensure!(digest(&bytes).eq_ignore_ascii_case(hash),"更新文件 SHA256 不匹配");
    let offset=bytes.get(0x3c..0x40).map(|b|u32::from_le_bytes(b.try_into().unwrap()) as usize).context("更新文件不是 Windows 程序")?;
    ensure!(bytes.starts_with(b"MZ") && bytes.get(offset..offset.saturating_add(6))==Some(&b"PE\0\0\x64\x86"[..]),"更新文件不是 Windows x64 EXE");
    Ok(())
}
pub async fn start(sync:&Arc<StateSync>,info:&Value,dir:&Path,cancel:&AtomicBool)->anyhow::Result<Value> {
    if BUSY.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).is_err(){return Ok(json!({"result":"no-op"}));}
    let _busy=Busy;
    cancel.store(false,Ordering::SeqCst);
    ensure!(pending().lock().unwrap().is_none(),"更新已就绪，请重启或取消");
    let a=&info["artifact"];
    ensure!(valid_artifact(a),"更新包不兼容或缺少校验信息");
    let stamp=SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let directory=dir.join(format!("portable-{stamp}"));
    std::fs::create_dir_all(&directory)?;
    let staged=directory.join("new.exe");
    let result=async {
        super::self_update::download(a["downloadUrl"].as_str().unwrap(),&staged,sync,cancel).await.map_err(anyhow::Error::msg)?;
        ensure!(!cancel.load(Ordering::SeqCst),"已取消更新");
        let hash=a["sha256"].as_str().unwrap().to_owned();let size=a["size"].as_u64().unwrap();
        verify(&staged,size,&hash)?;
        let target=std::fs::canonicalize(std::env::current_exe()?)?;
        // The worker is this trusted, already-running build, never unverified downloaded code.
        let worker=directory.join("update-worker.exe");
        std::fs::copy(&target,&worker)?;
        let job=directory.join("job.json");
        std::fs::write(&job,serde_json::to_vec(&Job{parent_pid:std::process::id(),target,staged:staged.clone(),sha256:hash,size})?)?;
        let mut slot=pending().lock().unwrap();
        ensure!(!cancel.load(Ordering::SeqCst),"已取消更新");
        *slot=Some(Pending{worker,job,directory:directory.clone(),version:info["version"].as_str().unwrap_or_default().into()});
        let _=sync.set("self-update-main","state","updateProgressInfo",json!({"phase":"waiting-for-restart","downloadingProgress":1,"averageDownloadSpeed":0,"downloadTimeLeft":0,"fileSize":size}));
        Ok(json!({"result":"ok"}))
    }.await;
    if result.is_err(){let _=std::fs::remove_file(&staged);}
    result
}
pub fn cancel() {
    if let Some(p)=pending().lock().unwrap().take(){
        // These are exact files in the app-created staging job; never recursively delete a computed path.
        for file in [&p.worker,&p.job,&p.directory.join("new.exe")] {let _=std::fs::remove_file(file);}
        let _=std::fs::remove_dir(p.directory);
    }
}
pub fn spawn_on_quit(data_dir:&Path) -> bool {
    let Some(p)=pending().lock().unwrap().take() else {return false};
    let mut cmd=std::process::Command::new(&p.worker);
    cmd.arg("--riftflow-apply-update").arg(&p.job);
    #[cfg(windows)] {use std::os::windows::process::CommandExt;cmd.creation_flags(0x08000000);}
    match cmd.spawn(){
        Ok(_)=>{let _=std::fs::write(data_dir.join("NEW_VERSION_FLAG"),serde_json::to_vec(&p.version).unwrap());},
        Err(_)=>{let _=std::fs::write(data_dir.join("NEW_VERSION_FLAG"),"\"999999.0.0\"");}
    }
    true
}
#[cfg(windows)]
fn wait_parent(pid:u32)->anyhow::Result<()> {
    #[link(name="kernel32")] unsafe extern "system" {fn OpenProcess(access:u32,inherit:i32,pid:u32)->isize;fn WaitForSingleObject(handle:isize,millis:u32)->u32;fn CloseHandle(handle:isize)->i32;fn GetLastError()->u32;}
    ensure!(pid!=0 && pid!=std::process::id(),"更新父进程无效");
    unsafe {
        let handle=OpenProcess(0x00100000,0,pid);
        if handle==0 {ensure!(GetLastError()==87,"无法等待原程序退出");return Ok(());}
        let result=WaitForSingleObject(handle,120_000);CloseHandle(handle);
        ensure!(result==0,"原程序尚未退出，已保留旧版本");
    }
    Ok(())
}
#[cfg(not(windows))] fn wait_parent(_:u32)->anyhow::Result<()> {anyhow::bail!("仅支持 Windows")}
fn apply(job:&Job,launch:impl FnOnce(&Path)->std::io::Result<()>)->anyhow::Result<()> {
    verify(&job.staged,job.size,&job.sha256)?;
    ensure!(job.target.is_absolute() && job.target.extension().is_some_and(|s|s.eq_ignore_ascii_case("exe")),"目标路径无效");
    let stamp=SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let next=job.target.with_extension(format!("update-{stamp}.exe"));
    let backup=job.target.with_extension(format!("backup-{stamp}.exe"));
    std::fs::copy(&job.staged,&next)?;
    verify(&next,job.size,&job.sha256)?;
    if let Err(e)=std::fs::rename(&job.target,&backup){let _=std::fs::remove_file(&next);return Err(e).context("无法替换旧程序，旧版已保留");}
    if let Err(e)=std::fs::rename(&next,&job.target){let _=std::fs::rename(&backup,&job.target);let _=std::fs::remove_file(&next);return Err(e).context("更新失败，已恢复旧程序");}
    if let Err(e)=launch(&job.target){let _=std::fs::remove_file(&job.target);std::fs::rename(&backup,&job.target).context("启动失败且恢复旧程序失败")?;return Err(e).context("新版无法启动，已恢复旧程序");}
    let _=std::fs::remove_file(&job.staged);
    // Keep the old executable beside the new one for manual recovery.
    Ok(())
}
pub fn run_worker()->Option<anyhow::Result<()>> {
    let mut args=std::env::args_os().skip(1);
    if args.next().as_deref()!=Some(std::ffi::OsStr::new("--riftflow-apply-update")){return None;}
    Some((|| {
        let path=PathBuf::from(args.next().context("缺少更新任务")?);
        ensure!(std::fs::metadata(&path)?.len()<16384,"更新任务无效");
        let job:Job=serde_json::from_slice(&std::fs::read(&path)?)?;
        ensure!(job.staged.parent()==path.parent(),"更新任务目录无效");
        wait_parent(job.parent_pid)?;
        let result=apply(&job,|target|std::process::Command::new(target).current_dir(target.parent().unwrap()).spawn().map(|_|()));
        if result.is_err(){let _=std::process::Command::new(&job.target).spawn();}
        result
    })())
}
#[cfg(test)] mod tests {
    use super::*;
    fn pe()->Vec<u8>{let mut b=vec![0u8;128];b[..2].copy_from_slice(b"MZ");b[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());b[64..70].copy_from_slice(b"PE\0\0\x64\x86");b}
    #[test] fn rejects_hash_and_non_executables(){let d=std::env::temp_dir().join(format!("rf-verify-{}",SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));std::fs::write(&d,pe()).unwrap();let b=pe();assert!(verify(&d,b.len() as u64,&digest(&b)).is_ok());assert!(verify(&d,b.len() as u64,&"0".repeat(64)).is_err());assert!(verify(&d,1,&digest(&b)).is_err());std::fs::remove_file(d).unwrap();}
    #[test] fn replaces_and_rolls_back_on_launch_failure(){
        let d=std::env::temp_dir().join(format!("rf-apply-{}",SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));std::fs::create_dir(&d).unwrap();let target=d.join("app.exe");let staged=d.join("new.exe");let bytes=pe();std::fs::write(&staged,&bytes).unwrap();std::fs::write(&target,b"old").unwrap();let j=Job{parent_pid:1,target:target.clone(),staged:staged.clone(),sha256:digest(&bytes),size:bytes.len() as u64};
        assert!(apply(&j,|_|Err(std::io::Error::other("test launch failure"))).is_err());assert_eq!(std::fs::read(&target).unwrap(),b"old");assert!(apply(&j,|_|Ok(())).is_ok());assert_eq!(std::fs::read(&target).unwrap(),bytes);
        for f in std::fs::read_dir(&d).unwrap(){std::fs::remove_file(f.unwrap().path()).unwrap();}std::fs::remove_dir(d).unwrap();
    }
}
