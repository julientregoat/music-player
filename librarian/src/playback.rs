use claxon::FlacReader;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Sample,
};
use log::{debug, error};
use std::path::PathBuf;

// GOALS
// - I want to get an iterator with an item that meets the cpal::Sample interface
// so I can collect samples into a vec
// - needs to implement Send so it can be used cross thread

trait SampleConvertIter<S: cpal::Sample>: Iterator {
    fn to_sample(val: Self::Item) -> S;
}

type FlacSampleIter<'r, R: std::io::Read> =
    claxon::FlacSamples<&'r mut claxon::input::BufferedReader<R>>;

// TODO this code should be able to be simplified once 24/32 bit support is impl
// right now it'll break or sound incorrect for non 16 bit vals
impl<'r, R: std::io::Read> SampleConvertIter<i16> for FlacSampleIter<'r, R> {
    fn to_sample(val: Result<i32, claxon::Error>) -> i16 {
        val.unwrap() as i16
    }
}

impl<'r, R: std::io::Read> SampleConvertIter<f32> for FlacSampleIter<'r, R> {
    fn to_sample(val: Result<i32, claxon::Error>) -> f32 {
        let sample: i16 = FlacSampleIter::<R>::to_sample(val);
        sample.to_f32()
    }
}

#[derive(Debug)]
pub struct TrackMetadata {
    pub bit_rate: u16,
    pub sample_rate: u32,
    pub channels: u16,
}

pub fn get_sample_chan(
    path: PathBuf,
) -> (
    Receiver<impl cpal::Sample>,
    std::thread::JoinHandle<()>,
    TrackMetadata,
) {
    let track_file = std::fs::File::open(&path).expect("Unable to open track file");
    match path.extension() {
        Some(e) if e == crate::parse::FLAC => {
            let (tx, rx) = std::sync::mpsc::channel::<f32>();
            println!("Got flac");
            let mut r = FlacReader::new(track_file).unwrap();
            println!("about to collect");

            let meta = r.streaminfo();

            let parse_thread = std::thread::spawn(move || {
                for s in r.samples() {
                    tx.send(FlacSampleIter::<std::fs::File>::to_sample(s))
                        .unwrap();
                }
            });

            (
                rx,
                parse_thread,
                TrackMetadata {
                    bit_rate: meta.bits_per_sample as u16,
                    sample_rate: meta.sample_rate,
                    channels: meta.channels as u16,
                },
            )
        }
        x => {
            unimplemented!("got other thing not supported yet {:?}", x);
        }
    }
}

fn get_output_stream<O, I>(
    device: &cpal::Device,
    sample_chan: std::sync::mpsc::Receiver<I>,
    config: &cpal::StreamConfig,
    channels: usize,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    I: cpal::Sample + Send + 'static,
    O: cpal::Sample,
{
    let mut idx = 0;
    let stream_chans = config.channels;
    device.build_output_stream(
        config,
        move |data: &mut [O], conf: &cpal::OutputCallbackInfo| {
            // debug!("callback idx {:?} buffer len {:?}", idx, data.len());
            for frame in data.chunks_mut(stream_chans as usize) {
                for point in 0..channels {
                    frame[point] = cpal::Sample::from::<I>(&sample_chan.recv().unwrap());
                    // println!("frame {:?}", frame[point]);
                    idx += 1;
                }
            }
        },
        move |err| {
            // TODO
            error!("err {:?}", err);
        },
    )
}

use std::sync::mpsc::{Receiver, Sender};

// FIXME rename fn
// TODO this should return an error if the track is not available. update db?
// TODO should this fn be async?
// if db access is separated, it can be removed for sure
// but are the async thread sleeping & fs calls worth it? tbd.
pub fn play_track(track_path: PathBuf) -> (cpal::Stream, std::thread::JoinHandle<()>) {
    let host = cpal::default_host();

    let device = host
        .default_output_device()
        .expect("no output device available");

    debug!("selected device {:?}", device.name().unwrap());

    let (rx, parse_thread, metadata) = get_sample_chan(track_path);

    println!("track meta {:?}", metadata);

    // TODO prioritize config w/ == channels, then >= channels, then < channels
    let config_range = device
        .supported_output_configs()
        .expect("error while querying configs")
        .find(|c| {
            println!("device config {:?}", c);
            c.channels() == metadata.channels
                && c.min_sample_rate().0 <= metadata.sample_rate
                && c.max_sample_rate().0 >= metadata.sample_rate
        })
        .expect("no matching config detected");

    let output_format = config_range.sample_format();

    let config = config_range
        .with_sample_rate(cpal::SampleRate(metadata.sample_rate))
        .config();

    let audio_chans = metadata.channels;

    // CAREFUL: stream must be stored in a var before playback
    let stream = match output_format {
        cpal::SampleFormat::U16 => {
            get_output_stream::<u16, _>(&device, rx, &config, audio_chans as usize)
        }
        cpal::SampleFormat::I16 => {
            get_output_stream::<i16, _>(&device, rx, &config, audio_chans as usize)
        }
        cpal::SampleFormat::F32 => {
            get_output_stream::<f32, _>(&device, rx, &config, audio_chans as usize)
        }
    }
    .unwrap();

    stream.play().unwrap();

    debug!("playing");

    (stream, parse_thread)
}

#[derive(Debug)]
pub enum StreamCommand {
    Pause,
    Play,
    Stop,
}
pub struct AudioStream {
    thread: std::thread::JoinHandle<()>,
    tx_stream: Sender<StreamCommand>,
}

impl AudioStream {
    pub fn from_path(source_path: PathBuf) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        // This was done because cpal::Stream is !Send, causing headaches upstream
        // Maybe there is a way to avoid this, but it seems it would require
        // keeping the stream on the main thread, which I'm not sure a lib
        // can guarantee.

        // this still needs to be external via exposed API
        // if self.stream_thread.is_some() {
        //     self.tx_stream.as_ref().unwrap().send(StreamCommand::Stop);
        // }

        let thread = std::thread::spawn(move || {
            // TODO thread should only last as long as duration of song
            let (s, pt) = play_track(source_path);
            while let Ok(res) = rx.recv() {
                debug!("received command {:?}", &res);
                match res {
                    StreamCommand::Pause => {
                        s.pause().unwrap();
                    }
                    StreamCommand::Play => {
                        s.play().unwrap();
                    }
                    StreamCommand::Stop => {
                        s.pause().unwrap();
                        break;
                    }
                }
            }
            // letting the thread handle drop appears to close the stream asap
            // pt.join().unwrap();
        });

        AudioStream {
            thread,
            tx_stream: tx,
        }
    }

    pub fn play(&self) {
        self.tx_stream.send(StreamCommand::Play).unwrap();
    }

    pub fn pause(&self) {
        self.tx_stream.send(StreamCommand::Pause).unwrap()
    }

    pub fn stop(&self) {
        self.tx_stream.send(StreamCommand::Stop).unwrap()
    }
}
