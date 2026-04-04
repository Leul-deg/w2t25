use yew_router::prelude::*;

#[derive(Clone, Routable, PartialEq, Debug)]
pub enum Route {
    #[at("/")]
    Home,
    #[at("/login")]
    Login,

    // ── Store ────────────────────────────────────────────────────────────
    #[at("/store")]
    Store,
    #[at("/orders")]
    Orders,

    // ── Admin ────────────────────────────────────────────────────────────
    #[at("/admin")]
    Admin,
    #[at("/admin/users")]
    AdminUsers,
    #[at("/admin/deletion-requests")]
    AdminDeletionRequests,
    #[at("/admin/products")]
    AdminProducts,
    #[at("/admin/orders")]
    AdminOrders,
    #[at("/admin/config")]
    AdminConfig,
    #[at("/admin/kpi")]
    AdminKpi,
    #[at("/admin/reports")]
    AdminReports,
    #[at("/admin/backups")]
    AdminBackups,
    #[at("/admin/logs")]
    AdminLogs,

    // ── Staff ────────────────────────────────────────────────────────────
    #[at("/teacher/classes")]
    TeacherClasses,

    // ── Check-in ─────────────────────────────────────────────────────────
    #[at("/checkin")]
    Checkin,
    #[at("/checkin/review")]
    CheckinReview,

    // ── Common ───────────────────────────────────────────────────────────
    #[at("/inbox")]
    Inbox,
    #[at("/preferences")]
    Preferences,
    #[at("/unauthorized")]
    Unauthorized,
    #[not_found]
    #[at("/404")]
    NotFound,
}
