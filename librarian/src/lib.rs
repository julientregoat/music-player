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
extern crate sqlx;

use directories_next::{BaseDirs, UserDirs};
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

const DEFAULT_DIR_NAME: &'static str = "recordplayer";

// configurations to implement
// - file + dir naming (e.g. Artist or Album top level, track name format)
// - how to handle duplicate track entries in the db
//   - replace track file path with new one
//   - don't import new one
//   - let user decide every time

struct UserConfig {
    config_dir: PathBuf,
    lib_dir: PathBuf,
    copy_on_import: bool,
}

impl UserConfig {
    pub fn new(
        config_dir: PathBuf,
        lib_dir: PathBuf,
        copy_on_import: bool,
    ) -> Self {
        UserConfig {
            config_dir,
            lib_dir,
            copy_on_import,
        }
    }

    pub fn config_dir(&self) -> &Path {
        self.config_dir.as_path()
    }

    pub fn lib_dir(&self) -> &Path {
        self.lib_dir.as_path()
    }
}

impl Default for UserConfig {
    fn default() -> Self {
        let sysdirs = BaseDirs::new().unwrap();
        let userdirs = UserDirs::new().unwrap();
        UserConfig {
            lib_dir: userdirs.audio_dir().unwrap().join(DEFAULT_DIR_NAME),
            config_dir: sysdirs.config_dir().join(DEFAULT_DIR_NAME),
            copy_on_import: true,
        }
    }
}

pub struct Library {
    db_pool: SqlitePool,
    stream: Option<AudioStream>,
    config: UserConfig,
}

impl Library {
    pub async fn open_or_create() -> Self {
        // FIXME ensure migrations are always present - compile w app?
        let dev_dir_override = std::env::var("RPLIB_DEV_ROOT");
        let copy_on_import = std::env::var("RPLIB_COPY_ON_IMPORT")
            .unwrap_or(String::from(""))
            == "true";

        let (default_config_dir, default_audio_dir) = match dev_dir_override {
            Ok(dir) => {
                let p = PathBuf::from(dir);
                (p.clone(), p)
            }
            _ => (
                BaseDirs::new().unwrap().config_dir().join(DEFAULT_DIR_NAME),
                UserDirs::new()
                    .unwrap()
                    .audio_dir()
                    .unwrap()
                    .join(DEFAULT_DIR_NAME),
            ),
        };

        if !default_config_dir.exists() {
            fs::create_dir_all(&default_config_dir)
                .expect("unable to create config dir");
        }

        let db_path = default_config_dir.join("librarian.db");

        if !db_path.exists() {
            debug!("db does not exist; creating at {:?}", db_path);
            fs::File::create(&db_path).expect("failed to create db");
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

        let migrations_dir = default_config_dir.join("/migrations");
        if !migrations_dir.exists() {
            panic!("migrations directory doesn't exist {:?}", migrations_dir);
        }

        let migrator =
            sqlx::migrate::Migrator::new(PathBuf::from(migrations_dir))
                .await
                .unwrap();
        migrator.run(&db_pool).await.unwrap();

        // TODO check database for user config?

        Library {
            db_pool,
            stream: None,
            config: UserConfig::new(
                default_config_dir,
                default_audio_dir,
                copy_on_import,
            ),
        }
    }

    pub async fn import_dir(
        &self,
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
        // FIXME if cannot be canoniclized, it doesn't exist
        // create if canonicalizing fails? since this should be from db, prob
        let import_target = import_to.canonicalize().unwrap();
        while let Some(msg) = rx.recv().await {
            debug!("copying idx {} {:?}", idx, msg);
            // TODO handle artist and album unknown
            // is it crazy to store empty strings for unknown artist? seems cleaner.
            // but that means needeing to check for empty strings to decide
            // if a DIFFERENT placeholder (e.g. Unknown Artist) should be used for
            // the path. it's a little messy.
            // there should also be a difference btw a user naming an artist
            // "Unknown Artist" and how the system internally handles the absence of a name
            // FIXME conditionally import file based on user config
            // otherwise skip path creation & copying
            println!("import to {:?}", import_to);

            let mut release_path = import_target.join(&msg.artists[0]);
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
                // FIXME if msg.path (source location) == track_path
                // then proceed without error - audio file is in right place
                error!("target track path exists, skipping import")
            } else {
                trace!("bout to copy file {:?}", &track_path);

                // TODO conditionally copy to path
                // TODO check fs handle limit with `ulimit -n`
                // try to raise? need to figure out how many I can safely acquire
                // TODO should the copy happen before the import? why not concurrent?
                // FIXME copy should be removed if db insert fails
                copies.push(
                    async_fs::copy(msg.path.clone(), track_path.clone())
                        .then(|res| match res {
                            Ok(_) => {
                                debug!("getting lock");
                                self.db_pool.acquire()
                            }
                            Err(e) => panic!("failed to copy track {:?}", e),
                        })
                        .then(|res| match res {
                            Ok(c) => {
                                // store the newly copied path in the db
                                let mut msg = msg;
                                msg.path = track_path;
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

    pub async fn get_tracklist(&self) -> Vec<models::DetailedTrack> {
        let mut conn = self.db_pool.acquire().await.unwrap();
        // TODO handle failure
        models::Track::get_all_detailed(&mut conn).await.unwrap()
    }

    pub async fn play_track(&mut self, track_id: i64) {
        let mut conn = self.db_pool.acquire().await.unwrap();
        let track = crate::models::Track::get(&mut conn, track_id)
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
