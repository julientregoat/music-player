extern crate dotenv;
extern crate env_logger;
extern crate gio;
extern crate glib;
extern crate gtk;
extern crate librarian;
extern crate log;
extern crate tokio;
extern crate tokio_compat_02;

use gio::prelude::*;
use gtk::prelude::*;
use log::{debug, error, info};
use std::env;
use std::sync::{mpsc, Arc, Mutex};
use tokio::sync::mpsc as tokio_mpsc;
use tokio_compat_02::FutureExt;

mod header;
mod track_list;

fn build_ui(application: &gtk::Application) -> gtk::ListStore {
    let window = gtk::ApplicationWindow::new(application);

    window.set_title("music player");
    // window.set_border_width(1);
    window.set_position(gtk::WindowPosition::Mouse);
    window.set_default_size(600, 400);

    let header = header::build_header();

    let (track_list, track_list_store) = track_list::build_track_list();

    let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layout.add(&header);
    layout.add(&track_list);

    window.add(&layout);
    window.show_all();

    track_list_store
}

const IMPORT_ACTION: &'static str = "import";

fn build_menu_bar(app: &gtk::Application) -> gio::Menu {
    // should the creation and registration of actions be separate?
    let import_action = gio::SimpleAction::new(IMPORT_ACTION, None);
    import_action.connect_activate(|_a, _v| {
        let chooser = gtk::FileChooserNativeBuilder::new()
            .title("title")
            .accept_label("import")
            .cancel_label("cancel")
            .action(gtk::FileChooserAction::SelectFolder)
            .build();

        chooser.connect_response(|chooser, resp_type| {
            if resp_type == gtk::ResponseType::Accept {
                match chooser.get_filename() {
                    Some(import_dir) => {
                        info!("importing {:?}", &import_dir);
                    }
                    None => error!("couldn't get filename for import dir"),
                }
            }
        });

        chooser.run();
    });
    app.add_action(&import_action);

    let menubar = gio::Menu::new();
    let file_menu = gio::Menu::new();
    let import_mitem = gio::MenuItem::new(Some("Import"), Some(&format!("app.{}", IMPORT_ACTION)));

    file_menu.append_item(&import_mitem);
    menubar.append_submenu(Some("File"), &file_menu);

    menubar
}

// TODO better names
#[derive(Debug)]
struct AppState {
    tracklist: Option<gtk::ListStore>,
}
#[derive(Debug)]
enum AppMsg {
    Init,
    Tracklist(Vec<librarian::models::DetailedTrack>),
}

#[derive(Debug)]
enum LibraryMsg {
    RefreshTracklist,
}

async fn event_loop(
    app: Arc<Mutex<AppState>>,
    rx_app: mpsc::Receiver<AppMsg>,
    tx_lib: tokio_mpsc::UnboundedSender<LibraryMsg>,
) {
    tx_lib
        .send(LibraryMsg::RefreshTracklist)
        .expect("Initial tracklist load failed.");
    while let Ok(msg) = rx_app.recv() {
        match msg {
            AppMsg::Tracklist(tracks) => {
                debug!("tracks {:?}", &tracks);
                app.tracklist.clear();
                for track in tracks {
                    track_list::insert_track(&app.tracklist, track)
                }
            }
            AppMsg::Init => error!("received random init message"),
        }
    }
}

use glib::MainContext;

// TODO library dir should be stored in db and checked for there first before
#[tokio::main]
pub async fn main() {
    env_logger::init();
    // dotenv().ok();

    // on error here, prompt user for desired db path
    let bin_path = std::env::current_exe().unwrap();
    let db_dir = bin_path.parent().unwrap().to_path_buf();
    let lib = librarian::Library::open_or_create(db_dir).compat().await;

    let application = gtk::ApplicationBuilder::new()
        .application_id("nyc.jules.music-player")
        .flags(Default::default())
        .register_session(true)
        .build();

    let menubar = build_menu_bar(&application);

    let (tx_lib, mut rx_lib) = tokio_mpsc::unbounded_channel();

    // TODO still need arc mutex?
    let app_state = Arc::new(Mutex::new(AppState { tracklist: None }));

    let tx_lib_2 = tx_lib.clone();
    let app_state_2 = app_state.clone();
    application.connect_activate(move |app| {
        app.set_menubar(Some(&menubar));
        let tracklist = build_ui(app);

        app_state_2.lock().unwrap().tracklist = Some(tracklist);
        tx_lib_2.send(LibraryMsg::RefreshTracklist).unwrap();
    });

    let main_ctx = MainContext::default();
    main_ctx.acquire();
    let app_state_3 = app_state.clone();
    let (tx_app, rx_app) = MainContext::channel(glib::PRIORITY_DEFAULT);

    // refactor into event loop fn
    rx_app.attach(Some(&main_ctx), move |msg| {
        match msg {
            // maybe on init refresh track? or just have a refresh track msgh
            AppMsg::Tracklist(tracks) => {
                if let Some(tracklist) = &app_state_3.lock().unwrap().tracklist {
                    debug!("tracks {:?}", &tracks);
                    tracklist.clear();
                    for track in tracks {
                        track_list::insert_track(tracklist, track);
                    }
                } else {
                    error!("recieved tracks before app was available")
                }
            }
            AppMsg::Init => error!("received random init message"),
        };

        glib::Continue(true)
    });

    tokio::spawn(async move {
        while let Some(msg) = rx_lib.recv().await {
            match msg {
                LibraryMsg::RefreshTracklist => {
                    let mut conn = lib.db_pool.acquire().compat().await.unwrap();
                    let result = librarian::models::Track::get_all_detailed(&mut conn)
                        .compat()
                        .await
                        .unwrap();
                    {
                        tx_app.send(AppMsg::Tracklist(result)).unwrap();
                    }
                }
            }
        }
    });

    // event_loop(app_state, rx_app, tx_lib)

    let args: Vec<_> = env::args().collect();
    application.run(&args);
}
