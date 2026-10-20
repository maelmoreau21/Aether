//! Operator-run PostgreSQL 18 custom archives. Never migrates SQLite data.
use sqlx::{ConnectOptions, Connection, postgres::PgConnectOptions};
use std::{fs::OpenOptions, path::Path, process::Stdio, str::FromStr};
use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum PostgresBackupError {
    #[error("invalid PostgreSQL connection configuration")]
    Configuration,
    #[error("backup path rejected")]
    Path,
    #[error("backup target already exists")]
    TargetExists,
    #[error("backup file operation failed")]
    Io,
    #[error("PostgreSQL 18 client tools must be installed and available on PATH")]
    Client,
    #[error("PostgreSQL archive command failed; no connection details are logged")]
    Command,
    #[error("PostgreSQL target inspection failed")]
    Database,
    #[error("restore requires an empty PostgreSQL database and a stopped Core")]
    NotEmpty,
}

fn configured_command(tool: &str, database_url: &str) -> Result<Command, PostgresBackupError> {
    let url = url::Url::parse(database_url).map_err(|_| PostgresBackupError::Configuration)?;
    if !matches!(url.scheme(), "postgres" | "postgresql") {
        return Err(PostgresBackupError::Configuration);
    }
    // SQLx warns with values for unknown URI options: reject before parsing.
    if url.query_pairs().any(|(key, _)| {
        !matches!(
            key.as_ref(),
            "sslmode"
                | "ssl-mode"
                | "host"
                | "hostaddr"
                | "port"
                | "user"
                | "dbname"
                | "password"
                | "statement-cache-capacity"
                | "sslrootcert"
                | "ssl-root-cert"
                | "ssl-ca"
                | "sslcert"
                | "ssl-cert"
                | "sslkey"
                | "ssl-key"
                | "application_name"
                | "options"
        )
    }) {
        return Err(PostgresBackupError::Configuration);
    }
    let mut command = Command::new(tool);
    // libpq does not expand a URI stored in PGDATABASE. Pass its components
    // separately through environment variables; secrets never become argv.
    let options =
        PgConnectOptions::from_str(database_url).map_err(|_| PostgresBackupError::Configuration)?;
    command.env(
        "PGDATABASE",
        options
            .get_database()
            .ok_or(PostgresBackupError::Configuration)?,
    );
    if let Some(socket) = options.get_socket() {
        command.env("PGHOST", socket);
    } else {
        command.env("PGHOST", options.get_host());
    }
    command.env("PGPORT", options.get_port().to_string());
    command.env("PGUSER", options.get_username());
    if let Some(password) = url.password() {
        let encoded = format!("value={}", password.replace('&', "%26").replace('+', "%2B"));
        let decoded = url::form_urlencoded::parse(encoded.as_bytes())
            .next()
            .ok_or(PostgresBackupError::Configuration)?
            .1
            .into_owned();
        command.env("PGPASSWORD", decoded);
    } else {
        command.env_remove("PGPASSWORD");
    }
    command.env(
        "PGSSLMODE",
        match options.get_ssl_mode() {
            sqlx::postgres::PgSslMode::Disable => "disable",
            sqlx::postgres::PgSslMode::Allow => "allow",
            sqlx::postgres::PgSslMode::Prefer => "prefer",
            sqlx::postgres::PgSslMode::Require => "require",
            sqlx::postgres::PgSslMode::VerifyCa => "verify-ca",
            sqlx::postgres::PgSslMode::VerifyFull => "verify-full",
        },
    );
    for (key, value) in url.query_pairs() {
        let variable = match key.as_ref() {
            "sslmode"
            | "ssl-mode"
            | "host"
            | "hostaddr"
            | "port"
            | "user"
            | "dbname"
            | "statement-cache-capacity" => continue,
            "password" => "PGPASSWORD",
            "sslrootcert" | "ssl-root-cert" | "ssl-ca" => "PGSSLROOTCERT",
            "sslcert" | "ssl-cert" => "PGSSLCERT",
            "sslkey" | "ssl-key" => "PGSSLKEY",
            "application_name" => "PGAPPNAME",
            "options" => "PGOPTIONS",
            _ => return Err(PostgresBackupError::Configuration),
        };
        command.env(variable, value.as_ref());
    }
    command.env("PGCONNECT_TIMEOUT", "15");
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command.kill_on_drop(true);
    Ok(command)
}

async fn require_client(tool: &str) -> Result<(), PostgresBackupError> {
    let output = Command::new(tool)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|_| PostgresBackupError::Client)?;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout).contains("(PostgreSQL) 18.")
    {
        return Err(PostgresBackupError::Client);
    }
    Ok(())
}

