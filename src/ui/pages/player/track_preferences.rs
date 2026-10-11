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
use relm4::gtk;
use relm4::prelude::*;
use std::future::Future;
use std::sync::Arc;
use tracing::{debug, info, warn};

impl PlayerPage {
    pub(super) const MAX_DELAY_MS: i64 = 10_000;

    pub(super) fn clamp_delay(delay_ms: i64) -> i64 {
        delay_ms.clamp(-Self::MAX_DELAY_MS, Self::MAX_DELAY_MS)
    }

    /// Run a track or delay change the user made, then remember the result
    /// for this title and refresh the track menus.
    pub(super) fn remember_tracks_after(
        &self,
        sender: &AsyncComponentSender<Self>,
        change: impl Future<Output = Result<()>> + 'static,
    ) {
        let (Some(player), Some(media_id)) = (self.player.clone(), self.media_item_id.clone())
        else {
            return;
        };
        let db = self.db.clone();
        let (audio_delay_ms, subtitle_delay_ms) = (self.audio_delay_ms, self.subtitle_delay_ms);
        let sender = sender.clone();
        glib::spawn_future_local(async move {
            if let Err(e) = change.await {
                warn!("Failed to change tracks: {}", e);
                return;
            }
            if let Err(e) =
                save_track_preferences(&player, &db, &media_id, audio_delay_ms, subtitle_delay_ms)
                    .await
            {
                warn!("Failed to save track preferences: {}", e);
            }
            sender.input(PlayerInput::UpdateTrackMenus);
        });
    }
}

/// Tracks and delays in effect after applying preferences to a freshly loaded title
#[derive(Debug, Clone)]
pub struct AppliedTracks {
    pub audio_track: i32,
    pub subtitle_track: i32,
    pub audio_delay_ms: i64,
    pub subtitle_delay_ms: i64,
}

/// Select the remembered or preferred tracks and set the delays for a loaded title.
pub async fn apply_track_preferences(
    player: &PlayerHandle,
    db: &Arc<DatabaseConnection>,
    media_id: &MediaItemId,
) -> Result<AppliedTracks> {
    let repo = TrackPreferenceRepositoryImpl::new(db.as_ref().clone());
    let own = repo.find_by_media_id(media_id.as_ref()).await?;
    let remembered = match own.clone() {
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
        // A failed selection must not keep the previous title's delays below
        if let Err(e) = player.set_audio_track(id).await {
            warn!("Failed to select audio track {}: {}", id, e);
        }
    }
    if let Some(id) = choose_subtitle_track(
        &subtitle_tracks,
        remembered.as_ref(),
        &playback.preferred_subtitle_language,
    ) {
        if let Err(e) = player.set_subtitle_track(id).await {
            warn!("Failed to select subtitle track {}: {}", id, e);
        }
    }

    // The player keeps delays across files, so always set them
    let (audio_delay_ms, subtitle_delay_ms) = own
        .map(|p| (p.audio_delay_ms, p.subtitle_delay_ms))
        .unwrap_or((0, 0));
    player
        .set_audio_delay(audio_delay_ms as f64 / 1000.0)
        .await?;
    player
        .set_subtitle_delay(subtitle_delay_ms as f64 / 1000.0)
        .await?;

    let applied = AppliedTracks {
        audio_track: player.get_current_audio_track().await?,
        subtitle_track: player.get_current_subtitle_track().await?,
        audio_delay_ms,
        subtitle_delay_ms,
    };
    info!("Applied track preferences: {:?}", applied);
    Ok(applied)
}

/// Remember the tracks currently playing and the given delays for this title.
pub async fn save_track_preferences(
    player: &PlayerHandle,
    db: &Arc<DatabaseConnection>,
    media_id: &MediaItemId,
    audio_delay_ms: i64,
    subtitle_delay_ms: i64,
) -> Result<()> {
    let audio_id = player.get_current_audio_track().await?;
    let subtitle_id = player.get_current_subtitle_track().await?;
    let audio = player
        .get_audio_tracks()
        .await?
        .into_iter()
        .find(|t| t.id == audio_id);
    let subtitle = player
        .get_subtitle_tracks()
        .await?
        .into_iter()
        .find(|t| t.id == subtitle_id && t.id >= 0);

    let preference = TrackPreferenceModel {
        media_id: media_id.to_string(),
        show_id: show_id_for(db, media_id).await?,
        audio_language: audio.as_ref().and_then(|t| t.language.clone()),
        audio_title: audio.as_ref().and_then(|t| t.title.clone()),
        subtitle_off: subtitle.is_none(),
        subtitle_language: subtitle.as_ref().and_then(|t| t.language.clone()),
        subtitle_title: subtitle.as_ref().and_then(|t| t.title.clone()),
        audio_delay_ms,
        subtitle_delay_ms,
        updated_at: chrono::Utc::now().naive_utc(),
    };
    debug!("Saving track preferences: {:?}", preference);
    TrackPreferenceRepositoryImpl::new(db.as_ref().clone())
        .save(preference)
        .await
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

/// The remembered track (same language and title, else same language), else
/// the first track in the preferred language.
fn choose_track(
    tracks: &[TrackInfo],
    remembered_language: Option<&str>,
    remembered_title: Option<&str>,
    preferred_language: &str,
) -> Option<i32> {
    // Releases tag the same language differently ("ger", "deu", "de")
    let remembered_language = remembered_language.map(language::canonical);
    let same_language =
        |t: &&TrackInfo| t.language.as_deref().map(language::canonical) == remembered_language;
    if remembered_language.is_some() || remembered_title.is_some() {
        let exact = tracks
            .iter()
            .filter(same_language)
            .find(|t| t.title.as_deref() == remembered_title);
        let by_language = remembered_language
            .as_ref()
            .and_then(|_| tracks.iter().find(same_language));
        if let Some(track) = exact.or(by_language) {
            return Some(track.id);
        }
    }

    tracks
        .iter()
        .find(|t| {
            t.language
                .as_deref()
                .is_some_and(|lang| language::track_matches(lang, preferred_language))
        })
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
        preferred_language,
    )
}

/// `Some(-1)` turns subtitles off. `None` leaves the player's own choice,
/// which keeps forced subtitles when no subtitle language is preferred.
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
        }
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

        assert_eq!(choose_subtitle_track(&tracks, None, "en"), Some(1));
        assert_eq!(choose_subtitle_track(&tracks, None, ""), None);
    }
}
