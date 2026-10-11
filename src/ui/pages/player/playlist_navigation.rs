use gtk::prelude::*;
use relm4::gtk;
use relm4::prelude::*;
use tracing::debug;

use super::{PlayerInput, PlayerPage};

/// Playlist navigation and position tracking methods
impl PlayerPage {
    /// Empty the title until the new item's details are loaded
    pub(super) fn clear_title(&self) {
        self.title_eyebrow_label.set_visible(false);
        self.title_eyebrow_label.set_text("");
        self.title_label.set_text("");
    }

    /// Handle previous episode/item navigation
    pub(super) fn handle_previous_navigation(&self, sender: &AsyncComponentSender<Self>) {
        debug!("Previous track requested");

        if let Some(ref context) = self.playlist_context {
            if let Some(prev_id) = context.get_previous_item() {
                // Keep the context and just load the previous media
                let mut new_context = context.clone();
                new_context.update_current_index(&prev_id);

                sender.input(PlayerInput::LoadMediaWithContext {
                    media_id: prev_id,
                    context: new_context,
                });
            } else {
                debug!("No previous episode available");
            }
        } else {
            debug!("No playlist context available for previous navigation");
        }
    }

    /// Handle next episode/item navigation
    pub(super) fn handle_next_navigation(&self, sender: &AsyncComponentSender<Self>) {
        debug!("Next track requested");

        if let Some(ref context) = self.playlist_context {
            if let Some(next_id) = context.get_next_item() {
                // Keep the context and just load the next media
                let mut new_context = context.clone();
                new_context.update_current_index(&next_id);

                sender.input(PlayerInput::LoadMediaWithContext {
                    media_id: next_id,
                    context: new_context,
                });
            } else {
                debug!("No next episode available");
            }
        } else {
            debug!("No playlist context available for next navigation");
        }
    }
}
