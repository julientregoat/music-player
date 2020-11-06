use gio::prelude::*;
use gtk::{
    prelude::*, CellRendererText, ListStore, PolicyType, ScrolledWindow, ScrolledWindowBuilder,
    ShadowType, TreeViewBuilder, TreeViewColumn, TreeViewColumnBuilder, TreeViewGridLines,
};

// eventually, we'll allow for more configuration with track list columns
// master list of columns + default setup
// allow for user config & store current column setup in config
// may be better as a part of an enum impl
const COLUMN_NAMES: &[&'static str] = &["Title", "Duration", "Artist", "Release"];

fn build_column(title: &str, pos: i32) -> TreeViewColumn {
    let cell = CellRendererText::new();

    let col = TreeViewColumnBuilder::new()
        .resizable(true)
        .reorderable(true)
        .title(title)
        .build();

    col.pack_start(&cell, true);
    // TODO make column data type configurable
    col.add_attribute(&cell, "text", pos);
    col
}

pub fn build_track_list() -> (ScrolledWindow, ListStore) {
    let column_types: Vec<_> = COLUMN_NAMES
        .into_iter()
        .map(|_c| String::static_type())
        .collect();

    // for now, all cols are strings. not sure of benefits of other types anyway
    let list = ListStore::new(&column_types);

    let view = TreeViewBuilder::new()
        .enable_grid_lines(TreeViewGridLines::Both)
        // .fixed_height_mode(true) // improves performance?
        // .enable_search(true)
        .headers_visible(true)
        // .reorderable(true)
        .model(&list)
        .build();

    COLUMN_NAMES.iter().enumerate().for_each(|(idx, name)| {
        let col = build_column(name, idx as i32);
        view.append_column(&col);
    });

    let scroll_container = ScrolledWindowBuilder::new()
        .shadow_type(ShadowType::EtchedIn)
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        // .border_width(5)
        .child(&view)
        // .vexpand_set(true)
        .build();

    (scroll_container, list)
}

pub fn insert_track(list: &ListStore, track: librarian::models::DetailedTrack) {
    // create a column -> Track property mapping?
    list.insert_with_values(
        None,
        &[0, 2, 3], // FIXME add duration
        &[
            &track.name,
            &track
                .artists
                .into_iter()
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
                .join(", "),
            &track.release.name,
        ],
    );
}