struct TemporaryArchive(std::path::PathBuf);
impl Drop for TemporaryArchive {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Snapshot into a new custom archive. Existing files are never overwritten.
pub async fn backup(
    database_url: &str,
    target: &Path,
    media_root: Option<&Path>,
) -> Result<u64, PostgresBackupError> {
    crate::backup::check_path_safety(target, "backup")
        .and_then(|()| crate::backup::check_not_in_media_root(target, media_root, "backup"))
        .map_err(|_| PostgresBackupError::Path)?;
    if target.try_exists().map_err(|_| PostgresBackupError::Io)? {
        return Err(PostgresBackupError::TargetExists);
    }
    require_client("pg_dump").await?;
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let temporary =
        TemporaryArchive(parent.join(format!(".media-backup-{}.tmp", uuid::Uuid::new_v4())));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(&temporary.0)
        .map_err(|_| PostgresBackupError::Io)?;
    let mut command = configured_command("pg_dump", database_url)?;
    command.args(["--no-password", "--format=custom", "--no-owner", "--no-acl"]);
    command.stdout(Stdio::from(
        file.try_clone().map_err(|_| PostgresBackupError::Io)?,
    ));
    if !command
        .status()
        .await
        .map_err(|_| PostgresBackupError::Command)?
        .success()
    {
        return Err(PostgresBackupError::Command);
    }
    file.sync_all().map_err(|_| PostgresBackupError::Io)?;
    let size = file.metadata().map_err(|_| PostgresBackupError::Io)?.len();
    drop(file);
    // Same-directory hard link publishes atomically and refuses an existing target.
    std::fs::hard_link(&temporary.0, target).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            PostgresBackupError::TargetExists
        } else {
            PostgresBackupError::Io
        }
    })?;
    Ok(size)
}

/// Restore only into an empty database. Stop Core first; no --clean or overwrite.
pub async fn restore(
    database_url: &str,
    source: &Path,
    media_root: Option<&Path>,
) -> Result<(), PostgresBackupError> {
    crate::backup::check_path_safety(source, "backup")
        .and_then(|()| crate::backup::check_not_in_media_root(source, media_root, "backup"))
        .map_err(|_| PostgresBackupError::Path)?;
    require_client("pg_restore").await?;
    let mut command = configured_command("pg_restore", database_url)?;
    let options = PgConnectOptions::from_str(database_url)
        .map_err(|_| PostgresBackupError::Configuration)?
        .disable_statement_logging();
    let mut connection = sqlx::PgConnection::connect_with(&options)
        .await
        .map_err(|_| PostgresBackupError::Database)?;
    let occupied: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' AND n.nspname NOT LIKE 'pg_temp%') OR EXISTS (SELECT 1 FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema')) OR EXISTS (SELECT 1 FROM pg_catalog.pg_type t JOIN pg_catalog.pg_namespace n ON n.oid=t.typnamespace WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND t.typtype IN ('e','d'))"
    ).fetch_one(&mut connection).await.map_err(|_| PostgresBackupError::Database)?;
    connection
        .close()
        .await
        .map_err(|_| PostgresBackupError::Database)?;
    if occupied {
        return Err(PostgresBackupError::NotEmpty);
    }
    let file = std::fs::File::open(source).map_err(|_| PostgresBackupError::Io)?;
    if !file
        .metadata()
        .map_err(|_| PostgresBackupError::Io)?
        .is_file()
    {
        return Err(PostgresBackupError::Path);
    }
    command.args([
        "--no-password",
        "--dbname=",
        "--single-transaction",
        "--exit-on-error",
        "--no-owner",
        "--no-acl",
    ]);
    command.stdin(Stdio::from(file));
    if !command
        .status()
        .await
        .map_err(|_| PostgresBackupError::Command)?
        .success()
    {
        return Err(PostgresBackupError::Command);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_secret_is_environment_only_and_errors_are_sanitized() {
        let url = "postgresql://operator:secret-test-only@localhost/media";
        let command = configured_command("pg_dump", url).unwrap();
        let command = command.as_std();
        assert_eq!(command.get_args().count(), 0);
        assert!(command.get_envs().any(
            |(key, value)| key == "PGDATABASE" && value == Some(std::ffi::OsStr::new("media"))
        ));
        assert!(command.get_envs().any(|(key, value)| key == "PGPASSWORD"
            && value == Some(std::ffi::OsStr::new("secret-test-only"))));
        for error in [
            PostgresBackupError::Configuration,
            PostgresBackupError::Database,
            PostgresBackupError::Command,
        ] {
            assert!(!error.to_string().contains("secret-test-only"));
        }
        assert!(configured_command("pg_dump", "sqlite:///secret-test-only").is_err());
    }

    #[test]
    fn encoded_password_and_tls_options_are_preserved() {
        let command = configured_command("pg_dump", "postgresql://operator:p%40ss%2B%26word@localhost/media?sslmode=verify-full&sslrootcert=ca.pem").unwrap();
        let command = command.as_std();
        for (key, expected) in [
            ("PGPASSWORD", "p@ss+&word"),
            ("PGSSLMODE", "verify-full"),
            ("PGSSLROOTCERT", "ca.pem"),
        ] {
            assert!(
                command
                    .get_envs()
                    .any(|(name, value)| name == key
                        && value == Some(std::ffi::OsStr::new(expected)))
            );
        }
        assert!(
            configured_command(
                "pg_dump",
                "postgresql://localhost/media?unknown=secret-test-only"
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn refuses_existing_target_and_media_directory_before_invoking_tools() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("existing.dump");
        std::fs::write(&target, b"preserve existing archive").unwrap();
        assert!(matches!(
            backup("postgresql://localhost/media", &target, None).await,
            Err(PostgresBackupError::TargetExists)
        ));
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"preserve existing archive"
        );
        let media_target = directory.path().join("inside-media.dump");
        assert!(matches!(
            backup(
                "postgresql://localhost/media",
                &media_target,
                Some(directory.path())
            )
            .await,
            Err(PostgresBackupError::Path)
        ));
        assert!(!media_target.exists());
    }
}
