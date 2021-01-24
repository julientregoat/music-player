extern crate aiff;
extern crate chrono;
extern crate claxon;
extern crate cpal;
extern crate directories_next;
extern crate futures;
extern crate hound;
extern crate log;
extern crate minimp3;
// TODO find a way to share or reuse reader from rtag
extern crate rtag; // TODO use id3
extern crate serde;
extern crate serde_derive;
extern crate sqlx;
extern crate toml;

use directories_next::BaseDirs;
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
mod userconfig;

use playback::AudioStream;
use userconfig::UserConfig;

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
        };
    }

    Ok(())
}

pub struct Library {
    db_pool: SqlitePool,
    stream: Option<AudioStream>,
    config: UserConfig,
    // collections: Vec<CollectionBase> // maybe only store raw collection data
    // current_collection: Collection
}

impl Library {
    pub async fn open_or_create() -> Self {
        let config_dir_override = std::env::var("RP_CONFIG_DIR");
        let config_dir = match config_dir_override {
            Ok(cpath) => PathBuf::from(cpath),
            _ => {
                if cfg!(debug_assertions) {
                    panic!("using default sysdirs in debug mode. set RP_CONFIG_DIR to librarian root");
                }

                BaseDirs::new().unwrap().config_dir().to_path_buf()
            }
        };
        println!("config dir {:?}", config_dir);

        if !config_dir.exists() {
            async_fs::create_dir_all(&config_dir)
                .await
                .expect("unable to create config dir");
        }

        let db_path = config_dir.join("librarian.db");
        let is_db_creation = !db_path.exists();
        if is_db_creation {
            debug!("db does not exist; creating at {:?}", db_path);
            async_fs::File::create(&db_path)
                .await
                .expect("failed to create db");
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

        sqlx::migrate!("./migrations").run(&db_pool).await.unwrap();

        if is_db_creation {
            // set up default collection
            let mut conn = db_pool.acquire().await.unwrap();
            models::CollectionBase::create(&mut conn, "Collection")
                .await
                .unwrap();
        }

        Library {
            db_pool,
            stream: None,
            config: UserConfig::load_from(config_dir.join("rpconfig.toml")),
        }
    }

    // TODO this whole thing needs to be cleaned up
    pub async fn import_dir(
        &self,
        import_from: PathBuf,
        target_collection: models::RowId, // maybe optional & set a default?
    ) -> Vec<models::OwnedTrack> {
        // TODO try out sync channel buffered to ulimit -n
        let (tx, mut rx) = tokio_mpsc::unbounded_channel();
        let import_thread = std::thread::spawn(move || {
            debug!("importing dir {:?}", import_from);
            parse_dir(&tx, import_from.as_ref()).unwrap()
        });

        let fs_handle_limit = 20;
        let mut copies = Vec::with_capacity(fs_handle_limit);
        let mut copies_idx = 0;
        let mut noncopies = Vec::new();
        let mut imported_tracks = Vec::new();
        while let Some(msg) = rx.recv().await {
            // TODO handle artist and album unknown
            // is it crazy to store empty strings for unknown artist? seems cleaner.
            // but that means needeing to check for empty strings to decide
            // if a DIFFERENT placeholder (e.g. Unknown Artist) should be used for
            // the path. it's a little messy.
            // there should also be a difference btw a user naming an artist
            // "Unknown Artist" and how the system internally handles the absence of a name

            if self.config.copy_on_import() {
                let mut release_path =
                    self.config.library_dir().join(&msg.artists[0]);
                release_path.push(&msg.album);

                trace!("about to create dir if needed {:?}", &release_path);
                match (release_path.exists(), release_path.is_dir()) {
                    (false, _) => fs::create_dir_all(&release_path).unwrap(),
                    (true, false) => {
                        panic!("target track dir exists but is not a dir")
                    }
                    (true, true) => (),
                };

                // TODO probably use track name + number as track name? expose config
                let mut track_path = release_path;
                track_path.push(&msg.path.file_name().unwrap());

                if track_path.exists() {
                    error!("target track path exists, skipping import")
                } else {
                    trace!("bout to copy file {:?}", &track_path);

                    // TODO check fs handle limit with `ulimit -n`
                    // try to raise? need to figure out how many I can safely acquire
                    // FIXME remove file if db insert fails - or switch order?
                    copies.push(
                        async_fs::copy(msg.path.clone(), track_path.clone())
                            .then(|res| match res {
                                Ok(_) => {
                                    debug!("getting lock");
                                    self.db_pool.acquire()
                                }
                                Err(e) => {
                                    panic!("failed to copy track {:?}", e)
                                }
                            })
                            .then(|res| match res {
                                Ok(c) => {
                                    let mut msg = msg;
                                    // update path to show import location
                                    msg.path = track_path;
                                    debug!("importing to db {:?}", msg);
                                    models::import_parse_result(
                                        c,
                                        target_collection,
                                        msg,
                                    )
                                }
                                Err(e) => {
                                    panic!("failed to acquire conn {:?}", e);
                                }
                            }),
                    );
                    copies_idx += 1;
                }
            } else {
                trace!("not copying track on import");
                noncopies.push(self.db_pool.acquire().then(|res| match res {
                    Ok(c) => {
                        models::import_parse_result(c, target_collection, msg)
                    }
                    Err(e) => {
                        panic!("failed to acquire conn {:?}", e);
                    }
                }));
            }

            if copies_idx > (fs_handle_limit - 1) {
                debug!("hit buf max copies, awaiting");
                copies_idx = 0;
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
        let mut final_import_copies = future::join_all(copies.drain(0..)).await;
        imported_tracks.append(&mut final_import_copies);

        let mut final_import_noncopies =
            future::join_all(noncopies.drain(0..)).await;
        imported_tracks.append(&mut final_import_noncopies);
        debug!("final copies futures joined");

        imported_tracks
    }

    pub async fn get_tracklist(&self) -> Vec<models::OwnedTrack> {
        let mut conn = self.db_pool.acquire().await.unwrap();
        // TODO handle failure
        models::OwnedTrack::get_all(&mut conn).await.unwrap()
    }

    // collections should contain artists etc? should it be an object with
    // all tracks, artists, etc
    // pub async fn load_collection(&self) -> models::Collection {}

    pub async fn play_track(&mut self, track_id: i64) {
        let mut conn = self.db_pool.acquire().await.unwrap();
        let track = crate::models::TrackBase::get(&mut conn, track_id)
            .await
            .unwrap();

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
