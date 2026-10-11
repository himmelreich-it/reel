use gtk::glib;
use gtk::prelude::ToVariant;
use gtk::prelude::*;
use relm4::gtk;
use relm4::prelude::*;
use tracing::debug;

use super::{PlayerInput, PlayerPage};

#[derive(Clone, Copy)]
enum TrackKind {
    Audio,
    Subtitle,
}

impl TrackKind {
    fn action_prefix(self) -> &'static str {
        match self {
            TrackKind::Audio => "audio",
            TrackKind::Subtitle => "subtitle",
        }
    }
}

/// Menu population methods for audio/subtitle/zoom/quality menus
impl PlayerPage {
    pub(super) fn populate_audio_menu(&self, sender: AsyncComponentSender<Self>) {
        self.populate_track_menu(TrackKind::Audio, sender);
    }

    pub(super) fn populate_subtitle_menu(&self, sender: AsyncComponentSender<Self>) {
        self.populate_track_menu(TrackKind::Subtitle, sender);
    }

    fn populate_track_menu(&self, kind: TrackKind, sender: AsyncComponentSender<Self>) {
        let Some(player) = &self.player else {
            return;
        };
        let player_clone = player.clone();
        let menu_button = match kind {
            TrackKind::Audio => self.audio_menu_button.clone(),
            TrackKind::Subtitle => self.subtitle_menu_button.clone(),
        };
        let popover_count = self.active_popover_count.clone();

        glib::spawn_future_local(async move {
            let (tracks, current_track) = match kind {
                TrackKind::Audio => (
                    player_clone.get_audio_tracks().await.unwrap_or_default(),
                    player_clone.get_current_audio_track().await.unwrap_or(-1),
                ),
                TrackKind::Subtitle => (
                    player_clone.get_subtitle_tracks().await.unwrap_or_default(),
                    player_clone
                        .get_current_subtitle_track()
                        .await
                        .unwrap_or(-1),
                ),
            };

            // Subtitle lists always start with "None", so one entry means no tracks
            let minimum_tracks = match kind {
                TrackKind::Audio => 1,
                TrackKind::Subtitle => 2,
            };
            if tracks.len() < minimum_tracks {
                menu_button.set_sensitive(false);
                menu_button.set_popover(None::<&gtk::Popover>);
                return;
            }
            menu_button.set_sensitive(true);

            // One stateful action makes the items radio buttons marking the playing track
            let action_name = format!("{}-track", kind.action_prefix());
            let menu = gtk::gio::Menu::new();
            for track in &tracks {
                let item = gtk::gio::MenuItem::new(Some(&track.name), None);
                item.set_action_and_target_value(
                    Some(&format!("player.{}", action_name)),
                    Some(&track.id.to_variant()),
                );
                menu.append_item(&item);
            }

            let action = gtk::gio::SimpleAction::new_stateful(
                &action_name,
                Some(glib::VariantTy::INT32),
                &current_track.to_variant(),
            );
            let sender_clone = sender.clone();
            action.connect_activate(move |action, parameter| {
                let Some(track_id) = parameter.and_then(|p| p.get::<i32>()) else {
                    return;
                };
                action.set_state(&track_id.to_variant());
                sender_clone.input(match kind {
                    TrackKind::Audio => PlayerInput::SetAudioTrack(track_id),
                    TrackKind::Subtitle => PlayerInput::SetSubtitleTrack(track_id),
                });
            });
            let action_group = gtk::gio::SimpleActionGroup::new();
            action_group.add_action(&action);

            let popover = gtk::PopoverMenu::from_model(Some(&menu));
            Self::track_popover_visibility(popover.upcast_ref(), popover_count);

            menu_button.insert_action_group("player", Some(&action_group));
            menu_button.set_popover(Some(&popover));
        });
    }

    /// Keep the controls visible while a popover is open
    fn track_popover_visibility(
        popover: &gtk::Popover,
        popover_count: std::rc::Rc<std::cell::RefCell<usize>>,
    ) {
        let popover_count_clone = popover_count.clone();
        popover.connect_show(move |_| {
            *popover_count_clone.borrow_mut() += 1;
            debug!("Popover shown, count: {}", *popover_count_clone.borrow());
        });
        popover.connect_hide(move |_| {
            let mut count = popover_count.borrow_mut();
            if *count > 0 {
                *count -= 1;
            }
            debug!("Popover hidden, count: {}", *count);
        });
    }

