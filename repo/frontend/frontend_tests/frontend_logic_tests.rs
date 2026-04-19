/// Frontend logic integration tests.
///
/// Tests pure-Rust business logic from the frontend crate — state management,
/// routing classification, API helper functions — without requiring a browser
/// or WASM runtime.
///
/// Run with:
///   cd frontend && cargo test --test frontend_logic_tests
use meridian_frontend::api::auth::map_http_login_error;
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

// ---------------------------------------------------------------------------
// map_http_login_error — HTTP status → LoginError mapping
// ---------------------------------------------------------------------------

#[test]
fn http_401_maps_to_invalid_credentials() {
    let err = map_http_login_error(401, "wrong password".into());
    assert!(matches!(err, LoginError::InvalidCredentials));
    // InvalidCredentials always shows the generic message, not the server message.
    assert_eq!(err.display_message(), "Invalid username or password.");
}

#[test]
fn http_429_maps_to_too_many_attempts_preserving_message() {
    let msg = "Too many login attempts. Try again in 5 minutes.".to_string();
    let err = map_http_login_error(429, msg.clone());
    match &err {
        LoginError::TooManyAttempts(m) => assert_eq!(m, &msg),
        other => panic!("expected TooManyAttempts, got {:?}", other),
    }
    assert_eq!(err.display_message(), msg.as_str());
}

#[test]
fn http_403_maps_to_account_blocked_preserving_message() {
    let msg = "Account is suspended.".to_string();
    let err = map_http_login_error(403, msg.clone());
    match &err {
        LoginError::AccountBlocked(m) => assert_eq!(m, &msg),
        other => panic!("expected AccountBlocked, got {:?}", other),
    }
    assert_eq!(err.css_class(), "error-blocked");
}

#[test]
fn http_422_maps_to_validation_error() {
    let err = map_http_login_error(422, "Username is required.".into());
    assert!(matches!(err, LoginError::ValidationError(_)));
    assert_eq!(err.css_class(), "error-validation");
}

#[test]
fn http_500_maps_to_network_error_with_code_in_message() {
    let err = map_http_login_error(500, "Internal Server Error".into());
    match &err {
        LoginError::NetworkError(msg) => assert!(msg.contains("500")),
        other => panic!("expected NetworkError, got {:?}", other),
    }
    assert_eq!(err.display_message(), "A connection error occurred. Please try again.");
}

#[test]
fn http_503_maps_to_network_error() {
    assert!(matches!(
        map_http_login_error(503, "Service Unavailable".into()),
        LoginError::NetworkError(_)
    ));
}

#[test]
fn http_200_unexpected_maps_to_network_error() {
    // A 200 in an error path (shouldn't happen, but guard the default branch).
    assert!(matches!(
        map_http_login_error(200, "unexpected".into()),
        LoginError::NetworkError(_)
    ));
}

// ---------------------------------------------------------------------------
// AppState — additional edge cases
// ---------------------------------------------------------------------------

#[test]
fn primary_role_none_when_user_has_empty_role_list() {
    let state = AppState {
        user: Some(make_user(vec![])),
        token: Some("tok".into()),
        loading: false,
    };
    assert_eq!(state.primary_role(), None, "empty roles list should give no primary role");
}

#[test]
fn has_role_is_case_sensitive() {
    let state = AppState {
        user: Some(make_user(vec!["Student"])),
        token: Some("tok".into()),
        loading: false,
    };
    assert!(!state.has_role("student"), "role check should be case-sensitive");
    assert!(!state.has_role("STUDENT"));
    assert!(state.has_role("Student"));
}

#[test]
fn is_authenticated_false_when_loading_true_with_no_user() {
    let state = AppState { user: None, token: None, loading: true };
    assert!(!state.is_authenticated(), "loading state without user must not be authenticated");
}

#[test]
fn multiple_roles_all_accessible_via_has_role() {
    let state = AppState {
        user: Some(make_user(vec!["Administrator", "Teacher", "AcademicStaff"])),
        token: Some("tok".into()),
        loading: false,
    };
    assert!(state.has_role("Administrator"));
    assert!(state.has_role("Teacher"));
    assert!(state.has_role("AcademicStaff"));
    assert!(!state.has_role("Student"));
}

// ---------------------------------------------------------------------------
// LoginError — remaining variant display coverage
// ---------------------------------------------------------------------------

#[test]
fn validation_error_surfaces_server_message() {
    let err = LoginError::ValidationError("Password must be 12+ characters.".into());
    assert_eq!(err.display_message(), "Password must be 12+ characters.");
}

#[test]
fn network_error_hides_internal_detail() {
    // The raw error detail is hidden; user sees a generic message.
    let err = LoginError::NetworkError("connection refused to 10.0.0.1:8080".into());
    assert_eq!(err.display_message(), "A connection error occurred. Please try again.");
}
