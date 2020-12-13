use crate::{track_list, AppStore};
use gtk::GtkListStoreExt;
use librarian::models::DetailedTrack;
use log::{debug, error, trace};

// maybe replace the app event loop with tokio watch channel, broadcasting
// to the widgets that need to be updated. that would scale better than this
#[derive(Debug)]
pub enum AppMsg {
    Tracklist(Vec<DetailedTrack>),
    ImportedTracks(Vec<DetailedTrack>),
}

pub fn app_event_loop(
    app_state: AppStore,
) -> impl FnMut(AppMsg) -> glib::Continue {
    move |msg| {
        match msg {
            AppMsg::Tracklist(tracks) => {
                if let Some(tracklist) = &app_state.lock().unwrap().tracklist {
                    trace!("got tracklist {:?}", &tracks);
                    tracklist.clear();
                    for track in tracks {
                        track_list::insert_track(tracklist, track);
                    }
                } else {
                    error!("recieved tracks before list was available")
                }
            }
            AppMsg::ImportedTracks(tracks) => {
                if let Some(tracklist) = &app_state.lock().unwrap().tracklist {
                    debug!("tracks {:?}", &tracks);
                    for track in tracks {
                        track_list::insert_track(tracklist, track);
                    }
                } else {
                    error!("recieved track import before list was available")
                }
            }
        };

        glib::Continue(true)
    }
}

use std::path::PathBuf;
use tokio::sync::mpsc as tokio_mpsc;
use tokio_compat_02::FutureExt;

#[derive(Debug)]
pub enum LibraryMsg {
    RefreshTracklist,
    ImportDir(PathBuf),
    PlayTrack(i64),
    PlayStream,
    PauseStream,
}

pub type LibEventSender = tokio_mpsc::UnboundedSender<LibraryMsg>;
pub type LibEventReceiver = tokio_mpsc::UnboundedReceiver<LibraryMsg>;

pub async fn librarian_event_loop(
    mut lib: librarian::Library,
    listener: LibEventReceiver,
    app_chan: glib::Sender<AppMsg>,
) {
    let mut listener = listener;
    while let Some(msg) = listener.recv().await {
        match msg {
            LibraryMsg::RefreshTracklist => {
                let mut conn = lib.db_pool.acquire().compat().await.unwrap();
                let result =
                    librarian::models::Track::get_all_detailed(&mut conn)
                        .compat()
                        .await
                        .unwrap();
                {
                    app_chan.send(AppMsg::Tracklist(result)).unwrap();
                }
            }
            LibraryMsg::ImportDir(path) => {
                // ideally, this should return tracks in a stream so the UI
                // is updated with information faster
                let imported_tracks = lib
                    .import_dir(
                        // FIXME get lib path properly. should be determined via librarian
                        PathBuf::from(
                            std::env::var("LIB_DIR")
                                .unwrap_or(String::from("./librariandemolib")),
                        )
                        .as_path(),
                        path,
                    )
                    .compat()
                    .await;
                {
                    app_chan
                        .send(AppMsg::ImportedTracks(imported_tracks))
                        .unwrap();
                }
            }
            LibraryMsg::PlayTrack(track_id) => {
                debug!("got track to play {}", track_id);
                lib.play_track(track_id).compat().await;
            }
            LibraryMsg::PlayStream => {
                lib.play_stream();
            }
            LibraryMsg::PauseStream => {
                debug!("pausing track");
                lib.pause_stream()
            }
        }
    }
}
