/// Library root for the meridian-frontend crate.
///
/// Re-exports the modules that contain testable pure-Rust logic so that
/// the integration tests in `frontend_tests/` can import them.
///
/// Heavy Yew/WASM page components are intentionally NOT re-exported here:
/// they are tested via inline `#[cfg(test)]` blocks inside each source file.
pub mod state;
pub mod router;
pub mod api;
