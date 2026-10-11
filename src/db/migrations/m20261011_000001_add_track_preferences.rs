use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Audio/subtitle choices and sync delays the user made for a title
        manager
            .create_table(
                Table::create()
                    .table(TrackPreferences::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TrackPreferences::MediaId)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TrackPreferences::ShowId).string())
                    .col(ColumnDef::new(TrackPreferences::AudioLanguage).string())
                    .col(ColumnDef::new(TrackPreferences::AudioTitle).string())
                    .col(ColumnDef::new(TrackPreferences::SubtitleLanguage).string())
                    .col(ColumnDef::new(TrackPreferences::SubtitleTitle).string())
                    .col(
                        ColumnDef::new(TrackPreferences::SubtitleOff)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(TrackPreferences::AudioDelayMs)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(TrackPreferences::SubtitleDelayMs)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(TrackPreferences::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_track_preferences_media_item")
                            .from(TrackPreferences::Table, TrackPreferences::MediaId)
                            .to(MediaItems::Table, MediaItems::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Episodes look up the latest choice made anywhere in their show
        manager
            .create_index(
                Index::create()
                    .name("idx_track_preferences_show_updated")
                    .table(TrackPreferences::Table)
                    .col(TrackPreferences::ShowId)
                    .col(TrackPreferences::UpdatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(TrackPreferences::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum TrackPreferences {
    Table,
    MediaId,
    ShowId,
    AudioLanguage,
    AudioTitle,
    SubtitleLanguage,
    SubtitleTitle,
    SubtitleOff,
    AudioDelayMs,
    SubtitleDelayMs,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum MediaItems {
    Table,
    Id,
}
