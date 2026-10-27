use std::{any::Any, collections::HashSet, path::Path, sync::Arc};

use futures_util::StreamExt;
use reqwest::{Client, StatusCode, header::{CONTENT_LENGTH, COOKIE, LOCATION, RANGE}};
use serde::{Deserialize, Serialize};
use shinki_emu_store::{BoxFuture, CancelToken, Capabilities, DownloadPlan, LoginKind, LoginRequest, LoginUpdates, LoginUpdate, OwnedTitle, PlanOptions, Progress, ProgressUpdates, ProviderDescriptor, SecretStore, StoreError, StoreProvider, StoreSession, StoredLogin, TitleDetail, TitleId, TransferSummary};
use tokio::{fs, io::AsyncWriteExt, sync::mpsc};
use zeroize::{Zeroize, ZeroizeOnDrop};

const ID: &str = "dlsite";
const PLAY: &str = "https://play.dlsite.com/api/v3";
const DOWNLOAD: &str = "https://play.dlsite.com/api/v3/download";
const PRODUCT: &str = "https://www.dlsite.com/home/api/=/product.json";
const METHODS: &[LoginKind] = &[LoginKind::BrowserCookies];
static DESCRIPTOR: ProviderDescriptor = ProviderDescriptor { id: ID, display_name: "DLsite", login_methods: METHODS, browser_login_url: Some("https://login.dlsite.com/login"), capabilities: Capabilities { branches: false, depots: false, verify: true } };

