//! Choosing audio/subtitle tracks on load and remembering what the user picks.
//!
//! A title's own saved choice wins, then the latest choice made in the same
//! show, then the languages from the preferences. Delays are specific to a
//! file, so they only come from the title's own saved choice.

use super::{PlayerInput, PlayerPage};
use crate::db::connection::DatabaseConnection;
use crate::db::entities::TrackPreferenceModel;
use crate::db::repository::{
    MediaRepositoryImpl, Repository, TrackPreferenceRepository, TrackPreferenceRepositoryImpl,
};
use crate::models::MediaItemId;
use crate::player::{PlayerHandle, TrackInfo, language};
use crate::services::config_service::CONFIG_SERVICE;
use anyhow::Result;
use gtk::glib;
use gtk::prelude::*;
use relm4::gtk;
use relm4::prelude::*;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Copy)]
pub enum DelayKind {
    Audio,
    Subtitle,
}

impl PlayerPage {
    pub(super) const MAX_DELAY_MS: i64 = 10_000;

    pub(super) fn format_delay(delay_ms: i64) -> String {
        if delay_ms == 0 {
            "0 ms".to_string()
        } else {
            format!("{:+} ms", delay_ms)
        }
    }

    pub(super) fn clamp_delay(delay_ms: i64) -> i64 {
        delay_ms.clamp(-Self::MAX_DELAY_MS, Self::MAX_DELAY_MS)
    }

    const DELAY_SAVE_DEBOUNCE: Duration = Duration::from_millis(1000);
    const OSD_MESSAGE_TIMEOUT: Duration = Duration::from_millis(1500);

    /// Apply a delay right away; saving waits until the user stops adjusting.
    pub(super) fn change_delay(
        &mut self,
        kind: DelayKind,
        delay_ms: i64,
        sender: &AsyncComponentSender<Self>,
    ) {
        let delay_ms = Self::clamp_delay(delay_ms);
        let current = match kind {
            DelayKind::Audio => self.audio_delay_ms,
            DelayKind::Subtitle => self.subtitle_delay_ms,
        };
        let Some(player) = self.player.clone() else {
            return;
        };
        if delay_ms == current {
            return;
        }

        match kind {
            DelayKind::Audio => self.audio_delay_ms = delay_ms,
            DelayKind::Subtitle => self.subtitle_delay_ms = delay_ms,
        }
        self.show_delays();

        glib::spawn_future_local(async move {
            let seconds = delay_ms as f64 / 1000.0;
            let result = match kind {
                DelayKind::Audio => player.set_audio_delay(seconds).await,
                DelayKind::Subtitle => player.set_subtitle_delay(seconds).await,
            };
            if let Err(e) = result {
                warn!("Failed to set {:?} delay: {}", kind, e);
            }
        });

        self.delay_save_pending = true;
        self.delay_save_generation += 1;
        let generation = self.delay_save_generation;
        let sender = sender.clone();
        glib::timeout_add_local_once(Self::DELAY_SAVE_DEBOUNCE, move || {
            sender.input(PlayerInput::SaveDelays(generation));
        });
    }

    /// Show the current delays in the sync popover
    pub(super) fn show_delays(&self) {
        self.audio_delay_label
            .set_label(&Self::format_delay(self.audio_delay_ms));
        self.subtitle_delay_label
            .set_label(&Self::format_delay(self.subtitle_delay_ms));
    }

    /// Persist the delays unless another change came in since `generation` was scheduled.
    pub(super) fn save_delays(&mut self, generation: u64) {
        if generation == self.delay_save_generation {
            self.flush_delay_save();
        }
    }

    /// Save a delay change that is still waiting for its debounce, for the
    /// title it was made on. Call before the title changes or playback ends.
    pub(super) fn flush_delay_save(&mut self) {
        if !self.delay_save_pending {
            return;
        }
        self.delay_save_pending = false;
        // A timer still running for this change finds a newer generation and does nothing
        self.delay_save_generation += 1;
        let Some(media_id) = self.media_item_id.clone() else {
            return;
        };
        let db = self.db.clone();
        let change = PreferenceChange::Delays {
            audio_ms: self.audio_delay_ms,
            subtitle_ms: self.subtitle_delay_ms,
        };
        glib::spawn_future_local(async move {
            if let Err(e) = save_preference_change(&db, &media_id, change).await {
                warn!("Failed to save sync delays: {}", e);
            }
        });
    }

