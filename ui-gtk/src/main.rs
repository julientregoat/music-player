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
use std::env;
use std::sync::mpsc;
use tokio_compat_02::FutureExt;

mod header;
mod track_list;

fn build_ui(application: &gtk::Application) -> App {
    let window = gtk::ApplicationWindow::new(application);

    window.set_title("music player");
    // window.set_border_width(1);
    window.set_position(gtk::WindowPosition::None);
    window.set_default_size(600, 400);

    let header = header::build_header();

    let (track_list, track_list_store) = track_list::build_track_list();

    for _ in 0..100 {
        track_list_store.insert_with_values(
            None,
            &[0, 2],
            &[&format!("cavern"), &format!("liquid liquid")],
        );
    }

    let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layout.add(&header);
    layout.add(&track_list);

    window.add(&layout);
    window.show_all();

    App {
        tracklist: track_list_store,
    }
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
struct App {
    tracklist: gtk::ListStore,
}

enum AppEvent {
    Tracklist(Vec<librarian::models::DetailedTrack>),
}

enum LibraryCommand {
    RefreshTracklist,
}

fn event_loop(app: App, chan: &mpsc::Receiver<AppEvent>) {
    while let Ok(msg) = chan.recv() {
        match msg {
            AppEvent::Tracklist(tracks) => {
                app.tracklist.clear();
                for track in tracks {
                    track_list::insert_track(&app.tracklist, track)
                }
            }
        }
    }
}

// TODO library dir should be stored in db and checked for there first before
#[tokio::main]
pub async fn main() {
    env_logger::init();
    // dotenv().ok();

    // on error here, prompt user for desired db path
    let bin_path = std::env::current_exe().unwrap();
    let db_dir = bin_path.parent().unwrap().to_path_buf();
    let lib = librarian::Library::open_or_create(db_dir).compat().await;
    // librarian::import_dir(
    //     &lib.db_pool,
    //     std::path::Path::new("/Users/jtregoat/Code/music-player/"),
    //     std::path::Path::new("/Users/jtregoat/Downloads/transmission").to_path_buf(),
    // )
    // .compat()
    // .await;

    // for row in result {
    //     println!("detailed result {:?}", row);
    // }

    let application = gtk::ApplicationBuilder::new()
        .application_id("nyc.jules.music-player")
        .flags(Default::default())
        .register_session(true)
        .build();

    let menubar = build_menu_bar(&application);

    let (tx, rx) = mpsc::channel();

    application.connect_activate(move |app| {
        app.set_menubar(Some(&menubar));
        let a = build_ui(app);
        event_loop(a, &rx)
    });

    tokio::spawn(async move {
        let mut conn = lib.db_pool.acquire().compat().await.unwrap();
        let result = librarian::models::Track::get_all_detailed(&mut conn)
            .compat()
            .await
            .unwrap();

        tx.send(AppEvent::Tracklist(result)).unwrap();
    });

    application.run(&env::args().collect::<Vec<_>>());
}
