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
extern crate tokio_stream;
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
use tokio_stream::{wrappers::UnboundedReceiverStream, StreamExt};

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
        if !db_path.exists() {
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

        Library {
            db_pool,
            stream: None,
            config: UserConfig::load_from(config_dir.join("rpconfig.toml")),
        }
    }

    pub async fn import_track(
        &self,
        msg: parse::ParseResult,
    ) -> models::DetailedTrack {
        let mut msg = msg;
        if self.config.copy_on_import() {
            let mut release_path =
                self.config.library_dir().join(&msg.artists[0]);
            release_path.push(&msg.album);

            trace!("about to create dir if needed {:?}", &release_path);
            match (release_path.exists(), release_path.is_dir()) {
                (false, _) => {
                    async_fs::create_dir_all(&release_path).await.unwrap()
                }
                (true, false) => {
                    panic!("target track dir exists but is not a dir")
                }
                (true, true) => (),
            };

            let mut track_path = release_path;
            track_path.push(&msg.path.file_name().unwrap());

            if track_path.exists() {
                error!("target track path exists, skipping import")
            }
            trace!("bout to copy file {:?}", &track_path);

            async_fs::copy(msg.path.clone(), track_path.clone())
                .await
                .unwrap();

            // update path to show import location
            msg.path = track_path;
        }
        let msg = msg;

        debug!("getting db lock for track import");
        let conn = self.db_pool.acquire().await.unwrap();
        debug!("importing to db {:?}", msg);
        models::import_from_parse_result(conn, msg).await
    }

    // TODO this whole thing needs to be cleaned up
    pub async fn import_dir(
        &'static self,
        import_from: PathBuf,
    ) -> Vec<models::DetailedTrack> {
        // TODO try out sync channel buffered to ulimit -n
        let (tx, mut rx) = tokio_mpsc::unbounded_channel();
        let import_thread = std::thread::spawn(move || {
            debug!("importing dir {:?}", import_from);
            parse_dir(&tx, import_from.as_ref()).unwrap()
        });

        // let fs_handle_limit = 20;s
        // let mut copies = Vec::with_capacity(fs_handle_limit);
        // let mut copies_idx = 0;
        // let mut noncopies = Vec::new();
        let (txt, mut rxt) = tokio_mpsc::unbounded_channel();

        let h = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                txt.send(self.import_track(msg).await).unwrap();

                // tokio::spawn(async {
                // });
            }
        });

        let rxt_stream = UnboundedReceiverStream::new(rxt);

        let c = rxt_stream.collect().await;
        h.await.unwrap();
        import_thread.join().unwrap();
        c
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
