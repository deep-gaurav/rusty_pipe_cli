
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

use rusty_pipe_cli::downloader::IncomingTask;
use rusty_pipe_cli::downloader::Reply;
use rusty_pipe_cli::decode_m4a::decode_file;
use rusty_pipe_cli::downloader::DownloaderS;
use rusty_pipe_cli::yt_downloader::YTDownloader;

fn main() -> Result<(), Error> {
    pretty_env_logger::init();

    let mut args = std::env::args();
    if args.any(|arg|arg.contains("server")){
        let port = rusty_pipe_cli::get_unused_port().expect("Not available port");
        println!("Server started on port {}",port);
        rusty_pipe_cli::run_server(port);
    }else{


        async_std::task::block_on(async {
            let (tx1, rx1) = futures::channel::mpsc::channel(2);
            let (tx2, rx2) = futures::channel::mpsc::channel(2);
    
            // let server_fut = server::run_server(
            //     rx1,
            //     tx2,
            //     std::env::var("PORT")
            //         .unwrap_or("3337".to_string())
            //         .parse()
            //         .unwrap_or(3337),
            // );
            let cli_fut = rusty_pipe_cli::cli_ui::run_tui_pipe(rx1, tx2);
            let player_fut = rusty_pipe_cli::r_player::run_audio_player(rx2, tx1);
            futures::join!(cli_fut, player_fut);
        });
    }

    Ok(())
}
