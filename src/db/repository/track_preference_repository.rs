use super::BaseRepository;
use crate::db::entities::{
    TrackPreference, TrackPreferenceActiveModel, TrackPreferenceModel, track_preferences,
};
use anyhow::Result;
use async_trait::async_trait;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use std::sync::Arc;

/// Repository for the tracks and delays remembered per title
#[async_trait]
pub trait TrackPreferenceRepository: Send + Sync {
    /// The preference saved for exactly this media item
    async fn find_by_media_id(&self, media_id: &str) -> Result<Option<TrackPreferenceModel>>;

    /// The most recently saved preference of any episode in the show
    async fn find_latest_for_show(&self, show_id: &str) -> Result<Option<TrackPreferenceModel>>;

    /// Insert or replace the preference for the model's media item
    async fn save(&self, preference: TrackPreferenceModel) -> Result<()>;
}

#[derive(Debug)]
pub struct TrackPreferenceRepositoryImpl {
    base: BaseRepository,
}

impl TrackPreferenceRepositoryImpl {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self {
            base: BaseRepository::new(db),
        }
    }
}

#[async_trait]
impl TrackPreferenceRepository for TrackPreferenceRepositoryImpl {
    async fn find_by_media_id(&self, media_id: &str) -> Result<Option<TrackPreferenceModel>> {
        Ok(TrackPreference::find_by_id(media_id.to_string())
            .one(self.base.db.as_ref())
            .await?)
    }

    async fn find_latest_for_show(&self, show_id: &str) -> Result<Option<TrackPreferenceModel>> {
        Ok(TrackPreference::find()
            .filter(track_preferences::Column::ShowId.eq(show_id))
            .order_by_desc(track_preferences::Column::UpdatedAt)
            .one(self.base.db.as_ref())
            .await?)
    }

    async fn save(&self, preference: TrackPreferenceModel) -> Result<()> {
        use track_preferences::Column;

        let active: TrackPreferenceActiveModel = preference.into();
        TrackPreference::insert(active)
            .on_conflict(
                OnConflict::column(Column::MediaId)
                    .update_columns([
                        Column::ShowId,
                        Column::AudioLanguage,
                        Column::AudioTitle,
                        Column::SubtitleLanguage,
                        Column::SubtitleTitle,
                        Column::SubtitleHearingImpaired,
                        Column::SubtitleOff,
                        Column::AudioDelayMs,
                        Column::SubtitleDelayMs,
                        Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(self.base.db.as_ref())
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::connection::get_test_db_connection;
    use crate::db::entities::{libraries, media_items, sources};
    use chrono::{Duration, NaiveDateTime, Utc};
    use sea_orm::{ActiveModelTrait, Set};

    async fn setup() -> (Arc<DatabaseConnection>, TrackPreferenceRepositoryImpl) {
        let db = Arc::new(get_test_db_connection().await.unwrap());
        let now = Utc::now().naive_utc();

        sources::ActiveModel {
            id: Set("source".to_string()),
            name: Set("Source".to_string()),
            source_type: Set("plex".to_string()),
            auth_provider_id: Set(None),
            connection_url: Set(None),
            connections: Set(None),
            machine_id: Set(None),
            is_owned: Set(false),
            is_online: Set(true),
            last_sync: Set(None),
            last_connection_test: Set(None),
            connection_failure_count: Set(0),
            connection_quality: Set(None),
            auth_status: Set("authenticated".to_string()),
            last_auth_check: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db.as_ref())
        .await
        .unwrap();

        libraries::ActiveModel {
            id: Set("library".to_string()),
            source_id: Set("source".to_string()),
            title: Set("Shows".to_string()),
            library_type: Set("show".to_string()),
            icon: Set(None),
            item_count: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db.as_ref())
        .await
        .unwrap();

        let repo = TrackPreferenceRepositoryImpl::new(db.clone());
        (db, repo)
    }

    async fn insert_media(db: &DatabaseConnection, id: &str, parent_id: Option<&str>) {
        media_items::ActiveModel {
            id: Set(id.to_string()),
            source_id: Set("source".to_string()),
            library_id: Set("library".to_string()),
            parent_id: Set(parent_id.map(str::to_string)),
            media_type: Set(if parent_id.is_some() {
                "episode"
            } else {
                "show"
            }
            .to_string()),
            title: Set(id.to_string()),
            updated_at: Set(Utc::now().naive_utc()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    fn preference(
        media_id: &str,
        audio_language: &str,
        updated_at: NaiveDateTime,
    ) -> TrackPreferenceModel {
        TrackPreferenceModel {
            media_id: media_id.to_string(),
            show_id: Some("show".to_string()),
            audio_language: Some(audio_language.to_string()),
            audio_title: None,
            subtitle_language: None,
            subtitle_title: None,
            subtitle_hearing_impaired: false,
            subtitle_off: true,
            audio_delay_ms: 0,
            subtitle_delay_ms: 0,
            updated_at,
        }
    }

    #[tokio::test]
    async fn save_replaces_existing_preference() {
        let (db, repo) = setup().await;
        insert_media(&db, "show", None).await;
        insert_media(&db, "ep1", Some("show")).await;
        let now = Utc::now().naive_utc();

        repo.save(preference("ep1", "eng", now)).await.unwrap();
        let mut updated = preference("ep1", "ger", now);
        updated.subtitle_delay_ms = -250;
        repo.save(updated.clone()).await.unwrap();

        let stored = repo.find_by_media_id("ep1").await.unwrap().unwrap();
        assert_eq!(stored.audio_language.as_deref(), Some("ger"));
        assert_eq!(stored.subtitle_delay_ms, -250);
    }

    #[tokio::test]
    async fn latest_for_show_picks_most_recent_episode() {
        let (db, repo) = setup().await;
        insert_media(&db, "show", None).await;
        insert_media(&db, "ep1", Some("show")).await;
        insert_media(&db, "ep2", Some("show")).await;
        let now = Utc::now().naive_utc();

        repo.save(preference("ep2", "jpn", now)).await.unwrap();
        repo.save(preference("ep1", "eng", now - Duration::hours(1)))
            .await
            .unwrap();

        let latest = repo.find_latest_for_show("show").await.unwrap().unwrap();
        assert_eq!(latest.media_id, "ep2");
        assert!(repo.find_latest_for_show("other").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn preference_is_removed_with_its_media_item() {
        let (db, repo) = setup().await;
        insert_media(&db, "show", None).await;
        insert_media(&db, "ep1", Some("show")).await;
        repo.save(preference("ep1", "eng", Utc::now().naive_utc()))
            .await
            .unwrap();

        media_items::Entity::delete_by_id("ep1".to_string())
            .exec(db.as_ref())
            .await
            .unwrap();

        assert!(repo.find_by_media_id("ep1").await.unwrap().is_none());
    }
}
