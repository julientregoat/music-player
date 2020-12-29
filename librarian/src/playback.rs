use claxon::FlacReader;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Sample, SupportedStreamConfigRange,
};
use log::{debug, error, trace};
use std::path::PathBuf;

trait SampleConvertIter<S: cpal::Sample>: Iterator {
    fn to_sample(val: Self::Item) -> S;
}

// may need cpal::SampleFormat prop or way to determine format is signed / float
#[derive(Debug)]
pub struct AudioMetadata {
    pub channels: u16,
    pub bit_depth: u16,
    pub sample_rate: u32,
}

pub enum SampleReceiver {
    I16(Receiver<i16>),
    I32(Receiver<i32>),
}

impl From<Receiver<i16>> for SampleReceiver {
    fn from(rx: Receiver<i16>) -> SampleReceiver {
        SampleReceiver::I16(rx)
    }
}

impl From<Receiver<i32>> for SampleReceiver {
    fn from(rx: Receiver<i32>) -> SampleReceiver {
        SampleReceiver::I32(rx)
    }
}

macro_rules! sample_channel_generator {
    ($fn_name:ident, $Reader:ty, $SamplePrimitive:ty, $transform:expr) => {
        pub fn $fn_name(
            mut reader: $Reader,
        ) -> (SampleReceiver, std::thread::JoinHandle<()>) {
            let (tx, rx) = std::sync::mpsc::channel::<$SamplePrimitive>();
            let parse_thread = std::thread::spawn(move || {
                // TODO need internal common trait for sample iter fn
                for s in reader.samples() {
                    let s: $SamplePrimitive = $transform(s);
                    match tx.send(s) {
                        Ok(_) => (),
                        Err(e) => {
                            println!("sample tx chan closed {:?}", e);
                            break;
                            // debug!("sample tx channel closed {:?}", e);
                        }
                    }
                }
            });

            (rx.into(), parse_thread)
        }
    };
}

sample_channel_generator!(
    flac_sample_chan_i16,
    FlacReader<std::fs::File>,
    i16,
    |x: Result<i32, claxon::Error>| x.unwrap() as i16
);

// FIXME should this return the unscaled i32?
sample_channel_generator!(
    flac_sample_chan_i24,
    FlacReader<std::fs::File>,
    i32,
    |x: Result<i32, claxon::Error>| cpal::Unpacked24::new(x.unwrap()).to_i32()
);

sample_channel_generator!(
    flac_sample_chan_i32,
    FlacReader<std::fs::File>,
    i32,
    |x: Result<i32, claxon::Error>| x.unwrap()
);

sample_channel_generator!(
    wav_sample_chan_i16,
    hound::WavReader<std::fs::File>,
    i16,
    |x: Result<i16, hound::Error>| x.unwrap()
);

// TODO validate 24 bit wav playback
sample_channel_generator!(
    wav_sample_chan_i24,
    hound::WavReader<std::fs::File>,
    i32,
    |x: Result<i32, hound::Error>| cpal::Unpacked24::new(x.unwrap()).to_i32()
);

sample_channel_generator!(
    wav_sample_chan_i32,
    hound::WavReader<std::fs::File>,
    i32,
    |x: Result<i32, hound::Error>| x.unwrap()
);

// FIXME this can be replaced with a macro once there is common iter trait
// hound + claxon both have the `samples` fn available to iter thru samples
fn mp3_sample_chan_i16(
    mut reader: minimp3::Decoder<std::fs::File>,
) -> (SampleReceiver, std::thread::JoinHandle<()>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let parse_thread = std::thread::spawn(move || {
        while let Ok(f) = reader.next_frame() {
            for s in f.data {
                match tx.send(s) {
                    Ok(_) => (),
                    Err(e) => {
                        println!("sample tx chan closed {:?}", e);
                        break;
                        // debug!("sample tx channel closed {:?}", e);
                    }
                }
            }
        }
    });

    (SampleReceiver::I16(rx), parse_thread)
}

pub fn create_sample_channel(
    path: PathBuf,
) -> (SampleReceiver, std::thread::JoinHandle<()>, AudioMetadata) {
    let track_file =
        std::fs::File::open(&path).expect("Unable to open track file");
    match path.extension() {
        Some(e) if e == crate::parse::FLAC => {
            debug!("Got flac");
            let r = FlacReader::new(track_file).unwrap();

            let meta = r.streaminfo();
            let (rx, parse_thread) = match meta.bits_per_sample as u16 {
                16 => flac_sample_chan_i16(r),
                24 => flac_sample_chan_i24(r),
                32 => flac_sample_chan_i32(r),
                _ => unimplemented!("unsupported bitrate flac"),
            };

            (
                rx,
                parse_thread,
                AudioMetadata {
                    channels: meta.channels as u16,
                    bit_depth: meta.bits_per_sample as u16,
                    sample_rate: meta.sample_rate,
                },
            )
        }
        Some(e) if e == crate::parse::WAV => {
            debug!("Got wav");
            let r = hound::WavReader::new(track_file).unwrap();

            // TODO check for 32 bit floats? need to support in AudioMetadata
            let meta = r.spec();
            let (rx, parse_thread) = match meta.bits_per_sample {
                16 => wav_sample_chan_i16(r),
                24 => wav_sample_chan_i24(r),
                32 => wav_sample_chan_i32(r),
                _ => unimplemented!("unsupported bitrate wav"),
            };

            (
                rx,
                parse_thread,
                AudioMetadata {
                    channels: meta.channels,
                    bit_depth: meta.bits_per_sample,
                    sample_rate: meta.sample_rate,
                },
            )
        }
        Some(e) if e == crate::parse::MP3 => {
            let mut r = minimp3::Decoder::new(track_file);
            // FIXME losing first frame to get sample rate
            let frame_meta = r.next_frame().unwrap();

            let (rx, parse_thread) = mp3_sample_chan_i16(r);
            (
                rx,
                parse_thread,
                AudioMetadata {
                    channels: frame_meta.channels as u16,
                    // convert kbits/sec to bits/sample(?)? or is it only i16?
                    bit_depth: 16,
                    sample_rate: frame_meta.sample_rate as u32,
                },
            )
        }
        x => {
            unimplemented!("unsupported format {:?}", x);
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
                    match sample_chan.recv() {
                        Ok(s) => {
                            frame[point] = cpal::Sample::from::<I>(&s);
                        }
                        Err(e) => {
                            debug!("sample rx channel closed {:?}", e);
                            break; // FIXME verify both loops are broken on err
                        }
                    };
                }
            }
        },
        move |err| {
            // TODO proper handling
            error!("err output stream {:?}", err);
        },
    )
}

