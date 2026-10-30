//! Fetch official patch articles on demand; cache plain text sections, never render remote HTML.
use super::{controller::ArchiveController,store::now};
use anyhow::{ensure,Context};
use serde_json::{json,Value};
use std::time::Duration;

pub(super) fn text(html:&str)->String {
    let mut output=String::new();let mut inside=false;
    for c in html.chars() {match c {'<'=>{inside=true;output.push(' ');},'>'=>inside=false,_ if !inside=>output.push(c),_=>{}}}
    decode_entities(&output).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decode plain text once, using the same HTML entity table as the HTML parser.
/// Keep unknown references literal and never interpret decoded text as markup.
fn decode_entities(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        rest = &rest[start + 1..];
        let end = rest.bytes().take(33).position(|c| c == b';');
        let decoded = end.and_then(|end| {
            let name = &rest[..end];
            if let Some(number) = name.strip_prefix('#') {
                let code = if let Some(hex) = number.strip_prefix('x').or_else(|| number.strip_prefix('X')) {
                    u32::from_str_radix(hex, 16).ok()
                } else { number.parse::<u32>().ok() }?;
                let character = char::from_u32(code).filter(|c| *c != '\0').unwrap_or('\u{fffd}');
                Some(character.to_string())
            } else {
                markup5ever::data::NAMED_ENTITIES.get(&rest[..=end]).and_then(|&(first, second)| {
                    let mut value = char::from_u32(first)?.to_string();
                    if second != 0 { value.push(char::from_u32(second)?); }
                    Some(value)
                })
            }
        });
        if let Some(value) = decoded {
            output.push_str(&value);
            rest = &rest[end.unwrap() + 1..];
        } else { output.push('&'); }
    }
    output.push_str(rest);
    output
}

fn decode_cached_sections(article: &mut Value) {
    if article["entitiesDecoded"] == true { return; }
    if let Some(sections) = article["sections"].as_array_mut() {
        for section in sections {
            for key in ["title", "context", "text"] {
                if let Some(value) = section[key].as_str() {
                    section[key] = json!(decode_entities(value));
                }
            }
        }
    }
}
fn normalize(value:&str)->String {value.chars().filter(|c|c.is_alphanumeric()).flat_map(char::to_lowercase).collect()}
fn article_body(value:&Value)->Option<&str> {
    match value {
        Value::Object(map)=>{if let Some(body)=map.get("body").and_then(Value::as_str).filter(|s|s.contains("patch-notes-container")){return Some(body);}map.values().find_map(article_body)},
        Value::Array(items)=>items.iter().find_map(article_body),_=>None,
    }
}
fn sections(html:&str)->Vec<Value> {
    let mut results=Vec::new();let mut offset=0;let mut context=String::new();
    while let Some(relative)=html[offset..].find("<h") {
        let start=offset+relative;let tail=&html[start..];
        let level=tail.as_bytes().get(2).copied().unwrap_or(0);
        if level!=b'2' && level!=b'3' {offset=start+2;continue;}
        let close=if level==b'2' {"</h2>"} else {"</h3>"};
        let Some(end)=tail.find(close) else {break};
        let title=text(&tail[..end]);offset=start+end+close.len();
        if level==b'2' {context=title;continue;}
        let remaining=&html[offset..];
        let end=remaining.find("<h3").into_iter().chain(remaining.find("<h2")).min().unwrap_or(remaining.len());
        let content=text(&remaining[..end]);
        if !content.is_empty() {results.push(json!({"title":title,"context":context,"text":content}));}
    }
    results
}

