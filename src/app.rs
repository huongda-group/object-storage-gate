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
        access_key_permissions, access_key_prefixes, access_keys, buckets, objects, users,
    },
    tasks,
    workers::downloader::DownloadWorker,
};

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
            .add_route(controllers::buckets::routes())
    }
    async fn connect_workers(ctx: &AppContext, queue: &Queue) -> Result<()> {
        queue.register(DownloadWorker::build(ctx)).await?;
        Ok(())
    }

    fn register_tasks(tasks: &mut Tasks) {
        tasks.register(crate::tasks::reconcile_quota::ReconcileQuota);
        // tasks-inject (do not remove)
    }
    async fn truncate(ctx: &AppContext) -> Result<()> {
        truncate_table(&ctx.db, objects::Entity).await?;
        truncate_table(&ctx.db, access_key_permissions::Entity).await?;
        truncate_table(&ctx.db, access_key_prefixes::Entity).await?;
        truncate_table(&ctx.db, access_keys::Entity).await?;
        truncate_table(&ctx.db, buckets::Entity).await?;
        truncate_table(&ctx.db, users::Entity).await?;
        Ok(())
    }
    async fn seed(ctx: &AppContext, base: &Path) -> Result<()> {
        let path = base.join("users.yaml").display().to_string();
        // ponytail: loco's db::seed ends with reset_autoincrement, which hard-errors on MySQL — but it fires only after every row is already inserted, and InnoDB advances the AUTO_INCREMENT counter on explicit-id inserts by itself, so nothing is left undone.
        // Swallow that one error, nothing else.
        // Upgrade path: patch loco upstream to no-op there instead of erroring.
        match db::seed::<users::ActiveModel>(&ctx.db, &path).await {
            Err(Error::Message(msg)) if msg.contains("Unsupported database backend: MySQL") => {
                Ok(())
            }
            other => other,
        }
    }
}
