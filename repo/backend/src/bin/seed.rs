/// Meridian seed runner.
/// Run with: cargo run --bin seed
///
/// Inserts baseline data for local development and verification:
///   - 1 Administrator, 1 Teacher, 1 AcademicStaff, 1 Parent, 1 Student
///   - Sample district / campus / school hierarchy
///   - Sample product with inventory
///   - Sample active check-in window
///
/// Passwords (all ≥ 12 characters, Argon2id hashed at runtime):
///   admin_user      → Admin@Meridian1!
///   teacher_jane    → Teacher@Meridian1!
///   staff_carlos    → Staff@Meridian1!
///   parent_morgan   → Parent@Meridian1!
///   student_alex    → Student@Meridian1!

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2,
};
use dotenv::dotenv;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use uuid::Uuid;

fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("password hashing failed")
        .to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    env_logger::init();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set in environment");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&database_url)
        .await?;

    println!("Connected to database. Running seed...");

    // ------------------------------------------------------------------
    // Roles (already seeded by migration; just fetch their IDs)
    // ------------------------------------------------------------------
    let role_id = |name: &str| {
        let pool = pool.clone();
        let name = name.to_string();
        async move {
            sqlx::query_scalar::<_, i32>("SELECT id FROM roles WHERE name = $1")
                .bind(name)
                .fetch_one(&pool)
                .await
        }
    };

    let admin_role_id = role_id("Administrator").await?;
    let teacher_role_id = role_id("Teacher").await?;
    let staff_role_id = role_id("AcademicStaff").await?;
    let parent_role_id = role_id("Parent").await?;
    let student_role_id = role_id("Student").await?;

    // ------------------------------------------------------------------
    // Users
    // ------------------------------------------------------------------
    struct SeedUser {
        id: Uuid,
        username: &'static str,
        email: &'static str,
        password: &'static str,
        display_name: &'static str,
        role_id: i32,
    }

    let users = vec![
        SeedUser {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            username: "admin_user",
            email: "admin@meridian.local",
            password: "Admin@Meridian1!",
            display_name: "System Administrator",
            role_id: admin_role_id,
        },
        SeedUser {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000006").unwrap(),
            username: "scoped_admin",
            email: "scoped.admin@meridian.local",
            password: "ScopedAdmin@Meridian1!",
            display_name: "North Campus Admin",
            role_id: admin_role_id,
        },
        SeedUser {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
            username: "teacher_jane",
            email: "jane.teacher@meridian.local",
            password: "Teacher@Meridian1!",
            display_name: "Jane Okonkwo",
            role_id: teacher_role_id,
        },
        SeedUser {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap(),
            username: "staff_carlos",
            email: "carlos.staff@meridian.local",
            password: "Staff@Meridian1!",
            display_name: "Carlos Reyes",
            role_id: staff_role_id,
        },
        SeedUser {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000004").unwrap(),
            username: "parent_morgan",
            email: "morgan.parent@meridian.local",
            password: "Parent@Meridian1!",
            display_name: "Morgan Patel",
            role_id: parent_role_id,
        },
        SeedUser {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000005").unwrap(),
            username: "student_alex",
            email: "alex.student@meridian.local",
            password: "Student@Meridian1!",
            display_name: "Alex Chen",
            role_id: student_role_id,
        },
    ];

    for u in &users {
        let hash = hash_password(u.password);
        let result = sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, display_name, account_state, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, 'active', NOW(), NOW())
             ON CONFLICT (username) DO UPDATE
               SET email = EXCLUDED.email,
                   password_hash = EXCLUDED.password_hash,
                   display_name = EXCLUDED.display_name,
                   updated_at = NOW()",
        )
        .bind(u.id)
        .bind(u.username)
        .bind(u.email)
        .bind(&hash)
        .bind(u.display_name)
        .execute(&pool)
        .await?;

        println!(
            "  upserted user '{}' ({})",
            u.username,
            if result.rows_affected() == 1 { "inserted" } else { "updated" }
        );

        // Assign role (idempotent)
        sqlx::query(
            "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(u.id)
        .bind(u.role_id)
        .execute(&pool)
        .await?;
    }

    // Mark admin_user as super-admin (unrestricted).
    // scoped_admin is intentionally left with is_super_admin = false
    // and will receive an explicit campus scope assignment below.
    sqlx::query(
        "UPDATE users SET is_super_admin = true WHERE id = $1",
    )
    .bind(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap())
    .execute(&pool)
    .await?;
    println!("  marked admin_user as super-admin");

    // ------------------------------------------------------------------
    // District / Campus / School hierarchy
    // ------------------------------------------------------------------
    let district_id = Uuid::parse_str("10000000-0000-0000-0000-000000000001").unwrap();
    sqlx::query(
        "INSERT INTO districts (id, name, state, contact_email)
         VALUES ($1, 'Meridian Unified School District', 'TX', 'district@meridian.local')
         ON CONFLICT (name) DO NOTHING",
    )
    .bind(district_id)
    .execute(&pool)
    .await?;
    println!("  upserted district");

    let campus_id = Uuid::parse_str("10000000-0000-0000-0000-000000000002").unwrap();
    sqlx::query(
        "INSERT INTO campuses (id, district_id, name, address)
         VALUES ($1, $2, 'North Campus', '100 North Ave, Meridian, TX 75001')
         ON CONFLICT (district_id, name) DO NOTHING",
    )
    .bind(campus_id)
    .bind(district_id)
    .execute(&pool)
    .await?;
    println!("  upserted campus");

    // Assign scoped_admin to North Campus only (demonstrates scoped-by-default path).
    let scoped_admin_id = Uuid::parse_str("00000000-0000-0000-0000-000000000006").unwrap();
    sqlx::query(
        "INSERT INTO admin_scope_assignments (admin_id, scope_type, scope_id)
         VALUES ($1, 'campus', $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(scoped_admin_id)
    .bind(campus_id)
    .execute(&pool)
    .await?;
    println!("  assigned scoped_admin to North Campus");

    let school_id = Uuid::parse_str("10000000-0000-0000-0000-000000000003").unwrap();
    sqlx::query(
        "INSERT INTO schools (id, campus_id, name, school_type)
         VALUES ($1, $2, 'Meridian Elementary', 'elementary')
         ON CONFLICT (campus_id, name) DO NOTHING",
    )
    .bind(school_id)
    .bind(campus_id)
    .execute(&pool)
    .await?;
    println!("  upserted school");

    // Assign teacher to school
    let teacher_id = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
    sqlx::query(
        "INSERT INTO user_school_assignments (user_id, school_id, assignment_type)
         VALUES ($1, $2, 'teacher')
         ON CONFLICT DO NOTHING",
    )
    .bind(teacher_id)
    .bind(school_id)
    .execute(&pool)
    .await?;

    // Create a class with teacher_jane
    let class_id = Uuid::parse_str("10000000-0000-0000-0000-000000000004").unwrap();
    sqlx::query(
        "INSERT INTO classes (id, school_id, teacher_id, name, grade_level, academic_year)
         VALUES ($1, $2, $3, 'Homeroom 4-A', '4', '2024')
         ON CONFLICT DO NOTHING",
    )
    .bind(class_id)
    .bind(school_id)
    .bind(teacher_id)
    .execute(&pool)
    .await?;
    println!("  upserted class");

    // Enroll student_alex
    let student_id = Uuid::parse_str("00000000-0000-0000-0000-000000000005").unwrap();
    sqlx::query(
        "INSERT INTO class_enrollments (class_id, student_id)
         VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(class_id)
    .bind(student_id)
    .execute(&pool)
    .await?;

    // Link parent_morgan → student_alex
    let parent_id = Uuid::parse_str("00000000-0000-0000-0000-000000000004").unwrap();
    sqlx::query(
        "INSERT INTO parent_student_links (parent_id, student_id, relationship)
         VALUES ($1, $2, 'parent')
         ON CONFLICT DO NOTHING",
    )
    .bind(parent_id)
    .bind(student_id)
    .execute(&pool)
    .await?;
    println!("  linked parent → student");

    // ------------------------------------------------------------------
    // Sample product
    // ------------------------------------------------------------------
    let product_id = Uuid::parse_str("20000000-0000-0000-0000-000000000001").unwrap();
    sqlx::query(
        "INSERT INTO products (id, name, description, price_cents, sku, category, active)
         VALUES ($1, 'Meridian Spirit T-Shirt', 'Official school spirit t-shirt with Meridian logo', 1500, 'MER-SHIRT-001', 'Apparel', true)
         ON CONFLICT (sku) DO NOTHING",
    )
    .bind(product_id)
    .execute(&pool)
    .await?;

    // Inventory for product
    sqlx::query(
        "INSERT INTO inventory (product_id, quantity, low_stock_threshold)
         VALUES ($1, 50, 5)
         ON CONFLICT (product_id) DO UPDATE SET quantity = EXCLUDED.quantity",
    )
    .bind(product_id)
    .execute(&pool)
    .await?;
    println!("  upserted product + inventory");

    // ------------------------------------------------------------------
    // Default commerce config + campaign toggles
    // ------------------------------------------------------------------
    sqlx::query(
        "INSERT INTO config_values (id, key, value, value_type, description, scope)
         VALUES
           (gen_random_uuid(), 'shipping_fee_cents', '695', 'integer',
            'Default shipping fee charged per order, in cents ($6.95)', 'global'),
           (gen_random_uuid(), 'points_rate_per_dollar', '1', 'integer',
            'Points earned per whole dollar of subtotal (1 point per $1.00)', 'global')
         ON CONFLICT (key) DO NOTHING",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "INSERT INTO campaign_toggles (id, name, description, enabled)
         VALUES
           (gen_random_uuid(), 'store_enabled', 'Enable the merch store for all users', true),
           (gen_random_uuid(), 'points_enabled', 'Enable loyalty points on purchases', true),
           (gen_random_uuid(), 'free_shipping', 'Offer free shipping when enabled', false)
         ON CONFLICT (name) DO NOTHING",
    )
    .execute(&pool)
    .await?;
    println!("  upserted commerce config + campaign toggles");

    // ------------------------------------------------------------------
    // Sample active check-in window (opens now, closes in 8 hours)
    // ------------------------------------------------------------------
    let admin_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let window_id = Uuid::parse_str("30000000-0000-0000-0000-000000000001").unwrap();
    sqlx::query(
        "INSERT INTO checkin_windows (id, school_id, class_id, title, description, opens_at, closes_at, allow_late, created_by, active)
         VALUES ($1, $2, $3, 'Morning Check-In — April 3, 2026', 'Regular morning attendance check-in', NOW(), NOW() + INTERVAL '8 hours', false, $4, true)
         ON CONFLICT DO NOTHING",
    )
    .bind(window_id)
    .bind(school_id)
    .bind(class_id)
    .bind(admin_id)
    .execute(&pool)
    .await?;
    println!("  upserted check-in window");

    println!("\nSeed complete. Seeded credentials:");
    println!("  admin_user    / Admin@Meridian1!          [Administrator, is_super_admin=true]");
    println!("  scoped_admin  / ScopedAdmin@Meridian1!    [Administrator, scoped to North Campus]");
    println!("  teacher_jane  / Teacher@Meridian1!        [Teacher]");
    println!("  staff_carlos  / Staff@Meridian1!          [AcademicStaff]");
    println!("  parent_morgan / Parent@Meridian1!         [Parent]");
    println!("  student_alex  / Student@Meridian1!        [Student]");

    Ok(())
}
