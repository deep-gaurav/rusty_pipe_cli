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
use crate::yt_downloader::YTDownloader;

mod cli_ui;
mod decode_m4a;
mod downloader;
mod output;
mod player;
mod r_player;
mod server;
mod yt_downloader;

fn main() -> Result<(), Error> {
    pretty_env_logger::init();

    async_std::task::block_on(async {
        let (tx1, rx1) = futures::channel::mpsc::channel(2);
        let (tx2, rx2) = futures::channel::mpsc::channel(2);

        let server_fut = server::run_server(
            rx1,
            tx2,
            std::env::var("PORT")
                .unwrap_or("3337".to_string())
                .parse()
                .unwrap_or(3337),
        );
        let player_fut = r_player::run_audio_player(rx2, tx1);
        futures::join!(server_fut, player_fut);
    });

    Ok(())
}

pub struct StreamResponse {
    url: String,
    current_position: usize,
    down_sender: crossbeam_channel::Sender<IncomingTask>,
    down_rcv: crossbeam_channel::Receiver<Reply>,
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
