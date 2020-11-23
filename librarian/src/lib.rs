extern crate chrono;
extern crate claxon;
extern crate futures;
extern crate hound;
extern crate log;
// TODO PR move serde to test dependencies for this crate
// TODO find a way to share or reuse reader from rtag
extern crate aiff;
extern crate cpal;
extern crate minimp3;
extern crate rtag; // TODO use id3
extern crate sqlx;

use chrono::Local;
use futures::future::{self, FutureExt};
use log::{debug, error, info, trace};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePool},
    Pool,
};
use std::env::args;
use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::mpsc,
};
use tokio::fs as async_fs;
use tokio::sync::mpsc as tokio_mpsc;

pub mod models;
pub mod parse;

// config options
// importing
// - if track with same name + release (and thus artist) exists
//      - don't import, log error or maybe allow user to review later, e.g. compare
//      sizes (maybe file is misnamed, wrong album, etc)
//      - add number to name (e.g. "<track> 1")
// - clean up duplicate tags if present
// - library file name formatting - if copy to lib dir, how to structure?

// pub fn parse_dir<P: AsRef<Path>>(
pub fn parse_dir(
    tx: &tokio_mpsc::UnboundedSender<parse::ParseResult>,
    path: &Path,
) -> std::io::Result<()> {
    for entry_result in fs::read_dir(path).unwrap() {
        let entry_path = entry_result.unwrap().path();
        trace!("reviewing entry {:?}", entry_path);
        match entry_path.is_dir() {
            false => {
                if let Some(result) = parse::parse_track(entry_path) {
                    trace!("parsed {:?}", &result);
                    match tx.send(result) {
                        Err(e) => error!("sending parse result failed {:?}", e),
                        _ => trace!("sent"),
                    }
                }
            }
            true => parse_dir(&tx, entry_path.as_ref()).unwrap(),
            _ => debug!("skipping unknown {:?}", entry_path),
        };
    }

    Ok(())
}

// FIXME why does tokio spawn require this fn to use tokio async chans?
pub async fn import_dir(
    pool: &SqlitePool,
    import_to: &Path,
    import_from: PathBuf,
) -> Vec<models::DetailedTrack> {
    // TODO try out sync channel buffered to ulimit -n
    let (tx, mut rx) = tokio_mpsc::unbounded_channel();
    let import_thread = std::thread::spawn(move || {
        debug!("importing dir {:?}", import_from);
        parse_dir(&tx, import_from.as_ref()).unwrap()
    });

    let fs_handle_limit = 20;
    let mut copies = Vec::with_capacity(fs_handle_limit);
    let mut idx = 0;
    let mut imported_tracks = Vec::new();
    while let Some(msg) = rx.recv().await {
        debug!("copying idx {} {:?}", idx, msg);
        // TODO need to change ParseResult.path if copied
        // should this happen before it's saved? or becomes pathbuf either way
        // should file handle be kept even instead of pathbuf? same reader

        // TODO handle artist and album unknown
        // is it crazy to store empty strings for unknown artist? seems cleaner.
        // but that means needeing to check for empty strings to decide
        // if a DIFFERENT placeholder (e.g. Unknown Artist) should be used for
        // the path. it's a little messy.
        // there should also be a difference btw a user naming an artist
        // "Unknown Artist" and how the system internally handles the absence of a name
        let mut track_path = import_to.join(&msg.artists[0]).join(&msg.album);

        trace!("about to create dir if needed {:?}", &track_path);
        match (track_path.exists(), track_path.is_dir()) {
            (false, _) => fs::create_dir_all(&track_path).unwrap(),
            (true, false) => panic!("target track dir exists but is not a dir"),
            (true, true) => (),
        };

        track_path.push(&msg.path.file_name().unwrap());

        if track_path.exists() {
            // this should be impossible since SQL should catch it
            // optionally skip dupes? log to user
            error!("target track path exists, skipping import")
        } else {
            trace!("bout to copy file");
            // TODO check fs handle limit with `ulimit -n`
            // try to raise? need to figure out how many I can safely acquire
            // TODO handle panics below - need to return future, maybe boxed?
            // TODO should the copy happen before the import? why not concurrent?
            // FIXME copy should be removed if db insert fails
            copies.push(
                async_fs::copy(msg.path.clone(), track_path)
                    .then(|res| match res {
                        Ok(_) => {
                            debug!("getting lock");
                            pool.acquire()
                        }
                        Err(e) => panic!("failed to copy track {:?}", e),
                    })
                    .then(|res| match res {
                        Ok(c) => {
                            debug!("importing to db {:?}", msg);
                            models::import_from_parse_result(c, msg)
                        }
                        Err(e) => {
                            panic!("failed to acquire conn {:?}", e);
                        }
                    }),
            );
            idx += 1;
        }

        if idx > (fs_handle_limit - 1) {
            debug!("hit buf max copies, awaiting");
            idx = 0;
            // FIXME this appears pretty inefficient
            // there needs to be a way to join futures directly from array
            // check FuturesUnordered?
            let mut imported = future::join_all(copies.drain(0..)).await;
            imported_tracks.append(&mut imported);
            debug!("finished awaiting copies");
        }
    }

    debug!("channel closed");

    import_thread.join().unwrap();
    debug!("import thread joined");

    // gotta be a way to do this in the loop?
    let mut final_import = future::join_all(copies.drain(0..)).await;
    imported_tracks.append(&mut final_import);
    debug!("final copies futures joined");

    imported_tracks
}

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

