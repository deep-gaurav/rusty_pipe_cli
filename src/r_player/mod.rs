use std::sync::{Arc, Mutex};

use async_std::{future, prelude::*};
use futures::{
    channel::mpsc::{Receiver, Sender},
    SinkExt, StreamExt,
};
use rusty_pipe::youtube_extractor::stream_extractor::YTStreamExtractor;
use symphonia::core::{
    codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL},
    formats::{FormatReader, Packet, SeekMode, SeekTo, Track},
    units::Time,
};

use crate::{
    downloader::DownloaderS,
    output::AudioOutput,
    player::{print_progress, print_update, PlayTrackOptions},
    server::schema::{PlayerMessage, PlayerStatus, ToPlayerMessages},
    yt_downloader::YTDownloader,
    StreamResponse,
};

pub async fn run_audio_player(
    mut msg_receiver: Receiver<ToPlayerMessages>,
    msg_sender: Sender<PlayerMessage>,
) {
    let pending_player_messages = Arc::new(Mutex::new(vec![]));
    let pending_messages_c = pending_player_messages.clone();

    let player_fut = async_std::task::spawn_blocking(move || {
        async_std::task::block_on(async { play_audio(pending_player_messages, msg_sender).await })
    });

    let messages_fut = async {
        while let Some(msg) = msg_receiver.next().await {
            match pending_messages_c.lock() {
                Ok(mut msgs) => msgs.push(msg),
                Err(err) => log::warn!("{:#?}", err),
            }
        }
    };
    futures::join!(player_fut, messages_fut);
}

