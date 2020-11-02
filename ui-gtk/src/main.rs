extern crate dotenv;
extern crate env_logger;
extern crate gio;
extern crate glib;
extern crate gtk;
extern crate librarian;
extern crate log;
extern crate tokio;

use dotenv::dotenv;
use gio::prelude::*;
use gtk::prelude::*;
use std::env;

mod header;
mod track_list;

fn build_ui(application: &gtk::Application) {
    let window = gtk::ApplicationWindow::new(application);

    window.set_title("music player");
    // window.set_border_width(1);
    window.set_position(gtk::WindowPosition::None);
    window.set_default_size(600, 400);

    let header = header::build_header();

    let (track_list_view, track_list_store) = track_list::build_track_list();

    track_list_store.insert_with_values(
        None,
        &[0, 1],
        &[&format!("chicago"), &format!("roy ayers")],
    );
    track_list_store.insert_with_values(
        None,
        &[0, 1],
        &[&format!("cavern"), &format!("liquid liquid")],
    );

    let layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layout.add(&header);
    layout.add(&track_list_view);

    window.add(&layout);
    window.show_all();
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
    let import_mitem = gio::MenuItem::new(
        Some("Import"),
        Some(&format!("app.{}", IMPORT_ACTION)),
    );

    file_menu.append_item(&import_mitem);
    menubar.append_submenu(Some("File"), &file_menu);

    menubar
}

// TODO library dir should be stored in db and checked for there first before
#[tokio::main]
pub async fn main() {
    let application = gtk::ApplicationBuilder::new()
        .application_id("nyc.jules.music-player")
        .flags(Default::default())
        .register_session(true)
        .build();

    let menubar = build_menu_bar(&application);

    application.connect_activate(move |app| {
        app.set_menubar(Some(&menubar));
        build_ui(app);
    });

    application.run(&env::args().collect::<Vec<_>>());

    // env_logger::init();
    // dotenv().ok();

    // on error here, prompt user for desired db path
    // let bin_path = std::env::current_exe().unwrap();
    // let db_dir = bin_path.parent().unwrap().to_path_buf();

    // let lib = librarian::Library::open_or_create(db_dir).await;

    // librarian::import_dir(&db_pool, target.as_ref(), lib_path);
}