    /// Start a new title: save what is pending for the previous one, then
    /// show zero delays until the new title's preferences are applied.
    pub(super) fn begin_track_preferences(&mut self, media_id: &MediaItemId) {
        self.flush_delay_save();
        self.media_item_id = Some(media_id.clone());
        self.audio_delay_ms = 0;
        self.subtitle_delay_ms = 0;
        self.show_delays();
    }

    /// Show a short message over the video, replacing any message still showing.
    pub(super) fn show_osd_message(
        &mut self,
        message: String,
        sender: &AsyncComponentSender<Self>,
    ) {
        self.osd_message.set_label(&message);
        self.osd_message.set_visible(true);
        self.osd_message_generation += 1;
        let generation = self.osd_message_generation;
        let sender = sender.clone();
        glib::timeout_add_local_once(Self::OSD_MESSAGE_TIMEOUT, move || {
            sender.input(PlayerInput::HideOsdMessage(generation));
        });
    }

    pub(super) fn hide_osd_message(&self, generation: u64) {
        if generation == self.osd_message_generation {
            self.osd_message.set_visible(false);
        }
    }

    /// Run a track change the user made, remember the resulting track for
    /// this title and refresh the track menus, also when the change failed.
    pub(super) fn remember_track_after(
        &self,
        sender: &AsyncComponentSender<Self>,
        kind: TrackKind,
        change: impl Future<Output = Result<()>> + 'static,
    ) {
        let (Some(player), Some(media_id)) = (self.player.clone(), self.media_item_id.clone())
        else {
            return;
        };
        let db = self.db.clone();
        let sender = sender.clone();
        glib::spawn_future_local(async move {
            match change.await {
                Ok(()) => {
                    if let Err(e) = save_track_choice(&player, &db, &media_id, kind).await {
                        warn!("Failed to save {:?} track choice: {}", kind, e);
                    }
                }
                Err(e) => warn!("Failed to change {:?} track: {}", kind, e),
            }
            sender.input(PlayerInput::UpdateTrackMenus);
        });
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TrackKind {
    Audio,
    Subtitle,
}

/// Delays in effect after applying preferences to a freshly loaded title
#[derive(Debug, Clone)]
pub struct AppliedDelays {
    pub audio_ms: i64,
    pub subtitle_ms: i64,
}

/// Set the delays and select the remembered or preferred tracks for a loaded title.
pub async fn apply_track_preferences(
    player: &PlayerHandle,
    db: &Arc<DatabaseConnection>,
    media_id: &MediaItemId,
) -> Result<AppliedDelays> {
    let repo = TrackPreferenceRepositoryImpl::new(db.as_ref().clone());
    let own = repo.find_by_media_id(media_id.as_ref()).await?;

    // Delays are specific to this file, so they only come from its own row
    let delays = AppliedDelays {
        audio_ms: own
            .as_ref()
            .map_or(0, |p| PlayerPage::clamp_delay(p.audio_delay_ms)),
        subtitle_ms: own
            .as_ref()
            .map_or(0, |p| PlayerPage::clamp_delay(p.subtitle_delay_ms)),
    };
    player
        .set_audio_delay(delays.audio_ms as f64 / 1000.0)
        .await?;
    player
        .set_subtitle_delay(delays.subtitle_ms as f64 / 1000.0)
        .await?;

    let remembered = match own {
        Some(preference) => Some(preference),
        None => match show_id_for(db, media_id).await? {
            Some(show_id) => repo.find_latest_for_show(&show_id).await?,
            None => None,
        },
    };

    let playback = CONFIG_SERVICE.get_playback_config().await;
    let audio_tracks = player.get_audio_tracks().await?;
    let subtitle_tracks = player.get_subtitle_tracks().await?;

    if let Some(id) = choose_audio_track(
        &audio_tracks,
        remembered.as_ref(),
        &playback.preferred_audio_language,
    ) {
        // Keep going, so a failed audio selection still selects subtitles
        if let Err(e) = player.set_audio_track(id).await {
            warn!("Failed to select audio track {}: {}", id, e);
        }
    }
    if let Some(id) = choose_subtitle_track(
        &subtitle_tracks,
        remembered.as_ref(),
        &playback.preferred_subtitle_language,
    ) && let Err(e) = player.set_subtitle_track(id).await
    {
        warn!("Failed to select subtitle track {}: {}", id, e);
    }

    info!("Applied track preferences, delays {:?}", delays);
    Ok(delays)
}

/// A single choice the user made, to be merged into the title's saved preference
#[derive(Debug, Clone, PartialEq)]
enum PreferenceChange {
    Audio(TrackInfo),
    /// `None` turns subtitles off
    Subtitle(Option<TrackInfo>),
    Delays {
        audio_ms: i64,
        subtitle_ms: i64,
    },
}

impl PreferenceChange {
    fn apply_to(self, preference: &mut TrackPreferenceModel) {
        match self {
            PreferenceChange::Audio(track) => {
                preference.audio_language = track.language;
                preference.audio_title = track.title;
            }
            PreferenceChange::Subtitle(track) => {
                preference.subtitle_off = track.is_none();
                preference.subtitle_hearing_impaired =
                    track.as_ref().is_some_and(TrackInfo::is_hearing_impaired);
                preference.subtitle_language = track.as_ref().and_then(|t| t.language.clone());
                preference.subtitle_title = track.and_then(|t| t.title);
            }
            PreferenceChange::Delays {
                audio_ms,
                subtitle_ms,
            } => {
                preference.audio_delay_ms = audio_ms;
                preference.subtitle_delay_ms = subtitle_ms;
            }
        }
    }
}

/// The preference a change is merged into: the title's own row, else the
/// tracks of the show's latest row (so one change doesn't forget the show's
/// other track), else nothing remembered.
fn base_preference(
    own: Option<TrackPreferenceModel>,
    show_latest: Option<TrackPreferenceModel>,
    media_id: &MediaItemId,
    show_id: Option<String>,
) -> TrackPreferenceModel {
    if let Some(own) = own {
        return own;
    }
    let tracks = show_latest.unwrap_or(TrackPreferenceModel {
        media_id: String::new(),
        show_id: None,
        audio_language: None,
        audio_title: None,
        subtitle_language: None,
        subtitle_title: None,
        subtitle_hearing_impaired: false,
        subtitle_off: false,
        audio_delay_ms: 0,
        subtitle_delay_ms: 0,
        updated_at: chrono::Utc::now().naive_utc(),
    });
    TrackPreferenceModel {
        media_id: media_id.to_string(),
        show_id,
        audio_delay_ms: 0,
        subtitle_delay_ms: 0,
        ..tracks
    }
}

/// Serialises the read-merge-write of preferences, so a track choice and a
/// delay save running at the same time don't overwrite each other.
static SAVE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn save_preference_change(
    db: &Arc<DatabaseConnection>,
    media_id: &MediaItemId,
    change: PreferenceChange,
) -> Result<()> {
    let _guard = SAVE_LOCK.lock().await;
    let repo = TrackPreferenceRepositoryImpl::new(db.as_ref().clone());
    let show_id = show_id_for(db, media_id).await?;
    let own = repo.find_by_media_id(media_id.as_ref()).await?;
    let show_latest = match (&own, &show_id) {
        (None, Some(show_id)) => repo.find_latest_for_show(show_id).await?,
        _ => None,
    };

    let mut preference = base_preference(own, show_latest, media_id, show_id);
    change.apply_to(&mut preference);
    preference.updated_at = chrono::Utc::now().naive_utc();
    debug!("Saving track preferences: {:?}", preference);
    repo.save(preference).await
}

/// Remember the audio or subtitle track now playing for this title.
async fn save_track_choice(
    player: &PlayerHandle,
    db: &Arc<DatabaseConnection>,
    media_id: &MediaItemId,
    kind: TrackKind,
) -> Result<()> {
    let change = match kind {
        TrackKind::Audio => {
            let id = player.get_current_audio_track().await?;
            let track = player
                .get_audio_tracks()
                .await?
                .into_iter()
                .find(|t| t.id == id)
                .ok_or_else(|| anyhow::anyhow!("Current audio track {} is not in the list", id))?;
            PreferenceChange::Audio(track)
        }
        TrackKind::Subtitle => {
            let id = player.get_current_subtitle_track().await?;
            if id < 0 {
                PreferenceChange::Subtitle(None)
            } else {
                let track = player
                    .get_subtitle_tracks()
                    .await?
                    .into_iter()
                    .find(|t| t.id == id)
                    .ok_or_else(|| {
                        anyhow::anyhow!("Current subtitle track {} is not in the list", id)
                    })?;
                PreferenceChange::Subtitle(Some(track))
            }
        }
    };
    save_preference_change(db, media_id, change).await
}

async fn show_id_for(
    db: &Arc<DatabaseConnection>,
    media_id: &MediaItemId,
) -> Result<Option<String>> {
    let media = MediaRepositoryImpl::new(db.as_ref().clone())
        .find_by_id(media_id.as_ref())
        .await?;
    Ok(media.filter(|m| m.is_episode()).and_then(|m| m.parent_id))
}

/// The best of several tracks in one language: full rather than forced, then
/// SDH or not as wanted (regular unless SDH was chosen before), then the
/// file's default, then the first.
fn best_track<'a>(
    tracks: impl Iterator<Item = &'a TrackInfo>,
    hearing_impaired: bool,
) -> Option<&'a TrackInfo> {
    tracks.min_by_key(|t| {
        (
            t.is_forced(),
            t.is_hearing_impaired() != hearing_impaired,
            !t.default,
        )
    })
}

