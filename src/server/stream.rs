use async_graphql::*;
use rusty_pipe::{
    utils::utils::fix_thumbnail_url,
    youtube_extractor::{search_extractor::YTSearchItem, stream_extractor::YTStreamExtractor},
};
use serde::{Deserialize, Serialize};

use crate::{
    server::search::{ChannelResult, PlaylistResult, VideoResult},
    yt_downloader::YTDownloader,
};

use super::search::{SearchResult, Thumbnail};

pub struct Video {
    pub extractor: YTStreamExtractor<YTDownloader>,
}

#[Object]
impl Video {
    async fn video_streams(&self) -> Result<Vec<StreamItem>, Error> {
        let streams = self.extractor.get_video_stream()?;
        let mut v = vec![];
        for stream in streams {
            let stream_str = serde_json::to_string(&stream)?;
            v.push(serde_json::from_str(&stream_str)?);
        }
        Ok(v)
    }
    async fn video_only_streams(&self) -> Result<Vec<StreamItem>, Error> {
        let streams = self.extractor.get_video_only_stream()?;
        let mut v = vec![];
        for stream in streams {
            let stream_str = serde_json::to_string(&stream)?;
            v.push(serde_json::from_str(&stream_str)?);
        }
        Ok(v)
    }
    async fn audio_only_streams(&self) -> Result<Vec<StreamItem>, Error> {
        let streams = self.extractor.get_audio_streams()?;
        let mut v = vec![];
        for stream in streams {
            let stream_str = serde_json::to_string(&stream)?;
            v.push(serde_json::from_str(&stream_str)?);
        }
        Ok(v)
    }

    async fn title(&self) -> Result<String, Error> {
        Ok(self.extractor.get_name()?)
    }

    async fn description(&self) -> Result<String, Error> {
        Ok(self.extractor.get_description(false)?.0)
    }

    async fn uploader_name(&self) -> Result<String, Error> {
        Ok(self.extractor.get_uploader_name()?)
    }

    async fn uploader_url(&self) -> Result<String, Error> {
        Ok(self.extractor.get_uploader_url()?)
    }

    async fn video_thumbnails(&self) -> Result<Vec<Thumbnail>, Error> {
        let thumbs = self.extractor.get_video_thumbnails()?;
        let mut thumbf = vec![];
        for thumb in thumbs {
            thumbf.push(Thumbnail {
                url: fix_thumbnail_url(&thumb.url),
                height: thumb.height as i32,
                width: thumb.width as i32,
            })
        }
        Ok(thumbf)
    }

    async fn uploader_thumbnails(&self) -> Result<Vec<Thumbnail>, Error> {
        let thumbs = self.extractor.get_uploader_avatar_url()?;
        let mut thumbf = vec![];
        for thumb in thumbs {
            thumbf.push(Thumbnail {
                url: fix_thumbnail_url(&thumb.url),
                height: thumb.height as i32,
                width: thumb.width as i32,
            })
        }
        Ok(thumbf)
    }

    async fn likes(&self) -> Result<i32, Error> {
        Ok(self.extractor.get_like_count()? as i32)
    }

    async fn dislikes(&self) -> Result<i32, Error> {
        Ok(self.extractor.get_dislike_count()? as i32)
    }

    async fn views(&self) -> Result<i32, Error> {
        Ok(self.extractor.get_view_count()? as i32)
    }

    async fn length(&self) -> Result<i32, Error> {
        Ok(self.extractor.get_length()? as i32)
    }

    async fn related(&self) -> Result<Vec<SearchResult>, Error> {
        let mut result = vec![];
        for item in self.extractor.get_related()? {
            result.push(match item {
                YTSearchItem::StreamInfoItem(vid) => SearchResult::VideoInfo(VideoResult {
                    name: vid.get_name()?,
                    video_id: vid.video_id()?,
                    is_ad: vid.is_ad().unwrap_or(false),
                    is_premium_video: vid.is_premium_video().unwrap_or(false),
                    url: vid.get_url()?,
                    is_live: vid.is_live().unwrap_or(false),
                    duration: vid.get_duration().ok(),
                    uploader_name: vid.get_uploader_name().ok(),
                    uploader_url: vid.get_uploader_url().ok(),
                    upload_date: vid.get_textual_upload_date().ok(),
                    view_count: vid.get_view_count().ok(),
                    thumbnail: vid
                        .get_thumbnails()?
                        .iter()
                        .map(|f| Thumbnail {
                            url: fix_thumbnail_url(&f.url),
                            width: f.width as i32,
                            height: f.height as i32,
                        })
                        .collect(),
                }),
                YTSearchItem::ChannelInfoItem(channel) => {
                    SearchResult::ChannelInfo(ChannelResult {
                        name: channel.get_name()?,
                        channel_id: channel.channel_id()?,
                        thumbnail: channel
                            .get_thumbnails()?
                            .iter()
                            .map(|f| Thumbnail {
                                url: fix_thumbnail_url(&f.url),
                                width: f.width as i32,
                                height: f.height as i32,
                            })
                            .collect(),
                        url: channel.get_url()?,
                        subscribers: channel.get_subscriber_count().ok(),
                        videos: channel.get_stream_count().ok(),
                        description: channel.get_description()?,
                    })
                }
                YTSearchItem::PlaylistInfoItem(playlist) => {
                    SearchResult::PlaylistInfo(PlaylistResult {
                        name: playlist.get_name()?,
                        playlist_id: playlist.playlist_id()?,
                        thumbnail: playlist
                            .get_thumbnails()?
                            .iter()
                            .map(|f| Thumbnail {
                                url: fix_thumbnail_url(&f.url),
                                width: f.width as i32,
                                height: f.height as i32,
                            })
                            .collect(),
                        url: playlist.get_url()?,
                        uploader_name: playlist.get_uploader_name().ok(),
                        videos: playlist.get_stream_count().ok(),
                    })
                }
            })
        }
        Ok(result)
    }
}

#[derive(SimpleObject, Serialize, Deserialize)]
pub struct StreamItem {
    pub url: String,
    pub itag: i32,
    pub approxDurationMs: Option<String>,
    pub audioChannels: Option<i32>,
    pub audioQuality: Option<String>,
    pub audioSampleRate: Option<String>,
    pub averageBitrate: Option<i32>,
    pub bitrate: i32,
    pub contentLength: Option<String>,
    pub height: Option<i32>,
    pub width: Option<i32>,
    pub quality: String,
    pub qualityLabel: Option<String>,
    pub lastModified: String,
    pub mimeType: String,
}
