extern crate gio;
extern crate glib;
extern crate gtk;
extern crate tokio;
extern crate log;
extern crate env_logger;
extern crate dotenv;
extern crate librarian;

use gio::prelude::*;
use gtk::prelude::*;
use std::env;
use dotenv::dotenv;

fn build_ui(application: &gtk::Application) {
    let window = gtk::ApplicationWindow::new(application);

    window.set_title("First GTK+ Clock");
    window.set_border_width(10);
    window.set_position(gtk::WindowPosition::Center);
    window.set_default_size(260, 40);

    let label = gtk::Label::new(None);
    label.set_text("hi");

    window.add(&label);

    window.show_all();

    // we are using a closure to capture the label (else we could also use a normal function)
    // let tick = move || {
    //     label.set_text("new text");
    //     // we could return glib::Continue(false) to stop our clock after this tick
    //     glib::Continue(true)
    // };

    // executes the closure once every second
    // gtk::timeout_add_seconds(1, tick);
}

// TODO library dir should be stored in db and checked for there first before
#[tokio::main]
pub async fn main() {
    // if gtk::init().is_err() {
    //     println!("Failed to initialize GTK.");
    //     return;
    // }
    // MessageDialog::new(None::<&Window>,
    //                    DialogFlags::empty(),
    //                    MessageType::Info,
    //                    ButtonsType::Ok,
    //                    "Hello World").run();

    let application = gtk::Application::new(
        Some("com.github.gtk-rs.examples.clock"),
        Default::default(),
    )
    .expect("Initialization failed...");

    application.connect_activate(|app| {
        build_ui(app);
    });

    application.run(&env::args().collect::<Vec<_>>());

    env_logger::init();
    dotenv().ok();

    // on error here, prompt user for desired db path
    let bin_path = std::env::current_exe().unwrap();
    let db_path = bin_path.parent().unwrap();

    let lib = librarian::Library::new(db_path.to_str().unwrap()).await;

    // librarian::import_dir(&db_pool, target.as_ref(), lib_path);
}