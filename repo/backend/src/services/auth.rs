use crate::errors::AppError;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

pub fn hash_password(password: &str) -> Result<String, AppError> {
    if password.len() < 12 {
        return Err(AppError::ValidationError(
            "Password must be at least 12 characters".into(),
        ));
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> Result<(), AppError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|e| AppError::InternalError(e.to_string()))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AppError::Unauthorized("Invalid password".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_too_short_rejected() {
        let result = hash_password("short");
        assert!(result.is_err(), "Password shorter than 12 chars should be rejected");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("12"), "Error should mention minimum length");
    }

    #[test]
    fn password_exactly_12_accepted() {
        let result = hash_password("ExactlyTwelv");
        assert!(result.is_ok(), "12-char password should be accepted");
    }

    #[test]
    fn password_long_accepted() {
        let result = hash_password("This Is A Very Long Password That Should Work Fine!");
        assert!(result.is_ok(), "Long passwords should be accepted");
    }

    #[test]
    fn hash_and_verify_roundtrip() {
        let password = "CorrectPassword123!";
        let hash = hash_password(password).expect("should hash");
        let verify_result = verify_password(password, &hash);
        assert!(verify_result.is_ok(), "Correct password should verify");
    }

    #[test]
    fn wrong_password_rejected() {
        let hash = hash_password("CorrectPassword123!").expect("should hash");
        let verify_result = verify_password("WrongPassword123!", &hash);
        assert!(verify_result.is_err(), "Wrong password should not verify");
    }
}
