use gio::prelude::*;
use gtk::{
    prelude::*, Align, Button, ButtonBox, HeaderBar, HeaderBarBuilder,
    Orientation, SearchEntryBuilder,
};

use crate::events;

fn build_music_controls(lib_chan: events::LibEventSender) -> ButtonBox {
    let music_controls = ButtonBox::new(Orientation::Horizontal);
    // TODO replace with icons
    let play_btn = Button::with_label("play");
    play_btn.set_tooltip_text(Some("dat funky music"));
    let chan1 = lib_chan.clone();
    play_btn.connect_clicked(move |_b| {
        chan1.send(events::LibraryMsg::PlayStream).unwrap();
    });

    let pause_btn = Button::with_label("pause");
    pause_btn.connect_clicked(move |_b| {
        lib_chan.send(events::LibraryMsg::PauseStream).unwrap();
    });

    music_controls.pack_start(&play_btn, false, false, 0);
    music_controls.pack_start(&pause_btn, false, false, 0);

    music_controls
}

pub fn build_header(lib_chan: events::LibEventSender) -> HeaderBar {
    let entry = SearchEntryBuilder::new()
        .editable(true)
        .placeholder_text("search")
        .name("test")
        .has_focus(false)
        .has_default(false)
        .build();

    let music_controls = build_music_controls(lib_chan);

    let header = HeaderBarBuilder::new()
        .title("now playing:")
        .hexpand(true)
        .valign(Align::Start)
        .build();

    header.pack_start(&music_controls);
    header.pack_end(&entry);

    header
}
