use gio::prelude::*;
use gtk::{
    prelude::*, Align, Button, ButtonBox, HeaderBar, HeaderBarBuilder, Orientation,
    SearchEntryBuilder,
};

fn build_music_controls() -> ButtonBox {
    let music_controls = ButtonBox::new(Orientation::Horizontal);
    // TODO replace with icons
    let play_btn = Button::with_label("play");
    play_btn.set_tooltip_text(Some("dat funky music"));
    let pause_btn = Button::with_label("pause");

    music_controls.pack_start(&play_btn, false, false, 0);
    music_controls.pack_start(&pause_btn, false, false, 0);

    music_controls
}

pub fn build_header() -> HeaderBar {
    let entry = SearchEntryBuilder::new()
        .editable(true)
        .placeholder_text("search")
        .name("test")
        .has_focus(false)
        .has_default(false)
        .build();

    let music_controls = build_music_controls();

    let header = HeaderBarBuilder::new()
        .title("now playing:")
        .hexpand(true)
        .valign(Align::Start)
        .build();

    header.pack_start(&music_controls);
    header.pack_end(&entry);

    header
}
