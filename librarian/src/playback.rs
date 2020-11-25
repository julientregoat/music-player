use std::io;

pub trait AudioSampleIter: Iterator {
    type SampleType: cpal::Sample;
    fn next_sample(&mut self) -> Option<Self::SampleType>;
}

// impl<R: io::Read, S: cpal::Sample + hound::Sample> AudioSampleIter<S>
//     for hound::WavSamples<'_, R, S>
// {
//     type SampleType = S;
//     fn next_sample(&mut self) -> Option<S> {
//         self.next().unwrap().ok()
//     }
// }

impl<R: io::Read> AudioSampleIter
    for claxon::FlacSamples<&'_ mut claxon::input::BufferedReader<R>>
{
    type SampleType = f32;
    fn next_sample(&mut self) -> Option<f32> {
        // FIXME need to determine integer type conversion based on FlacReader bit rate
        Some((self.next().unwrap().unwrap().abs() as f64 / i32::MAX as f64) as f32)
    }
}

pub trait AudioReader<'r> {
    type SampleIterator: AudioSampleIter;
    fn sample_iter(&'r mut self) -> Self::SampleIterator;
}

// impl<'wr, R: io::Read + 'wr> AudioReader<'wr> for hound::WavReader<R> {
//     // FIXME make this generic over cpal::Sample + hound::Sample
//     // keep getting err E0207
//     type SampleType = i16;
//     type SampleIterator = hound::WavSamples<'wr, R, Self::SampleType>;
//     fn sample_iter(&'wr mut self) -> Self::SampleIterator {
//         self.samples()
//     }
// }

impl<'r, R: io::Read + 'r> AudioReader<'r> for claxon::FlacReader<R> {
    type SampleIterator = claxon::FlacSamples<&'r mut claxon::input::BufferedReader<R>>;
    fn sample_iter(&'r mut self) -> claxon::FlacSamples<&'r mut claxon::input::BufferedReader<R>> {
        self.samples()
    }
}

struct AudioDecoder<R> {
    reader: R,
}

impl<'r, R: AudioReader<'r>> AudioDecoder<R> {
    fn new(reader: R) -> Self {
        AudioDecoder { reader }
    }

    pub fn samples(&'r mut self) -> impl AudioSampleIter + 'r {
        // pub fn samples(&'r mut self) -> R::SampleIterator {
        self.reader.sample_iter()
    }
}

pub fn get_samples(path: &std::path::Path) -> AudioDecoder<impl AudioReader> {
    // can do this with tokio fs as well, but needed?
    let track_file = std::fs::File::open(&path).expect("Unable to open track file");
    match path.extension() {
        // Some(e) if e == parse::WAV => {
        //     println!("Got wav");
        //     let mut r = hound::WavReader::new(track_file).unwrap();
        //     AudioDecoder::new(r)
        // },
        Some(e) if e == crate::parse::FLAC => {
            println!("Got flac");
            let r = claxon::FlacReader::new(track_file).unwrap();
            AudioDecoder::new(r)
        }
        x => {
            unimplemented!("got other thing not supported yet {:?}", x);
        }
    }
}