    /// The sync button with its popover of audio and subtitle delay controls
    pub(super) fn build_sync_menu(
        sender: &AsyncComponentSender<Self>,
        popover_count: std::rc::Rc<std::cell::RefCell<usize>>,
    ) -> (gtk::MenuButton, gtk::Label, gtk::Label) {
        let grid = gtk::Grid::builder()
            .row_spacing(6)
            .column_spacing(12)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .build();

        // Each row: name, −, current value, +
        let delay_row = |row: i32, title: &str, on_step: fn(i64) -> PlayerInput| {
            let title = gtk::Label::builder().label(title).xalign(0.0).build();
            let value = gtk::Label::builder()
                .label(Self::format_delay(0))
                .width_chars(9)
                .build();
            let step_button = |icon: &str, step_ms: i64| {
                let button = gtk::Button::from_icon_name(icon);
                button.add_css_class("circular");
                button.add_css_class("flat");
                let sender = sender.clone();
                button.connect_clicked(move |_| sender.input(on_step(step_ms)));
                button
            };
            grid.attach(&title, 0, row, 1, 1);
            grid.attach(&step_button("list-remove-symbolic", -50), 1, row, 1, 1);
            grid.attach(&value, 2, row, 1, 1);
            grid.attach(&step_button("list-add-symbolic", 50), 3, row, 1, 1);
            value
        };
        let audio_delay_label = delay_row(0, "Audio delay", PlayerInput::AdjustAudioDelay);
        let subtitle_delay_label = delay_row(1, "Subtitle delay", PlayerInput::AdjustSubtitleDelay);

        let reset_button = gtk::Button::with_label("Reset");
        let reset_sender = sender.clone();
        reset_button.connect_clicked(move |_| reset_sender.input(PlayerInput::ResetDelays));
        grid.attach(&reset_button, 0, 2, 4, 1);

        let popover = gtk::Popover::new();
        popover.set_child(Some(&grid));

        // The popover is its own surface, so the player's shortcuts don't see
        // its keys; keep space for pausing and resuming here too
        let space_controller = gtk::EventControllerKey::new();
        space_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        let space_sender = sender.clone();
        space_controller.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::space {
                space_sender.input(PlayerInput::PlayPause);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        popover.add_controller(space_controller);
        Self::track_popover_visibility(&popover, popover_count);

        let sync_menu_button = gtk::MenuButton::new();
        sync_menu_button.set_popover(Some(&popover));
        (sync_menu_button, audio_delay_label, subtitle_delay_label)
    }

    pub(super) fn populate_zoom_menu(&self, sender: AsyncComponentSender<Self>) {
        let zoom_menu_button = self.zoom_menu_button.clone();
        let popover_count = self.active_popover_count.clone();
        let current_mode = self.current_zoom_mode;

        zoom_menu_button.set_sensitive(true);
        zoom_menu_button.set_tooltip_text(Some("Video Zoom"));

        // Create menu
        let menu = gtk::gio::Menu::new();

        // Add zoom modes
        let modes = [
            (crate::player::ZoomMode::Fit, "Fit", "Fit video to window"),
            (
                crate::player::ZoomMode::Fill,
                "Fill",
                "Fill window (may crop)",
            ),
            (
                crate::player::ZoomMode::Zoom16_9,
                "16:9",
                "Force 16:9 aspect ratio",
            ),
            (
                crate::player::ZoomMode::Zoom4_3,
                "4:3",
                "Force 4:3 aspect ratio",
            ),
            (
                crate::player::ZoomMode::Zoom2_35,
                "2.35:1",
                "Cinematic aspect ratio",
            ),
        ];

        for (mode, label, _description) in modes {
            let item = gtk::gio::MenuItem::new(Some(label), None);
            let action_name = format!(
                "player.zoom-{}",
                label.to_lowercase().replace([':', '.'], "-")
            );
            item.set_action_and_target_value(Some(&action_name), None);

            // Add checkmark for current mode
            if mode == current_mode {
                item.set_attribute_value("icon", Some(&"object-select-symbolic".to_variant()));
            }

            menu.append_item(&item);
        }

        // Add custom zoom levels
        menu.append_item(&gtk::gio::MenuItem::new(Some("──────"), None));
        let custom_zooms = [(1.1, "110%"), (1.2, "120%"), (1.3, "130%"), (1.5, "150%")];

        for (level, label) in custom_zooms {
            let item = gtk::gio::MenuItem::new(Some(label), None);
            let action_name = format!("player.zoom-custom-{}", label.replace('%', ""));
            item.set_action_and_target_value(Some(&action_name), None);

            // Check if it matches custom zoom
            if let crate::player::ZoomMode::Custom(current_level) = current_mode
                && (current_level - level).abs() < 0.01
            {
                item.set_attribute_value("icon", Some(&"object-select-symbolic".to_variant()));
            }

            menu.append_item(&item);
        }

        // Create popover
        let popover = gtk::PopoverMenu::from_model(Some(&menu));

        // Track popover state to prevent control hiding
        let popover_count_clone = popover_count.clone();
        popover.connect_show(move |_| {
            *popover_count_clone.borrow_mut() += 1;
            debug!(
                "Zoom popover shown, count: {}",
                *popover_count_clone.borrow()
            );
        });
        popover.connect_hide(move |_| {
            let mut count = popover_count.borrow_mut();
            if *count > 0 {
                *count -= 1;
            }
            debug!("Zoom popover hidden, count: {}", *count);
        });

        // Create action group
        let action_group = gtk::gio::SimpleActionGroup::new();

        // Add actions for preset modes
        for (mode, label, _) in modes {
            let action_name = format!("zoom-{}", label.to_lowercase().replace([':', '.'], "-"));
            let action = gtk::gio::SimpleAction::new(&action_name, None);
            let sender_clone = sender.clone();
            let mode_copy = mode;
            action.connect_activate(move |_, _| {
                sender_clone.input(PlayerInput::SetZoomMode(mode_copy));
            });
            action_group.add_action(&action);
        }

        // Add actions for custom zoom levels
        for (level, label) in custom_zooms {
            let action_name = format!("zoom-custom-{}", label.replace('%', ""));
            let action = gtk::gio::SimpleAction::new(&action_name, None);
            let sender_clone = sender.clone();
            action.connect_activate(move |_, _| {
                sender_clone.input(PlayerInput::SetZoomMode(crate::player::ZoomMode::Custom(
                    level,
                )));
            });
            action_group.add_action(&action);
        }

        // Insert the action group
        zoom_menu_button.insert_action_group("player", Some(&action_group));
        zoom_menu_button.set_popover(Some(&popover));
    }

    pub(super) fn populate_quality_menu(&self, sender: AsyncComponentSender<Self>) {
        let quality_menu_button = self.quality_menu_button.clone();
        let popover_count = self.active_popover_count.clone();
        let current_mode = self.current_upscaling_mode;
        let is_mpv = self.is_mpv_backend;

        if !is_mpv {
            // Disable button for non-MPV backends
            quality_menu_button.set_sensitive(false);
            quality_menu_button.set_tooltip_text(Some("Upscaling only available with MPV player"));
            return;
        }

        quality_menu_button.set_sensitive(true);
        quality_menu_button.set_tooltip_text(Some("Video Quality"));

        // Create menu
        let menu = gtk::gio::Menu::new();

        // Add upscaling modes
        let modes = [
            (crate::player::UpscalingMode::None, "None", "No upscaling"),
            (
                crate::player::UpscalingMode::HighQuality,
                "High Quality",
                "Enhanced quality upscaling",
            ),
            (
                crate::player::UpscalingMode::FSR,
                "FSR",
                "AMD FidelityFX Super Resolution",
            ),
            (
                crate::player::UpscalingMode::Anime,
                "Anime",
                "Optimized for anime content",
            ),
        ];

        for (mode, label, _description) in modes {
            let item = gtk::gio::MenuItem::new(Some(label), None);
            let action_name = format!("player.quality-{}", label.to_lowercase().replace(' ', "-"));
            item.set_action_and_target_value(Some(&action_name), None);

            // Add checkmark for current mode
            if mode == current_mode {
                item.set_attribute_value("icon", Some(&"object-select-symbolic".to_variant()));
            }

            menu.append_item(&item);
        }

        // Create popover from menu model
        let popover = gtk::PopoverMenu::from_model(Some(&menu));

        // Track popover state to prevent control hiding
        let popover_count_clone = popover_count.clone();
        popover.connect_show(move |_| {
            *popover_count_clone.borrow_mut() += 1;
            debug!(
                "Quality popover shown, count: {}",
                *popover_count_clone.borrow()
            );
        });
        popover.connect_hide(move |_| {
            let mut count = popover_count.borrow_mut();
            if *count > 0 {
                *count -= 1;
            }
            debug!("Quality popover hidden, count: {}", *count);
        });

        // Add actions for each mode
        let action_group = gtk::gio::SimpleActionGroup::new();
        for (mode, label, _) in modes {
            let action_name = format!("quality-{}", label.to_lowercase().replace(' ', "-"));
            let action = gtk::gio::SimpleAction::new(&action_name, None);
            let sender_clone = sender.clone();
            let mode_copy = mode;
            action.connect_activate(move |_, _| {
                sender_clone.input(PlayerInput::SetUpscalingMode(mode_copy));
            });
            action_group.add_action(&action);
        }

        // Insert the action group
        quality_menu_button.insert_action_group("player", Some(&action_group));
        quality_menu_button.set_popover(Some(&popover));
    }
}
