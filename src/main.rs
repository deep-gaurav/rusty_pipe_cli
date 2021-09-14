use rusty_pipe::downloader_trait::Downloader;
use rusty_pipe::youtube_extractor::search_extractor::*;
use rusty_pipe::youtube_extractor::stream_extractor::*;
use std::io;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::conv::IntoSample;

use async_trait::async_trait;
use failure::Error;
use rusty_pipe::youtube_extractor::error::ParsingError;
use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::str::FromStr;
use urlencoding::encode;

mod decode_m4a;
mod output;
mod player;
#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut search_query = String::new();
    println!("Enter Search Query");
    io::stdin()
        .read_line(&mut search_query)
        .expect("Cannot Read Input");

    search_query = encode(&search_query);

    let search_extractor = YTSearchExtractor::new::<DownloaderExample>(&search_query, None).await?;
    let mut items = search_extractor.search_results()?;
    let first_item = items
        .iter()
        .filter(|ite| match ite {
            YTSearchItem::StreamInfoItem(_) => true,
            YTSearchItem::ChannelInfoItem(_) => false,
            YTSearchItem::PlaylistInfoItem(_) => false,
        })
        .nth(0)
        .expect("No stream found for query");
    if let YTSearchItem::StreamInfoItem(stream_info) = first_item {
        let video_id = stream_info.video_id()?;
        println!("Downloading id {}", stream_info.video_id()?);
        println!("Name {}", stream_info.get_name()?);
        let downloader = DownloaderExample {};

        println!("Extracting Stream");
        let mut stream_extractor = YTStreamExtractor::new("SAdQnnMI05U", downloader).await?;
        let audio_streams = stream_extractor.get_audio_streams()?;
        let stream_info = audio_streams
            .iter()
            .filter(|f| f.mimeType.contains("mp4"))
            .nth(0)
            .expect("No mpeg4 stream");
        let url = stream_info.url.clone().expect("No url in stream");

        println!("Downloading stream");
        let body = reqwest::get(url).await?.bytes().await?;
        use bytes::{Buf, Bytes};
        use std::io::Cursor;
        let len = body.len();
        println!("Decoding stream len {}", len);
        let mut c = Cursor::new(Vec::new());
        c.write_all(&body).expect("Cant write all");
        c.seek(SeekFrom::Start(0)).expect("Cant seek to 0");
        // let bf = BufReader::new(inner)
        let mut out = Vec::new();
        c.read_to_end(&mut out).expect("Cant read");
        let f = std::fs::write("data.m4a", out).expect("Cant write");
        c.seek(SeekFrom::Start(0)).expect("Cant seek to 0");

        let decoded_data = decode_m4a::decode(c);
        player::play(
            decoded_data,
            None,
            None,
            &DecoderOptions { verify: false },
            false,
        )
        .expect("Cant play");
    }
    Ok(())
}

struct DownloaderExample {}

#[async_trait]
impl Downloader for DownloaderExample {
    async fn download(url: &str) -> Result<String, ParsingError> {
        // println!("query url : {}", url);
        let resp = reqwest::get(url)
            .await
            .map_err(|er| ParsingError::DownloadError {
                cause: er.to_string(),
            })?;
        // println!("got response ");
        let body = resp
            .text()
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
        let client = reqwest::Client::new();
        let res = client.get(url);
        let mut headers = reqwest::header::HeaderMap::new();
        for header in header {
            headers.insert(
                reqwest::header::HeaderName::from_str(&header.0).map_err(|e| e.to_string())?,
                header.1.parse().unwrap(),
            );
        }
        let res = res.headers(headers);
        let res = res.send().await.map_err(|er| er.to_string())?;
        let body = res.text().await.map_err(|er| er.to_string())?;
        Ok(String::from(body))
    }

    fn eval_js(script: &str) -> Result<String, String> {
        use quick_js::{Context, JsValue};
        let context = Context::new().expect("Cant create js context");
        // println!("decryption code \n{}",decryption_code);
        // println!("signature : {}",encrypted_sig);
        // println!("jscode \n{}", script);
        let res = context.eval(script).unwrap_or(quick_js::JsValue::Null);
        // println!("js result : {:?}", result);
        let result = res.into_string().unwrap_or("".to_string());
        // print!("JS result: {}", result);
        Ok(result)
    }
}
