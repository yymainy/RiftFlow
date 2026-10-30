use anyhow::{ensure, Context};
use serde_json::{json, Value};
use super::{controller::ArchiveController, store::now};

fn official_url(url: &str) -> bool {
    reqwest::Url::parse(url).ok().is_some_and(|url| url.scheme()=="https" && url.host_str()==Some("lol.qq.com") && url.username().is_empty() && url.port().is_none() && (url.path().starts_with("/gicp/news/410/") || url.path()=="/news/detail.shtml"))
}

pub(super) fn catalog_rows(body: &str) -> anyhow::Result<Vec<Value>> {
    // The public website returns JSONP. Parse the object, never execute a callback.
    let begin=body.find('{').context("国服公告目录格式已变化")?;
    let end=body.rfind('}').context("国服公告目录格式已变化")?;
    ensure!(begin<=end,"国服公告目录格式已变化");
    let data: Value=serde_json::from_str(&body[begin..=end])?;
    ensure!(data["status"]==1,"国服官网暂时未返回公告目录");
    let mut rows=Vec::new();
    for row in data.pointer("/data/result").and_then(Value::as_array).context("国服公告目录缺少条目")? {
        let title=row["sTitle"].as_str().unwrap_or("").trim();
        let Some(version)=title.strip_suffix("版本更新公告") else {continue};
        let Some((year,patch))=version.split_once('.') else {continue};
        let (Ok(year),Ok(patch))=(year.parse::<u16>(),patch.parse::<u16>()) else {continue};
        if !(26..=99).contains(&year) || !(1..=30).contains(&patch) {continue;}
        let mut url=row["sRedirectURL"].as_str().unwrap_or("").to_string();
        if url.starts_with("//") {url=format!("https:{url}");}
        if url.is_empty() {
            let id=row["iDocID"].as_str().unwrap_or("");
            if !id.is_empty() && id.len()<=30 && id.bytes().all(|b|b.is_ascii_digit()) {url=format!("https://lol.qq.com/news/detail.shtml?docid={id}");}
        }
        if official_url(&url) {rows.push(json!({"version":format!("{year}.{patch}"),"year":year,"patch":patch,"url":url,"publishedAt":row["sCreated"]}));}
    }
    rows.sort_by_key(|row| std::cmp::Reverse((row["year"].as_u64(),row["patch"].as_u64())));
    rows.dedup_by(|a,b|a["version"]==b["version"]);
    Ok(rows)
}

pub(super) async fn fetch_catalog(client: &reqwest::Client) -> anyhow::Result<Value> {
    let mut articles=Vec::new();
    for page in 1..=3 {
        let url=format!("https://apps.game.qq.com/cmc/zmMcnTargetContentList?r0=jsonp&page={page}&num=100&target=24&source=web_pc");
        let response=client.get(&url).header("Referer","https://lol.qq.com/").send().await?.error_for_status()?;
        ensure!(response.content_length().unwrap_or(0)<=4*1024*1024,"国服公告目录响应过大");
        let body=response.text().await?;
        ensure!(body.len()<=4*1024*1024,"国服公告目录响应过大");
        articles.extend(catalog_rows(&body)?);
        articles.sort_by_key(|row|std::cmp::Reverse((row["year"].as_u64(),row["patch"].as_u64())));
        articles.dedup_by(|a,b|a["version"]==b["version"]);
        if articles.len()>=6 {break;}
    }
    ensure!(!articles.is_empty(),"国服官网暂时没有可识别的版本公告");
    let versions:Vec<_>=articles.iter().map(|row|row["version"].clone()).collect();
    Ok(json!({"at":now(),"versions":versions,"articles":articles}))
}

fn article_body(html: &str) -> Option<&str> {
    let marker=html.find("id=\"article\"")?;
    let start=marker+html[marker..].find('>')?+1;
    let mut offset=start;
    let mut depth=1;
    while offset<html.len() {
        let open=html[offset..].find("<div").map(|at|offset+at);
        let close=html[offset..].find("</div").map(|at|offset+at)?;
        if open.is_some_and(|open|open<close) {depth+=1;offset=open?+4;}
        else {depth-=1;if depth==0 {return Some(&html[start..close]);}offset=close+5;}
    }
    None
}

