use gio::prelude::*;
use glib::types::Type as GLibType;
use gtk::{
    prelude::*, CellRendererText, ListStore, PolicyType, ScrolledWindow,
    ScrolledWindowBuilder, ShadowType, TreeView, TreeViewBuilder,
    TreeViewColumn, TreeViewColumnBuilder, TreeViewGridLines,
};
use log::debug;

// eventually, we'll allow for more configuration with track list columns
// master list of columns + default setup
// allow for user config & store current column setup in config
// may be better as a part of an enum impl
const COLUMNS: &[(&'static str, GLibType)] = &[
    ("Id", GLibType::I64),
    ("Title", GLibType::String),
    ("Duration", GLibType::String),
    ("Artist", GLibType::String),
    ("Release", GLibType::String),
];

fn build_column(title: &str, pos: i32, is_visible: bool) -> TreeViewColumn {
    let cell = CellRendererText::new();

    let col = TreeViewColumnBuilder::new()
        .resizable(true)
        .reorderable(true)
        .title(title)
        .build();

    col.pack_start(&cell, true);
    // TODO make column data type configurable
    col.add_attribute(&cell, "text", pos);
    col.set_visible(is_visible);
    col
}

pub fn build_track_list() -> (ScrolledWindow, TreeView, ListStore) {
    // TODO is there a way to borrow these vals?
    let (column_names, column_types): (Vec<_>, Vec<_>) =
        COLUMNS.iter().cloned().unzip();

    // for now, all cols are strings. not sure of benefits of other types anyway
    let list = ListStore::new(&column_types);

    // if treeview doesn't work out, ListBox could be a viable alternative
    let view = TreeViewBuilder::new()
        .enable_grid_lines(TreeViewGridLines::Both)
        // .fixed_height_mode(true) // improves performance? set sizing needed
        .headers_visible(true)
        .headers_clickable(true)
        .rubber_banding(true)
        // .reorderable(true)
        .model(&list)
        .build();

    column_names.iter().enumerate().for_each(|(idx, name)| {
        let is_vis;
        if idx == 0 {
            is_vis = false;
        } else {
            is_vis = true;
        }
        let col = build_column(name, idx as i32, is_vis);
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

    (scroll_container, view, list)
}

pub fn insert_track(list: &ListStore, track: librarian::models::DetailedTrack) {
    // create a column -> Track property mapping?
    list.insert_with_values(
        None,
        &[0, 1, 3, 4], // FIXME add duration
        &[
            &track.id,
            &track.name,
            &track
                .artists
                .into_iter()
                .map(|a| a.name)
                .collect::<Vec<_>>()
                .join(", "),
            &track.release.name,
        ],
    );
}
