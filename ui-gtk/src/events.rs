use crate::{track_list, AppStore};
use gtk::GtkListStoreExt;
use log::{debug, error};
use tokio_compat_02::FutureExt;

#[derive(Debug)]
pub enum AppMsg {
  Tracklist(Vec<librarian::models::DetailedTrack>),
}

pub fn app_event_loop(app_state: AppStore) -> impl FnMut(AppMsg) -> glib::Continue {
  move |msg| {
    match msg {
      AppMsg::Tracklist(tracks) => {
        if let Some(tracklist) = &app_state.lock().unwrap().tracklist {
          debug!("tracks {:?}", &tracks);
          tracklist.clear();
          for track in tracks {
            track_list::insert_track(tracklist, track);
          }
        } else {
          error!("recieved tracks before app was available")
        }
      }
    };

    glib::Continue(true)
  }
}

use std::path::PathBuf;
use tokio::sync::mpsc as tokio_mpsc;
#[derive(Debug)]
pub enum LibraryMsg {
  RefreshTracklist,
  ImportDir(PathBuf),
}

// FIXME need to refactor librarian api to hide dealing w threads
pub async fn librarian_event_loop(
  lib: librarian::Library,
  listener: tokio_mpsc::UnboundedReceiver<LibraryMsg>,
  app_chan: glib::Sender<AppMsg>,
) {
  let mut listener = listener;
  while let Some(msg) = listener.recv().await {
    match msg {
      LibraryMsg::RefreshTracklist => {
        let mut conn = lib.db_pool.acquire().compat().await.unwrap();
        let result = librarian::models::Track::get_all_detailed(&mut conn)
          .compat()
          .await
          .unwrap();
        {
          app_chan.send(AppMsg::Tracklist(result)).unwrap();
        }
      }
      LibraryMsg::ImportDir(path) => {
        debug!("got import msg {:?}", &path);
        // let imported_tracks = librarian::import_dir(
        //   &lib.db_pool,
        //   // FIXME get lib path properly. should be determined inside librarian
        //   PathBuf::from("/Users/jtregoat/Code/demolib").as_path(),
        //   path,
        // )
        // .compat()
        // .await;
        // debug!("imoprteed dir {:?}", imported_tracks);
      }
    }
  }
}
