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
use gtk::CssProvider;
use std::env;

fn build_ui(application: &gtk::Application) {
    let window = gtk::ApplicationWindow::new(application);

    window.set_title("music player");
    // window.set_border_width(1);
    window.set_position(gtk::WindowPosition::None);
    window.set_default_size(600, 400);

    let css = CssProvider::new();
    css.load_from_path("./test.css").unwrap();

    // for some reason, css doesn't fully work here
    // let style_ctx = window.get_style_context();

    // here, everything css works
    let label = gtk::Label::new(None);
    label.set_text("music player");
    // label.set_halign(gtk::Align::Center);

    // let label_ctx = label.get_style_context();
    // label_ctx.add_class("poop");
    // label_ctx.add_provider(&css, 0);

    window.add(&label);
    window.show_all();
}

// TODO library dir should be stored in db and checked for there first before
#[tokio::main]
pub async fn main() {
    let application = gtk::ApplicationBuilder::new()
        .application_id("nyc.jules.music-player")
        .flags(Default::default())
        .register_session(true)
        .build();

    let import_action = gio::SimpleAction::new("import", None);
    import_action.connect_activate(|_a, _v| {
        let chooser = gtk::FileChooserNativeBuilder::new()
            .title("title")
            .accept_label("import")
            .cancel_label("cancel")
            .action(gtk::FileChooserAction::SelectFolder)
            .build();

        chooser.run();
    });
    application.add_action(&import_action);

    let menubar = gio::Menu::new();
    let file_menu = gio::Menu::new();
    let import_mitem = gio::MenuItem::new(Some("Import"), Some("app.import"));

    file_menu.append_item(&import_mitem);
    menubar.append_submenu(Some("File"), &file_menu);

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