trait AudioSampleIter {
    type SampleType: cpal::Sample;
    // type SampleItem: cpal::Sample;
    // type SampleIterator: Iterator<Item = Self::SampleItem>;
    fn next_sample(&mut self) -> Self::SampleType;
}

impl<R: std::io::Read> AudioSampleIter for hound::WavSamples<'_, R, i16> {
    type SampleType = i16;
    fn next_sample(&mut self) -> Self::SampleType {
        self.next().unwrap().unwrap()
    }
}

impl AudioSampleIter for claxon::FlacSamples<&'_ mut claxon::input::BufferedReader<std::fs::File>> {
    type SampleType = i16;
    fn next_sample(&mut self) -> Self::SampleType {
        // FIXME this will result in clipping, need to use my cpal branch with 32 bit conversion support
        self.next().unwrap().unwrap() as i16
    }
}


trait AudioReader<'r> {
    type SampleIterator: AudioSampleIter;
    fn sample_iter(&'r mut self) -> Self::SampleIterator;
}

impl<'wr> AudioReader<'wr> for hound::WavReader<std::fs::File> {
    type SampleIterator = hound::WavSamples<'wr, std::fs::File, i16>;
    fn sample_iter(&'wr mut self) -> Self::SampleIterator {
        self.samples()
    }
}

impl<'r> AudioReader<'r> for claxon::FlacReader<std::fs::File> {
    type SampleIterator = claxon::FlacSamples<&'r mut claxon::input::BufferedReader<std::fs::File>>;
    fn sample_iter(&'r mut self) -> Self::SampleIterator {
        self.samples()
    }
}


struct AudioDecoder<'r, R: AudioReader<'r>> {
    reader: R,
    phantom: std::marker::PhantomData<&'r R>, // FIXME if possible?
}

impl<'r, R: AudioReader<'r>> AudioDecoder<'r, R> {
    fn new(reader: R) -> Self {
        AudioDecoder {
            reader,
            phantom: std::marker::PhantomData,
        }
    }

    // fn samples() -> impl Iterator<Item = cpal::Sample> {}
}

