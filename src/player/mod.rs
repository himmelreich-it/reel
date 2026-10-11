pub mod controller;
pub mod factory;
#[cfg(feature = "gstreamer")]
pub mod gstreamer;
#[cfg(feature = "gstreamer")]
pub mod gstreamer_player;
pub mod language;
#[cfg(all(feature = "mpv", not(target_os = "macos")))]
pub mod mpv_player;
pub mod types;

pub use controller::{PlayerController, PlayerHandle};
pub use factory::Player;
#[allow(unused_imports)]
pub use factory::PlayerState;
pub use types::{SubtitleStyle, TrackInfo, UpscalingMode, ZoomMode};

#[cfg(feature = "gstreamer")]
pub use gstreamer_player::{BufferingState, GStreamerPlayer};
#[cfg(all(feature = "mpv", not(target_os = "macos")))]
pub use mpv_player::MpvPlayer;