#[derive(Clone, Debug, Deserialize)]
struct Count { user: u64 }
#[derive(Clone, Debug, Deserialize)]
struct Sale { workno: String, #[serde(default)] sales_date: Option<String> }
#[derive(Clone, Debug, Deserialize)]
struct Works { #[serde(default)] works: Vec<Work> }
#[derive(Clone, Debug, Deserialize)]
struct Work { #[serde(alias="workno")] id: String, #[serde(default)] name: Name, #[serde(default)] upgrade_date: Option<String>, #[serde(default)] content_size: Option<u64> }
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(untagged)]
enum Name { Text(String), Localized(std::collections::BTreeMap<String,String>), #[default] Missing }
impl Name { fn text(&self) -> String { match self { Self::Text(s) => s.clone(), Self::Localized(m) => m.get("en_US").or_else(||m.get("ja_JP")).or_else(||m.values().next()).cloned().unwrap_or_default(), Self::Missing => String::new() } } }
#[derive(Clone, Debug, Deserialize)]
struct PublicWork { #[serde(default, alias="product_id")] id: String, #[serde(default, alias="work_name")] name: String, #[serde(default)] update_date: Option<String>, #[serde(default)] contents_file_size: Option<serde_json::Value> }
#[derive(Clone, Debug, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct Auth { cookie_header: String, account: String }
#[derive(Clone)]
struct Http { client: Client, cookie: Arc<String> }
#[derive(Clone)]
struct PlannedFile { url: String, name: String, expected_size: Option<u64> }
#[derive(Clone)]
struct PlanData { files: Vec<PlannedFile>, version: Option<String> }

impl Http {
    fn new(cookie: String) -> Result<Self, StoreError> { let client = Client::builder().user_agent("ShinkiEMU").redirect(reqwest::redirect::Policy::none()).build().map_err(|_| StoreError::Network)?; Ok(Self { client, cookie: Arc::new(cookie) }) }
    async fn get_json<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T, StoreError> { let r = self.client.get(url).header(COOKIE, self.cookie.as_str()).send().await.map_err(|_| StoreError::Network)?; if r.status()==StatusCode::UNAUTHORIZED { return Err(StoreError::SessionExpired); } r.error_for_status().map_err(|_|StoreError::Network)?.json().await.map_err(|_|StoreError::Metadata) }
    async fn library(&self) -> Result<Vec<Work>, StoreError> {
        let count: Count = self.get_json(&format!("{PLAY}/content/count?last=0")).await?;
        if count.user == 0 { return Ok(Vec::new()); }
        let sales: Vec<Sale> = self.get_json(&format!("{PLAY}/content/sales?last=0")).await?;
        let ids: Vec<String> = sales.into_iter().map(|s| s.workno).collect();
        let mut seen = HashSet::new(); let mut works = Vec::new();
        for batch in ids.chunks(100) {
            let r = self.client.post(format!("{PLAY}/content/works")).header(COOKIE, self.cookie.as_str()).json(batch).send().await.map_err(|_| StoreError::Network)?;
            if r.status()==StatusCode::UNAUTHORIZED { return Err(StoreError::SessionExpired); }
            let mut page: Works = r.error_for_status().map_err(|_| StoreError::Network)?.json().await.map_err(|_| StoreError::Metadata)?;
            for work in page.works.drain(..) { if seen.insert(work.id.clone()) { works.push(work); } }
        }
        Ok(works)
    }
    async fn metadata(&self, id: &str) -> Result<Option<PublicWork>, StoreError> {
        let r = self.client.get(PRODUCT).query(&[("workno", id)]).header(COOKIE,self.cookie.as_str()).send().await.map_err(|_| StoreError::Network)?;
        if !r.status().is_success() { return Ok(None); }
        let values: Vec<PublicWork> = r.json().await.map_err(|_| StoreError::Metadata)?;
        Ok(values.into_iter().find(|w| w.id==id))
    }
    async fn download_plan(&self, id: &str) -> Result<PlanData, StoreError> {
        let r = self.client.get(DOWNLOAD).query(&[("workno",id)]).header(COOKIE,self.cookie.as_str()).send().await.map_err(|_|StoreError::Network)?;
        if r.status()==StatusCode::UNAUTHORIZED { return Err(StoreError::SessionExpired); }
        if !r.status().is_redirection() { return Err(StoreError::Unavailable); }
        let location = r.headers().get(LOCATION).and_then(|v|v.to_str().ok()).ok_or(StoreError::Metadata)?;
        let url = r.url().join(location).map_err(|_|StoreError::Metadata)?;
        let host = url.host_str().unwrap_or_default(); let path = url.path();
        let (urls, serial_page) = if host=="www.dlsite.com" && (path.starts_with("/home/download/split") || path.starts_with("/home/split")) { (extract_links(&self.fetch_text(url.as_str()).await?, url.as_str()), false) }
        else if host=="www.dlsite.com" && (path.starts_with("/home/download/serial") || path.starts_with("/home/serial")) { (extract_links(&self.fetch_text(url.as_str()).await?, url.as_str()), true) }
        else if host=="www.dlsite.com" && path.starts_with("/home/download") { (vec![url.to_string()],false) }
        else { return Err(StoreError::Unavailable); };
        let mut files: Vec<_> = urls.into_iter().filter(|u| u.contains("/home/download") && u.contains("product_id")).enumerate().map(|(i,url)| PlannedFile { url, name: format!("part-{:03}.bin",i+1), expected_size: None }).collect();
        if serial_page { files.retain(|f| !f.url.contains("/number/")); }
        files.sort_by(|a,b| natural_cmp(&a.url,&b.url));
        if files.is_empty() { return Err(StoreError::Metadata); }
        Ok(PlanData { files, version: None })
    }
    async fn fetch_text(&self, url: &str) -> Result<String, StoreError> { let mut url=url.to_owned(); for _ in 0..8 { let r=self.client.get(&url).header(COOKIE,self.cookie.as_str()).send().await.map_err(|_|StoreError::Network)?; if r.status().is_redirection() { let loc=r.headers().get(LOCATION).and_then(|v|v.to_str().ok()).ok_or(StoreError::Metadata)?; url=r.url().join(loc).map_err(|_|StoreError::Metadata)?.to_string(); continue; } return r.error_for_status().map_err(|_|StoreError::Network)?.text().await.map_err(|_|StoreError::Network); } Err(StoreError::TransferFailed) }
}
fn extract_links(html:&str, base:&str)->Vec<String>{ let mut out=Vec::new(); for quote in ['"','\''] { let mut start=None; for (i,c) in html.char_indices(){ if c==quote { if let Some(s)=start.take(){ if let Ok(u)=reqwest::Url::parse(base).and_then(|b|b.join(&html[s..i]).map_err(Into::into)){out.push(u.to_string());} } else {start=Some(i+1);} } } } out.sort();out.dedup();out }
fn natural_cmp(a:&str,b:&str)->std::cmp::Ordering { let num=|s:&str| s.split("/number/").nth(1).and_then(|x|x.split('/').next()).and_then(|x|x.parse::<u32>().ok()); num(a).cmp(&num(b)).then_with(||a.cmp(b)) }
fn update_available(current:Option<&str>, installed:Option<&str>)->bool { current.zip(installed).is_some_and(|(a,b)|a!=b) }

pub struct Provider;
impl StoreProvider for Provider {
 fn descriptor(&self)->&'static ProviderDescriptor { &DESCRIPTOR }
 fn login<'a>(&'a self, request:LoginRequest,secrets:&'a dyn SecretStore,_codes:&'a mut mpsc::Receiver<String>,updates:LoginUpdates,cancel:CancelToken)->BoxFuture<'a,Box<dyn StoreSession>> { Box::pin(async move { let (cookie,account)=match request { LoginRequest::BrowserCookies{cookies} if !cookies.trim().is_empty()=> (cookies.to_string(),"DLsite account".to_owned()), LoginRequest::BrowserCookies{..}=>return Err(StoreError::CookiesRequired), _=>return Err(StoreError::UnsupportedLogin) }; if cancel.is_cancelled(){return Err(StoreError::Cancelled)} updates(LoginUpdate::Connecting); let http=Http::new(cookie.clone())?; http.library().await?; let auth=Auth{cookie_header:cookie,account:account.clone()}; let secret=serde_json::to_string(&auth).map_err(|_|StoreError::SecretStore)?; secrets.save(ID,&StoredLogin{account:account.clone(),secret}).map_err(|_|StoreError::SecretStore)?; updates(LoginUpdate::SignedIn(account.clone())); Ok(Box::new(Session{account,http}) as Box<dyn StoreSession>) }) }
 fn restore<'a>(&'a self,secrets:&'a dyn SecretStore,cancel:CancelToken)->BoxFuture<'a,Option<Box<dyn StoreSession>>> { Box::pin(async move { if cancel.is_cancelled(){return Ok(None)} let Some(stored)=secrets.load(ID).map_err(|_|StoreError::SecretStore)? else{return Ok(None)}; let auth:Auth=serde_json::from_str(&stored.secret).map_err(|_|StoreError::SessionExpired)?; let http=Http::new(auth.cookie_header)?; http.library().await?; Ok(Some(Box::new(Session{account:auth.account,http}) as Box<dyn StoreSession>)) }) }
}
pub struct Session { account:String,http:Http }
impl Session { async fn find(&self,id:&TitleId)->Result<Work,StoreError>{self.http.library().await?.into_iter().find(|w|w.id==id.0).ok_or(StoreError::AccessDenied)} }
impl StoreSession for Session {
 fn account(&self)->&str{&self.account}
 fn library(&self)->BoxFuture<'_,Vec<OwnedTitle>>{Box::pin(async move{let titles=self.http.library().await?.into_iter().map(|w|OwnedTitle{id:TitleId(w.id),name:w.name.text(),platforms:vec!["Windows".into()],languages:Vec::new(),size_bytes:w.content_size.unwrap_or_default(),unavailable_reason:None}).collect();Ok(titles)})}
 fn detail<'a>(&'a self,id:&'a TitleId)->BoxFuture<'a,TitleDetail>{Box::pin(async move{let w=self.find(id).await?;let m=self.http.metadata(&w.id).await?;let title=OwnedTitle{id:id.clone(),name:m.as_ref().map(|p|p.name.clone()).filter(|s|!s.is_empty()).unwrap_or_else(||w.name.text()),platforms:vec!["Windows".into()],languages:vec![],size_bytes:w.content_size.unwrap_or_default(),unavailable_reason:None};Ok(TitleDetail{title,branches:vec![],depots:vec![]})})}
 fn plan<'a>(&'a self,id:&'a TitleId,_options:PlanOptions)->BoxFuture<'a,DownloadPlan>{Box::pin(async move{let w=self.find(id).await?;let mut title=OwnedTitle{id:id.clone(),name:w.name.text(),platforms:vec!["Windows".into()],languages:vec![],size_bytes:w.content_size.unwrap_or_default(),unavailable_reason:None};let data=match self.http.download_plan(&w.id).await{Ok(data)=>data,Err(e)=>{title.unavailable_reason=Some("DLsite did not provide a verified PC download plan for this work.".into());return Err(e)}};Ok(DownloadPlan{title,branch:String::new(),depots:vec![],payload:Box::new(data)})})}
 fn download<'a>(&'a self,plan:&'a DownloadPlan,dest:&'a Path,work_dir:&'a Path,progress:ProgressUpdates,cancel:CancelToken)->BoxFuture<'a,TransferSummary>{Box::pin(async move{let data=plan.payload.downcast_ref::<PlanData>().ok_or(StoreError::Metadata)?;fs::create_dir_all(dest).await?;fs::create_dir_all(work_dir).await?;let mut summary=TransferSummary::default();let total=data.files.len() as u64;for(i,file)in data.files.iter().enumerate(){if cancel.is_cancelled(){summary.cancelled=true;break}let partial=work_dir.join(format!("{}.partial",file.name));let existing=fs::metadata(&partial).await.map(|m|m.len()).unwrap_or(0);let mut req=self.http.client.get(&file.url).header(COOKIE,self.http.cookie.as_str());if existing>0{req=req.header(RANGE,format!("bytes={existing}-"));}let mut response=req.send().await.map_err(|_|StoreError::Network)?;for _ in 0..8{if response.status().is_redirection(){let loc=response.headers().get(LOCATION).and_then(|v|v.to_str().ok()).ok_or(StoreError::Metadata)?;let u=response.url().join(loc).map_err(|_|StoreError::Metadata)?;let mut next=self.http.client.get(u);if existing>0{next=next.header(RANGE,format!("bytes={existing}-"));}response=next.send().await.map_err(|_|StoreError::Network)?;}else{break}}if !response.status().is_success(){return Err(StoreError::TransferFailed)}let append=existing>0&&response.status()==StatusCode::PARTIAL_CONTENT;let response_length=response.headers().get(CONTENT_LENGTH).and_then(|v|v.to_str().ok()).and_then(|v|v.parse::<u64>().ok());let mut f=if append{fs::OpenOptions::new().append(true).open(&partial).await?}else{fs::File::create(&partial).await?};let mut stream=response.bytes_stream();let mut received=0u64;while let Some(chunk)=stream.next().await{if cancel.is_cancelled(){summary.cancelled=true;break}let chunk=chunk.map_err(|_|StoreError::TransferFailed)?;received+=chunk.len() as u64;f.write_all(&chunk).await?;}f.flush().await?;if summary.cancelled{break}if response_length.is_some_and(|n|n!=received){return Err(StoreError::Integrity)}let size=fs::metadata(&partial).await?.len();if file.expected_size.is_some_and(|n|n!=size){return Err(StoreError::Integrity)}let output=dest.join(&file.name);fs::rename(partial,output).await?;summary.files_completed+=1;summary.bytes_downloaded+=received;progress(Progress{bytes_done:summary.bytes_downloaded,bytes_total:plan.title.size_bytes,files_done:i as u64+1,files_total:total,..Progress::default()});}Ok(summary)})}
 fn has_update<'a>(&'a self,id:&'a TitleId,installed_dir:&'a Path,_options:PlanOptions)->BoxFuture<'a,bool>{Box::pin(async move{let work=self.find(id).await?;let metadata=self.http.metadata(&work.id).await?;let current=metadata.and_then(|m|m.update_date).or(work.upgrade_date);let installed=fs::read_to_string(installed_dir.join(".dlsite-version")).await.ok();Ok(update_available(current.as_deref(),installed.as_deref()))})}
 fn logout<'a>(&'a self,secrets:&'a dyn SecretStore)->BoxFuture<'a,()>{Box::pin(async move{secrets.clear(ID).map_err(|_|StoreError::SecretStore)?;Ok(())})}
}

#[cfg(test)]mod tests{use super::*;
#[test]fn count_sales_and_works_real_shapes_decode(){let c:Count=serde_json::from_str(r#"{"user":2,"production":2,"page_limit":50,"concurrency":500}"#).unwrap();let s:Vec<Sale>=serde_json::from_str(r#"[{"workno":"RJ123456","sales_date":"2024-01-01T00:00:00Z"}]"#).unwrap();let w:Works=serde_json::from_str(r#"{"works":[{"workno":"RJ123456","name":{"ja_JP":"作品"},"maker":{"id":"RG1","name":{"ja_JP":"Circle"}},"work_type":"SOU","age_category":"r18","work_files":{},"upgrade_date":"2025-01-01"}]}"#).unwrap();assert_eq!(c.user,2);assert_eq!(s[0].workno,"RJ123456");assert_eq!(w.works[0].id,"RJ123456");}
#[test]fn split_part_order_is_numeric(){let mut urls=vec!["https://www.dlsite.com/home/download/split/number/10/product_id/RJ1".to_owned(),"https://www.dlsite.com/home/download/split/number/2/product_id/RJ1".to_owned()];urls.sort_by(|a,b|natural_cmp(a,b));assert!(urls[0].contains("/number/2/"));}
#[test]fn update_needs_changed_store_timestamp(){assert!(update_available(Some("2025-02"),Some("2024-01")));assert!(!update_available(Some("2024-01"),Some("2024-01")));assert!(!update_available(Some("2025-02"),None));}
#[test]fn fixed_error_display_does_not_echo_secrets(){let e=StoreError::TransferFailed;assert!(!e.to_string().contains("cookie"));assert!(!e.to_string().contains("token"));}
}
