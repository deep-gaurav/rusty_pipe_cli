use downloader::IncomingTask;
use downloader::Reply;
use rusty_pipe::downloader_trait::Downloader;
use rusty_pipe::youtube_extractor::search_extractor::*;
use rusty_pipe::youtube_extractor::stream_extractor::*;
use std::io;
use std::io::BufReader;
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use surf::Response;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::conv::IntoSample;
use symphonia::core::io::MediaSource;

use async_std::prelude::*;
use async_trait::async_trait;
use failure::Error;
use rusty_pipe::youtube_extractor::error::ParsingError;
use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::str::FromStr;
use urlencoding::encode;

use crate::decode_m4a::decode_file;
use crate::downloader::DownloaderS;

mod decode_m4a;
mod downloader;
mod output;
mod player;

fn main() -> Result<(), Error> {
    pretty_env_logger::init();
    use async_std::task;
    let (txdsend, rxdsend) = std::sync::mpsc::channel();
    let (txdrecv, rxdrecv) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut down = DownloaderS::new(rxdsend, txdrecv);
        async_std::task::block_on(async move {
            down.run().await;
        })
    });
    task::block_on(async { async_main(txdsend, rxdrecv).await });
    Ok(())
}

async fn async_main(
    down_sender: Sender<IncomingTask>,
    down_rcv: Receiver<Reply>,
) -> Result<(), Error> {
    let mut search_query = String::new();
    println!("Enter Search Query: ");
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
        let mut stream_extractor = YTStreamExtractor::new(&video_id, downloader).await?;
        let audio_streams = stream_extractor.get_audio_streams()?;
        let stream_info = audio_streams
            .iter()
            .filter(|f| f.mimeType.contains("mp4"))
            .nth(0)
            .expect("No mpeg4 stream");
        let url = stream_info.url.clone().expect("No url in stream");

        println!("Downloading stream url {}", url);
        let response = surf::get(&url).send().await.unwrap();
        let length = response.len();
        println!("Downloading");
        // let mut d = vec![];
        // StreamResponse {
        //     url: url.clone(),
        //     current_position: 0,
        //     down_rcv,
        //     down_sender,
        //     total_length: length,
        //     response: surf::get(&url).send().await.expect("response fail"),
        // }
        // .read_to_end(&mut d)
        // .expect("Cant read to end");
        // println!("Writing");
        // std::fs::write("testdata.m4a", d).expect("Cant write to testdata");
        let decoded_data = decode_m4a::decode(StreamResponse {
            url,
            current_position: 0,
            down_rcv,
            down_sender,
            total_length: length,
        });
        // let decoded_data =
        //     decode_file(std::fs::File::open("videoplayback.m4a").expect("Cant open file"));
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

pub struct StreamResponse {
    url: String,
    current_position: usize,
    down_sender: Sender<IncomingTask>,
    down_rcv: Receiver<Reply>,
    total_length: Option<usize>,
}

impl Read for StreamResponse {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let task = IncomingTask {
            url: self.url.to_string(),
            pos: self.current_position,
            buff: buf.len(),
        };
        self.down_sender
            .send(task.clone())
            .expect("Cant send to downloader");
        // log::info!("Download with downloader size {}", buf.len());
        let mut data = loop {
            let reply = self.down_rcv.recv().expect("Cant receive from downloader");
            if reply.task == task {
                break reply;
            }
        };
        // log::info!("downloaded data len {}", data.data.len());
        self.current_position += data.data.len();
        if data.data.len() == 0 {
            return Ok(0);
        }
        buf[..data.data.len()].copy_from_slice(&data.data[..]);
        // async_std::task::block_on(async { self.response.read(buf).await })
        Ok(data.data.len())
    }
}

impl Seek for StreamResponse {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        log::info!("Seek requested {:#?}", pos);
        log::info!("Current pos {}", self.current_position);
        let new_pos = match pos {
            SeekFrom::Start(pos) => {
                self.current_position = pos as usize;
                Ok(self.current_position as u64)
            }
            SeekFrom::End(pos) => {
                if let Some(total_length) = self.total_length {
                    self.current_position = total_length - pos as usize;
                }
                Ok(self.current_position as u64)
            }
            SeekFrom::Current(pos) => {
                let new_pos = self.current_position as i64 + pos;
                if new_pos > 0 {
                    self.current_position = new_pos as usize;
                } else {
                    self.current_position = 0;
                }
                Ok(self.current_position as u64)
            }
        };
        log::info!("new pos {:#?}", new_pos);
        new_pos
    }
}

impl MediaSource for StreamResponse {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.total_length.map(|f| f as u64)
    }
}

struct DownloaderExample {}

#[async_trait]
impl Downloader for DownloaderExample {
    async fn download(url: &str) -> Result<String, ParsingError> {
        // println!("query url : {}", url);
        let mut resp = surf::get(url)
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
        let client = surf::client();
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
