use adw::prelude::*;
use libadwaita as adw;
use relm4::gtk;
use relm4::prelude::*;

use crate::config::PlaybackConfig;
use crate::db::connection::DatabaseConnection;
use crate::player::{SubtitleStyle, language};
use crate::services::config_service::CONFIG_SERVICE;

#[tracker::track]
#[derive(Debug)]
#[allow(dead_code)]
pub struct PreferencesDialog {
    #[do_not_track]
    db: DatabaseConnection,
    // Player preferences
    default_player: String,
    hardware_acceleration: bool,
    // Display preferences
    items_per_page: i32,
    // Cache preferences
    cache_size_mb: i32,
    auto_clean_cache: bool,
    #[do_not_track]
    subtitle_style_rows: SubtitleStyleRows,
}

/// The rows that make up the subtitle style, read together whenever one changes
#[derive(Debug)]
struct SubtitleStyleRows {
    font_size: adw::SpinRow,
    color: gtk::ColorDialogButton,
    border_size: adw::SpinRow,
    background: adw::SwitchRow,
}

impl SubtitleStyleRows {
    fn style(&self) -> SubtitleStyle {
        let rgba = self.color.rgba();
        let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        SubtitleStyle {
            font_size: self.font_size.value() as u32,
            color: format!(
                "#{:02X}{:02X}{:02X}",
                channel(rgba.red()),
                channel(rgba.green()),
                channel(rgba.blue())
            ),
            border_size: self.border_size.value(),
            background: self.background.is_active(),
        }
    }
}

/// A language combo row whose first entry is `none_label` (stored as an empty code)
fn language_row(
    title: &str,
    subtitle: &str,
    none_label: &str,
    selected_code: &str,
    on_change: impl Fn(String) + 'static,
) -> adw::ComboRow {
    let mut labels = vec![none_label];
    labels.extend(language::LANGUAGES.iter().map(|l| l.name));
    let row = adw::ComboRow::builder()
        .title(title)
        .subtitle(subtitle)
        .model(&gtk::StringList::new(&labels))
        .build();
    let selected = language::LANGUAGES
        .iter()
        .position(|l| l.code == selected_code)
        .map_or(0, |index| index + 1);
    row.set_selected(selected as u32);
    row.connect_selected_notify(move |row| {
        let code = match row.selected() {
            0 => String::new(),
            index => language::LANGUAGES[index as usize - 1].code.to_string(),
        };
        on_change(code);
    });
    row
}

/// The "Playback" page: preferred track languages and subtitle styling
fn build_playback_page(
    playback: &PlaybackConfig,
    sender: &AsyncComponentSender<PreferencesDialog>,
) -> (adw::PreferencesPage, SubtitleStyleRows) {
    let page = adw::PreferencesPage::builder()
        .title("Playback")
        .icon_name("media-playback-start-symbolic")
        .build();

    let languages = adw::PreferencesGroup::builder()
        .title("Languages")
        .description("Used when a title has no audio or subtitle choice of its own. Choices made while watching are remembered per film and per show.")
        .build();
    let audio_sender = sender.clone();
    languages.add(&language_row(
        "Audio",
        "Preferred audio language",
        "File default",
        &playback.preferred_audio_language,
        move |code| audio_sender.input(PreferencesDialogInput::SetAudioLanguage(code)),
    ));
    let subtitle_sender = sender.clone();
    languages.add(&language_row(
        "Subtitles",
        "Preferred subtitle language",
        "File default",
        &playback.preferred_subtitle_language,
        move |code| subtitle_sender.input(PreferencesDialogInput::SetSubtitleLanguage(code)),
    ));
    page.add(&languages);

    let style = adw::PreferencesGroup::builder()
        .title("Subtitle Style")
        .description("Applies to text subtitles. Styled (ASS) subtitles keep their own look. GStreamer only uses the size.")
        .build();
    let rows = SubtitleStyleRows {
        font_size: adw::SpinRow::builder()
            .title("Size")
            .adjustment(&gtk::Adjustment::new(
                playback.subtitle_font_size as f64,
                20.0,
                120.0,
                1.0,
                5.0,
                0.0,
            ))
            .build(),
        color: gtk::ColorDialogButton::builder()
            .dialog(&gtk::ColorDialog::builder().with_alpha(false).build())
            .rgba(
                &gtk::gdk::RGBA::parse(&playback.subtitle_color)
                    .expect("subtitle colour is checked when the config is loaded"),
            )
            .valign(gtk::Align::Center)
            .build(),
        border_size: adw::SpinRow::builder()
            .title("Outline")
            .digits(1)
            .adjustment(&gtk::Adjustment::new(
                playback.subtitle_border_size,
                0.0,
                10.0,
                0.5,
                1.0,
                0.0,
            ))
            .build(),
        background: adw::SwitchRow::builder()
            .title("Background")
            .subtitle("Draw a dark box behind the text")
            .active(playback.subtitle_background)
            .build(),
    };
    let color_row = adw::ActionRow::builder().title("Color").build();
    color_row.add_suffix(&rows.color);
    color_row.set_activatable_widget(Some(&rows.color));

    let changed = {
        let sender = sender.clone();
        move || sender.input(PreferencesDialogInput::SubtitleStyleChanged)
    };
    let on_change = changed.clone();
    rows.font_size.connect_value_notify(move |_| on_change());
    let on_change = changed.clone();
    rows.color.connect_rgba_notify(move |_| on_change());
    let on_change = changed.clone();
    rows.border_size.connect_value_notify(move |_| on_change());
    rows.background.connect_active_notify(move |_| changed());

    style.add(&rows.font_size);
    style.add(&color_row);
    style.add(&rows.border_size);
    style.add(&rows.background);
    page.add(&style);

    (page, rows)
}

