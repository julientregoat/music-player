use gio::prelude::*;
use gtk::{
    prelude::*, CellRendererText, ListStore, TreeView, TreeViewBuilder, TreeViewColumn,
    TreeViewGridLines,
};

// eventually, we'll allow for more configuration with track list columns
// master list of columns + default set of columns which is stored in the
// config and updated as the user does
const COLUMN_NAMES: &[&'static str] = &["Title", "Artist", "Album", "Release"];

fn build_column(title: &str, pos: i32) -> TreeViewColumn {
    let col = TreeViewColumn::new();
    let cell = CellRendererText::new();
    col.pack_start(&cell, true);
    // TODO make column data type configurable
    col.add_attribute(&cell, "text", pos);
    col.set_resizable(true);
    col.set_title(title);
    col
}

pub fn build_track_list() -> (TreeView, ListStore) {
    let view = TreeViewBuilder::new()
        .enable_grid_lines(TreeViewGridLines::Both)
        // .fixed_height_mode(true)
        // .enable_search(true)
        .headers_visible(true)
        // .reorderable(true)
        // .valign(Align::End)
        .build();

    let column_types: Vec<_> = COLUMN_NAMES
        .into_iter()
        .map(|_c| String::static_type())
        .collect();

    // for now, all cols are strings. not sure of benefits of other types anyway
    let list = ListStore::new(&column_types);
    view.set_model(Some(&list));

    COLUMN_NAMES.iter().enumerate().for_each(|(idx, name)| {
        let col = build_column(name, idx as i32);
        view.append_column(&col);
    });

    (view, list)
}