pub(super) fn sections(html: &str) -> anyhow::Result<Vec<Value>> {
    let html=article_body(html).context("国服公告正文结构已变化，请打开官网原文查看")?;
    // Tencent uses h3 for groups and h4 for champions/items.
    let item_level=if html.contains("<h4") {b'4'} else {b'3'};
    let group_level=item_level-1;
    let mut rows=Vec::new();let mut context=String::new();let mut offset=0;
    while let Some(relative)=html[offset..].find("<h") {
        let start=offset+relative;let tail=&html[start..];let level=tail.as_bytes().get(2).copied().unwrap_or(0);
        if level!=item_level && level!=group_level {offset=start+2;continue;}
        let closing=format!("</h{}>",level as char);
        let Some(end)=tail.find(&closing) else {break};
        let title=super::patches::text(&tail[..end]);offset=start+end+closing.len();
        if level==group_level {context=title;continue;}
        let remaining=&html[offset..];
        let end=remaining.find(&format!("<h{}",item_level as char)).into_iter().chain(remaining.find(&format!("<h{}",group_level as char))).min().unwrap_or(remaining.len());
        let blocks=remaining[..end].replace("</p>","\n").replace("</li>","\n").replace("<br />","\n").replace("<br>","\n");
        let content=blocks.lines().map(super::patches::text).filter(|line|!line.is_empty()).collect::<Vec<_>>().join("\n");
        if !content.is_empty() {rows.push(json!({"title":title,"context":context,"text":content}));}
    }
    ensure!(!rows.is_empty(),"国服公告未能识别英雄或装备章节，请打开官网原文查看");
    Ok(rows)
}

impl ArchiveController {
    pub(super) async fn china_article(&self, client: &reqwest::Client, version: &str) -> anyhow::Result<Value> {
        let catalog=self.store.document("patch-catalog-cn")?;
        let article=catalog["articles"].as_array().and_then(|rows|rows.iter().find(|row|row["version"]==version)).context("未找到该版本国服公告，请先刷新国服版本目录")?;
        let url=article["url"].as_str().filter(|url|official_url(url)).context("国服公告来源地址无效")?;
        let response=client.get(url).send().await?.error_for_status()?;
        ensure!(response.content_length().unwrap_or(0)<=8*1024*1024,"国服公告响应过大");
        let html=response.text_with_charset("gbk").await?;
        ensure!(html.len()<=8*1024*1024,"国服公告响应过大");
        Ok(json!({"at":now(),"sections":sections(&html)?,"source":url,"publishedAt":article["publishedAt"]}))
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn distinguishes_lol_from_tft_and_rejects_nonofficial_redirects() {
        let body=r#"callback({"status":1,"data":{"result":[{"sTitle":"26.21版本更新公告","sRedirectURL":"https://lol.qq.com/gicp/news/410/1.html"},{"sTitle":"18.5云顶之弈版本更新公告","sRedirectURL":"https://lol.qq.com/gicp/news/662/2.html"},{"sTitle":"26.20版本更新公告","sRedirectURL":"https://other.example/news"}]}})"#;
        let rows=catalog_rows(body).unwrap();assert_eq!(rows.len(),1);assert_eq!(rows[0]["version"],"26.21");
    }
    #[test] fn keeps_chinese_sections_and_context_as_plain_text() {
        let html="<div id=\"article\"><h3>英雄</h3><h4><em>德邦总管 赵信</em></h4><p>Q伤害：10 → 20</p><h3>装备</h3><h4>无尽之刃</h4><p>攻击力提高</p></div><h4>侧栏</h4>";
        let rows=sections(html).unwrap();assert_eq!(rows.len(),2);assert_eq!(rows[0]["context"],"英雄");assert_eq!(rows[0]["title"],"德邦总管 赵信");assert_eq!(rows[1]["text"],"攻击力提高");
    }
}
