use claxon::FlacReader;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Sample,
};
use log::{debug, error};
use num_traits::NumCast;
use std::path::PathBuf;

pub fn is_cpal_sample<I: NumCast + std::cmp::Eq>(val: I) -> bool {
    match val {
        x if x == NumCast::from(16).unwrap() => true,
        x if x == NumCast::from(24).unwrap() || x == NumCast::from(32).unwrap() => false,
        _ => false,
    }
}

trait SampleConvertIter<S: cpal::Sample>: Iterator {
    fn to_sample(val: Self::Item) -> S;
}

type FlacSampleIter<'r, R> = claxon::FlacSamples<&'r mut claxon::input::BufferedReader<R>>;

// TODO this should be generically implemented when cpal gets 24/32 bit support
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

impl<'r, R: std::io::Read, S: cpal::Sample + hound::Sample> SampleConvertIter<S>
    for hound::WavSamples<'r, R, S>
{
    fn to_sample(val: Result<S, hound::Error>) -> S {
        val.unwrap()
    }
}

#[derive(Debug)]
pub struct AudioMetadata {
    pub channels: u16,
    pub bit_rate: u16,
    pub sample_rate: u32,
}

pub fn create_sample_channel(
    path: PathBuf,
) -> (
    Receiver<impl cpal::Sample>,
    std::thread::JoinHandle<()>,
    AudioMetadata,
) {
    let track_file = std::fs::File::open(&path).expect("Unable to open track file");
    match path.extension() {
        Some(e) if e == crate::parse::FLAC => {
            let (tx, rx) = std::sync::mpsc::channel();
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
                AudioMetadata {
                    channels: meta.channels as u16,
                    bit_rate: meta.bits_per_sample as u16,
                    sample_rate: meta.sample_rate,
                },
            )
        }
        Some(e) if e == crate::parse::WAV => {
            println!("Got wav");
            let mut r = hound::WavReader::new(track_file).unwrap();
            println!("about to collect");

            // TODO do I need to check for floats?
            let meta = r.spec();
            let (tx, rx) = match meta.bits_per_sample {
                16 => std::sync::mpsc::channel(),
                // 24 | 32 => std::sync::mpsc::channel::<i32>(),
                _ => unimplemented!("no chans"),
            };

            let parse_thread = std::thread::spawn(move || {
                for s in r.samples() {
                    match meta.bits_per_sample {
                        16 => tx
                            .send(hound::WavSamples::<std::fs::File, i16>::to_sample(s))
                            .unwrap(),
                        // 24 | 32 => tx
                        //     .send(hound::WavSamples::<std::fs::File, i32>::to_sample(s))
                        //     .unwrap(),
                        _ => unimplemented!("nope"),
                    }
                }
            });

            (
                rx,
                parse_thread,
                AudioMetadata {
                    channels: meta.channels,
                    bit_rate: meta.bits_per_sample,
                    sample_rate: meta.sample_rate,
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
    let stream_chans = config.channels;
    device.build_output_stream(
        config,
        move |data: &mut [O], _conf: &cpal::OutputCallbackInfo| {
            for frame in data.chunks_mut(stream_chans as usize) {
                for point in 0..channels {
                    frame[point] = cpal::Sample::from::<I>(&sample_chan.recv().unwrap());
                }
            }
        },
        move |err| {
            // TODO
            error!("err {:?}", err);
        },
    )
}

use std::sync::mpsc::{Receiver, SyncSender};

// TODO this should return an error if the track is not available. update db?
pub fn create_stream(source: PathBuf) -> (cpal::Stream, std::thread::JoinHandle<()>) {
    let host = cpal::default_host();

    let device = host
        .default_output_device()
        .expect("no output device available");

    debug!("selected device {:?}", device.name().unwrap());

    let (rx, parse_thread, metadata) = create_sample_channel(source);

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

    // NB stream must be stored in a var before playback
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

    (stream, parse_thread)
}

#[derive(Debug)]
pub enum StreamCommand {
    Pause,
    Play,
    Stop,
}
pub struct AudioStream {
    tx_stream: SyncSender<StreamCommand>,
    _thread: std::thread::JoinHandle<()>,
}

impl AudioStream {
    pub fn from_path(source: PathBuf) -> Self {
        // This implementation is as it is because cpal::Stream is !Send
        // Maybe there is a way to avoid this, but it seems it would require
        // keeping the stream on the main thread, which I'm not sure a lib
        // can guarantee.

        let (tx, rx) = std::sync::mpsc::sync_channel(64);
        let thread = std::thread::spawn(move || {
            let (s, _pt) = create_stream(source);
            s.play().unwrap();

            debug!("playing");

            while let Ok(res) = rx.recv() {
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
            _thread: thread,
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
