use std::io::{self, Read, Seek, SeekFrom};

use downloader::{DownloaderInput, IncomingTask, Reply};
use symphonia::core::io::MediaSource;

use crate::yt_downloader::YTDownloader;

pub mod cli_ui;
pub mod decode_m4a;
pub mod downloader;
mod output;
mod player;
pub mod r_player;
mod server;
pub mod yt_downloader;

pub fn run_server(port: u16) {
    async_std::task::block_on(async {
        let (tx1, rx1) = futures::channel::mpsc::channel(2);
        let (tx2, rx2) = futures::channel::mpsc::channel(2);

        let server_fut = server::run_server(rx1, tx2, YTDownloader {  },port);
        // let cli_fut = crate::cli_ui::run_tui_pipe(rx1, tx2);
        let player_fut = crate::r_player::run_audio_player(rx2, tx1);
        futures::join!(server_fut, player_fut);
    });
}

pub fn get_unused_port() -> Option<u16> {
    let port = portpicker::pick_unused_port();
    port
}

pub struct StreamResponse {
    url: String,
    video_id: String,
    file_name: Option<String>,
    current_position: usize,
    down_sender: crossbeam_channel::Sender<DownloaderInput>,
    down_rcv: crossbeam_channel::Receiver<Reply>,
    total_length: Option<usize>,
}

impl Read for StreamResponse {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let task = IncomingTask {
            url: self.url.to_string(),
            pos: self.current_position,
            buff: buf.len(),
            video_id: self.video_id.clone(),
            file_path: self.file_name.clone(),
        };
        log::debug!("trying to send downnload task to downloader");
        self.down_sender
            .send(DownloaderInput::DownloadTask(task.clone()))
            .expect("Cant send to downloader");
        // log::info!("Download with downloader size {}", buf.len());
        let mut data = loop {
            log::debug!("trying to recceive data from downloader");
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
        log::debug!("Downloaded some bytes, sending to player to play");
        Ok(data.data.len())
    }
}

impl Drop for StreamResponse {
    fn drop(&mut self) {
        if let Err(err) = self
            .down_sender
            .send(DownloaderInput::RemoveDownload(self.video_id.clone()))
        {
            log::error!("Cant remove download {:#?}", err);
        }
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
