use claxon::{FlacReader, FlacReaderOptions};
use hound::WavReader;
use log::{debug, error, trace, warn};
use rtag::{
    frame::FrameBody,
    metadata::{MetadataReader, Unit},
};
use std::{fs, path::PathBuf};

pub const UNKNOWN_ENTRY: &'static str = "";
pub const UNKNOWN_ARTIST_DIR: &'static str = "Unknown Artist";
pub const UNKNOWN_ALBUM_DIR: &'static str = "Unknown Album";

// TODO better name than ParseResult
// parsing may not be the right word for this module. more focused on metadata
// TODO return Result instead of Option

#[derive(Debug)]
pub struct ParseResultBuilder {
    path: PathBuf,
    artists: Vec<String>,
    album: Option<String>,
    date: Option<String>,
    track: Option<String>,
    track_pos: Option<i32>,
    channels: Option<u16>,
    bit_depth: Option<u16>,
    sample_rate: Option<u32>,
}

// would be ideal to tie the builder to the path a bit more strongly, such that
// it's impossible for the data not to correlate with the file at that path
impl ParseResultBuilder {
    pub fn new(path: PathBuf) -> ParseResultBuilder {
        ParseResultBuilder {
            path,
            artists: vec![],
            album: None,
            date: None,
            track: None,
            track_pos: None,
            channels: None,
            bit_depth: None,
            sample_rate: None,
        }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn artist(&mut self, a: String) {
        self.artists.push(a)
    }

    pub fn album(&mut self, a: String) {
        self.album = Some(a)
    }

    pub fn date(&mut self, d: String) {
        self.date = Some(d)
    }

    pub fn track(&mut self, t: String) {
        self.track = Some(t)
    }

    pub fn track_pos(&mut self, t: i32) {
        self.track_pos = Some(t)
    }

    pub fn channels(&mut self, c: u16) {
        self.channels = Some(c)
    }

    pub fn bit_depth(&mut self, b: u16) {
        self.bit_depth = Some(b)
    }

    pub fn sample_rate(&mut self, s: u32) {
        self.sample_rate = Some(s)
    }

    pub fn has_bare_minimum(&self) -> bool {
        match (self.channels, self.bit_depth, self.sample_rate) {
            (Some(_), Some(_), Some(_)) => true,
            _ => false,
        }
    }

    pub fn has_required(&self) -> bool {
        match (
            self.artists.len() > 0,
            &self.album,
            &self.track,
            self.channels,
            self.bit_depth,
            self.sample_rate,
        ) {
            (true, Some(_), Some(_), Some(_), Some(_), Some(_)) => true,
            _ => false,
        }
    }

    pub fn has_all(&self) -> bool {
        match (
            self.artists.len() > 0,
            &self.album,
            &self.date,
            &self.track,
            self.track_pos,
            self.channels,
            self.bit_depth,
            self.sample_rate,
        ) {
            (
                true,
                Some(_),
                Some(_),
                Some(_),
                Some(_),
                Some(_),
                Some(_),
                Some(_),
            ) => true,
            _ => false,
        }
    }

    pub fn complete(
        mut self,
        populate_unknown_fields: bool,
    ) -> Option<ParseResult> {
        if populate_unknown_fields {
            if self.artists.len() == 0 {
                self.artist(UNKNOWN_ENTRY.to_owned());
            }
            if self.album == None {
                self.album(UNKNOWN_ENTRY.to_owned());
            }
            if self.track == None {
                // TODO if no artist or track then attempt to parse file name
                let file_name = self
                    .path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string());
                match file_name {
                    Some(name) => self.track(name),
                    None => warn!("unable to get fallback track name"),
                }
            }
        }

        // TODO should this be optional?
        if self.artists.len() > 1 {
            self.artists.dedup();
        }

        match (
            self.artists.len() > 0,
            self.album,
            self.track,
            self.channels,
            self.bit_depth,
            self.sample_rate,
        ) {
            (
                true,
                Some(album),
                Some(track),
                Some(channels),
                Some(bit_depth),
                Some(sample_rate),
            ) => Some(ParseResult {
                path: self.path,
                artists: self.artists,
                album,
                date: self.date,
                track,
                track_pos: self.track_pos,
                channels,
                bit_depth,
                sample_rate,
            }),
            _ => {
                warn!("ParseResultBuilder unable to complete");
                None
            }
        }
    }
}

