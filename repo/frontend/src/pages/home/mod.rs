pub mod student;
pub mod parent;
pub mod teacher;
pub mod staff;
pub mod admin;

use yew::prelude::*;
use crate::state::AppStateContext;
use super::unauthorized::UnauthorizedPage;

#[function_component(HomePage)]
pub fn home_page() -> Html {
    let state = use_context::<AppStateContext>().expect("AppState context not found");

    match state.primary_role() {
        Some("Administrator") => html! { <admin::AdminHome /> },
        Some("Teacher") => html! { <teacher::TeacherHome /> },
        Some("AcademicStaff") => html! { <staff::StaffHome /> },
        Some("Parent") => html! { <parent::ParentHome /> },
        Some("Student") => html! { <student::StudentHome /> },
        _ => html! { <UnauthorizedPage /> },
    }
}
