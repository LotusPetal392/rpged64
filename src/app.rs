// SPDX-License-Identifier: GPL-3.0

use crate::config::{AppTheme, CONFIG_VERSION, Config, State};
use crate::fl;
use crate::key_binds::key_binds;
use crate::menu::menu_bar;

use cosmic::prelude::*;
use cosmic::{
    app::context_drawer,
    cosmic_config::{self, CosmicConfigEntry},
    dialog::file_chooser,
    iced::{
        Length, Size, Subscription,
        alignment::{Horizontal, Vertical},
        event::{self, Event},
        keyboard::{Key, Modifiers, key::Physical},
        window::Event as WindowEvent,
    },
    surface,
    widget::{self, about::About, menu, nav_bar, segmented_button, settings, tab_bar},
};
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    fs, io,
    path::PathBuf,
    process,
};

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/apps/com.galacticpirateradio.rpged64.svg");

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// Display a context drawer with the designated page if defined.
    context_page: ContextPage,
    /// The about page this app.
    about: About,
    /// Contains items assigned to the nav bar panel.
    nav: nav_bar::Model,
    /// Contains open editor tabs.
    tabs: segmented_button::SingleSelectModel,
    /// Key bindings for the application's menu bar.
    pub key_binds: HashMap<menu::KeyBind, MenuAction>,
    /// Configuration data that persists between application runs.
    pub config: Config,
    /// Settings page / app theme dropdown labels
    app_theme_labels: Vec<String>,

    pub is_condensed: bool,

    config_handler: Option<cosmic_config::Config>,
    state_handler: Option<cosmic_config::Config>,
    pub state: crate::config::State,
    project: Option<Project>,
    collapsed_project_file_kinds: HashSet<ProjectFileKind>,
    selected_project_file: Option<PathBuf>,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    AppTheme(AppTheme),
    KeyPressed {
        key: Key,
        modifiers: Modifiers,
        physical_key: Physical,
    },
    LaunchUrl(String),
    OpenProject,
    ProjectFolderDialogCancelled,
    ProjectFolderDialogError(String),
    ProjectFolderSelected(PathBuf),
    TabActivated(segmented_button::Entity),
    TabClosed(segmented_button::Entity),
    Quit,
    Surface(surface::Action),
    ToggleContextPage(ContextPage),
    UpdateConfig(Config),
    WindowResized(Size),
}

