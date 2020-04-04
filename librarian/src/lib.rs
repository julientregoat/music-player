extern crate chrono;
extern crate claxon;
extern crate dotenv;
extern crate env_logger;
extern crate futures;
extern crate hound;
extern crate log;
extern crate tokio;
// TODO PR move serde to test dependencies for this crate
// TODO find a way to share or reuse reader from rtag
extern crate aiff;
extern crate minimp3;
extern crate rtag; // TODO use id3
extern crate sqlx;

use dotenv::dotenv;
use futures::future::{self, FutureExt};
use log::{debug, error, info, trace};
use sqlx::{sqlite::SqlitePool, Pool};
use std::{env, fs, path::Path, sync::mpsc};
use tokio::fs as async_fs;

pub mod models;
pub mod parse;

const FLAC: &'static str = "flac";
const WAV: &'static str = "wav";
const MP3: &'static str = "mp3";
const AIF: &'static str = "aif";
const AIFF: &'static str = "aiff";

const PG_UNIQUE_VIOLATION: &'static str = "23505";

// config options
// importing
// - if track with same name + release (and thus artist) exists
//      - don't import, log error or maybe allow user to review later, e.g. compare
//      sizes (maybe file is misnamed, wrong album, etc)
//      - add number to name (e.g. "<track> 1")
// - clean up duplicate tags if present
// - library file name formatting - if copy to lib dir, how to structure?

pub async fn import_track(
    conn: models::SqlitePoolConn,
    metadata: parse::ParseResult,
) -> models::Track {
    let mut conn = conn;
    // TODO execute in parallel - requires conn pool
    let mut artists = vec![];
    for curr_artist in metadata.artists {
        let new_artist = match models::Artist::create(&mut conn, &curr_artist)
            .await
        {
            Ok(a) => a,
            Err(sqlx::Error::Database(d)) => match d.code() {
                Some(code) if code == PG_UNIQUE_VIOLATION => {
                    models::Artist::get(&mut conn, &curr_artist).await.unwrap()
                }
                _ => panic!("new artist failed db {:?}", d),
            },
            Err(e) => panic!("new artist failed {:?}", e),
        };
        artists.push(new_artist);
    }

    // check first to prevent duplicate artist release entries
    // there is no constraint to prevent this
    // TODO hacky fix - see models.rs
    let (mut c, r) =
        match models::Release::get_artist_releases(&mut conn, artists[0].id)
            .await
        {
            Ok(releases) => {
                let album = metadata.album;
                let maybe = releases.iter().find(|r| r.name == album);
                if let Some(release) = maybe {
                    // TODO avoid clone?
                    (conn, release.clone())
                } else {
                    models::Release::create(
                        conn,
                        &album,
                        metadata.date.as_deref(),
                        artists.iter().map(|a| a.id).collect(),
                    )
                    .await
                    .unwrap()
                }
            }
            Err(e) => panic!("get artist releases {:?}", e),
        };

    let t = match models::Track::create(
        &mut c,
        &metadata.track,
        r.id,
        metadata.path.to_str().unwrap(), // TODO
        metadata.channels,
        metadata.sample_rate,
        metadata.bit_rate,
        metadata.track_pos,
    )
    .await
    {
        Ok(track) => track,
        Err(sqlx::Error::Database(e)) => match e.code() {
            Some(code) if code == PG_UNIQUE_VIOLATION => {
                panic!("track with same data already exists {:?}", e)
            }
            Some(code) => panic!("track insert db failure {:?}", code),
            None => panic!("track insert db fail no code"),
        },
        Err(e) => panic!("track insert failed {:?}", e),
    };

    t
}

pub fn parse_dir<P: AsRef<Path>>(
    tx: &mpsc::Sender<parse::ParseResult>,
    path: P,
) -> std::io::Result<()> {
    for entry_result in fs::read_dir(path).unwrap() {
        let entry_path = entry_result.unwrap().path();
        trace!("reviewing entry {:?}", entry_path);
        // TODO handle ext casing, normalize?
        match (entry_path.is_dir(), entry_path.extension()) {
            (false, Some(ext)) => {
                let parsed = match ext.to_str().unwrap() {
                    FLAC => parse::parse_flac(entry_path),
                    WAV => parse::parse_wav(entry_path),
                    MP3 => parse::parse_mp3(entry_path),
                    AIF | AIFF => parse::parse_aiff(entry_path),
                    some_ext => {
                        debug!("skipping unsupported file type {:?}", some_ext);
                        None
                    }
                };

                if let Some(result) = parsed {
                    trace!("parsed {:?}", &result);
                    match tx.send(result) {
                        Err(e) => error!("sending parse result failed {:?}", e),
                        _ => trace!("sent"),
                    }
                }
            }
            (true, _) => parse_dir(&tx, entry_path).unwrap(),
            _ => debug!("skipping unknown {:?}", entry_path),
        };
    }

    Ok(())
}

// TODO library dir should be stored in db and checked for there first before
#[tokio::main]
pub async fn start() {
    env_logger::init();
    dotenv().ok();

    info!("environment loaded, connecting to database...");

    let target = env::var("IMPORT_TARGET").expect("IMPORT_TARGET not set");
    let library = env::var("LIB_DIR").expect("LIB_DIR must be set");
    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    println!("db url {}", db_url);
    // currently defaults to max 10 conns simultaneously
    let db_pool: SqlitePool =
        Pool::connect(&db_url).await.expect("Error opening db pool");
    info!("connected to db");

    // TODO need to validate? check if exists, create dir
    let lib_path = Path::new(&library);
    if !lib_path.exists() {
        debug!("library doesn't exist, creating at {:?}", lib_path);
        if let Err(e) = fs::create_dir(lib_path) {
            panic!("failed to create library dir {:?}", e);
        }
    } else if !lib_path.is_dir() {
        panic!("file exists at library location but it is not a dir")
    } else if lib_path.is_relative() {
        panic!("library path can't be a relative path")
    }

    // TODO try out sync channel buffered to ulimit -n
    let (tx, rx) = std::sync::mpsc::channel();
    let import_thread = std::thread::spawn(move || {
        debug!("importing dir {:?}", target);
        parse_dir(&tx, target).unwrap()
    });

    let fs_handle_limit = 20;
    let mut copies = Vec::with_capacity(fs_handle_limit);
    let mut idx = 0;
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
        let mut track_path = lib_path.join(&msg.artists[0]).join(&msg.album);

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
                            db_pool.acquire()
                        }
                        Err(e) => panic!("failed to copy track {:?}", e),
                    })
                    .then(|res| match res {
                        Ok(c) => {
                            debug!("importing to db {:?}", msg);
                            import_track(c, msg)
                        }
                        Err(e) => {
                            panic!("failed to acquire conn {:?}", e);
                        }
                    })
                    .then(|t| {
                        debug!("finished db import {:?}", t);
                        future::ok::<(), ()>(())
                    }),
            );
            idx += 1;
        }

        if idx > (fs_handle_limit - 1) {
            debug!("hit buf max copies, awaiting");
            idx = 0;
            future::join_all(copies.drain(0..)).await;
            debug!("finished awaiting copies");
        }
    }

    debug!("channel closed");

    import_thread.join().unwrap();
    debug!("import thread joined");

    future::join_all(copies.drain(0..)).await;
    debug!("final copies futures joined");
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn runner() {
        start()
    }
}