fn get_samples(path: &Path) -> AudioDecoder<impl AudioReader> {
    // can do this with tokio fs as well, but needed?
    let track_file =
        std::fs::File::open(&path).expect("Unable to open track file");
    match path.extension() {
        // Some(e) if e == parse::WAV => {
        //     println!("Got wav");
        //     let mut r = hound::WavReader::new(track_file).unwrap();
        //     AudioDecoder::new(r)
        // },
        Some(e) if e == parse::FLAC => {
            println!("Got flac");
            let  r = claxon::FlacReader::new(track_file).unwrap();
            AudioDecoder::new(r)
        }
        x => {
            unimplemented!("got other thing not supported yet {:?}", x);
        }
    }
}

// TODO this should return an error if the track is not available. store in db?
// TODO should this fn be async?
// if db access is separated, it can be removed for sure
// but are the async thread sleeping & fs calls worth it? tbd.
pub async fn play_track(pool: &SqlitePool, track_id: i64) {
    let mut conn = pool.acquire().await.unwrap();
    let track = models::Track::get(&mut conn, track_id).await.unwrap();
    let track_path = PathBuf::from(track.file_path);
    let samples = get_samples(track_path.as_path());

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no output device available");
    let mut supported_configs_range = device
        .supported_output_configs()
        .expect("error while querying configs");
    let config = supported_configs_range
        .next()
        .expect("no supported config?!")
        .with_max_sample_rate()
        .config();

    debug!("selected device {:?}", device.name().unwrap());

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                println!("cb");
                // react to stream events and read or write stream data here.
            },
            move |err| {
                println!("err {:?}", err);
                // react to errors here.
            },
        )
        .unwrap();

    stream.play().unwrap();

    // FIXME thread needs to sleep for the duration of the song
    // there is prob a tokio async fn for this instead, but if the Track is
    // passed in then this fn doesn't need to be async otherwise.
    // std::thread::sleep_ms(1000);
}

// TODO store library metadata somewhere. db? user editable config file may be >
// - current library base path; where files are copied to on import
pub struct Library {
    pub db_pool: SqlitePool,
}

impl Library {
    pub async fn open_or_create(db_dir: PathBuf) -> Self {
        let mut db_path = db_dir;
        db_path.push("librarian.db");

        if !db_path.exists() {
            debug!("db does not exist; creating at {:?}", db_path);
            std::fs::File::create(&db_path).expect("failed to create db");
        }

        let conn_opts = SqliteConnectOptions::new()
            .foreign_keys(true)
            .filename(&db_path);

        let db_pool = sqlx::pool::PoolOptions::new()
            // sqlite can only write at once, so the async calls get blocked
            // I can temporarily add a timeout which should help importing
            // FIXME separate reader / writer conns to bypass (sqlx is working on it)
            .max_connections(1)
            .connect_with(conn_opts)
            .await
            .expect("Error opening db pool");

        info!("connected to db");
        
        // FIXME get migration dir properly
        let mpath = if cfg!(any(target_os = "linux", target_os = "macos"))  {
            let home = std::env::var("HOME").unwrap();
            format!("{}{}", home, "/Code/music-player/librarian/migrations")
        } else if cfg!(target_os = "windows") {
            String::from("/Users/jt-in/Code/music-player/librarian/migrations")
        } else {
            unimplemented!("whoops")
        };

        let m = sqlx::migrate::Migrator::new(PathBuf::from(mpath))
        .await
        .unwrap();
        m.run(&db_pool).await.unwrap();

        // TODO determine where libdir should be - same as db?
        // let lib_path: PathBuf = library.into();
        // if !lib_path.exists() {
        //     debug!("library doesn't exist, creating at {:?}", lib_path);
        //     if let Err(e) = fs::create_dir(lib_path) {
        //         panic!("failed to create library dir {:?}", e);
        //     }
        // } else if !lib_path.is_dir() {
        //     panic!("file exists at library location but it is not a dir")
        // } else if lib_path.is_relative() {
        //     panic!("library path can't be a relative path")
        // }

        Library { db_pool }
    }
}