/// The remembered track (same language and title, else the best track in
/// that language), else the best track in the preferred language.
fn choose_track(
    tracks: &[TrackInfo],
    remembered_language: Option<&str>,
    remembered_title: Option<&str>,
    remembered_hearing_impaired: bool,
    preferred_language: &str,
) -> Option<i32> {
    // Releases tag the same language differently ("ger", "deu", "de")
    let remembered_language = remembered_language.map(language::canonical);
    let same_language =
        |t: &&TrackInfo| t.language.as_deref().map(language::canonical) == remembered_language;
    if remembered_language.is_some() || remembered_title.is_some() {
        // A remembered title is a deliberate choice, even a forced track
        let exact = tracks
            .iter()
            .filter(same_language)
            .find(|t| t.title.as_deref() == remembered_title);
        let by_language = remembered_language.as_ref().and_then(|_| {
            best_track(
                tracks.iter().filter(same_language),
                remembered_hearing_impaired,
            )
        });
        if let Some(track) = exact.or(by_language) {
            return Some(track.id);
        }
    }

    best_track(
        tracks.iter().filter(|t| {
            t.language
                .as_deref()
                .is_some_and(|lang| language::track_matches(lang, preferred_language))
        }),
        remembered_hearing_impaired,
    )
    .map(|t| t.id)
}

