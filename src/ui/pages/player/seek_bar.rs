use gtk::prelude::*;
use relm4::AsyncComponentSender;
use relm4::gtk;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use super::{PlayerInput, PlayerPage, format_duration};
use crate::models::ChapterMarker;

/// What the marks drawn over the seek bar need, in seconds
#[derive(Debug, Default)]
struct SeekMarks {
    duration: f64,
    position: f64,
    intro: Option<(f64, f64)>,
    credits: Option<(f64, f64)>,
}

impl SeekMarks {
    /// Dark ticks at the intro's start and end and the credits' start, and a
    /// lighter stretch for the credits still ahead
    fn draw(&self, cr: &gtk::cairo::Context, width: f64, height: f64) {
        if self.duration <= 0.0 {
            return;
        }
        let x_at = |seconds: f64| (seconds / self.duration).clamp(0.0, 1.0) * width;
        let knob_x = x_at(self.position);
        let mid = height / 2.0;

        if let Some((start, end)) = self.credits {
            // Start past the knob so the stretch never covers it
            let from = x_at(start).max(knob_x + SeekBarManager::KNOB_RADIUS);
            let to = x_at(end);
            if to > from {
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.45);
                cr.rectangle(from, mid - 2.0, to - from, 4.0);
                let _ = cr.fill();
            }
        }

        cr.set_source_rgb(0.04, 0.04, 0.04);
        let ticks = self
            .intro
            .into_iter()
            .flat_map(|(start, end)| [start, end])
            .chain(self.credits.map(|(start, _)| start));
        for tick in ticks {
            let x = x_at(tick);
            // A tick under the knob would sit on top of it
            if (x - knob_x).abs() > SeekBarManager::KNOB_RADIUS {
                cr.rectangle(x - 1.0, mid - 3.0, 2.0, 6.0);
            }
        }
        let _ = cr.fill();
    }
}

/// Manages seek bar widget and position/duration display.
/// Handles click and drag gestures for seeking, tooltip preview,
/// and position/duration label updates.
pub struct SeekBarManager {
    seek_bar: gtk::Scale,
    // The seek bar with the intro/credits marks drawn over it
    seek_widget: gtk::Overlay,
    marks_area: gtk::DrawingArea,
    marks: Rc<RefCell<SeekMarks>>,
    position_label: gtk::Label,
    remaining_label: gtk::Label,
    duration: Duration,
    is_seeking: bool,
}

impl SeekBarManager {
    const KNOB_RADIUS: f64 = 7.0;

