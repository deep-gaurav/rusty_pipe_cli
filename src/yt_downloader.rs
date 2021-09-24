use std::collections::HashMap;

use async_trait::async_trait;
use rusty_pipe::{downloader_trait::Downloader, youtube_extractor::error::ParsingError};
use surf::Client;
lazy_static::lazy_static! {
    static ref SURF_CLIENT:surf::Client = surf::Client::new();
}

pub struct YTDownloader {}

#[async_trait]
impl Downloader for YTDownloader {
    async fn download(url: &str) -> Result<String, ParsingError> {
        // println!("query url : {}", url);
        let mut resp = SURF_CLIENT
            .get(url)
            .await
            .map_err(|er| ParsingError::DownloadError {
                cause: er.to_string(),
            })?;
        // println!("got response ");
        let body = resp
            .body_string()
            .await
            .map_err(|er| ParsingError::DownloadError {
                cause: er.to_string(),
            })?;
        // println!("suceess query");
        Ok(String::from(body))
    }

    async fn download_with_header(
        url: &str,
        header: HashMap<String, String>,
    ) -> Result<String, ParsingError> {
        let client = &SURF_CLIENT;
        let mut res = client.get(url);
        // let mut headers = reqwest::header::HeaderMap::new();
        for header in header {
            res = res.header(header.0.as_str(), header.1.as_str())
            // headers.insert(

            //     reqwest::header::HeaderName::from_str(&header.0).map_err(|e| e.to_string())?,
            //     header.1.parse().unwrap(),
            // );
        }
        // res.header(key, value)
        // let res = res.headers(headers);
        let mut res = res.send().await.map_err(|er| er.to_string())?;
        let body = res.body_string().await.map_err(|er| er.to_string())?;
        Ok(String::from(body))
    }

    fn eval_js(script: &str) -> Result<String, String> {
        let mut context = boa::Context::default();
        let res = context.eval(script).expect("JS Failed");
        let result = res
            .as_string()
            .ok_or("Output not string".to_string())?
            .to_string();
        // print!("JS result: {}", result);
        Ok(result)
    }
}
