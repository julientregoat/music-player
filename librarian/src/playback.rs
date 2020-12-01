use claxon::FlacReader;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Sample,
};
use log::debug;
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

// easy way out - returning a collected vec instead of the iterator
pub fn get_samples(path: PathBuf) -> (Vec<impl cpal::Sample>, TrackMetadata) {
    let track_file =
        std::fs::File::open(&path).expect("Unable to open track file");
    match path.extension() {
        Some(e) if e == crate::parse::FLAC => {
            println!("Got flac");
            let mut r = FlacReader::new(track_file).unwrap();
            println!("about to collect");

            // FIXME this takes a long time; passing iterator to stream is faster
            // maybe SIMD will help tho
            let s: Vec<f32> = r
                .samples()
                // .map(|i| i.unwrap())
                .map(|i| FlacSampleIter::<std::fs::File>::to_sample(i))
                .collect();
            println!("collected {:?}", s.len());

            // let s = s.iter().map(|i| *i as i16).collect();
            // println!("second collect");
            let meta = r.streaminfo();
            (
                s,
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
    samples: Vec<I>,
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
            println!("callback idx {:?} buffer len {:?}", idx, data.len());
            // FIXME need to check frame or buffer size to prevent overrunning
            for frame in data.chunks_mut(stream_chans as usize) {
                for point in 0..channels {
                    frame[point] = cpal::Sample::from::<I>(&samples[idx]);
                    // println!("frame {:?}", frame[point]);
                    idx += 1;
                }
            }
        },
        move |err| {
            println!("err {:?}", err);
            // react to errors here.
        },
    )
}

// TODO at least part of this function should be separated and impl'd on Library
// TODO this should return an error if the track is not available. update db?
// TODO should this fn be async?
// if db access is separated, it can be removed for sure
// but are the async thread sleeping & fs calls worth it? tbd.
pub async fn play_track(pool: &sqlx::SqlitePool, track_id: i64) {
    let mut conn = pool.acquire().await.unwrap();
    let track = crate::models::Track::get(&mut conn, track_id)
        .await
        .unwrap();
    let track_path = PathBuf::from(track.file_path);
    // let mut decoder = playback::get_samples(track_path);

    // FIXME the issue lies with the config partially
    let host = cpal::default_host();

    let mut device = host
        .default_output_device()
        .expect("no output device available");

    debug!("selected device {:?}", device.name().unwrap());

    let (samples, metadata) = get_samples(track_path);
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
        cpal::SampleFormat::U16 => get_output_stream::<u16, _>(
            &device,
            samples,
            &config,
            audio_chans as usize,
        ),
        cpal::SampleFormat::I16 => get_output_stream::<i16, _>(
            &device,
            samples,
            &config,
            audio_chans as usize,
        ),
        cpal::SampleFormat::F32 => get_output_stream::<f32, _>(
            &device,
            samples,
            &config,
            audio_chans as usize,
        ),
    }
    .unwrap();

    stream.play().unwrap();

    debug!("playing");

    // FIXME thread needs to sleep for the duration of the song
    // there is prob a tokio async fn for this instead, but if the Track is
    // passed in then this fn doesn't need to be async otherwise.
    std::thread::sleep_ms(60000);
}
