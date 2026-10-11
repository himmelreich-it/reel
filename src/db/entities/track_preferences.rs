use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// The audio/subtitle tracks and sync delays last chosen while playing a title.
///
/// Tracks are stored by language and title because track ids differ between
/// player backends. `show_id` lets other episodes of the show reuse the choice.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "track_preferences")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub media_id: String,
    pub show_id: Option<String>,
    pub audio_language: Option<String>,
    pub audio_title: Option<String>,
    pub subtitle_language: Option<String>,
    pub subtitle_title: Option<String>,
    /// The chosen subtitles are for the hearing impaired (SDH), so a file
    /// that names its SDH track differently still gets SDH
    pub subtitle_hearing_impaired: bool,
    pub subtitle_off: bool,
    pub audio_delay_ms: i64,
    pub subtitle_delay_ms: i64,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::media_items::Entity",
        from = "Column::MediaId",
        to = "super::media_items::Column::Id"
    )]
    MediaItem,
}

impl Related<super::media_items::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MediaItem.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
