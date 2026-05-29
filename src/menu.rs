// SPDX-License-Identifier: GPL-3.0

use crate::app::{AppModel, MenuAction, Message};
use crate::fl;

use cosmic::Application;
use cosmic::{
    Element,
    widget::{
        menu::{self, ItemHeight, ItemWidth},
        responsive_menu_bar,
    },
};
use std::sync::LazyLock;

static MENU_ID: LazyLock<cosmic::widget::Id> =
    LazyLock::new(|| cosmic::widget::Id::new("responsive_menu"));

pub fn menu_bar<'a>(app: &AppModel) -> Element<'a, Message> {
    let file_items = vec![
        menu::Item::Button(fl!("open"), None, MenuAction::Open),
        menu::Item::Divider,
        menu::Item::Button(fl!("quit"), None, MenuAction::Quit),
    ];

    let view_items = vec![
        menu::Item::Button(fl!("settings-menu"), None, MenuAction::Settings),
        menu::Item::Divider,
        menu::Item::Button(fl!("about-rpged64"), None, MenuAction::About),
    ];

    responsive_menu_bar()
        .item_height(ItemHeight::Dynamic(40))
        .item_width(ItemWidth::Uniform(260))
        .spacing(1.0)
        .into_element(
            app.core(),
            &app.key_binds,
            MENU_ID.clone(),
            Message::Surface,
            vec![(fl!("file"), file_items), (fl!("view"), view_items)],
        )
}
