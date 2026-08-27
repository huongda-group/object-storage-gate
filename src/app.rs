use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use loco_rs::{
    app::{AppContext, Hooks, Initializer},
    bgworker::{BackgroundWorker, Queue},
    boot::{create_app, BootResult, StartMode},
    config::Config,
    controller::AppRoutes,
    db::{self, truncate_table},
    environment::Environment,
    task::Tasks,
    Error, Result,
};
use migration::Migrator;
use std::path::Path;

#[allow(unused_imports)]
use crate::{
    controllers,
    models::_entities::{
        access_key_permissions, access_key_prefixes, access_keys, audit_logs, buckets,
        multipart_uploads, objects, pools, users,
    },
    tasks,
    workers::downloader::DownloadWorker,
};

/// Loads one fixture file.
///
/// ponytail: loco's `db::seed` ends with `reset_autoincrement`, which hard-errors on `MySQL` — but it fires only after every row is already inserted, and `InnoDB` advances the `AUTO_INCREMENT` counter on explicit-id inserts by itself, so nothing is left undone.
/// Swallow that one error, nothing else.
/// Upgrade path: patch loco upstream to no-op there instead of erroring.
async fn seed_one<A>(ctx: &AppContext, base: &Path, file: &str) -> Result<()>
where
    A: sea_orm::ActiveModelTrait + Send + Sync,
    for<'de> <<A as sea_orm::ActiveModelTrait>::Entity as sea_orm::EntityTrait>::Model:
        sea_orm::IntoActiveModel<A> + serde::de::Deserialize<'de>,
{
    let path = base.join(file).display().to_string();
    match db::seed::<A>(&ctx.db, &path).await {
        Err(Error::Message(msg)) if msg.contains("Unsupported database backend: MySQL") => Ok(()),
        other => other,
    }
}

pub struct App;
#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        env!("CARGO_CRATE_NAME")
    }

    fn app_version() -> String {
        format!(
            "{} ({})",
            env!("CARGO_PKG_VERSION"),
            option_env!("BUILD_SHA")
                .or(option_env!("GITHUB_SHA"))
                .unwrap_or("dev")
        )
    }

    async fn boot(
        mode: StartMode,
        environment: &Environment,
        config: Config,
    ) -> Result<BootResult> {
        create_app::<Self, Migrator>(mode, environment, config).await
    }

    // Refuse to start production with a missing, malformed, or publicly known master key: every access-key secret and backend-store credential would otherwise be encrypted at rest with a key anyone can read from git.
    // See `models::crypto`.
    // This hook (not `boot`) is the guard point because the loco CLI calls `create_app` directly and never goes through `Hooks::boot`.
    async fn after_context(ctx: AppContext) -> Result<AppContext> {
        if ctx.environment == Environment::Production {
            let key = std::env::var("OSG_MASTER_KEY").map_err(|_| {
                Error::string(
                    "OSG_MASTER_KEY must be set in production (base64-encoded 32-byte key)",
                )
            })?;
            crate::models::crypto::validate_master_key(&key)?;

            // loco signs JWTs with `EncodingKey::from_base64_secret`, so a JWT_SECRET that is not
            // valid base64 lets the app boot and then fails every single login with a generic
            // "unauthorized!" — indistinguishable from a wrong password. Refuse at boot instead.
            let jwt = ctx.config.get_jwt_config()?;
            STANDARD.decode(jwt.secret.trim()).map_err(|_| {
                Error::string(
                    "JWT_SECRET must be base64 (loco signs with from_base64_secret); generate one with `openssl rand -base64 32`",
                )
            })?;
        }
        Ok(ctx)
    }

    // The trailing-slash bucket form cannot go through `Routes`: loco treats `/{bucket}/` as a
    // duplicate of `/{bucket}` and panics, while axum routes them separately.
    async fn after_routes(router: axum::Router, ctx: &AppContext) -> Result<axum::Router> {
        Ok(router.merge(controllers::s3::trailing_slash_bucket_router(ctx.clone())))
    }

    async fn initializers(_ctx: &AppContext) -> Result<Vec<Box<dyn Initializer>>> {
        Ok(vec![Box::new(
            crate::initializers::rate_limit::RateLimitInitializer,
        )])
    }

    fn routes(_ctx: &AppContext) -> AppRoutes {
        AppRoutes::with_default_routes() // controller routes below
            .add_route(controllers::auth::routes())
            .add_route(controllers::api::routes())
            .add_route(controllers::admin::routes())
            .add_route(controllers::admin_pools::routes())
            .add_route(controllers::admin_audit::routes())
            .add_route(controllers::admin_pools::user_routes())
            .add_route(controllers::buckets::routes())
            // Last on purpose: /{bucket}/{*key} matches nearly everything, so the S3 tree must sit behind /api/*, the static console and the health endpoints.
            .add_route(controllers::s3::routes())
    }
    async fn connect_workers(ctx: &AppContext, queue: &Queue) -> Result<()> {
        queue.register(DownloadWorker::build(ctx)).await?;
        queue
            .register(crate::workers::audit::AuditWorker::build(ctx))
            .await?;
        Ok(())
    }

    fn register_tasks(tasks: &mut Tasks) {
        tasks.register(crate::tasks::reconcile_quota::ReconcileQuota);
        tasks.register(crate::tasks::cleanup_multipart::CleanupMultipart);
        tasks.register(crate::tasks::cleanup_audit::CleanupAudit);
        // tasks-inject (do not remove)
    }
    async fn truncate(ctx: &AppContext) -> Result<()> {
        truncate_table(&ctx.db, audit_logs::Entity).await?;
        truncate_table(&ctx.db, objects::Entity).await?;
        truncate_table(&ctx.db, multipart_uploads::Entity).await?;
        truncate_table(&ctx.db, access_key_permissions::Entity).await?;
        truncate_table(&ctx.db, access_key_prefixes::Entity).await?;
        truncate_table(&ctx.db, access_keys::Entity).await?;
        // Buckets before pools: the foreign key is RESTRICT, so a pool with rows pointing at it cannot be emptied first.
        truncate_table(&ctx.db, buckets::Entity).await?;
        truncate_table(&ctx.db, pools::Entity).await?;
        truncate_table(&ctx.db, users::Entity).await?;
        Ok(())
    }
    async fn seed(ctx: &AppContext, base: &Path) -> Result<()> {
        seed_one::<users::ActiveModel>(ctx, base, "users.yaml").await?;
        // Pools after users and before anything that references them.
        seed_one::<pools::ActiveModel>(ctx, base, "pools.yaml").await?;
        Ok(())
    }
}
