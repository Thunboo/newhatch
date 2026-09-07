use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use rand_core::OsRng;

fn main() -> anyhow::Result<()> {
    let password = rpassword::prompt_password("Password: ")?;
    let confirm = rpassword::prompt_password("Confirm password: ")?;
    anyhow::ensure!(
        !password.is_empty() && password.len() <= 1024,
        "password must contain 1..1024 bytes"
    );
    anyhow::ensure!(password == confirm, "passwords do not match");
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default().hash_password(password.as_bytes(), &salt)?;
    println!("AUTH_PASSWORD_HASH='{hash}'");
    Ok(())
}
