use claxon::FlacReader;

// GOALS
// - I want to get an iterator with an item that meets the cpal::Sample interface
// so I can collect samples into a vec
// - needs to implement Send so it can be used cross thread

trait SampleConvertIter: Iterator {
    type SampleType: cpal::Sample;
    fn to_sample(val: Self::Item) -> Self::SampleType;
}

type FlacSampleIter<'r, R: std::io::Read> =
    claxon::FlacSamples<&'r mut claxon::input::BufferedReader<R>>;

impl<'r, R: std::io::Read> SampleConvertIter for FlacSampleIter<'r, R> {
    type SampleType = f32;
    fn to_sample(val: Result<i32, claxon::Error>) -> f32 {
        (val.unwrap().abs() as f64 / i32::MAX as f64) as f32
    }
}

pub struct TrackMetadata {
    pub bit_rate: u16,
    pub sample_rate: u32,
    pub channels: u16,
}

// easy way out
pub fn get_samples(path: std::path::PathBuf) -> (Vec<impl cpal::Sample>, TrackMetadata) {
    let track_file = std::fs::File::open(&path).expect("Unable to open track file");
    match path.extension() {
        Some(e) if e == crate::parse::FLAC => {
            println!("Got flac");
            let mut r = FlacReader::new(track_file).unwrap();
            println!("about to collect");

            // FIXME why does this take so long?
            let s: Vec<_> = r
                .samples()
                // .map(|i| i.unwrap())
                .map(|i| (i.unwrap().abs() as f64 / i32::MAX as f64) as f32)
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

// TODO would be nice to return an iterator so things can be done as needed
// pub fn get_sample_iter(path: std::path::PathBuf) -> Box<impl Iterator<Item = dyn cpal::Sample>> {
//     let track_file = std::fs::File::open(&path).expect("Unable to open track file");
//     match path.extension() {
//         Some(e) if e == crate::parse::FLAC => {
//             println!("Got flac");
//             let r = FlacReader::new(track_file).unwrap();
//             // FIXME need to store the reader
//             unimplemented!()
//         }
//         x => {
//             unimplemented!("got other thing not supported yet {:?}", x);
//         }
//     }
// }
