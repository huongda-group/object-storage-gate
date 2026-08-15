use loco_rs::schema::*;
use sea_orm_migration::prelude::*;

const IDX_AKP_KEY_ACTION: &str = "idx_akp_key_action";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        create_table(
            m,
            "access_key_permissions",
            &[("id", ColType::PkAuto), ("action", ColType::String)],
            &[("access_keys", "")],
        )
        .await?;
        // `has_index` thay cho `IF NOT EXISTS`: MySQL không có cú pháp đó cho index.
        if !m
            .has_index("access_key_permissions", IDX_AKP_KEY_ACTION)
            .await?
        {
            m.create_index(
                Index::create()
                    .name(IDX_AKP_KEY_ACTION)
                    .table(Alias::new("access_key_permissions"))
                    .col(Alias::new("access_key_id"))
                    .col(Alias::new("action"))
                    .unique()
                    .to_owned(),
            )
            .await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        drop_table(m, "access_key_permissions").await?;
        Ok(())
    }
}
