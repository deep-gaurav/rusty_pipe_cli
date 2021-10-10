// Symphonia
// Copyright (c) 2019-2021 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Platform-dependant Audio Outputs

use std::result;

use symphonia::core::audio::{AudioBufferRef, SignalSpec};
use symphonia::core::units::Duration;

pub trait AudioOutput {
    fn write(&mut self, decoded: AudioBufferRef<'_>) -> Result<()>;
    fn flush(&mut self);
}

#[allow(dead_code)]
#[allow(clippy::enum_variant_names)]
#[derive(Debug)]
pub enum AudioOutputError {
    OpenStreamError,
    PlayStreamError,
    StreamClosedError,
}

pub type Result<T> = result::Result<T, AudioOutputError>;

mod cpal {
    use super::{AudioOutput, AudioOutputError, Result};

    use symphonia::core::audio::{AudioBufferRef, SampleBuffer, SignalSpec};
    use symphonia::core::conv::ConvertibleSample;
    use symphonia::core::units::Duration;

    use cpal::{self, SampleFormat};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use rb::*;

    use log::{debug, error};

    pub struct CpalAudioOutput;

    trait AudioOutputSample: cpal::Sample + ConvertibleSample + std::marker::Send + 'static {
        fn to_f32(&self) -> f32;
        fn from_f32(n: f32) -> Self;
    }

    impl AudioOutputSample for f32 {
        fn to_f32(&self) -> f32 {
            *self
        }

        fn from_f32(n: f32) -> Self {
            n
        }
    }
    impl AudioOutputSample for i16 {
        fn to_f32(&self) -> f32 {
            (*self) as f32
        }

        fn from_f32(n: f32) -> Self {
            n as i16
        }
    }
    impl AudioOutputSample for u16 {
        fn to_f32(&self) -> f32 {
            (*self) as f32
        }

        fn from_f32(n: f32) -> Self {
            n as u16
        }
    }

