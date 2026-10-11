/// Common types used by player backends

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UpscalingMode {
    None,
    HighQuality,
    FSR,
    Anime,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoomMode {
    Fit,         // Fit entire video in window (default, may show black bars)
    Fill,        // Fill window (may crop video)
    Zoom16_9,    // Crop to 16:9 aspect ratio
    Zoom4_3,     // Crop to 4:3 aspect ratio
    Zoom2_35,    // Crop to 2.35:1 (cinematic)
    Custom(f64), // Custom zoom level (1.0 = original, >1.0 = zoomed in)
}

impl UpscalingMode {
    pub fn to_string(&self) -> &'static str {
        match self {
            UpscalingMode::None => "None",
            UpscalingMode::HighQuality => "High Quality",
            UpscalingMode::FSR => "FSR",
            UpscalingMode::Anime => "Anime",
            UpscalingMode::Custom => "Custom",
        }
    }
}

impl Default for UpscalingMode {
    fn default() -> Self {
        Self::None
    }
}

impl ZoomMode {
    pub fn to_string(&self) -> String {
        match self {
            ZoomMode::Fit => "Fit".to_string(),
            ZoomMode::Fill => "Fill".to_string(),
            ZoomMode::Zoom16_9 => "16:9".to_string(),
            ZoomMode::Zoom4_3 => "4:3".to_string(),
            ZoomMode::Zoom2_35 => "2.35:1".to_string(),
            ZoomMode::Custom(level) => format!("{:.0}%", level * 100.0),
        }
    }

    pub fn zoom_level(&self) -> f64 {
        match self {
            ZoomMode::Fit => 1.0,
            ZoomMode::Fill => 1.0, // Will be calculated based on aspect ratios
            ZoomMode::Zoom16_9 => 1.0, // Will be calculated based on video
            ZoomMode::Zoom4_3 => 1.0, // Will be calculated based on video
            ZoomMode::Zoom2_35 => 1.0, // Will be calculated based on video
            ZoomMode::Custom(level) => *level,
        }
    }
}

impl Default for ZoomMode {
    fn default() -> Self {
        Self::Fit
    }
}

/// An audio or subtitle track as reported by a player backend.
///
/// `id` is backend specific (mpv ids are 1-based, GStreamer indices 0-based),
/// so anything persisted must use `language` and `title` instead.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackInfo {
    pub id: i32,
    pub name: String,
    pub language: Option<String>,
    pub title: Option<String>,
    /// Marked as forced (only parts in another language). GStreamer doesn't
    /// expose the container's forced flag, so there only the title tells.
    pub forced: bool,
    /// Marked as the file's default track
    pub default: bool,
    /// Marked as for the hearing impaired (SDH). GStreamer doesn't expose
    /// this flag either, so there only the title tells.
    pub hearing_impaired: bool,
}

impl TrackInfo {
    /// Forced by its flag or by a title like "English (Forced)", which is how
    /// many releases mark forced subtitles.
    pub fn is_forced(&self) -> bool {
        self.forced
            || self
                .title
                .as_deref()
                .is_some_and(|title| title.to_lowercase().contains("forced"))
    }

    /// For the hearing impaired by its flag or by a title like "English SDH"
    pub fn is_hearing_impaired(&self) -> bool {
        self.hearing_impaired
            || self.title.as_deref().is_some_and(|title| {
                let title = title.to_lowercase();
                title.contains("sdh") || title.contains("hearing impaired")
            })
    }

    /// The "no subtitles" entry that heads every subtitle track list.
    pub fn subtitles_off() -> Self {
        Self {
            id: -1,
            name: "None".to_string(),
            language: None,
            title: None,
            forced: false,
            default: false,
            hearing_impaired: false,
        }
    }
}

/// Styling applied to text subtitles (ASS subtitles keep their own styling).
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleStyle {
    pub font_size: u32,
    /// Text colour as `#RRGGBB`.
    pub color: String,
    pub border_size: f64,
    pub background: bool,
}
