pub mod admin;
pub mod auth;
pub mod backups;
pub mod checkins;
pub mod config_routes;
pub mod logs;
pub mod notifications;
pub mod orders;
pub mod preferences;
pub mod products;
pub mod reports;
pub mod users;

use actix_web::web;

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/v1")
            .configure(auth::configure)
            .configure(admin::configure)
            .configure(users::configure)
            .configure(checkins::configure)
            .configure(products::configure)
            .configure(orders::configure)
            .configure(reports::configure)
            .configure(config_routes::configure)
            .configure(logs::configure)
            .configure(backups::configure)
            .configure(notifications::configure)
            .configure(preferences::configure),
    );
}
