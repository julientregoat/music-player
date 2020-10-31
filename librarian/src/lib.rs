extern crate chrono;
extern crate claxon;
extern crate futures;
extern crate hound;
extern crate log;
// TODO PR move serde to test dependencies for this crate
// TODO find a way to share or reuse reader from rtag
extern crate aiff;
extern crate minimp3;
extern crate rtag; // TODO use id3
extern crate sqlx;

use chrono::Local;
use futures::future::{self, FutureExt};
use log::{debug, error, info, trace};
use sqlx::{sqlite::SqlitePool, Pool};
use std::env::args;
use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::mpsc,
};
use tokio::fs as async_fs;

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
    tx: &mpsc::Sender<parse::ParseResult>,
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

pub async fn import_dir(
    pool: &SqlitePool,
    import_to: &Path,
    import_from: PathBuf,
) -> Vec<models::Track> {
    // TODO try out sync channel buffered to ulimit -n
    let (tx, rx) = std::sync::mpsc::channel();
    let import_thread = std::thread::spawn(move || {
        debug!("importing dir {:?}", import_from);
        parse_dir(&tx, import_from.as_ref()).unwrap()
    });

    let fs_handle_limit = 20;
    let mut copies = Vec::with_capacity(fs_handle_limit);
    let mut idx = 0;
    let mut imported_tracks = Vec::new();
    while let Ok(msg) = rx.recv() {
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

        trace!("about to create dir if needed");
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
    db_pool: SqlitePool
}

const SQLITE_URL_PROTOCOL: &str = "sqlite:";

impl Library {
    pub async fn open_or_create(db_dir: PathBuf) -> Self {
        let mut db_path = db_dir;
        db_path.push("librarian.db");

        if !db_path.exists() {
            debug!("db does not exist; creating at {:?}", db_path);
            std::fs::File::create(&db_path).expect("failed to create db");
        }

        let mut db_url = db_path.into_os_string().into_string().expect("unable to coerce db_path pathbuf to String");
        db_url.insert_str(0, SQLITE_URL_PROTOCOL);

        // currently defaults to max 10 conns simultaneously
        let db_pool =
        Pool::connect(&db_url).await.expect("Error opening db pool");
        info!("connected to db");

        // TODO get lib dir, check if exists, create if not
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