impl ArchiveController {
    pub async fn patches(&self, input:&Value)->anyhow::Result<Value> {
        if input["action"]=="preferences" {return self.store.document("patch-preferences");}
        let region=input["region"].as_str().unwrap_or("international");
        ensure!(region=="cn" || region=="international","请选择国服或国际服");
        if input["action"]=="options" {return self.patch_options(input["refresh"]==true,region).await;}
        ensure!(!matches!(self.phase().as_str(),"InProgress"|"ChampSelect"|"ReadyCheck"|"GameStart"),"选人或游戏中暂停下载版本公告");
        fn version(s:&str)->anyhow::Result<(u16,u16)> {
            let (year,patch)=s.split_once('.').context("版本格式应为 26.21")?;
            let pair=(year.parse::<u16>()?,patch.parse::<u16>()?);
            ensure!(pair.0>=26 && pair.0<=99 && pair.1>0 && pair.1<=30,"目前支持 26 年起的正式版本公告");Ok(pair)
        }
        let requested = if let Some(list) = input["versions"].as_array() {
            ensure!(!list.is_empty() && list.len()<=6,"请选择 1 到 6 个已发布版本");
            list.iter().map(|v|version(v.as_str().context("无效版本")?)).collect::<anyhow::Result<Vec<_>>>()?
        } else {
            let a=version(input["start"].as_str().context("请选择公告版本范围")?)?;
            let b=version(input["end"].as_str().context("请选择公告版本范围")?)?;
            ensure!(a.0==b.0 && a.1<=b.1 && b.1-a.1<6,"每次选择同一年度内最多 6 个版本");
            (a.1..=b.1).map(|patch|(a.0,patch)).collect()
        };
        let ids=input["champions"].as_array().context("请选择关注英雄")?;
        let champions:Value=serde_json::from_str(include_str!("../../assets/champion-summary.json"))?;
        let aliases=champions.as_array().unwrap().iter().filter(|c|ids.contains(&c["id"]))
            .map(|c|(c["id"].clone(),normalize(c[if region=="cn" {"name"} else {"alias"}].as_str().unwrap_or("")))).collect::<Vec<_>>();
        let mut keywords=input["keywords"].as_str().unwrap_or("").split(',').map(str::trim).filter(|s|!s.is_empty()).map(normalize).take(20).collect::<Vec<_>>();
        if let Some(ids)=input["items"].as_array() {
            ensure!(ids.len()<=30,"一次最多选择 30 件装备");
            let catalog=self.store.document("patch-items")?;
            for id in ids {
                let item=catalog["items"].as_array().and_then(|all|all.iter().find(|item|item["value"]==*id)).context("装备目录已更新，请刷新后重新选择装备")?;
                keywords.push(normalize(item[if region=="cn" {"label"} else {"english"}].as_str().context("装备名称资料不完整")?));
            }
        }
        ensure!(!aliases.is_empty() || !keywords.is_empty(),"请选择关注英雄或装备");
        let client=reqwest::Client::builder().timeout(Duration::from_secs(25)).redirect(reqwest::redirect::Policy::limited(3)).build()?;
        let mut reports=Vec::new();
        for (year,patch) in requested {
            let version=format!("{year}.{patch}");let key=if region=="cn" {format!("patch-cn-{version}")} else {format!("patch-{version}")};
            let mut cached=self.store.document(&key)?;
            decode_cached_sections(&mut cached);
            let url=format!("https://www.leagueoflegends.com/en-us/news/game-updates/league-of-legends-patch-{year}-{patch}-notes/");
            let mut warning=String::new();
            let article=if cached["at"].as_u64().map_or(false,|at|now().saturating_sub(at)<6*60*60*1000) {cached.clone()} else {
                let fetched=async {
                    if region=="cn" {return self.china_article(&client,&version).await;}
                    let response=client.get(&url).send().await?;
                    ensure!(response.status().is_success(),"官方公告返回 HTTP {}",response.status().as_u16());
                    ensure!(response.content_length().unwrap_or(0)<=4*1024*1024,"公告响应过大");
                    let html=response.text().await?;ensure!(html.len()<=4*1024*1024,"公告响应过大");
                    let after=html.split("id=\"__NEXT_DATA__\"").nth(1).context("官方公告结构已变化")?;
                    let body=after.split_once('>').context("缺少公告数据")?.1.split("</script>").next().unwrap();
                    let value:Value=serde_json::from_str(body)?;
                    let sections=sections(article_body(&value).context("缺少公告正文")?);
                    ensure!(!sections.is_empty(),"公告没有可识别的改动章节");
                    Ok::<_,anyhow::Error>(json!({"at":now(),"sections":sections}))
                }.await;
                match fetched {
                    Ok(mut value)=>{value["entitiesDecoded"]=json!(true);self.store.put_document(&key,&value)?;value},
                    Err(error)=>{warning=format!("官方公告读取失败：{error}；有缓存时显示旧资料");cached.clone()},
                }
            };
            let relevant=article["sections"].as_array().into_iter().flatten().filter_map(|section| {
                let title=normalize(section["title"].as_str().unwrap_or(""));
                let related=aliases.iter().filter(|(id,alias)|!alias.is_empty() && (title.contains(alias) || (*id==json!(62) && title.contains("wukong"))))
                    .map(|(id,_)|id.clone()).collect::<Vec<_>>();
                if !related.is_empty() || keywords.iter().any(|keyword|title.contains(keyword)) {let mut row=section.clone();row["championIds"]=json!(related);Some(row)} else {None}
            }).collect::<Vec<_>>();
            let source=if region=="cn" {
                article["source"].as_str().map(str::to_string).or_else(|| self.store.document("patch-catalog-cn").ok().and_then(|data| data["articles"].as_array().and_then(|rows|rows.iter().find(|row|row["version"]==version)).and_then(|row|row["url"].as_str()).map(str::to_string))).unwrap_or_else(||"https://lol.qq.com/main.shtml".into())
            } else {url};
            reports.push(json!({"version":version,"source":source,"region":if region=="cn" {"腾讯国服 · 中文原文"} else {"Riot 国际服 · 英文原文"},"at":article["at"],"publishedAt":article["publishedAt"],"warning":warning,"sections":relevant,"available":article["sections"].is_array()}));
        }
        self.store.put_document("patch-preferences",input)?;
        Ok(json!({"reports":reports}))
    }
}