    impl CpalAudioOutput {
        pub fn try_open(spec: SignalSpec, duration: Duration) -> Result<Box<dyn AudioOutput>> {
            // Get default host.
            log::debug!("Get default host");
            let host = cpal::default_host();
            log::debug!("Host found");
            let default_output_device = host.default_output_device();
            log::debug!("Got default output device {:#?}", default_output_device.is_some());

            log::debug!("Total devices {:#?}",host.devices().map(|d|d.count()));
            log::debug!("Output devices {:#?}",host.output_devices().map(|d|d.count()));
            // Get the default audio output device.
            let devices = match host.output_devices() {
                Ok(device) => {
                    log::debug!("Got devices ");
                    device
                },
                _ => {
                    error!("failed to get default audio output device");
                    return Err(AudioOutputError::OpenStreamError);
                }
            };

            let mut need_h_sam = false;
            // log::info!("found devices {:#?}",devices);

            let (config, device) = {
                log::debug!("create config and devices");
                let mut c = None;
                let mut d = None;
                let mut min_sample_rate = u32::MAX;
                let mut max_sample_rate = 0;
                let mut close_d = None;
                let mut close_val = std::i64::MAX;
                log::debug!("Loop devices");
                let ignore_nonf = {
                    let mut ig = false;
                    if let Ok(devices) = host.devices(){
                        for d in devices{
                            if let Ok(conf) = d.supported_output_configs(){
                                for c in conf{
                                    if c.sample_format() == SampleFormat::F32{
                                        ig = true;
                                        break;
                                    }
                                }
                                if ig{
                                    break;
                                }
                            }
                        }
                    }
                    ig
                };
                for device in devices {
                    log::info!("Get supported configs");
                    match device.supported_output_configs() {
                        Ok(mut config) => {
                            log::debug!("Received configs");
                            if let Some(config) = config.next() {
                                log::debug!("Config rates {:#?} - {:#?}, format {:#?}",config.min_sample_rate(),config.max_sample_rate(),config.sample_format());
                                if ignore_nonf && config.sample_format()!=SampleFormat::F32{
                                    continue;
                                }
                                if spec.rate <= config.max_sample_rate().0
                                    && spec.rate >= config.min_sample_rate().0
                                {
                                    log::debug!("Setting config");
                                    c = Some(config.with_sample_rate(cpal::SampleRate(spec.rate)));
                                    d = Some(device);
                                    break;
                                }
                                if config.min_sample_rate().0 < min_sample_rate {
                                    min_sample_rate = config.min_sample_rate().0;
                                }
                                if config.max_sample_rate().0 > max_sample_rate {
                                    max_sample_rate = config.max_sample_rate().0;
                                }

                                log::info!("Current Confir min {:#?} max {:#?} format {:#?}",config.min_sample_rate(),config.max_sample_rate(),config.sample_format());
                                let min_diff = config.min_sample_rate().0 as i64-spec.rate as i64;
                                let max_diff = config.max_sample_rate().0 as i64-spec.rate as i64;
                                let new_close_val = std::cmp::min(min_diff.abs(),max_diff.abs() );
                                if new_close_val < close_val {
                                    log::info!("Old close val {}",close_val);
                                    log::info!("New close val {}", new_close_val);
                                    close_val = new_close_val;
                                    let rate  = {
                                        if close_val == max_diff.abs(){
                                            config.max_sample_rate()
                                        }else{
                                            config.min_sample_rate()
                                        }
                                    };
                                    close_d = Some((config.with_sample_rate(rate),device));

                                }
                            }
                        }
                        Err(err) => {
                            error!("failed to get default audio output device config: {}", err);
                            debug!(
                                "Supported configs {:#?}",
                                device.supported_output_configs().map(|c| c.count())
                            )
                        }
                    };
                }
                log::debug!("set default device");
                if c.is_none() {
                    log::debug!("No suitable config set");
                    if let Some((config,device)) = close_d{
                        c = Some(config);
                        d = Some(device);
                        need_h_sam = true;
                    }
                    else if let Some(device) = host.default_output_device() {
                        if let Ok(config) = device.default_output_config() {
                            log::debug!("Use default config");
                            c = Some(config);
                            d = Some(device);
                            need_h_sam = true;
                        }
                    }
                }
                log::debug!("Return device and config");
                if let Some(out) = c {
                    (out, d.unwrap())
                } else {
                    log::error!("No device and config found");
                    log::info!(
                        "Min -> {} Max -> {} Spec {}",
                        min_sample_rate,
                        max_sample_rate,
                        spec.rate
                    );
                    return Err(AudioOutputError::OpenStreamError);
                }
            };

            log::debug!("Supported config {:#?}", config);
            // Select proper playback routine based on sample format.
            let rate = if need_h_sam {
                config.sample_rate().0
            } else {
                spec.rate
            };
            match config.sample_format() {
                cpal::SampleFormat::F32 => {
                    log::debug!("Open f32");
                    CpalAudioOutputImpl::<f32>::try_open(spec, duration, &device, rate)
                }
                cpal::SampleFormat::I16 => {
                    log::debug!("Open i16");
                    CpalAudioOutputImpl::<i16>::try_open(spec, duration, &device, rate)
                }
                cpal::SampleFormat::U16 => {
                    log::debug!("Open u16");
                    CpalAudioOutputImpl::<u16>::try_open(spec, duration, &device, rate)
                }
            }
        }
    }

    struct CpalAudioOutputImpl<T: AudioOutputSample>
    where
        T: AudioOutputSample,
    {
        ring_buf_producer: rb::Producer<T>,
        sample_buf: SampleBuffer<T>,
        stream: cpal::Stream,
        rate: u32,
        original_rate: u32,
        channels: usize,
    }