impl PreferencesDialog {
    fn backend_subtitle(&self) -> String {
        if cfg!(target_os = "macos") {
            "GStreamer is required on macOS; playback always uses the GStreamer engine.".to_string()
        } else {
            let active = if self.default_player.eq_ignore_ascii_case("mpv") {
                "MPV"
            } else {
                "GStreamer"
            };

            format!(
                "Choose your preferred video player. Currently active: {}. Changing this restarts the stream with the new engine.",
                active
            )
        }
    }
}

#[derive(Debug)]
pub enum PreferencesDialogInput {
    SetDefaultPlayer(String),
    HydrateDefaultPlayer(String),
    SetAudioLanguage(String),
    SetSubtitleLanguage(String),
    SubtitleStyleChanged,
    ReloadConfig,
    Close,
}

#[derive(Debug)]
pub enum PreferencesDialogOutput {
    Closed,
}

#[allow(unused_assignments)]
#[relm4::component(pub async)]
impl AsyncComponent for PreferencesDialog {
    type Init = DatabaseConnection;
    type Input = PreferencesDialogInput;
    type Output = PreferencesDialogOutput;
    type CommandOutput = ();

    view! {
        #[root]
        adw::PreferencesDialog {
            set_title: "Preferences",
            set_content_width: 500,
            set_content_height: 400,

            // Close button is automatically provided by PreferencesDialog
            connect_closed => PreferencesDialogInput::Close,

            add = &adw::PreferencesPage {
                set_title: "Settings",
                set_icon_name: Some("preferences-system-symbolic"),

                add = &adw::PreferencesGroup {
                    set_title: "Player",
                    set_description: Some("Configure media playback settings"),
                    set_margin_top: 24,
                    set_margin_bottom: 24,
                    set_margin_start: 24,
                    set_margin_end: 24,

                    // Default Player Backend - The only visible setting
                    add = &adw::ActionRow {
                        set_title: "Default Player Backend",
                        #[track(model.changed(PreferencesDialog::default_player()))]
                        set_subtitle: &model.backend_subtitle(),

                        add_suffix = &gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 6,
                            set_valign: gtk::Align::Center,

                            gtk::DropDown {
                                set_model: Some(&gtk::StringList::new(
                                    if cfg!(target_os = "macos") {
                                        // On macOS, only show GStreamer
                                        &["GStreamer"]
                                    } else {
                                        // On other platforms, show both options
                                        &["MPV (Recommended)", "GStreamer"]
                                    }
                                )),
                                #[track(model.changed(PreferencesDialog::default_player()))]
                                set_selected: if cfg!(target_os = "macos") {
                                    0 // Always select GStreamer on macOS
                                } else if model.default_player == "mpv" { 0 } else { 1 },
                                connect_selected_notify[sender] => move |dropdown| {
                                    let selected = dropdown.selected();
                                    let player = if cfg!(target_os = "macos") {
                                        "gstreamer" // Always GStreamer on macOS
                                    } else if selected == 0 { "mpv" } else { "gstreamer" };
                                    sender.input(PreferencesDialogInput::SetDefaultPlayer(player.to_string()));
                                }
                            }
                        }
                    },
                },
            },
        }
    }

    async fn init(
        db: Self::Init,
        root: Self::Root,
        sender: AsyncComponentSender<Self>,
    ) -> AsyncComponentParts<Self> {
        // Load preferences from ConfigService
        let config = CONFIG_SERVICE.get_config().await;
        let (playback_page, subtitle_style_rows) = build_playback_page(&config.playback, &sender);

        let model = Self {
            db,
            default_player: config.playback.player_backend,
            hardware_acceleration: config.playback.hardware_acceleration,
            items_per_page: 48,
            cache_size_mb: config.playback.mpv_cache_size_mb as i32,
            auto_clean_cache: true,
            subtitle_style_rows,
            tracker: 0,
        };

        let widgets = view_output!();
        root.add(&playback_page);

        AsyncComponentParts { model, widgets }
    }

    async fn update(
        &mut self,
        msg: Self::Input,
        sender: AsyncComponentSender<Self>,
        root: &Self::Root,
    ) {
        self.reset(); // Reset tracker to track changes

        match msg {
            PreferencesDialogInput::SetDefaultPlayer(player) => {
                self.set_default_player(player.clone());
                tracing::info!("Default player set to: {}", self.default_player);

                // Update via ConfigService which will handle saving and broadcasting
                relm4::spawn_local(async move {
                    if let Err(e) = CONFIG_SERVICE.set_player_backend(player).await {
                        tracing::error!("Failed to save preference: {}", e);
                    } else {
                        tracing::info!("Player preference saved successfully");
                    }
                });
            }
            PreferencesDialogInput::HydrateDefaultPlayer(player) => {
                self.set_default_player(player);
                tracing::info!("Preferences dialog hydrated from config");
            }
            PreferencesDialogInput::SetAudioLanguage(code) => {
                relm4::spawn_local(async move {
                    if let Err(e) = CONFIG_SERVICE
                        .update_playback(|p| p.preferred_audio_language = code)
                        .await
                    {
                        tracing::error!("Failed to save audio language: {}", e);
                    }
                });
            }
            PreferencesDialogInput::SetSubtitleLanguage(code) => {
                relm4::spawn_local(async move {
                    if let Err(e) = CONFIG_SERVICE
                        .update_playback(|p| p.preferred_subtitle_language = code)
                        .await
                    {
                        tracing::error!("Failed to save subtitle language: {}", e);
                    }
                });
            }
            PreferencesDialogInput::SubtitleStyleChanged => {
                let style = self.subtitle_style_rows.style();
                relm4::spawn_local(async move {
                    if let Err(e) = CONFIG_SERVICE
                        .update_playback(|p| {
                            p.subtitle_font_size = style.font_size;
                            p.subtitle_color = style.color;
                            p.subtitle_border_size = style.border_size;
                            p.subtitle_background = style.background;
                        })
                        .await
                    {
                        tracing::error!("Failed to save subtitle style: {}", e);
                    }
                });
            }
            PreferencesDialogInput::ReloadConfig => {
                let sender_clone = sender.clone();
                relm4::spawn_local(async move {
                    let config = CONFIG_SERVICE.get_config().await;
                    sender_clone.input(PreferencesDialogInput::HydrateDefaultPlayer(
                        config.playback.player_backend,
                    ));
                });
            }
            PreferencesDialogInput::Close => {
                root.close();
                sender.output(PreferencesDialogOutput::Closed).unwrap();
            }
        }
    }
}
