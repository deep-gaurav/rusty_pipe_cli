use std::{collections::HashMap, fmt::format};

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
        log::info!("Download url");
        let mut resp = SURF_CLIENT
            .get(url)
            .await
            .map_err(|er| ParsingError::DownloadError {
                cause: er.to_string(),
            })?;
        // println!("got response ");
        log::info!("Response Headers {:#?}",resp.header_values());

        let body = resp
            .body_string()
            .await
            .map_err(|er| ParsingError::DownloadError {
                cause: er.to_string(),
            })?;
        // println!("suceess query");
        log::info!("Download complete");
        Ok(String::from(body))
    }

    async fn download_with_header(
        url: &str,
        header: HashMap<String, String>,
    ) -> Result<String, ParsingError> {
        log::info!("Download url with header");
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
        log::info!("Response Headers {:#?}",res.header_values());
        let body = res.body_string().await.map_err(|er| er.to_string())?;
        log::info!("Download Complete");

        Ok(String::from(body))
    }

    fn eval_js(script: &str) -> Result<String, String> {
        log::info!("Eval js {}",script);
        let mut context = quick_js::Context::new().expect("Cant create quick-js context");
        log::debug!("run script {}", script);
        let res = context.eval(script).map_err(|er| format!("{:#?}", er));
        
        log::info!("Eval complete");
        let res = match res {
            Ok(res) => res,
            Err(err) => {
                log::error!("Failed {:#?}", err);
                return Err(err);
            }
        };
        let result = res.into_string().ok_or("Output not string".to_string());
        let result = match result {
            Ok(result) => result,
            Err(err) => {
                log::error!("Error {:#?}", err);
                return Err(err);
            }
        };
        let result = result.to_string();
        // print!("JS result: {}", result);
        log::info!("Eval coml js complete");
        Ok(result)
    }
}
