use std::env;
use std::fs::File;
use std::io::Cursor;
use std::path::Path;

// use rodio::Source;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::StreamResponse;

// pub struct SympOut {
//     pub data: Option<SampleBuffer<f32>>,
//     pub rate: u32,
// }

// impl Iterator for SympOut {
//     type Item = Sample;

//     fn next(&mut self) -> Option<Self::Item> {
//         todo!()
//     }
// }

// impl Source for SympOut {
//     fn current_frame_len(&self) -> Option<usize> {
//         todo!()
//     }

//     fn channels(&self) -> u16 {
//         1
//     }

//     fn sample_rate(&self) -> u32 {
//         self.rate
//     }

//     fn total_duration(&self) -> Option<std::time::Duration> {
//         None
//     }
// }
pub fn decode_file(data: File) -> Box<dyn FormatReader> {
    let mss = MediaSourceStream::new(Box::new(data), Default::default());

    // Create a hint to help the format registry guess what format reader is appropriate. In this
    // example we'll leave it empty.
    let hint = Hint::new();

    // Use the default options when reading and decoding.
    let format_opts: FormatOptions = Default::default();
    let metadata_opts: MetadataOptions = Default::default();
    let decoder_opts: DecoderOptions = Default::default();

    // Probe the media source stream for a format.
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .unwrap();

    // Get the format reader yielded by the probe operation.
    let mut format = probed.format;

    return format;
}
pub fn decode(data: StreamResponse) -> Box<dyn FormatReader> {
    // Get command line arguments.

    // Create a media source. Note that the MediaSource trait is automatically implemented for File,
    // among other types.

    // Create the media source stream using the boxed media source from above.
    let mss = MediaSourceStream::new(Box::new(data), Default::default());

    // Create a hint to help the format registry guess what format reader is appropriate. In this
    // example we'll leave it empty.
    let hint = Hint::new();

    // Use the default options when reading and decoding.
    let format_opts: FormatOptions = Default::default();
    let metadata_opts: MetadataOptions = Default::default();
    let decoder_opts: DecoderOptions = Default::default();

    // Probe the media source stream for a format.
    log::info!("Probing stream");
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .unwrap();
    log::info!("Format probed");

    // Get the format reader yielded by the probe operation.
    let mut format = probed.format;

    return format;
    // Get the default track.
    // let track = format.default_track().unwrap();

    // // Create a decoder for the track.
    // let mut decoder = symphonia::default::get_codecs()
    //     .make(&track.codec_params, &decoder_opts)
    //     .unwrap();

    // // Store the track identifier, we'll use it to filter packets.
    // let track_id = track.id;

    // let mut sample_count = 0;
    // let mut sample_buf = None;
    // let mut rate = 0;

    // loop {
    //     // Get the next packet from the format reader.
    //     let packet = format.next_packet().unwrap();

    //     // If the packet does not belong to the selected track, skip it.
    //     if packet.track_id() != track_id {
    //         continue;
    //     }

    //     // Decode the packet into audio samples, ignoring any decode errors.
    //     match decoder.decode(&packet) {
    //         Ok(audio_buf) => {
    //             // The decoded audio samples may now be accessed via the audio buffer if per-channel
    //             // slices of samples in their native decoded format is desired. Use-cases where
    //             // the samples need to be accessed in an interleaved order or converted into
    //             // another sample format, or a byte buffer is required, are covered by copying the
    //             // audio buffer into a sample buffer or raw sample buffer, respectively. In the
    //             // example below, we will copy the audio buffer into a sample buffer in an
    //             // interleaved order while also converting to a f32 sample format.

    //             // If this is the *first* decoded packet, create a sample buffer matching the
    //             // decoded audio buffer format.
    //             if sample_buf.is_none() {
    //                 // Get the audio buffer specification.
    //                 let spec = *audio_buf.spec();
    //                 rate = spec.rate;
    //                 // Get the capacity of the decoded buffer. Note: This is capacity, not length!
    //                 let duration = audio_buf.capacity() as u64;

    //                 // Create the f32 sample buffer.
    //                 sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
    //             }

    //             // Copy the decoded audio buffer into the sample buffer in an interleaved format.
    //             if let Some(buf) = &mut sample_buf {
    //                 buf.copy_interleaved_ref(audio_buf);

    //                 // The samples may now be access via the `samples()` function.
    //                 sample_count += buf.samples().len();
    //                 print!("\rDecoded {} samples", sample_count);
    //             }
    //         }
    //         Err(Error::DecodeError(_)) => (),
    //         Err(_) => break,
    //     }
    // }
    // SympOut {
    //     data: sample_buf,
    //     rate,
    // }
}
