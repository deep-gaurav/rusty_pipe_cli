use std::sync::Arc;

use async_std::sync::Mutex;
use futures::{
    channel::mpsc::{Receiver, Sender},
    SinkExt, Stream,
};

use async_graphql::*;
use rusty_pipe::youtube_extractor::{
    search_extractor::YTSearchExtractor, stream_extractor::YTStreamExtractor,
};

use crate::yt_downloader::YTDownloader;

use super::{search::Search, stream::Video};

#[derive(Debug, Clone)]
pub enum ToPlayerMessages {
    Play(PlayOptions),
    Resume,
    Pause,
    Seek(i64),
}
#[derive(Debug, Clone)]
pub struct PlayOptions {
    pub video_id: String,
    pub url: String,
    pub length: Option<usize>,
    pub file_path: Option<String>,
}

#[derive(Union, PartialEq, Clone)]
pub enum PlayerMessage {
    Status(PlayerStatus),
}

#[derive(SimpleObject, PartialEq, Clone)]
pub struct PlayerStatus {
    pub playing: bool,
    pub current_status: Option<u64>,
    pub total_time: Option<u64>,
}

#[derive(Clone)]
pub struct Storage {
    pub to_player_message: Arc<Mutex<Sender<ToPlayerMessages>>>,
    pub from_player_message: Arc<Mutex<Option<Sender<PlayerMessage>>>>,
}

pub struct QueryRoot {}

#[Object]
impl QueryRoot {
    async fn video(&self, video_id: String) -> Result<Video, Error> {
        log::info!("Readying stream extractor");
        let ytextractor = YTStreamExtractor::new(&video_id, YTDownloader {}).await?;
        log::info!("Stream extractor read length, file_path: todo!() y");
        Ok(Video {
            extractor: ytextractor,
        })
    }

    async fn search(&self, query: String, page_url: Option<String>) -> Result<Search, Error> {
        let extractor = YTSearchExtractor::new::<YTDownloader>(&query, page_url).await?;
        Ok(Search { extractor })
    }

    async fn play<'ctx>(
        &self,
        ctx: &Context<'_>,
        video_id: String,
        url: String,
        file_path: Option<String>,
    ) -> Result<bool, Error> {
        log::info!("Get storage");
        let data = ctx.data::<Storage>()?;
        log::info!("Get length");
        let mut length = {
            if let Ok(url) = surf::Url::parse(&url) {
                let response = surf::get(&url).send().await;
                response.ok().and_then(|r| r.len())
            } else {
                None
            }
        };

        if length.is_none() {
            if let Some(file_path) = &file_path {
                if let Ok(file) = async_std::fs::File::open(&file_path).await {
                    if let Ok(metadata) = file.metadata().await {
                        length = Some(metadata.len() as usize);
                    }
                }
            }
        }

        log::info!("Try to lock to_player_msg");

        let mut to_player_msg = data.to_player_message.lock().await;
        log::info!("Locked player messages");
        to_player_msg
            .send(ToPlayerMessages::Play(PlayOptions {
                video_id,
                url,
                length,
                file_path,
            }))
            .await?;
        Ok(true)
    }

    async fn pause<'ctx>(&self, ctx: &Context<'_>) -> Result<bool, Error> {
        let data = ctx.data::<Storage>()?;
        let mut to_player_msg = data.to_player_message.lock().await;
        to_player_msg.send(ToPlayerMessages::Pause).await?;
        Ok(true)
    }
    async fn resume<'ctx>(&self, ctx: &Context<'_>) -> Result<bool, Error> {
        let data = ctx.data::<Storage>()?;
        let mut to_player_msg = data.to_player_message.lock().await;
        to_player_msg.send(ToPlayerMessages::Resume).await?;
        Ok(true)
    }

    async fn seek<'ctx>(&self, ctx: &Context<'_>, seconds: i64) -> Result<bool, Error> {
        let data = ctx.data::<Storage>()?;
        let mut to_player_msg = data.to_player_message.lock().await;
        to_player_msg.send(ToPlayerMessages::Seek(seconds)).await?;
        Ok(true)
    }
}

pub struct SubscriptionRoot {}

#[Subscription]
impl SubscriptionRoot {
    async fn player_messages<'ctx>(
        &self,
        ctx: &Context<'_>,
    ) -> Result<impl Stream<Item = PlayerMessage>, async_graphql::Error> {
        let (tx, rx) = futures::channel::mpsc::channel(2);
        {
            let data = ctx.data::<Storage>()?;
            let mut from_player_message = data.from_player_message.lock().await;
            *from_player_message = Some(tx);
        }
        Ok(rx)
    }
}
