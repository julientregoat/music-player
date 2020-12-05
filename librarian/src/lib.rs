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
extern crate num_traits;
extern crate rtag; // TODO use id3
extern crate sqlx;

use futures::future::{self, FutureExt};
use log::{debug, error, info, trace};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tokio::fs as async_fs;
use tokio::sync::mpsc as tokio_mpsc;

pub mod models;
pub mod parse;
pub mod playback;

use playback::AudioStream;

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

// TODO store library metadata somewhere. db? user editable config file may be >
// - current library base path; where files are copied to on import
pub struct Library {
    pub db_pool: SqlitePool,
    // abstract sthread + tx to struct that requires both
    stream: Option<AudioStream>,
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
        let mpath = if cfg!(any(target_os = "linux", target_os = "macos")) {
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

        Library {
            db_pool,
            stream: None,
        }
    }

    pub async fn play_track(&mut self, track_id: i64) {
        let mut conn = self.db_pool.acquire().await.unwrap();
        let track = crate::models::Track::get(&mut conn, track_id)
            .await
            .unwrap();

        if !playback::is_cpal_sample(track.bit_rate) {
            panic!("unsupported bitrate");
        }

        let track_path = PathBuf::from(track.file_path);

        if self.stream.is_some() {
            self.stream.as_ref().unwrap().stop();
        }

        self.stream = Some(AudioStream::from_path(track_path));
    }

    pub fn play_stream(&self) {
        if self.stream.is_some() {
            self.stream.as_ref().unwrap().play();
        }
    }

    pub fn pause_stream(&self) {
        if self.stream.is_some() {
            self.stream.as_ref().unwrap().pause();
        }
    }
}