#[cfg(test)]mod tests {use super::*;#[test]fn extracts_champion_and_mode_context_without_html(){let rows=sections("<h2>Champions</h2><h3>Annie</h3><p>Q damage 5 ⇒ 7</p><h2>ARAM</h2><h3>Annie</h3><p>Damage received</p>");assert_eq!(rows.len(),2);assert_eq!(rows[1]["context"],"ARAM");assert_eq!(rows[0]["text"],"Q damage 5 ⇒ 7");}}

#[cfg(test)]
mod entity_tests {
    use super::*;

    #[test]
    fn decodes_official_article_entities_and_numeric_references() {
        assert_eq!(text("<p>&emsp;在削弱后 &rArr; 15 &#8594; 30 &#x21D2; 45 &le; 60</p>"),
            "在削弱后 ⇒ 15 → 30 ⇒ 45 ≤ 60");
        assert_eq!(text("&lt;10 &amp; &quot;Q&quot; &unknown;"), "<10 & \"Q\" &unknown;");
        assert_eq!(decode_entities("&amp;rArr;"), "&rArr;");
        assert_eq!(decode_entities("&#0; &#xD800;"), "� �");
    }

    #[test]
    fn repairs_legacy_cache_without_removing_line_breaks_or_comparisons() {
        let mut article = json!({"sections":[{"title":"赵信", "context":"英雄", "text":"&emsp;在削弱后\n伤害 < 30 &rArr; 45"}]});
        decode_cached_sections(&mut article);
        assert_eq!(article["sections"][0]["text"], "\u{2003}在削弱后\n伤害 < 30 ⇒ 45");
        article["entitiesDecoded"] = json!(true);
        article["sections"][0]["text"] = json!("Literal &rArr;");
        decode_cached_sections(&mut article);
        assert_eq!(article["sections"][0]["text"], "Literal &rArr;");
    }
}