// should file path be stored in here?
// TODO fields shouldn't be public
#[derive(Debug)]
pub struct ParseResult {
    pub path: PathBuf,
    pub artists: Vec<String>,
    pub album: String,
    pub date: Option<String>,
    pub track: String,
    pub track_pos: Option<i32>,
    pub channels: u16,
    pub bit_depth: u16,
    pub sample_rate: u32,
}

// TODO split out import file types - MP3 etc. can have a trait or enum impl?
// TODO handle invalid track numbers in tags. the main culprit are fraction
// strings like "1/9". that should be a simple enough edge case to handle.
// those tags should be updated too

pub fn parse_flac(p: PathBuf) -> Option<ParseResult> {
    trace!("parsing flac {:?}", &p);
    let mut builder = ParseResultBuilder::new(p);
    let reader = match FlacReader::open_ext(
        builder.path(),
        FlacReaderOptions {
            metadata_only: true,
            read_vorbis_comment: true,
        },
    ) {
        Ok(r) => r,
        Err(e) => {
            error!("failed to read flac {:?} {:?}", builder, e);
            return None;
        }
    };

    trace!(
        "all flac tags {:?}",
        reader.tags().collect::<Vec<(&str, &str)>>()
    );

    let file_info = reader.streaminfo();
    builder.channels(file_info.channels as u16);
    builder.bit_depth(file_info.bits_per_sample as u16);
    builder.sample_rate(file_info.sample_rate);

    for artist in reader.get_tag("artist") {
        builder.artist(artist.to_owned());
    }

    if let Some(t) = reader.get_tag("title").next() {
        builder.track(t.to_owned());
    }

    if let Some(a) = reader.get_tag("album").next() {
        builder.album(a.to_owned());
    }

    if let Some(d) = reader.get_tag("date").next() {
        builder.date(d.to_owned());
    }

    if let Some(tn) = reader.get_tag("tracknumber").next() {
        match tn.parse() {
            Ok(x) => builder.track_pos(x),
            Err(e) => warn!(
                "failed to parse track number from v1 framer {:?} {:?} {:?}",
                builder, tn, e
            ),
        };
    }

    builder.complete(true)
}
// TODO what other WAV tagging formats are there to keep an eye out for?
// TODO check out crate wav_reader
pub fn parse_wav(path: PathBuf) -> Option<ParseResult> {
    trace!("parsing wav {:?}", &path);
    let w = match WavReader::open(&path) {
        Ok(reader) => reader,
        Err(e) => {
            error!("failed to read wav {:?} {:?}", &path, e);
            return None;
        }
    };

    let encoding = w.spec();
    Some(ParseResult {
        path,
        artists: vec![UNKNOWN_ENTRY.to_string()],
        album: UNKNOWN_ENTRY.to_string(),
        track: UNKNOWN_ENTRY.to_string(),
        date: None,
        track_pos: None,
        channels: encoding.channels,
        bit_depth: encoding.bits_per_sample,
        sample_rate: encoding.sample_rate,
    })
}

