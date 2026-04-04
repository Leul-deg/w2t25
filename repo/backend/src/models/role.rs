use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Role {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
}

// The 5 system roles
pub const ROLE_ADMINISTRATOR: &str = "Administrator";
pub const ROLE_TEACHER: &str = "Teacher";
pub const ROLE_ACADEMIC_STAFF: &str = "AcademicStaff";
pub const ROLE_PARENT: &str = "Parent";
pub const ROLE_STUDENT: &str = "Student";