/// Unique identifier in RDNN (reverse domain name notation) format.
pub const APP_ID: &'static str = "com.galacticpirateradio.rpged64";

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = Flags;

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = APP_ID;

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        // Create a nav bar with three page items.
        let nav = nav_bar::Model::default();

        // Create the about widget
        let about = About::default()
            .name(fl!("app-title"))
            .icon(widget::icon::from_svg_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .links([(fl!("repository"), REPOSITORY)])
            .license(env!("CARGO_PKG_LICENSE"));

        // Construct the app model with the runtime's core.
        let mut app = AppModel {
            core,
            context_page: ContextPage::default(),
            about,
            nav,
            tabs: segmented_button::Model::default(),
            key_binds: key_binds(),
            config: cosmic_config::Config::new(APP_ID, CONFIG_VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => config,
                })
                .unwrap_or_default(),
            app_theme_labels: vec![fl!("match-desktop"), fl!("dark"), fl!("light")],

            is_condensed: false,
            config_handler: _flags.config_handler,
            state_handler: _flags.state_handler,
            state: _flags.state.clone(),
            project: None,
            collapsed_project_file_kinds: HashSet::new(),
            selected_project_file: None,
        };

        // Create a startup command that sets the window title.
        let update_title = app.update_title();

        (app, Task::batch([update_title]))
    }

    /// Elements to pack at the start of the header bar.
    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        let menu_bar = menu_bar(self);
        vec![menu_bar.into()]
    }

    /// Enables the COSMIC application to create a nav bar with this model.
    fn nav_model(&self) -> Option<&nav_bar::Model> {
        Some(&self.nav)
    }

    /// Display a context drawer if the context page is requested.
    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }

        Some(match self.context_page {
            ContextPage::About => context_drawer::about(
                &self.about,
                |url| Message::LaunchUrl(url.to_string()),
                Message::ToggleContextPage(ContextPage::About),
            ),
            ContextPage::Settings => context_drawer::context_drawer(
                self.settings(),
                Message::ToggleContextPage(ContextPage::Settings),
            )
            .title(fl!("settings")),
        })
    }

    /// Describes the interface based on the current state of the application model.
    fn view(&self) -> Element<'_, Self::Message> {
        let content: Element<'_, Self::Message> = if self.tabs.len() == 0 {
            match &self.project {
                Some(project) => widget::column([
                    widget::text(format!("Project: {}", project.root.display())).into(),
                    widget::text(format!("{} project files", project.files.len())).into(),
                    widget::text(fl!("select-file-to-open")).into(),
                ])
                .into(),
                None => widget::column([widget::text(fl!("no-project-open")).into()]).into(),
            }
        } else {
            let tab_bar = tab_bar::horizontal(&self.tabs)
                .show_close_icon_on_hover(true)
                .on_activate(Message::TabActivated)
                .on_close(Message::TabClosed)
                .width(Length::Fill)
                .into();

            widget::column([tab_bar, self.active_tab_view()]).into()
        };

        widget::container(content)
            .apply(widget::container)
            .height(Length::Fill)
            .width(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Top)
            .into()
    }

    /// Register subscriptions for this application.
    fn subscription(&self) -> Subscription<Self::Message> {
        // Add subscriptions which are always active.
        let subscriptions = vec![
            event::listen_with(|event, _status, _window_id| match event {
                Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed {
                    key,
                    modifiers,
                    physical_key,
                    ..
                }) => Some(Message::KeyPressed {
                    key,
                    modifiers,
                    physical_key,
                }),
                Event::Window(WindowEvent::CloseRequested) => Some(Message::Quit),
                Event::Window(WindowEvent::Closed) => Some(Message::Quit),
                Event::Window(WindowEvent::Resized(size)) => Some(Message::WindowResized(size)),
                _ => None,
            }),
            // Watch for application configuration changes.
            self.core().watch_config::<Config>(APP_ID).map(|update| {
                // for why in update.errors {
                //     tracing::error!(?why, "app config error");
                // }

                Message::UpdateConfig(update.config)
            }),
        ];

        Subscription::batch(subscriptions)
    }

    /// Handles messages emitted by the application and its widgets.
    ///
    /// Tasks may be returned for asynchronous execution of code in the background
    /// on the application's async runtime.
    fn update(&mut self, message: Self::Message) -> cosmic::Task<cosmic::Action<Self::Message>> {
        self.is_condensed = self.core().is_condensed();

        // Helper for updating configuration
        macro_rules! config_set {
            ($name: ident, $value: expr) => {
                match &self.config_handler {
                    Some(config_handler) => {
                        match paste::paste! { self.config.[<set_ $name>](&config_handler, $value) }
                        {
                            Ok(_) => {}
                            Err(err) => {
                                log::warn!(
                                    "failed to save config {:?}: {}",
                                    stringify!($name),
                                    err
                                );
                            }
                        }
                    }
                    None => {
                        self.config.$name = $value;
                        log::warn!(
                            "failed to save config {:?}: no config handler",
                            stringify!($name)
                        );
                    }
                }
            };
        }

        // Helper for updating application state
        macro_rules! state_set {
            ($name: ident, $value: expr) => {
                match &self.state_handler {
                    Some(state_handler) => {
                        match paste::paste! { self.state.[<set_ $name>](&state_handler, $value) } {
                            Ok(_) => {}
                            Err(err) => {
                                log::warn!("failed to save state {:?}: {}", stringify!($name), err);
                            }
                        }
                    }
                    None => {
                        self.state.$name = $value;
                        log::warn!(
                            "failed to save state {:?}: no state (config) handler",
                            stringify!($name)
                        );
                    }
                }
            };
        }

        match message {
            Message::AppTheme(app_theme) => {
                config_set!(app_theme, app_theme);
                return self.update_config();
            }

            Message::KeyPressed {
                key,
                modifiers,
                physical_key,
            } => {
                if let Some(action) = self.key_binds.iter().find_map(|(key_bind, action)| {
                    key_bind
                        .matches(modifiers, &key, Some(&physical_key))
                        .then_some(action)
                }) {
                    return self.update(<MenuAction as menu::action::MenuAction>::message(action));
                }
            }

            Message::LaunchUrl(url) => match open::that_detached(&url) {
                Ok(()) => {}
                Err(err) => {
                    eprintln!("failed to open {url:?}: {err}");
                }
            },

            Message::OpenProject => {
                return cosmic::task::future(async move {
                    let dialog = file_chooser::open::Dialog::new()
                        .title(fl!("open-project-folder-title"))
                        .accept_label(fl!("open"));

                    match dialog.open_folder().await {
                        Ok(response) => match response.url().to_file_path() {
                            Ok(path) => Message::ProjectFolderSelected(path),
                            Err(()) => Message::ProjectFolderDialogError(format!(
                                "{} is not a local folder path",
                                response.url()
                            )),
                        },
                        Err(file_chooser::Error::Cancelled) => {
                            Message::ProjectFolderDialogCancelled
                        }
                        Err(err) => Message::ProjectFolderDialogError(err.to_string()),
                    }
                });
            }

            Message::ProjectFolderDialogCancelled => {}

            Message::ProjectFolderDialogError(err) => {
                eprintln!("failed to open project folder dialog: {err}");
            }

            Message::ProjectFolderSelected(path) => match Project::open(path) {
                Ok(project) => {
                    self.project = Some(project);
                    self.tabs.clear();
                    self.collapsed_project_file_kinds.clear();
                    self.selected_project_file = None;
                    self.rebuild_project_nav();
                    return self.update_title();
                }
                Err(err) => {
                    eprintln!("failed to open project: {err}");
                }
            },

            Message::TabActivated(id) => {
                self.tabs.activate(id);
            }

            Message::TabClosed(id) => {
                self.close_tab(id);
            }

            Message::Quit => {
                process::exit(0);
            }

            Message::Surface(action) => {
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(action),
                ));
            }

            Message::ToggleContextPage(context_page) => {
                if self.context_page == context_page {
                    // Close the context drawer if the toggled context page is the same.
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    // Open the context drawer to display the requested context page.
                    self.context_page = context_page;
                    self.core.window.show_context = true;
                }
            }

            Message::UpdateConfig(config) => {
                self.config = config;
            }

            Message::WindowResized(size) => {
                let window_width = size.width;
                let window_height = size.height;
                state_set!(window_width, window_width);
                state_set!(window_height, window_height);
            }
        }
        Task::none()
    }

    /// Called when a nav item is selected.
    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<cosmic::Action<Self::Message>> {
        match self.nav.data::<ProjectNavItem>(id).cloned() {
            Some(ProjectNavItem::Category(kind)) => {
                if !self.collapsed_project_file_kinds.insert(kind) {
                    self.collapsed_project_file_kinds.remove(&kind);
                }
                self.rebuild_project_nav();
            }
            Some(ProjectNavItem::File(path)) => {
                self.selected_project_file = Some(path.clone());
                self.open_file_tab(path);
                self.nav.activate(id);
            }
            None => {
                self.nav.activate(id);
            }
        }

        self.update_title()
    }

    /// Footer area
    fn footer(&self) -> Option<Element<'_, Message>> {
        None
    }
}