pub fn parse_mp3(path: PathBuf) -> Option<ParseResult> {
    trace!("parsing mp3 {:?}", &path);
    let mut builder = ParseResultBuilder::new(path);

    let mut dec = match fs::File::open(builder.path())
        .map(|f| minimp3::Decoder::new(f))
    {
        Ok(f) => f,
        Err(e) => {
            error!("failed to read mp3 {:?} {:?}", builder, e);
            return None;
        }
    };

    let f = match dec.next_frame() {
        Ok(fr) => fr,
        Err(e) => {
            error!("failed to get mp3 frame {:?} {:?}", builder, e);
            return None;
        }
    };

    builder.channels(f.channels as u16);
    builder.sample_rate(f.sample_rate as u32);
    // FIXME how to get bit depths of an mp3?
    builder.bit_depth(f.bitrate as u16);

    let path_str = match builder.path().to_str() {
        Some(p) => p,
        None => {
            error!("failed to parse mp3 path {:?}", builder);
            return None;
        }
    };

    let meta_read = match MetadataReader::new(path_str) {
        Ok(r) => r,
        Err(e) => {
            error!("failed to read mp3 meta {:?} {:?}", builder, e);
            return None;
        }
    };

    // TODO prioritize v1 or v2 tags?
    // https://id3.org/id3v2.3.0#Declared_ID3v2_frames
    for m in meta_read {
        match m {
            Unit::FrameV1(f) => {
                builder.album(f.album);
                builder.track(f.title);
                builder.date(f.year);
                builder.artist(f.artist);
                match f.track.parse() {
                    Ok(x) => builder.track_pos(x),
                    Err(e) => warn!(
                        "failed to parse track number from v1 framer {:?} \
                         {:?} {:?}",
                        builder, f.track, e
                    ),
                };
            }
            Unit::FrameV2(_, FrameBody::PIC(p)) => {
                trace!("ignoring pic {:?}", p.description)
            }
            Unit::FrameV2(_, FrameBody::APIC(p)) => {
                trace!("ignoring pic {:?}", p.description)
            }
            Unit::FrameV2(_, FrameBody::PRIV(p)) => {
                trace!("ignoring priv tag from owner: {:?}", p.owner_identifier)
            }
            Unit::FrameV2(_, FrameBody::TALB(a)) => {
                builder.album(a.text);
            }
            Unit::FrameV2(_, FrameBody::TIT2(t)) => {
                builder.track(t.text);
            }
            Unit::FrameV2(_, FrameBody::TPE1(a)) => {
                builder.artist(a.text);
            }
            Unit::FrameV2(_, FrameBody::TYER(y)) => {
                builder.date(y.text);
            }
            Unit::FrameV2(_, FrameBody::TDAT(y)) => {
                // should this be a backup? is this used
                // for year sometimes as well?
                // TODO more research on usage
                trace!("date (date?) {:?}", y);
                // date = y.text;
            }
            Unit::FrameV2(_, FrameBody::TRCK(t)) => match t.text.parse() {
                Ok(pos) => builder.track_pos(pos),
                Err(e) => warn!(
                    "failed to parse track number from TRCK frame {:?} {:?} \
                     {:?}",
                    builder, t, e
                ),
            },
            Unit::FrameV2(_, b) => {
                trace!("v2 tag {:?}", b);
            }
            Unit::Header(h) => trace!("header {:?}", h),
            Unit::ExtendedHeader(h) => trace!("ext header {:?}", h),
        }
    }

    builder.complete(true)
}

pub fn parse_aiff(p: PathBuf) -> Option<ParseResult> {
    trace!("skipping aif/f file {:?}", &p);
    None
}

pub const FLAC: &'static str = "flac";
pub const WAV: &'static str = "wav";
pub const MP3: &'static str = "mp3";
pub const AIF: &'static str = "aif";
pub const AIFF: &'static str = "aiff";

// TODO handle ext casing, normalize?
pub fn parse_track(path: PathBuf) -> Option<ParseResult> {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e == FLAC => parse_flac(path),
        Some(e) if e == WAV => parse_wav(path),
        Some(e) if e == MP3 => parse_mp3(path),
        Some(e) if (e == AIF || e == AIFF) => parse_aiff(path),
        some_ext => {
            debug!("skipping unsupported file type {:?}", some_ext);
            None
        }
    }
}