use std::sync::mpsc::{Receiver, SyncSender};

// FIXME account for U16/U32 i/o -- needs to be accomodated in AudioMetadata
fn get_config_score(
    input_meta: &AudioMetadata,
    config: &SupportedStreamConfigRange,
) -> u16 {
    // +2 has exactly right number of channels
    // +1 has more channels than needed
    // 0 has less channels than needed
    let channel_score = match (input_meta.channels, config.channels()) {
        (i, o) if i == o => 2,
        (i, o) if i < o => 1,
        _ => 0,
    };

    // +2 input audio requires no conversion to output format
    // +1 input audio requires upcast (ideally lossless but prob lossy)
    // 0 audio requires downcast (lossy)
    let format_score = match (input_meta.bit_depth, config.sample_format()) {
        (i, o)
            if (i == 16 && o == cpal::SampleFormat::I16)
                || (i == 24 && o == cpal::SampleFormat::I24)
                || (i == 32 && o == cpal::SampleFormat::I32) =>
        {
            2
        }
        (i, o)
            if (i == 16
                && (o == cpal::SampleFormat::I24
                    || o == cpal::SampleFormat::I32
                    || o == cpal::SampleFormat::F32))
                || (i == 24 && (o == cpal::SampleFormat::I32)) =>
        {
            1
        }
        _ => 0,
    };

    channel_score + format_score
}

// TODO this should return an error if the track is not available. update db?
pub fn create_stream(
    source: PathBuf,
) -> (cpal::Stream, std::thread::JoinHandle<()>) {
    let host = cpal::default_host();

    let device = host
        .default_output_device()
        .expect("no output device available");

    debug!("selected device {:?}", device.name().unwrap());

    let (sample_rx, parse_thread, input_meta) = create_sample_channel(source);

    debug!("selected track meta {:?}", input_meta);

    let mut sorted_configs = device
        .supported_output_configs()
        .expect("error while querying configs")
        .filter(|c| {
            c.channels() > 0
                && c.min_sample_rate().0 <= input_meta.sample_rate
                && c.max_sample_rate().0 >= input_meta.sample_rate
        })
        .collect::<Vec<_>>();

    sorted_configs.sort_by(|a, b| {
        let a_score = get_config_score(&input_meta, a);
        let b_score = get_config_score(&input_meta, b);
        a_score.cmp(&b_score)
    });

    // debug!("sorted configs {:?}", sorted_configs);

    let config_range = match sorted_configs.pop() {
        Some(c) => c,
        None => panic!("no valid configs available"),
    };

    debug!("chosen config {:?}", config_range);

    let output_format = config_range.sample_format();
    let audio_chans = input_meta.channels;

    let config = config_range
        .with_sample_rate(cpal::SampleRate(input_meta.sample_rate))
        .config();

    // FIXME not thrilled about how messy this is... macro?
    let stream = match (output_format, sample_rx) {
        (cpal::SampleFormat::U16, SampleReceiver::I16(rx)) => {
            get_output_stream::<u16, _>(
                &device,
                rx,
                &config,
                audio_chans as usize,
            )
        }
        (cpal::SampleFormat::U16, SampleReceiver::I32(rx)) => {
            get_output_stream::<u16, _>(
                &device,
                rx,
                &config,
                audio_chans as usize,
            )
        }
        (cpal::SampleFormat::I16, SampleReceiver::I16(rx)) => {
            get_output_stream::<i16, _>(
                &device,
                rx,
                &config,
                audio_chans as usize,
            )
        }
        (cpal::SampleFormat::I16, SampleReceiver::I32(rx)) => {
            get_output_stream::<i16, _>(
                &device,
                rx,
                &config,
                audio_chans as usize,
            )
        }
        (cpal::SampleFormat::I24, SampleReceiver::I32(rx))
        | (cpal::SampleFormat::I32, SampleReceiver::I32(rx)) => {
            println!("32 bit in 24/32 bit");
            get_output_stream::<i32, _>(
                &device,
                rx,
                &config,
                audio_chans as usize,
            )
        }
        (cpal::SampleFormat::I24, SampleReceiver::I16(rx))
        | (cpal::SampleFormat::I32, SampleReceiver::I16(rx)) => {
            println!("16 bit in 24/32 bit out");
            get_output_stream::<i32, _>(
                &device,
                rx,
                &config,
                audio_chans as usize,
            )
        }
        // cpal::SampleFormat::I24 => unimplemented!("24 bit output unsupported"),
        (cpal::SampleFormat::F32, SampleReceiver::I16(rx)) => {
            get_output_stream::<f32, _>(
                &device,
                rx,
                &config,
                audio_chans as usize,
            )
        }
        (cpal::SampleFormat::F32, SampleReceiver::I32(rx)) => {
            get_output_stream::<f32, _>(
                &device,
                rx,
                &config,
                audio_chans as usize,
            )
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