impl AppModel {
    /// Updates the header and window titles.
    pub fn update_title(&mut self) -> Task<cosmic::Action<Message>> {
        let mut window_title = fl!("app-title");

        let page = self.nav.text(self.nav.active());

        if page.is_some() {
            window_title.push_str(" — ");
            window_title.push_str(page.unwrap());
        }

        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(window_title, id)
        } else {
            Task::none()
        }
    }

    fn active_tab_view(&self) -> Element<'_, Message> {
        let Some(tab) = self.tabs.active_data::<EditorTab>() else {
            return widget::container(widget::text(fl!("select-file-to-open")))
                .height(Length::Fill)
                .width(Length::Fill)
                .align_x(Horizontal::Center)
                .align_y(Vertical::Center)
                .into();
        };

        let editor_content = match tab.kind {
            ProjectFileKind::DungeonRoomSet => fl!("dungeon-room-set-editor"),
            ProjectFileKind::Map => fl!("map-editor"),
            ProjectFileKind::MetaTiles => fl!("meta-tiles-editor"),
            ProjectFileKind::ProjectConfig => fl!("project-config-editor"),
            ProjectFileKind::TileSet2d => fl!("tile-set-2d-editor"),
            ProjectFileKind::TileSet3d => fl!("tile-set-3d-editor"),
            ProjectFileKind::Unknown => fl!("unknown-file-editor"),
        };

        widget::container(widget::column([
            widget::text(format!("{} — {}", tab.name, tab.kind.editor_title())).into(),
            widget::text(tab.path.display().to_string()).into(),
            widget::text(editor_content).into(),
        ]))
        .height(Length::Fill)
        .width(Length::Fill)
        .align_x(Horizontal::Center)
        .align_y(Vertical::Top)
        .into()
    }

    fn close_tab(&mut self, id: segmented_button::Entity) {
        if self.tabs.is_active(id) {
            if let Some(position) = self.tabs.position(id) {
                let next_position = if position == 0 {
                    position + 1
                } else {
                    position - 1
                };
                self.tabs.activate_position(next_position);
            }
        }

        self.tabs.remove(id);
    }

    fn open_file_tab(&mut self, path: PathBuf) {
        let existing_tab = self.tabs.iter().find(|id| {
            self.tabs
                .data::<EditorTab>(*id)
                .is_some_and(|tab| tab.path == path)
        });

        if let Some(id) = existing_tab {
            self.tabs.activate(id);
            return;
        }

        let Some(project_file) = self
            .project
            .as_ref()
            .and_then(|project| project.files.iter().find(|file| file.path == path))
        else {
            return;
        };

        let tab = EditorTab {
            path: project_file.path.clone(),
            name: project_file.name.clone(),
            kind: project_file.kind,
        };

        let id = self
            .tabs
            .insert()
            .text(tab.name.clone())
            .closable()
            .data(tab)
            .id();
        self.tabs.activate(id);
    }

    fn rebuild_project_nav(&mut self) {
        self.nav.clear();

        let Some(project) = &self.project else {
            return;
        };

        for kind in ProjectFileKind::ALL {
            let files = project
                .files
                .iter()
                .filter(|file| file.kind == kind)
                .collect::<Vec<_>>();

            if files.is_empty() {
                continue;
            }

            let collapsed = self.collapsed_project_file_kinds.contains(&kind);
            let triangle = if collapsed { "▸" } else { "▾" };
            self.nav
                .insert()
                .text(format!("{triangle} {}", kind.label()))
                .data(ProjectNavItem::Category(kind));

            if collapsed {
                continue;
            }

            for file in files {
                self.nav
                    .insert()
                    .text(file.name.clone())
                    .indent(1)
                    .data(ProjectNavItem::File(file.path.clone()));
            }
        }
    }

    /// Settings page content
    fn settings(&self) -> Element<'_, Message> {
        let app_theme_selected = match self.config.app_theme {
            AppTheme::Dark => 1,
            AppTheme::Light => 2,
            AppTheme::System => 0,
        };

        settings::view_column(vec![
            settings::section()
                .title(fl!("appearance"))
                .add({
                    widget::settings::item::builder(fl!("theme")).control(widget::dropdown(
                        &self.app_theme_labels,
                        Some(app_theme_selected),
                        move |index| {
                            Message::AppTheme(match index {
                                1 => AppTheme::Dark,
                                2 => AppTheme::Light,
                                _ => AppTheme::System,
                            })
                        },
                    ))
                })
                .into(),
        ])
        .into()
    }

    /// Updates the cosmic config, in particular the theme
    fn update_config(&mut self) -> Task<cosmic::Action<Message>> {
        cosmic::command::set_theme(self.config.app_theme.theme())
    }
}