    impl<T: AudioOutputSample> CpalAudioOutputImpl<T> {
        pub fn try_open(
            spec: SignalSpec,
            duration: Duration,
            device: &cpal::Device,
            rate: u32,
        ) -> Result<Box<dyn AudioOutput>> {
            // Output audio stream config.
            let config = cpal::StreamConfig {
                channels: spec.channels.count() as cpal::ChannelCount,
                sample_rate: cpal::SampleRate(rate),
                buffer_size: cpal::BufferSize::Default,
            };
            log::debug!("Opening config {:#?}", config);

            // Instantiate a ring buffer capable of buffering 8K (arbitrarily chosen) samples.
            let ring_buf = SpscRb::new(8 * 1024);
            let (ring_buf_producer, ring_buf_consumer) = (ring_buf.producer(), ring_buf.consumer());

            let stream_result = device.build_output_stream(
                &config,
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    // Write out as many samples as possible from the ring buffer to the audio
                    // output.
                    let written = ring_buf_consumer.read(data).unwrap_or(0);
                    // Mute any remaining samples.
                    data[written..].iter_mut().for_each(|s| *s = T::MID);
                },
                move |err| error!("audio output error: {}", err),
            );

            if let Err(err) = stream_result {
                error!("audio output stream open error: {}", err);

                return Err(AudioOutputError::OpenStreamError);
            }

            let stream = stream_result.unwrap();

            // Start the output stream.
            if let Err(err) = stream.play() {
                error!("audio output stream play error: {}", err);

                return Err(AudioOutputError::PlayStreamError);
            }

            let sample_buf = SampleBuffer::<T>::new(duration, spec);

            Ok(Box::new(CpalAudioOutputImpl {
                ring_buf_producer,
                sample_buf,
                stream,
                rate,
                original_rate: spec.rate,
                channels: spec.channels.count(),
            }))
        }
    }

    impl<T: AudioOutputSample> AudioOutput for CpalAudioOutputImpl<T> {
        fn write(&mut self, decoded: AudioBufferRef<'_>) -> Result<()> {
            // Do nothing if there are no audio frames.
            if decoded.frames() == 0 {
                return Ok(());
            }

            // Audio samples must be interleaved for cpal. Interleave the samples in the audio
            // buffer into the sample buffer.
            self.sample_buf.copy_interleaved_ref(decoded);

            let mut i = 0;
            let converted_samples = {
                if self.rate != self.original_rate {
                    log::debug!("trying to create sample rate converter");
                    let converter = samplerate::Samplerate::new(
                        samplerate::ConverterType::SincBestQuality,
                        self.original_rate,
                        self.rate,
                        self.channels,
                    )
                    .expect("Cant create converter");
                    log::debug!("trying to convert to new sample rate");
                    let new_sample = converter
                        .process_last(
                            &self
                                .sample_buf
                                .samples()
                                .iter()
                                .map(|f| AudioOutputSample::to_f32(f))
                                .collect::<Vec<_>>(),
                        )
                        .expect("Cant convert");
                    let new_sample = new_sample
                        .iter()
                        .map(|f| T::from_f32(*f))
                        .collect::<Vec<_>>();
                    log::info!(
                        "Converted from {} -> {}, Rate {} -> {}",
                        &self.sample_buf.samples().len(),
                        new_sample.len(),
                        self.original_rate,
                        self.rate
                    );
                    new_sample
                } else {
                    (self.sample_buf.samples())
                        .iter()
                        .map(|f| T::from_f32(AudioOutputSample::to_f32(f)))
                        .collect::<Vec<_>>()
                }
            };
            // Write out all samples in the sample buffer to the ring buffer.
            while i < converted_samples.len() {
                let writeable_samples = &converted_samples[i..];

                // Write as many samples as possible to the ring buffer. This blocks until some
                // samples are written or the consumer has been destroyed (None is returned).
                if let Some(written) = self.ring_buf_producer.write_blocking(writeable_samples) {
                    i += written;
                } else {
                    // Consumer destroyed, return an error.
                    return Err(AudioOutputError::StreamClosedError);
                }
            }

            Ok(())
        }

        fn flush(&mut self) {
            // Flush is best-effort, ignore the returned result.
            let _ = self.stream.pause();
        }
    }
}

pub fn try_open(spec: SignalSpec, duration: Duration) -> Result<Box<dyn AudioOutput>> {
    cpal::CpalAudioOutput::try_open(spec, duration)
}
