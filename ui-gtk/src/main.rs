extern crate dotenv;
extern crate gio;
extern crate glib;
extern crate gtk;
extern crate librarian;
extern crate log;
extern crate log4rs;
extern crate tokio;
extern crate tokio_compat_02;

use gio::prelude::*;
use gtk::prelude::*;
use log::{debug, error, info};
use std::env;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc as tokio_mpsc;
use tokio_compat_02::FutureExt;

mod events;
mod header;
mod track_list;

fn build_ui(application: &gtk::Application, lib_chan: events::LibEventSender) -> gtk::ListStore {
    let window = gtk::ApplicationWindow::new(application);

    window.set_title("music player");
    // window.set_border_width(1);
    window.set_position(gtk::WindowPosition::Mouse);
    window.set_default_size(600, 400);

    let header = header::build_header(lib_chan.clone());

    let (track_list_window, track_list_view, track_list_store) = track_list::build_track_list();

    track_list_view.connect_row_activated(move |v, _path, _col| {
        let sel = v.get_selection().get_selected().unwrap();
        let track_id: i64 = sel.0.get_value(&sel.1, 0).get().unwrap().unwrap();
        lib_chan
            .send(events::LibraryMsg::PlayTrack(track_id))
            .unwrap();
    });

    let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layout.add(&header);
    layout.add(&track_list_window);

    window.add(&layout);
    window.show_all();

    track_list_store
}

const IMPORT_ACTION: &'static str = "import";

fn build_menu_bar(app: &gtk::Application, lib_chan: events::LibEventSender) -> gio::Menu {
    // should the creation and registration of actions be separate?
    // not a fan of the nested closures. seems necessary for menu bar tho?
    let import_action = gio::SimpleAction::new(IMPORT_ACTION, None);
    import_action.connect_activate(move |_a, _v| {
        let chooser = gtk::FileChooserNativeBuilder::new()
            .title("title")
            .accept_label("import")
            .cancel_label("cancel")
            .action(gtk::FileChooserAction::SelectFolder)
            .build();

        let lib_chan_2 = lib_chan.clone();
        chooser.connect_response(move |chooser, resp_type| {
            if resp_type == gtk::ResponseType::Accept {
                match chooser.get_filename() {
                    Some(import_dir) => {
                        info!("importing {:?}", &import_dir);
                        lib_chan_2
                            .send(events::LibraryMsg::ImportDir(import_dir))
                            .unwrap();
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
pub struct AppState {
    tracklist: Option<gtk::ListStore>,
}

type AppStore = Arc<Mutex<AppState>>;

use glib::MainContext;
use log::LevelFilter;
use log4rs::append::console::ConsoleAppender;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Logger, Root};
use log4rs::encode::pattern::PatternEncoder;

// TODO library dir should be stored in db and checked for there first before
#[tokio::main]
pub async fn main() {
    // env_logger::init();
    // dotenv().ok();

    // on error here, prompt user for desired db path
    let bin_path = std::env::current_exe().unwrap();
    let db_dir = bin_path.parent().unwrap().to_path_buf();
    //
    let mut log_path = db_dir.clone();
    log_path.push("uigtklog.txt");
    let stdout = ConsoleAppender::builder().build();
    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} - {m}{n}")))
        .build(log_path)
        .unwrap();

    let config = Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .build(
            Root::builder()
                .appender("stdout")
                .appender("logfile")
                .build(LevelFilter::Trace),
        )
        .unwrap();

    let handle = log4rs::init_config(config).unwrap();

    let app_state = Arc::new(Mutex::new(AppState { tracklist: None }));

    let main_ctx = MainContext::default();
    if main_ctx.acquire() == false {
        panic!("failed to acquire main context");
    }

    let (tx_app, rx_app) = MainContext::channel(glib::PRIORITY_DEFAULT);
    let (tx_lib, rx_lib) = tokio_mpsc::unbounded_channel();

    rx_app.attach(Some(&main_ctx), events::app_event_loop(app_state.clone()));

    let lib = librarian::Library::open_or_create(db_dir).compat().await;
    tokio::spawn(async move { events::librarian_event_loop(lib, rx_lib, tx_app.clone()).await });

    let application = gtk::ApplicationBuilder::new()
        .application_id("nyc.jules.music-player")
        .flags(Default::default())
        .register_session(true)
        .build();
    let menubar = build_menu_bar(&application, tx_lib.clone());

    let tx_lib_2 = tx_lib.clone();
    let app_state_2 = app_state.clone();
    application.connect_activate(move |app| {
        app.set_menubar(Some(&menubar));
        let tracklist = build_ui(app, tx_lib_2.clone());

        app_state_2.lock().unwrap().tracklist = Some(tracklist);
        tx_lib_2.send(events::LibraryMsg::RefreshTracklist).unwrap();
    });

    let args: Vec<_> = env::args().collect();
    application.run(&args);
}
