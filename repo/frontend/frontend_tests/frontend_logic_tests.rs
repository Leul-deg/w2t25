/// Frontend logic integration tests.
///
/// Tests pure-Rust business logic from the frontend crate — state management,
/// routing classification, API helper functions — without requiring a browser
/// or WASM runtime.
///
/// Run with:
///   cd frontend && cargo test --test frontend_logic_tests
use meridian_frontend::state::{AppState, LoginError, UserPublic};

// ---------------------------------------------------------------------------
// AppState — authentication helpers
// ---------------------------------------------------------------------------

fn make_user(roles: Vec<&str>) -> UserPublic {
    UserPublic {
        id: "00000000-0000-0000-0000-000000000001".into(),
        username: "test_user".into(),
        email: "test@test.local".into(),
        display_name: None,
        account_state: "active".into(),
        roles: roles.into_iter().map(String::from).collect(),
        created_at: "2024-01-01T00:00:00Z".into(),
    }
}

#[test]
fn unauthenticated_state_is_not_authenticated() {
    let state = AppState { user: None, token: None, loading: false };
    assert!(!state.is_authenticated());
}

#[test]
fn state_with_token_but_no_user_is_not_authenticated() {
    let state = AppState { user: None, token: Some("tok".into()), loading: false };
    assert!(!state.is_authenticated(), "token alone is not sufficient");
}

#[test]
fn state_with_user_and_token_is_authenticated() {
    let state = AppState {
        user: Some(make_user(vec!["Student"])),
        token: Some("tok".into()),
        loading: false,
    };
    assert!(state.is_authenticated());
}

#[test]
fn primary_role_returns_first_role() {
    let state = AppState {
        user: Some(make_user(vec!["Administrator", "Teacher"])),
        token: Some("tok".into()),
        loading: false,
    };
    assert_eq!(state.primary_role(), Some("Administrator"));
}

#[test]
fn primary_role_none_when_no_user() {
    let state = AppState { user: None, token: None, loading: false };
    assert_eq!(state.primary_role(), None);
}

#[test]
fn has_role_matches_assigned_role() {
    let state = AppState {
        user: Some(make_user(vec!["Student"])),
        token: Some("tok".into()),
        loading: false,
    };
    assert!(state.has_role("Student"));
    assert!(!state.has_role("Administrator"));
}

#[test]
fn has_role_false_when_no_user() {
    let state = AppState { user: None, token: None, loading: false };
    assert!(!state.has_role("Administrator"));
}

// ---------------------------------------------------------------------------
// LoginError — display messages and CSS classes
// ---------------------------------------------------------------------------

#[test]
fn invalid_credentials_display_message() {
    assert_eq!(
        LoginError::InvalidCredentials.display_message(),
        "Invalid username or password."
    );
}

#[test]
fn too_many_attempts_surfaces_server_message() {
    let err = LoginError::TooManyAttempts("Too many login attempts.".into());
    assert_eq!(err.display_message(), "Too many login attempts.");
}

#[test]
fn account_blocked_surfaces_server_message() {
    let err = LoginError::AccountBlocked("Account is disabled.".into());
    assert_eq!(err.display_message(), "Account is disabled.");
}

#[test]
fn network_error_shows_generic_message() {
    let err = LoginError::NetworkError("timeout".into());
    assert_eq!(
        err.display_message(),
        "A connection error occurred. Please try again."
    );
}

#[test]
fn login_error_css_classes_are_distinct() {
    let e_invalid  = LoginError::InvalidCredentials;
    let e_lockout  = LoginError::TooManyAttempts("x".into());
    let e_blocked  = LoginError::AccountBlocked("x".into());
    let e_val      = LoginError::ValidationError("x".into());
    let e_network  = LoginError::NetworkError("x".into());
    let classes = [
        e_invalid.css_class(),
        e_lockout.css_class(),
        e_blocked.css_class(),
        e_val.css_class(),
        e_network.css_class(),
    ];
    // Every error variant must have a unique CSS class.
    let mut seen = std::collections::HashSet::new();
    for c in &classes {
        assert!(seen.insert(*c), "duplicate CSS class: {}", c);
    }
}
