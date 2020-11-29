use claxon::FlacReader;
use cpal::Sample;

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
pub fn get_samples(
    path: std::path::PathBuf,
) -> (Vec<impl cpal::Sample>, TrackMetadata) {
    let track_file =
        std::fs::File::open(&path).expect("Unable to open track file");
    match path.extension() {
        Some(e) if e == crate::parse::FLAC => {
            println!("Got flac");
            let mut r = FlacReader::new(track_file).unwrap();
            println!("about to collect");

            // FIXME this takes a long time; iterator would be faster
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