/// Flags passed into the app
#[derive(Clone, Debug)]
pub struct Flags {
    pub config_handler: Option<cosmic_config::Config>,
    pub state_handler: Option<cosmic_config::Config>,
    pub state: State,
}

/// The context page to display in the context drawer.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
    Settings,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    About,
    Open,
    Quit,
    Settings,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
            MenuAction::Open => Message::OpenProject,
            MenuAction::Quit => Message::Quit,
            MenuAction::Settings => Message::ToggleContextPage(ContextPage::Settings),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProjectFileKind {
    DungeonRoomSet,
    Map,
    MetaTiles,
    ProjectConfig,
    TileSet2d,
    TileSet3d,
    Unknown,
}

#[derive(Clone, Debug)]
enum ProjectNavItem {
    Category(ProjectFileKind),
    File(PathBuf),
}

#[derive(Clone, Debug)]
struct EditorTab {
    path: PathBuf,
    name: String,
    kind: ProjectFileKind,
}

pub struct ProjectFile {
    pub path: PathBuf,
    pub name: String,
    pub kind: ProjectFileKind,
}

pub struct Project {
    pub root: PathBuf,
    pub files: Vec<ProjectFile>,
}

impl Project {
    pub fn open(root: PathBuf) -> io::Result<Self> {
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a directory", root.display()),
            ));
        }

        let mut files = Vec::new();
        let mut pending = vec![root.clone()];

        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                let path = entry.path();
                let file_type = entry.file_type()?;

                if file_type.is_dir() {
                    pending.push(path);
                    continue;
                }

                if file_type.is_file() {
                    let name = path
                        .file_stem()
                        .or_else(|| path.file_name())
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();

                    let kind = ProjectFileKind::from_path(&path);

                    files.push(ProjectFile { path, name, kind });
                }
            }
        }

        files.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(Self { root, files })
    }
}

