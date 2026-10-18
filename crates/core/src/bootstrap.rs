use media_poc::identity::IdentityStore;
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
};
use zeroize::Zeroizing;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        // Display only: database error Debug/source may contain sensitive values.
        eprintln!("{error}");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args_os().len() != 1 || !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            "bootstrap requires a local interactive terminal and accepts no arguments".into(),
        );
    }
    let root = PathBuf::from(
        std::env::var_os("MEDIA_LIBRARY_ROOT")
            .or_else(|| std::env::var_os("MEDIA_FIXTURE_DIR"))
            .unwrap_or_else(|| "tests/fixtures/generated".into()),
    );
    let path = media_poc::identity::configured_path(&root)?;
    // Do not silently create directories or modify their access rights.
    println!("Local administrator bootstrap. Protect the database directory with OS permissions.");
    print!("Login (lowercase): ");
    io::stdout().flush()?;
    let mut login = String::new();
    io::stdin().read_line(&mut login)?;
    let password = Zeroizing::new(rpassword::prompt_password("Password: ")?);
    let confirmation = Zeroizing::new(rpassword::prompt_password("Confirm password: ")?);
    if *password != *confirmation {
        return Err("password confirmation does not match".into());
    }
    drop(confirmation);
    let store = IdentityStore::open(&path).await?;
    let result = store.bootstrap(login.trim(), password).await;
    store.close().await;
    result?;
    println!("Administrator created. Sign in through /app/login/.");
    Ok(())
}
