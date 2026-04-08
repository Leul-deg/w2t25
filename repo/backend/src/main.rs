use actix_cors::Cors;
use actix_web::{middleware::Logger, web, App, HttpServer};
use dotenv::dotenv;
use log::info;

mod config;
mod db;
mod errors;
mod middleware;
mod models;
mod routes;
mod services;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let cfg = config::Config::from_env();

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(&cfg.log_level),
    )
    .init();

    info!("Meridian backend starting on {}:{}", cfg.host, cfg.port);

    let pool = db::create_pool(&cfg.database_url)
        .await
        .expect("Failed to create database pool");

    info!("Database pool created");

    // Run migrations on startup
    db::run_migrations(&pool)
        .await
        .expect("Failed to run database migrations");

    info!("Migrations applied successfully");

    // Spawn background scheduler (auto-close unpaid orders after 30 min)
    let scheduler_pool = pool.clone();
    tokio::spawn(async move {
        services::scheduler::run_scheduler(scheduler_pool).await;
    });
    info!("Background scheduler started");

    let pool = web::Data::new(pool);
    let cfg_data = web::Data::new(cfg.clone());
    let bind_addr = format!("{}:{}", cfg.host, cfg.port);

    info!("Starting HTTP server at http://{}", bind_addr);
    info!(
        "Exports dir: {}  Backups dir: {}  Backup key: {}",
        cfg.exports_dir,
        cfg.backups_dir,
        if cfg.backup_encryption_key.is_empty() { "NOT SET (backups disabled)" } else { "configured" }
    );

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .app_data(pool.clone())
            .app_data(cfg_data.clone())
            .wrap(cors)
            .wrap(Logger::default())
            .configure(routes::configure_routes)
    })
    .bind(&bind_addr)?
    .run()
    .await
}
