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

    // css doesnt work on windows
    // I need to create a container that fills the window and apply styles to that
    // let style_ctx = window.get_style_context();

    let entry = gtk::SearchEntryBuilder::new()
        .editable(true)
        .placeholder_text("search")
        .name("test")
        .has_focus(false)
        .has_default(false)
        .build();

    let music_controls = gtk::ButtonBox::new(gtk::Orientation::Horizontal);
    // TODO replace with icons
    let play_btn = gtk::Button::with_label("play");
    play_btn.set_tooltip_text(Some("dat funky music"));
    let pause_btn = gtk::Button::with_label("pause");

    music_controls.pack_start(&play_btn, false, false, 0);
    music_controls.pack_start(&pause_btn, false, false, 0);

    let header = gtk::HeaderBarBuilder::new()
        .title("now playing:")
        .hexpand(true)
        .valign(gtk::Align::Start)
        .build();

    header.pack_start(&music_controls);
    header.pack_end(&entry);

    let track_list_view = gtk::TreeViewBuilder::new()
        .enable_grid_lines(gtk::TreeViewGridLines::Both)
        // .fixed_height_mode(true)
        // .enable_search(true)
        .headers_visible(true)
        // .reorderable(true)
        // .valign(gtk::Align::End)
        .build();

    let track_list = gtk::ListStore::new(&[String::static_type(), String::static_type()]);
    track_list_view.set_model(Some(&track_list));
    track_list_view.set_headers_visible(true);

    let title_col = gtk::TreeViewColumn::new();
    let title_cell = gtk::CellRendererText::new();
    title_col.pack_start(&title_cell, true);
    title_col.add_attribute(&title_cell, "text", 0);
    title_col.set_resizable(true);
    title_col.set_title("Track Name");
    track_list_view.append_column(&title_col);

    let artist_col = gtk::TreeViewColumn::new();
    let artist_cell = gtk::CellRendererText::new();
    artist_col.pack_start(&artist_cell, true);
    artist_col.add_attribute(&artist_cell, "text", 1);
    artist_col.set_resizable(true);
    artist_col.set_title("Artist");
    track_list_view.append_column(&artist_col);

    track_list.insert_with_values(None, &[0, 1], &[&format!("chicago"), &format!("roy ayers")]);
    track_list.insert_with_values(
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

// TODO check out cell + tree view for track listing - or grid?

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