async fn play_audio(
    messages: Arc<Mutex<Vec<ToPlayerMessages>>>,
    mut msg_sender: Sender<PlayerMessage>,
) {
    let (txdsend, rxdsend) = crossbeam_channel::unbounded();
    let (txdrecv, rxdrecv) = crossbeam_channel::unbounded();
    let mut down = DownloaderS::new(rxdsend, txdrecv);

    let down_loader_task = async move {
        down.run().await;
    };

    let player_task = async {
        log::info!("Player started");
        async_std::task::spawn_blocking(move || {
            async_std::task::block_on(async {
                let mut playing_data: Option<PlayingData> = None;
                let mut last_sent = None;

                loop {
                    // log::info!("Get play msg");
                    let msg = {
                        match messages.lock() {
                            Ok(mut msgs) => {
                                if let Some(msg) = msgs.get(0) {
                                    let msg = msg.clone();
                                    msgs.remove(0);
                                    Some(msg)
                                } else {
                                    None
                                }
                            }
                            Err(err) => {
                                log::warn!("Error err {:#?}", err);
                                log::warn!("Ending");
                                return;
                            }
                        }
                    };
                    // log::info!("Got msg {:#?}", msg);
                    if let Some(msg) = msg {
                        match msg {
                            ToPlayerMessages::Play(options) => {
                                if let Some(playing_data) = &mut playing_data {
                                    if &playing_data.url == &options.url {
                                        playing_data.is_playing = true;
                                        continue;
                                    }
                                }
                                let new_playing_data = create_new_player(
                                    &options.url,
                                    &rxdrecv,
                                    &txdsend,
                                    options.length,
                                    options.file_path,
                                    options.video_id,
                                    playing_data.and_then(|d| d.audio_output),
                                );

                                // Decode the packet into audio samples.
                                playing_data = new_playing_data;
                            }
                            ToPlayerMessages::Pause => {
                                if let Some(playing_data) = &mut playing_data {
                                    playing_data.is_playing = false;
                                }
                            }
                            ToPlayerMessages::Seek(secs) => {
                                if let Some(playing_data) = &mut playing_data {
                                    if let Some(tb) = &playing_data.tb {
                                        if let Some(packet) = &playing_data.last_packet {
                                            let t = tb.calc_time(packet.pts()).seconds;
                                            let nt = Time::new((t as i64 + secs) as u64, 0.0);
                                            playing_data.reader.seek(
                                                SeekMode::Accurate,
                                                SeekTo::Time {
                                                    time: nt,
                                                    track_id: None,
                                                },
                                            );
                                        }
                                    }
                                }
                            }
                            ToPlayerMessages::Resume => {
                                if let Some(pd) = &mut playing_data {
                                    pd.is_playing = true;
                                }
                            }
                        }
                    }

                    if let Some(playing_data) = &mut playing_data {
                        if playing_data.is_playing {
                            // log::info!("playing data is play");
                            log::debug!("Trying to play");
                            playing_data.play();
                            log::debug!("Play done");
                            let to_send = PlayerMessage::Status(PlayerStatus {
                                playing: true,
                                current_status: {
                                    if let Some(tb) = playing_data.tb {
                                        if let Some(packet) = &playing_data.last_packet {
                                            let t = tb.calc_time(packet.pts()).seconds;
                                            Some(t)
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                },
                                total_time: {
                                    if let Some(tb) = playing_data.tb {
                                        if let Some(packet) = &playing_data.last_packet {
                                            if let Some(dur) = playing_data.dur {
                                                let d = tb.calc_time(dur);
                                                Some(d.seconds)
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                },
                            });
                            // log::info!("playing frame ended");

                            if last_sent != Some(to_send.clone()) {
                                let err = msg_sender.send(to_send.clone()).await;
                                last_sent = Some(to_send);
                            }
                        } else {
                            let to_send = PlayerMessage::Status(PlayerStatus {
                                playing: false,
                                current_status: {
                                    if let Some(tb) = playing_data.tb {
                                        if let Some(packet) = &playing_data.last_packet {
                                            let t = tb.calc_time(packet.pts()).seconds;
                                            Some(t)
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                },
                                total_time: {
                                    if let Some(tb) = playing_data.tb {
                                        if let Some(packet) = &playing_data.last_packet {
                                            if let Some(dur) = playing_data.dur {
                                                let d = tb.calc_time(dur);
                                                Some(d.seconds)
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                },
                            });
                            if last_sent != Some(to_send.clone()) {
                                let err = msg_sender.send(to_send.clone()).await;
                                last_sent = Some(to_send);
                            }
                        }
                    }
                    match &playing_data {
                        Some(pd) => {
                            if !pd.is_playing {
                                async_std::task::sleep(std::time::Duration::from_millis(50)).await;
                            } else if let Some(packe) = &pd.last_packet {
                                if packe.pts() >= pd.dur.unwrap_or(std::u64::MAX) {
                                    async_std::task::sleep(std::time::Duration::from_millis(50))
                                        .await;
                                }
                            }
                        }
                        None => {
                            async_std::task::sleep(std::time::Duration::from_millis(50)).await;
                        }
                    }
                }
            })
        })
        .await
    };

    futures::join!(player_task, down_loader_task);
}

fn create_new_player(
    url: &String,
    rxdrecv: &crossbeam_channel::Receiver<crate::downloader::Reply>,
    txdsend: &crossbeam_channel::Sender<crate::downloader::DownloaderInput>,
    length: Option<usize>,
    file_path: Option<String>,
    video_id: String,
    audio_output: Option<Box<dyn AudioOutput>>,
) -> Option<PlayingData> {
    log::info!("Decoding stream");
    let decoded_data = crate::decode_m4a::decode(StreamResponse {
        url: url.clone(),
        current_position: 0,
        down_rcv: rxdrecv.clone(),
        down_sender: txdsend.clone(),
        total_length: length,
        video_id,
        file_name: file_path,
    });
    log::info!("Decoded stream");
    let mut reader = decoded_data;
    let track_num: Option<usize> = None;
    let seek_time: Option<f64> = None;
    let decode_opts = &DecoderOptions { verify: false };
    let mut no_progress = true;
    let track = track_num
        .and_then(|t| reader.tracks().get(t))
        .or_else(|| first_supported_track(reader.tracks()));
    let mut track_id = match track {
        Some(track) => track.id,
        _ => {
            log::warn!("No tracks found");
            return None;
        }
    };
    let seek_ts = if let Some(time) = seek_time {
        let seek_to = SeekTo::Time {
            time: Time::from(time),
            track_id: Some(track_id),
        };

        // Attempt the seek. If the seek fails, ignore the error and return a seek timestamp of 0 so
        // that no samples are trimmed.
        match reader.seek(SeekMode::Accurate, seek_to) {
            Ok(seeked_to) => seeked_to.required_ts,
            Err(symphonia::core::errors::Error::ResetRequired) => {
                // print_tracks(reader.tracks());
                track_id = first_supported_track(reader.tracks()).unwrap().id;
                0
            }
            Err(err) => {
                // Don't give-up on a seek error.
                log::warn!("seek error: {}", err);
                0
            }
        }
    } else {
        // If not seeking, the seek timestamp is 0.
        0
    };
    // let mut audio_output = None;
    let mut track_info = PlayTrackOptions { track_id, seek_ts };
    let mut play_opts = track_info;
    let track = match reader
        .tracks()
        .iter()
        .find(|track| track.id == play_opts.track_id)
    {
        Some(track) => track,
        _ => return None,
    };
    let mut decoder = match symphonia::default::get_codecs().make(&track.codec_params, decode_opts)
    {
        Ok(val) => val,
        Err(err) => {
            log::warn!("Decoder error {:#?}", err);
            return None;
        }
    };
    let mut tb = track.codec_params.time_base;
    let mut dur = track
        .codec_params
        .n_frames
        .map(|frames| track.codec_params.start_ts + frames);

    log::info!("Player Created");
    Some(PlayingData {
        decoder,
        reader,
        // packet,
        audio_output,
        play_opts,
        no_progress,
        dur,
        tb,
        is_playing: true,
        url: url.to_string(),
        last_packet: None,
    })
}

struct PlayingData {
    decoder: Box<dyn Decoder>,
    reader: Box<dyn FormatReader>,
    // packet: symphonia::core::formats::Packet,
    audio_output: Option<Box<dyn AudioOutput>>,
    play_opts: PlayTrackOptions,
    no_progress: bool,
    dur: Option<u64>,
    tb: Option<symphonia::core::units::TimeBase>,

    is_playing: bool,
    url: String,
    last_packet: Option<Packet>,
}
impl PlayingData {
    fn play(&mut self) -> Result<(), symphonia::core::errors::Error> {
        // log::info!("Play");
        let decoder = &mut self.decoder;
        // let packet = &mut self.packet;
        let audio_output = &mut self.audio_output;
        let play_opts = &mut self.play_opts;
        let no_progress = &mut self.no_progress;
        let dur = &mut self.dur;
        let tb = &mut self.tb;

        log::debug!("Trying to get packet");
        let packet = match self.reader.next_packet() {
            Ok(packet) => {
                log::debug!("Packet received");
                packet
            }
            Err(err) => {
                log::warn!("Cant get packet");
                return Err(err);
            }
        };

        log::debug!("Check player track");
        // If the packet does not belong to the selected track, skip it.
        if packet.track_id() != play_opts.track_id {
            return Ok(());
        }

        log::debug!("Get metadata");
        //Print out new metadata.
        while !self.reader.metadata().is_latest() {
            self.reader.metadata().pop();

            if let Some(rev) = self.reader.metadata().current() {
                print_update(rev);
            }
        }
        log::debug!("Decode packet");
        let r = match decoder.decode(&packet) {
            Ok(decoded) => {
                log::debug!("Decoded packet");
                // If the audio output is not open, try to open it.
                if audio_output.is_none() {
                    log::debug!("Create output");
                    // Get the audio buffer specification. This is a description of the decoded
                    // audio buffer's sample format and sample rate.
                    let spec = *decoded.spec();

                    // Get the capacity of the decoded buffer. Note that this is capacity, not
                    // length! The capacity of the decoded buffer is constant for the life of the
                    // decoder, but the length is not.
                    let duration = decoded.capacity() as u64;

                    // Try to open the audio output.
                    log::debug!("Try open cpal");
                    audio_output.replace(super::output::try_open(spec, duration).unwrap());
                    log::debug!("Cpal opened");
                } else {
                    // TODO: Check the audio spec. and duration hasn't changed.
                }

                // Write the decoded audio samples to the audio output if the presentation timestamp
                // for the packet is >= the seeked position (0 if not seeking).
                if packet.pts() >= play_opts.seek_ts {
                    // if let Some(tb) = tb {
                    //     let t = tb.calc_time(packet.pts()).seconds;
                    //     // if t > 50 {
                    //     //     reader
                    //     //         .seek(
                    //     //             SeekMode::Accurate,
                    //     //             SeekTo::Time {
                    //     //                 time: Time::from_ss(10, 0).unwrap(),
                    //     //                 track_id: None,
                    //     //             },
                    //     //         )
                    //     //         .expect("Cant Seek");
                    //     // } else if t > 30 && t < 35 {
                    //     //     reader
                    //     //         .seek(
                    //     //             SeekMode::Coarse,
                    //     //             SeekTo::Time {
                    //     //                 time: Time::from_ss(40, 0).unwrap(),
                    //     //                 track_id: None,
                    //     //             },
                    //     //         )
                    //     //         .expect("Cant seek");
                    //     // } else
                    //     if t > 1 && t < 50 {
                    //         reader
                    //             .seek(
                    //                 SeekMode::Accurate,
                    //                 SeekTo::Time {
                    //                     time: Time::from_mmss(2, 30, 0).unwrap(),
                    //                     track_id: None,
                    //                 },
                    //             )
                    //             .expect("Can seek");
                    //     }
                    // }
                    if !*no_progress {
                        print_progress(packet.pts(), *dur, *tb);
                    }

                    if let Some(audio_output) = audio_output {
                        // log::info!("Audio output wrting");
                        audio_output.write(decoded).unwrap()
                    } else {
                        log::warn!("No audio output");
                    }
                }
                // log::info!("next frame");
                Ok(())
            }
            Err(symphonia::core::errors::Error::DecodeError(err)) => {
                // Decode errors are not fatal. Print the error message and try to decode the next
                // packet as usual.
                log::warn!("decode error: {}", err);
                Ok(())
            }
            Err(err) => {
                log::warn!("{:#?}", err);
                Err(err)
            }
        };

        self.last_packet = Some(packet);
        r
    }
}

fn first_supported_track(tracks: &[Track]) -> Option<&Track> {
    tracks
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
}
