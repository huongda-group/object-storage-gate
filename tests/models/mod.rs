mod access_keys;
mod buckets;
mod concurrency;
mod multipart_uploads;
mod objects;
mod pools;
mod portability;
mod quota;
mod users;
mod users_account;

use object_storage_gate::models::pools as pool_model;

/// A pool for tests that only need a bucket to hang off something.
/// Returns the seeded fixture pool when `seed` has run, and creates one otherwise, so a test does not have to care which.
pub async fn any_pool(db: &sea_orm::DatabaseConnection) -> i32 {
    if let Ok(found) = pool_model::Model::find_by_name(db, "main").await {
        return found.id;
    }
    pool_model::Model::create(
        db,
        &pool_model::CreateParams {
            name: "main".to_string(),
            provider: pool_model::PROVIDER_MINIO.to_string(),
            physical_bucket: "osg-main".to_string(),
            ..Default::default()
        },
    )
    .await
    .expect("create test pool")
    .id
}