    /// Create new SeekBarManager with initialized widgets and gesture handlers
    pub fn new(sender: &AsyncComponentSender<PlayerPage>) -> Self {
        // Create seek bar widget
        let seek_bar = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
        seek_bar.set_draw_value(false);
        seek_bar.set_has_tooltip(true);
        seek_bar.add_css_class("player-seek-bar");

        // Add tooltip to show time at cursor position
        seek_bar.connect_query_tooltip(|scale, x, _y, _keyboard_mode, tooltip| {
            let adjustment = scale.adjustment();
            // x is already relative to the widget
            let width = scale.width() as f64;
            let ratio = x as f64 / width;
            let max = adjustment.upper();
            let value = ratio * max;
            // Clamp value to ensure it's non-negative
            let duration = Duration::from_secs_f64(value.max(0.0));
            tooltip.set_text(Some(&format_duration(duration)));
            true
        });

        // Create time labels
        let position_label = gtk::Label::new(Some("0:00"));
        let remaining_label = gtk::Label::new(Some("--:--"));

        let marks = Rc::new(RefCell::new(SeekMarks::default()));
        let marks_area = gtk::DrawingArea::builder().can_target(false).build();
        {
            let marks = marks.clone();
            marks_area.set_draw_func(move |_, cr, width, height| {
                marks.borrow().draw(cr, width as f64, height as f64);
            });
        }
        let seek_widget = gtk::Overlay::new();
        seek_widget.set_child(Some(&seek_bar));
        seek_widget.add_overlay(&marks_area);

        // Setup seek bar gesture handlers - handle clicks and drags for video seeking
        {
            let sender_start = sender.clone();
            let sender_end = sender.clone();
            let seek_bar_for_click = seek_bar.clone();

            // Handle direct clicks and drags for video seeking
            let click_gesture = gtk::GestureClick::new();
            click_gesture.set_button(gtk::gdk::BUTTON_PRIMARY);

            // Start seeking on press
            click_gesture.connect_pressed(move |_gesture, _n_press, x, _y| {
                sender_start.input(PlayerInput::StartSeeking);

                // Calculate position from click location
                let widget_width = seek_bar_for_click.width() as f64;
                let adjustment = seek_bar_for_click.adjustment();
                let range = adjustment.upper() - adjustment.lower();
                let value = adjustment.lower() + (x / widget_width) * range;

                // Update the scale value and seek
                seek_bar_for_click.set_value(value);
                sender_start.input(PlayerInput::Seek(Duration::from_secs_f64(value.max(0.0))));
            });

            // End seeking on release
            click_gesture.connect_released(move |_gesture, _n_press, _x, _y| {
                sender_end.input(PlayerInput::StopSeeking);
            });

            seek_bar.add_controller(click_gesture);

            // Handle dragging
            let sender_drag = sender.clone();
            let seek_bar_for_drag = seek_bar.clone();
            let drag_gesture = gtk::GestureDrag::new();
            drag_gesture.set_button(gtk::gdk::BUTTON_PRIMARY);

            drag_gesture.connect_drag_update(move |_gesture, offset_x, _offset_y| {
                // Calculate position from drag location
                let widget_width = seek_bar_for_drag.width() as f64;
                let adjustment = seek_bar_for_drag.adjustment();
                let range = adjustment.upper() - adjustment.lower();

                // Get current position and add offset
                let current_value = seek_bar_for_drag.value();
                let value_per_pixel = range / widget_width;
                let new_value = (current_value + offset_x * value_per_pixel)
                    .clamp(adjustment.lower(), adjustment.upper());

                // Update the scale value and seek
                seek_bar_for_drag.set_value(new_value);
                sender_drag.input(PlayerInput::Seek(Duration::from_secs_f64(
                    new_value.max(0.0),
                )));
            });

            seek_bar.add_controller(drag_gesture);
        }

        Self {
            seek_bar,
            seek_widget,
            marks_area,
            marks,
            position_label,
            remaining_label,
            duration: Duration::ZERO,
            is_seeking: false,
        }
    }

    /// The seek bar with its intro/credits marks
    pub fn get_seek_widget(&self) -> &gtk::Overlay {
        &self.seek_widget
    }

    /// Get reference to position label widget
    pub fn get_position_label(&self) -> &gtk::Label {
        &self.position_label
    }

    /// Get reference to the remaining time label
    pub fn get_remaining_label(&self) -> &gtk::Label {
        &self.remaining_label
    }

    /// Update position display and seek bar value
    /// Only updates seek bar if not currently seeking (prevents flicker during drag)
    pub fn update_position(&mut self, position: Duration) {
        self.position_label.set_text(&format_duration(position));
        self.remaining_label.set_text(&format!(
            "−{}",
            format_duration(self.duration.saturating_sub(position))
        ));

        // Only update seek bar position if we're not actively seeking
        // This prevents the bar from jumping around during drag operations
        if !self.is_seeking {
            self.seek_bar.set_value(position.as_secs_f64());
        }
        self.marks.borrow_mut().position = position.as_secs_f64();
        self.marks_area.queue_draw();
    }

    /// Update the seek bar range
    pub fn update_duration(&mut self, duration: Duration) {
        self.duration = duration;
        self.seek_bar.set_range(0.0, duration.as_secs_f64());
        self.marks.borrow_mut().duration = duration.as_secs_f64();
        self.marks_area.queue_draw();
    }

    /// Mark the intro and credits on the seek bar
    pub fn set_markers(&self, intro: Option<&ChapterMarker>, credits: Option<&ChapterMarker>) {
        let span = |marker: &ChapterMarker| {
            (
                marker.start_time.as_secs_f64(),
                marker.end_time.as_secs_f64(),
            )
        };
        let mut marks = self.marks.borrow_mut();
        marks.intro = intro.map(span);
        marks.credits = credits.map(span);
        drop(marks);
        self.marks_area.queue_draw();
    }

    /// Set seeking state (true when user is dragging seek bar)
    pub fn set_seeking(&mut self, seeking: bool) {
        self.is_seeking = seeking;
    }

    /// Reset to initial state (0:00 position, unknown duration)
    pub fn reset(&mut self) {
        self.seek_bar.set_value(0.0);
        self.position_label.set_text("0:00");
        self.remaining_label.set_text("--:--");
        self.duration = Duration::ZERO;
        *self.marks.borrow_mut() = SeekMarks::default();
        self.marks_area.queue_draw();
        self.is_seeking = false;
    }
}