impl ProjectFileKind {
    const ALL: [Self; 7] = [
        Self::ProjectConfig,
        Self::Map,
        Self::MetaTiles,
        Self::TileSet2d,
        Self::TileSet3d,
        Self::DungeonRoomSet,
        Self::Unknown,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::DungeonRoomSet => "Dungeon Room Sets",
            Self::Map => "Maps",
            Self::MetaTiles => "Meta Tiles",
            Self::ProjectConfig => "Project Configs",
            Self::TileSet2d => "2D Tile Sets",
            Self::TileSet3d => "3D Tile Sets",
            Self::Unknown => "Other Files",
        }
    }

    fn editor_title(self) -> &'static str {
        match self {
            Self::DungeonRoomSet => "Dungeon Room Set Editor",
            Self::Map => "Map Editor",
            Self::MetaTiles => "Meta Tiles Editor",
            Self::ProjectConfig => "Project Config Editor",
            Self::TileSet2d => "2D Tile Set Editor",
            Self::TileSet3d => "3D Tile Set Editor",
            Self::Unknown => "File Viewer",
        }
    }

    fn from_path(path: &std::path::Path) -> Self {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("2dtiles") => Self::TileSet2d,
            Some("3dtiles") => Self::TileSet3d,
            Some("map") => Self::Map,
            Some("metatiles") => Self::MetaTiles,
            Some("rooms") => Self::DungeonRoomSet,
            Some("toml") => Self::ProjectConfig,
            _ => Self::Unknown,
        }
    }
}