fn choose_audio_track(
    tracks: &[TrackInfo],
    remembered: Option<&TrackPreferenceModel>,
    preferred_language: &str,
) -> Option<i32> {
    choose_track(
        tracks,
        remembered.and_then(|p| p.audio_language.as_deref()),
        remembered.and_then(|p| p.audio_title.as_deref()),
        false,
        preferred_language,
    )
}

/// `Some(-1)` turns subtitles off. `None` leaves the player's own choice
/// (the file's default or forced subtitles), used when neither a remembered
/// nor the preferred language is in the file.
fn choose_subtitle_track(
    tracks: &[TrackInfo],
    remembered: Option<&TrackPreferenceModel>,
    preferred_language: &str,
) -> Option<i32> {
    if remembered.is_some_and(|p| p.subtitle_off) {
        return Some(-1);
    }
    let real_tracks: Vec<TrackInfo> = tracks.iter().filter(|t| t.id >= 0).cloned().collect();
    choose_track(
        &real_tracks,
        remembered.and_then(|p| p.subtitle_language.as_deref()),
        remembered.and_then(|p| p.subtitle_title.as_deref()),
        remembered.is_some_and(|p| p.subtitle_hearing_impaired),
        preferred_language,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: i32, language: Option<&str>, title: Option<&str>) -> TrackInfo {
        TrackInfo {
            id,
            name: format!("Track {}", id),
            language: language.map(str::to_string),
            title: title.map(str::to_string),
            forced: false,
            default: false,
            hearing_impaired: false,
        }
    }

    fn flagged(mut track: TrackInfo, forced: bool, default: bool) -> TrackInfo {
        track.forced = forced;
        track.default = default;
        track
    }

    fn remembered(
        audio: (Option<&str>, Option<&str>),
        subtitle: (Option<&str>, Option<&str>),
        subtitle_off: bool,
    ) -> TrackPreferenceModel {
        TrackPreferenceModel {
            media_id: "ep1".to_string(),
            show_id: Some("show".to_string()),
            audio_language: audio.0.map(str::to_string),
            audio_title: audio.1.map(str::to_string),
            subtitle_language: subtitle.0.map(str::to_string),
            subtitle_title: subtitle.1.map(str::to_string),
            subtitle_hearing_impaired: false,
            subtitle_off,
            audio_delay_ms: 0,
            subtitle_delay_ms: 0,
            updated_at: chrono::Utc::now().naive_utc(),
        }
    }

    #[test]
    fn audio_uses_preferred_language_without_remembered_choice() {
        let tracks = [track(1, Some("eng"), None), track(2, Some("ger"), None)];
        assert_eq!(choose_audio_track(&tracks, None, "de"), Some(2));
        assert_eq!(choose_audio_track(&tracks, None, "fr"), None);
        assert_eq!(choose_audio_track(&tracks, None, ""), None);
    }

    #[test]
    fn audio_prefers_remembered_title_over_language_and_config() {
        let tracks = [
            track(1, Some("eng"), Some("Main")),
            track(2, Some("eng"), Some("Commentary")),
            track(3, Some("ger"), None),
        ];
        let choice = remembered((Some("eng"), Some("Commentary")), (None, None), true);
        assert_eq!(choose_audio_track(&tracks, Some(&choice), "de"), Some(2));

        // A title that isn't in this file falls back to the remembered language
        let choice = remembered((Some("eng"), Some("Director")), (None, None), true);
        assert_eq!(choose_audio_track(&tracks, Some(&choice), "de"), Some(1));

        // A language that isn't in this file falls back to the configured one
        let choice = remembered((Some("jpn"), None), (None, None), true);
        assert_eq!(choose_audio_track(&tracks, Some(&choice), "de"), Some(3));
    }

    #[test]
    fn remembered_language_matches_other_tags_for_the_same_language() {
        let tracks = [track(1, Some("en-US"), None), track(2, Some("deu"), None)];
        let choice = remembered((Some("ger"), None), (None, None), true);
        assert_eq!(choose_audio_track(&tracks, Some(&choice), "en"), Some(2));
        let choice = remembered((Some("eng"), None), (None, None), true);
        assert_eq!(choose_audio_track(&tracks, Some(&choice), "de"), Some(1));
    }

    #[test]
    fn subtitles_follow_remembered_off_and_skip_the_none_entry() {
        let tracks = [
            TrackInfo::subtitles_off(),
            track(1, Some("eng"), Some("Forced")),
            track(2, Some("eng"), Some("Full")),
        ];
        let off = remembered((None, None), (None, None), true);
        assert_eq!(choose_subtitle_track(&tracks, Some(&off), "en"), Some(-1));

        let full = remembered((None, None), (Some("eng"), Some("Full")), false);
        assert_eq!(choose_subtitle_track(&tracks, Some(&full), ""), Some(2));

        // A remembered forced track was picked on purpose
        let forced = remembered((None, None), (Some("eng"), Some("Forced")), false);
        assert_eq!(choose_subtitle_track(&tracks, Some(&forced), ""), Some(1));

        assert_eq!(choose_subtitle_track(&tracks, None, "en"), Some(2));
        assert_eq!(choose_subtitle_track(&tracks, None, ""), None);
    }

    #[test]
    fn preferred_language_skips_forced_and_prefers_default_tracks() {
        let tracks = [
            TrackInfo::subtitles_off(),
            flagged(track(1, Some("eng"), None), true, true),
            track(2, Some("eng"), Some("SDH")),
            flagged(track(3, Some("eng"), None), false, true),
            track(4, Some("ger"), None),
        ];
        assert_eq!(choose_subtitle_track(&tracks, None, "en"), Some(3));

        // A remembered language whose title isn't in this file
        let gone = remembered((None, None), (Some("eng"), Some("Commentary")), false);
        assert_eq!(choose_subtitle_track(&tracks, Some(&gone), "de"), Some(3));

        // Only forced tracks in the language: still better than nothing
        let forced_only = [
            TrackInfo::subtitles_off(),
            flagged(track(1, Some("eng"), None), true, false),
        ];
        assert_eq!(choose_subtitle_track(&forced_only, None, "en"), Some(1));
    }

    #[test]
    fn sdh_follows_to_an_episode_that_names_it_differently() {
        let mut sdh = track(2, Some("en-GB"), Some("British | SDH"));
        sdh.hearing_impaired = true;
        let tracks = [
            TrackInfo::subtitles_off(),
            track(1, Some("en-GB"), None),
            sdh,
        ];
        let mut chose_sdh = remembered((None, None), (Some("en-GB"), Some("SDH")), false);
        chose_sdh.subtitle_hearing_impaired = true;
        assert_eq!(
            choose_subtitle_track(&tracks, Some(&chose_sdh), "en"),
            Some(2)
        );

        // Without an SDH choice the regular track wins, even over a default SDH track
        let chose_regular = remembered((None, None), (Some("en-GB"), Some("English")), false);
        assert_eq!(
            choose_subtitle_track(&tracks, Some(&chose_regular), "en"),
            Some(1)
        );
        let default_sdh = [
            TrackInfo::subtitles_off(),
            flagged(track(1, Some("eng"), Some("SDH")), false, true),
            track(2, Some("eng"), None),
        ];
        assert_eq!(choose_subtitle_track(&default_sdh, None, "en"), Some(2));
    }

    #[test]
    fn choosing_sdh_remembers_the_flag() {
        let mut preference = remembered((None, None), (None, None), false);
        PreferenceChange::Subtitle(Some(track(2, Some("eng"), Some("English SDH"))))
            .apply_to(&mut preference);
        assert!(preference.subtitle_hearing_impaired);
        PreferenceChange::Subtitle(Some(track(1, Some("eng"), None))).apply_to(&mut preference);
        assert!(!preference.subtitle_hearing_impaired);
        PreferenceChange::Subtitle(None).apply_to(&mut preference);
        assert!(!preference.subtitle_hearing_impaired);
    }

    #[test]
    fn no_track_in_the_preferred_language_leaves_the_file_default() {
        let tracks = [TrackInfo::subtitles_off(), track(1, Some("fre"), None)];
        assert_eq!(choose_subtitle_track(&tracks, None, "en"), None);
    }

    fn media_id(id: &str) -> MediaItemId {
        MediaItemId::new(id)
    }

    #[test]
    fn a_change_keeps_the_other_track_from_the_show() {
        let show_latest = remembered((Some("eng"), None), (Some("ger"), Some("Full")), false);
        let mut preference = base_preference(
            None,
            Some(show_latest),
            &media_id("ep2"),
            Some("show".to_string()),
        );
        PreferenceChange::Audio(track(2, Some("jpn"), None)).apply_to(&mut preference);

        assert_eq!(preference.media_id, "ep2");
        assert_eq!(preference.audio_language.as_deref(), Some("jpn"));
        assert_eq!(preference.subtitle_language.as_deref(), Some("ger"));
        assert_eq!(preference.subtitle_title.as_deref(), Some("Full"));
        assert!(!preference.subtitle_off);
    }

    #[test]
    fn delays_are_not_copied_from_other_episodes() {
        let mut show_latest = remembered((Some("eng"), None), (None, None), false);
        show_latest.audio_delay_ms = 300;
        show_latest.subtitle_delay_ms = -200;
        let preference = base_preference(None, Some(show_latest), &media_id("ep2"), None);
        assert_eq!(
            (preference.audio_delay_ms, preference.subtitle_delay_ms),
            (0, 0)
        );
    }

    #[test]
    fn only_choosing_none_turns_subtitles_off() {
        let mut preference = remembered((Some("eng"), None), (Some("eng"), None), false);

        PreferenceChange::Delays {
            audio_ms: 100,
            subtitle_ms: -50,
        }
        .apply_to(&mut preference);
        PreferenceChange::Audio(track(1, Some("ger"), None)).apply_to(&mut preference);
        assert!(!preference.subtitle_off);
        assert_eq!(preference.subtitle_language.as_deref(), Some("eng"));
        assert_eq!(
            (preference.audio_delay_ms, preference.subtitle_delay_ms),
            (100, -50)
        );

        PreferenceChange::Subtitle(None).apply_to(&mut preference);
        assert!(preference.subtitle_off);
        assert_eq!(preference.subtitle_language, None);

        PreferenceChange::Subtitle(Some(track(3, Some("eng"), Some("SDH"))))
            .apply_to(&mut preference);
        assert!(!preference.subtitle_off);
        assert_eq!(preference.subtitle_title.as_deref(), Some("SDH"));
    }
}
