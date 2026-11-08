//! Upgrade tests on databases created from historical `schema.sql` revisions.
//!
//! Every revision in `tests/fixtures/schema_history` is installed, filled with representative
//! rows, copied, then opened by the current Core. We check data, integrity, structural parity with
//! a fresh install, repeated starts, and recovery after a failed upgrade.
//!
//! SQLite always runs. PostgreSQL needs a disposable server:
//! `MEDIA_TEST_POSTGRES_URL=postgres://... cargo test -p media-poc --test schema_upgrade -- --ignored`
use media_poc::database::initialize_fresh_schema;
use sqlx::{AnyConnection, Connection, Row};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;

/// Revisions of `schema.sql`, oldest first. Their commit ids are the fixture names.
const REVISIONS: &[&str] = &["1be8ad5", "38ca514", "7ee0404", "9f10420"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Engine {
    Sqlite,
    Postgres,
}

fn legacy_schema(revision: &str, engine: Engine) -> String {
    let name = match engine {
        Engine::Sqlite => "sqlite",
        Engine::Postgres => "postgres",
    };
    let path = format!(
        "{}/tests/fixtures/schema_history/{revision}.{name}.sql.gz",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let mut text = String::new();
    flate2::read::GzDecoder::new(bytes.as_slice())
        .read_to_string(&mut text)
        .unwrap();
    text
}

macro_rules! sql {
    ($text:expr) => {
        sqlx::raw_sql(sqlx::AssertSqlSafe(AsRef::<str>::as_ref(&$text).to_owned()))
    };
}

fn pg_base_url() -> String {
    std::env::var("MEDIA_TEST_POSTGRES_URL")
        .expect("set MEDIA_TEST_POSTGRES_URL to a disposable PostgreSQL 18 server")
}

/// One isolated database: a SQLite file in a temp dir, or a throw-away PostgreSQL schema.
struct Db {
    engine: Engine,
    conn: AnyConnection,
    dir: tempfile::TempDir,
    sqlite_path: PathBuf,
    pg_schema: Option<(String, String)>,
}

impl Db {
    async fn open(engine: Engine) -> Self {
        sqlx::any::install_default_drivers();
        let dir = tempfile::tempdir().unwrap();
        let sqlite_path = dir.path().join("aether.db");
        match engine {
            Engine::Sqlite => {
                let mut db = Self {
                    engine,
                    conn: Self::connect_sqlite(&sqlite_path).await,
                    dir,
                    sqlite_path,
                    pg_schema: None,
                };
                db.pragma_foreign_keys_on().await;
                db
            }
            Engine::Postgres => {
                let base = pg_base_url();
                let schema = format!("upgrade_{}", uuid::Uuid::new_v4().simple());
                let mut admin = AnyConnection::connect(&base).await.unwrap();
                sql!(format!("CREATE SCHEMA {schema}"))
                    .execute(&mut admin)
                    .await
                    .unwrap();
                let mut url = url::Url::parse(&base).unwrap();
                url.query_pairs_mut()
                    .append_pair("options", &format!("-csearch_path={schema}"));
                let url = url.to_string();
                Self {
                    engine,
                    conn: AnyConnection::connect(&url).await.unwrap(),
                    dir,
                    sqlite_path,
                    pg_schema: Some((base, schema)),
                }
            }
        }
    }

    async fn connect_sqlite(path: &std::path::Path) -> AnyConnection {
        let url = format!(
            "sqlite://{}?mode=rwc",
            path.to_string_lossy().replace('\\', "/")
        );
        AnyConnection::connect(&url).await.unwrap()
    }

    async fn pragma_foreign_keys_on(&mut self) {
        sql!("PRAGMA foreign_keys=ON")
            .execute(&mut self.conn)
            .await
            .unwrap();
    }

    async fn exec(&mut self, text: &str) {
        sql!(text).execute(&mut self.conn).await.unwrap();
    }

    async fn one_i64(&mut self, text: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(text.to_owned()))
            .fetch_one(&mut self.conn)
            .await
            .unwrap()
    }

    async fn strings(&mut self, text: &str) -> Vec<String> {
        sqlx::query(sqlx::AssertSqlSafe(text.to_owned()))
            .fetch_all(&mut self.conn)
            .await
            .unwrap()
            .iter()
            .map(|row| row.get::<String, _>(0))
            .collect()
    }

    async fn install_legacy(&mut self, revision: &str) {
        let schema = legacy_schema(revision, self.engine);
        self.exec(&schema).await;
    }

    async fn upgrade(&mut self) -> Result<(), sqlx::Error> {
        initialize_fresh_schema(&mut self.conn).await
    }

    async fn teardown(self) {
        if let Some((base, schema)) = self.pg_schema {
            drop(self.conn);
            let mut admin = AnyConnection::connect(&base).await.unwrap();
            sql!(format!("DROP SCHEMA {schema} CASCADE"))
                .execute(&mut admin)
                .await
                .unwrap();
        }
        drop(self.dir);
    }

    /// User tables, excluding SQLite's internal and full-text shadow tables.
    async fn tables(&mut self) -> Vec<String> {
        match self.engine {
            Engine::Sqlite => self.strings("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' AND name NOT LIKE 'inventory_works_fts\\_%' ESCAPE '\\' ORDER BY name").await,
            Engine::Postgres => self.strings("SELECT CAST(table_name AS TEXT) FROM information_schema.tables WHERE table_schema=current_schema() AND table_type='BASE TABLE' ORDER BY 1").await,
        }
    }

    async fn row_counts(&mut self) -> BTreeMap<String, i64> {
        let mut counts = BTreeMap::new();
        for table in self.tables().await {
            let count = self.one_i64(&format!("SELECT COUNT(*) FROM {table}")).await;
            counts.insert(table, count);
        }
        counts
    }

    /// Deterministic text dump of selected columns, for before/after data comparison.
    async fn dump(&mut self, table: &str, columns: &str, order: &str) -> Vec<String> {
        let expression = columns
            .split(',')
            .map(|c| format!("COALESCE(CAST({c} AS TEXT),'~')"))
            .collect::<Vec<_>>()
            .join("||'|'||");
        self.strings(&format!(
            "SELECT {expression} FROM {table} ORDER BY {order}"
        ))
        .await
    }

    /// Everything the structural comparison looks at, one line per fact.
    async fn shape(&mut self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        match self.engine {
            Engine::Sqlite => {
                for table in self.tables().await {
                    let rows = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT name,type,\"notnull\",COALESCE(dflt_value,''),pk FROM pragma_table_info('{table}')")))
                        .fetch_all(&mut self.conn).await.unwrap();
                    for row in rows {
                        out.insert(
                            format!("column {table}.{}", row.get::<String, _>(0)),
                            format!(
                                "{} notnull={} default={} pk={}",
                                row.get::<String, _>(1).replace("BIGINT", "INTEGER"),
                                row.get::<i64, _>(2),
                                row.get::<String, _>(3),
                                row.get::<i64, _>(4)
                            ),
                        );
                    }
                    let fks = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT \"from\",\"table\",COALESCE(\"to\",''),on_delete FROM pragma_foreign_key_list('{table}')")))
                        .fetch_all(&mut self.conn).await.unwrap();
                    for row in fks {
                        out.insert(
                            format!("fk {table}.{}", row.get::<String, _>(0)),
                            format!(
                                "-> {}({}) on delete {}",
                                row.get::<String, _>(1),
                                row.get::<String, _>(2),
                                row.get::<String, _>(3)
                            ),
                        );
                    }
                }
                let indexes = sqlx::query("SELECT name,tbl_name FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' AND tbl_name NOT LIKE 'inventory_works_fts\\_%' ESCAPE '\\'")
                    .fetch_all(&mut self.conn).await.unwrap();
                for row in indexes {
                    let name: String = row.get(0);
                    let columns = self
                        .strings(&format!("SELECT COALESCE(name,'<expr>') FROM pragma_index_xinfo('{name}') WHERE key=1 ORDER BY seqno"))
                        .await
                        .join(",");
                    let flags = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT \"unique\",partial FROM pragma_index_list('{}') WHERE name='{name}'", row.get::<String, _>(1))))
                        .fetch_one(&mut self.conn).await.unwrap();
                    out.insert(
                        format!("index {name}"),
                        format!(
                            "on {} ({columns}) unique={} partial={}",
                            row.get::<String, _>(1),
                            flags.get::<i64, _>(0),
                            flags.get::<i64, _>(1)
                        ),
                    );
                }
                for (kind, label) in [("trigger", "trigger"), ("view", "view")] {
                    for name in self.strings(&format!("SELECT name || ' on ' || tbl_name FROM sqlite_master WHERE type='{kind}' AND tbl_name NOT LIKE 'inventory_works_fts\\_%' ESCAPE '\\'")).await {
                        out.insert(format!("{label} {name}"), String::new());
                    }
                }
            }
            Engine::Postgres => {
                let rows = sqlx::query("SELECT CAST(table_name AS TEXT),CAST(column_name AS TEXT),CAST(data_type AS TEXT),CAST(is_nullable AS TEXT),COALESCE(column_default,'') FROM information_schema.columns WHERE table_schema=current_schema()")
                    .fetch_all(&mut self.conn).await.unwrap();
                for row in rows {
                    out.insert(
                        format!(
                            "column {}.{}",
                            row.get::<String, _>(0),
                            row.get::<String, _>(1)
                        ),
                        format!(
                            "{} nullable={} default={}",
                            row.get::<String, _>(2),
                            row.get::<String, _>(3),
                            row.get::<String, _>(4)
                        ),
                    );
                }
                let rows = sqlx::query("SELECT CAST(indexname AS TEXT),CAST(indexdef AS TEXT) FROM pg_indexes WHERE schemaname=current_schema()")
                    .fetch_all(&mut self.conn).await.unwrap();
                for row in rows {
                    let definition: String = row.get(1);
                    let schema = self
                        .pg_schema
                        .as_ref()
                        .map(|(_, schema)| format!("{schema}."))
                        .unwrap_or_default();
                    let definition = definition
                        .split(" ON ")
                        .nth(1)
                        .unwrap_or(&definition)
                        .replace("ONLY ", "")
                        .replace(&schema, "");
                    let unique = row.get::<String, _>(1).contains("UNIQUE");
                    out.insert(
                        format!("index {}", row.get::<String, _>(0)),
                        format!("{definition} unique={unique}"),
                    );
                }
                let rows = sqlx::query("SELECT CAST(c.conrelid::regclass AS TEXT),CAST(c.conname AS TEXT),pg_get_constraintdef(c.oid) FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace WHERE n.nspname=current_schema()")
                    .fetch_all(&mut self.conn).await.unwrap();
                for row in rows {
                    out.insert(
                        format!(
                            "constraint {}.{}",
                            row.get::<String, _>(0),
                            row.get::<String, _>(1)
                        ),
                        row.get::<String, _>(2),
                    );
                }
                for name in self
                    .strings("SELECT CAST(tgname AS TEXT) FROM pg_trigger WHERE NOT tgisinternal")
                    .await
                {
                    out.insert(format!("trigger {name}"), String::new());
                }
                for name in self.strings("SELECT CAST(table_name AS TEXT) FROM information_schema.views WHERE table_schema=current_schema()").await {
                    out.insert(format!("view {name}"), String::new());
                }
            }
        }
        out
    }

    async fn integrity(&mut self) {
        match self.engine {
            Engine::Sqlite => {
                assert_eq!(self.strings("PRAGMA integrity_check").await, ["ok"]);
                let violations = self
                    .strings("SELECT \"table\" FROM pragma_foreign_key_check")
                    .await;
                assert!(
                    violations.is_empty(),
                    "foreign key violations in {violations:?}"
                );
            }
            Engine::Postgres => {
                // Constraints are enforced on write; nothing to scan beyond a validity check.
                let invalid = self.one_i64("SELECT COUNT(*) FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace WHERE n.nspname=current_schema() AND NOT c.convalidated").await;
                assert_eq!(invalid, 0, "unvalidated constraints");
            }
        }
    }
}

/// Rows that exist in every revision. Unicode titles guard future localized metadata.
const SEED: &str = "
INSERT INTO users(id,role,created_at,display_name) VALUES('u-admin','administrator',100,'Admin'),('u-user','user',101,'Amélie 千尋');
INSERT INTO local_credentials(user_id,login,password_hash) VALUES('u-admin','admin','hash-a'),('u-user','amelie','hash-b');
INSERT INTO sessions(token_hash,user_id,created_at,last_seen_at,expires_at) VALUES('tok1','u-user',1,2,2000000000);
INSERT INTO libraries(id,name,kind,node_id,root,enabled) VALUES('lib-films','Films','films','local','/media/films',1),('lib-series','Séries','series','local','/media/series',1);
INSERT INTO library_permissions(library_id,user_id) VALUES('lib-films','u-user');
INSERT INTO assets(sha256,size,mime,created_at) VALUES('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',2147483647,'video/mp4',1);
INSERT INTO inventory_works(id,library_id,identity_key,title,external_ids_json) VALUES('w1','lib-films','film:le-film','Le Voyage de Chihiro — 千と千尋の神隠し','{}'),('w2','lib-films','film:ete','Été 2024 ☀','{}');
INSERT INTO inventory_editions(id,work_id) VALUES('e1','w1');
INSERT INTO inventory_versions(id,edition_id) VALUES('v1','e1');
INSERT INTO inventory_sources(root_snapshot,id,version_id,library_id) VALUES('snap','s1','v1','lib-films');
INSERT INTO inventory_assets(id,source_id,library_id,relative_path,size,modified_ns,mime,probe_json,available) VALUES('a1','s1','lib-films','film.mkv',1000,'1','video/x-matroska','{}',1);
INSERT INTO manual_timeline_markers(asset_id,marker_type,start_seconds,end_seconds,enabled,updated_by,updated_at) VALUES('a1','intro',1.0,30.0,1,'u-admin',5);
INSERT INTO durable_tasks(id,library_id,owner,idempotency_key,node_id,category,created_at,updated_at,available_at,priority,attempt) VALUES('parent','lib-films','test','parent','local','scan',1,2,3,321,2),('child','lib-films','test','child','local','analyze',4,5,6,0,0);
INSERT INTO automation_dependencies(task_id,prerequisite_id) VALUES('child','parent');
";

const SEEDED: &[(&str, &str, &str)] = &[
    ("users", "id,role,created_at,display_name", "id"),
    (
        "local_credentials",
        "user_id,login,password_hash",
        "user_id",
    ),
    (
        "sessions",
        "token_hash,user_id,created_at,last_seen_at,expires_at",
        "token_hash",
    ),
    ("libraries", "id,name,kind,node_id,root,enabled", "id"),
    (
        "library_permissions",
        "library_id,user_id",
        "library_id,user_id",
    ),
    ("assets", "sha256,size,mime,created_at", "sha256"),
    ("inventory_works", "id,library_id,identity_key,title", "id"),
    (
        "inventory_assets",
        "id,source_id,library_id,relative_path,size,available",
        "id",
    ),
    (
        "manual_timeline_markers",
        "asset_id,marker_type,start_seconds,end_seconds,enabled,updated_by,updated_at",
        "asset_id,marker_type",
    ),
    (
        "durable_tasks",
        "id,library_id,owner,idempotency_key,node_id,category,priority,attempt,created_at,updated_at,available_at",
        "id",
    ),
    (
        "automation_dependencies",
        "task_id,prerequisite_id",
        "task_id,prerequisite_id",
    ),
];

/// Tables the upgrade is allowed to add rows to (seeded singletons, converted legacy intervals).
/// Everything else must keep exactly its row count.
const SEEDED_BY_UPGRADE: &[&str] = &[
    "acquisition_settings",
    "statistics_settings",
    "statistics_wrapped_settings",
    "statistics_visibility",
    "system_task_schedules",
    "task_triggers",
    "task_trigger_migrations",
    "user_groups",
    "avatar_gallery_initialization",
    "avatar_builtin_seeds",
    "avatar_requirements",
    "avatar_policy",
    "avatar_packs",
    "avatar_images",
    "schema_migrations",
    "server_settings",
    "media_kinds",
    "role_group_members",
    "user_group_members",
    "user_group_libraries",
    "extension_update_revisions",
];

async fn seed(db: &mut Db) {
    db.exec(SEED).await;
}

async fn seeded_dump(db: &mut Db) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for (table, columns, order) in SEEDED {
        out.insert((*table).to_owned(), db.dump(table, columns, order).await);
    }
    out
}

/// Differences between two shapes, as readable lines.
fn diff(expected: &BTreeMap<String, String>, actual: &BTreeMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    for (key, value) in expected {
        match actual.get(key) {
            None => out.push(format!("missing after upgrade: {key} = {value}")),
            Some(found) if found != value => out.push(format!(
                "different: {key}\n    fresh:    {value}\n    upgraded: {found}"
            )),
            _ => {}
        }
    }
    for key in actual.keys().filter(|key| !expected.contains_key(*key)) {
        out.push(format!("extra after upgrade: {key}"));
    }
    out
}

/// Differences that are intentional and documented in docs/MIGRATIONS.md.
fn accepted(engine: Engine, line: &str) -> bool {
    let _ = engine;
    let _ = line;
    false
}

async fn legacy_upgrade_preserves_data_and_matches_fresh(engine: Engine) {
    let mut fresh = Db::open(engine).await;
    fresh.upgrade().await.unwrap();
    let fresh_shape = fresh.shape().await;
    fresh.teardown().await;

    let mut all_differences: Vec<String> = Vec::new();
    for revision in REVISIONS {
        let mut db = Db::open(engine).await;
        db.install_legacy(revision).await;
        seed(&mut db).await;
        db.integrity().await;
        let counts_before = db.row_counts().await;
        let data_before = seeded_dump(&mut db).await;
        let schema_before = db.shape().await;

        // The recovery copy: what an operator takes before starting the new Core.
        let backup = db.dir.path().join("before-upgrade.db");
        if engine == Engine::Sqlite {
            std::fs::copy(&db.sqlite_path, &backup).unwrap();
        }

        db.upgrade()
            .await
            .unwrap_or_else(|e| panic!("{revision}: {e}"));
        db.integrity().await;

        assert_eq!(
            seeded_dump(&mut db).await,
            data_before,
            "{revision}: seeded rows changed"
        );
        let counts_after = db.row_counts().await;
        for (table, before) in &counts_before {
            let after = counts_after
                .get(table)
                .unwrap_or_else(|| panic!("{revision}: {table} vanished"));
            if SEEDED_BY_UPGRADE.contains(&table.as_str()) {
                assert!(after >= before, "{revision}: {table} lost rows");
            } else {
                assert_eq!(after, before, "{revision}: {table} row count changed");
            }
        }

        let upgraded_shape = db.shape().await;
        for (key, value) in &schema_before {
            if key.starts_with("index ") || key.starts_with("trigger ") {
                assert!(
                    upgraded_shape.contains_key(key),
                    "{revision}: {key} = {value} was dropped"
                );
            }
        }
        let differences: Vec<String> = diff(&fresh_shape, &upgraded_shape)
            .into_iter()
            .filter(|line| !accepted(engine, line))
            .collect();
        all_differences.extend(
            differences
                .into_iter()
                .map(|line| format!("[{revision}] {line}")),
        );

        // A second and third start change nothing.
        let journal = db
            .dump(
                "schema_migrations",
                "name,kind,checksum,applied_at,updated_at",
                "name",
            )
            .await;
        let data = seeded_dump(&mut db).await;
        db.upgrade().await.unwrap();
        db.upgrade().await.unwrap();
        assert_eq!(
            db.shape().await,
            upgraded_shape,
            "{revision}: schema changed on restart"
        );
        assert_eq!(seeded_dump(&mut db).await, data);
        assert_eq!(
            db.dump(
                "schema_migrations",
                "name,kind,checksum,applied_at,updated_at",
                "name"
            )
            .await,
            journal
        );

        // PostgreSQL sizes are 64-bit after the upgrade.
        db.exec("INSERT INTO assets(sha256,size,mime,created_at) VALUES('bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',5000000000,'video/mp4',2)").await;

        if engine == Engine::Sqlite {
            // The pre-upgrade copy is untouched and can still be restored.
            let mut copy = Db::connect_sqlite(&backup).await;
            let tables: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='schema_migrations'",
            )
            .fetch_one(&mut copy)
            .await
            .unwrap();
            assert_eq!(tables, 0, "{revision}: recovery copy was modified");
            let checked: String = sqlx::query_scalar("PRAGMA integrity_check")
                .fetch_one(&mut copy)
                .await
                .unwrap();
            assert_eq!(checked, "ok");
            let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
                .fetch_one(&mut copy)
                .await
                .unwrap();
            assert_eq!(users, 2);
        }
        db.teardown().await;
    }
    assert!(
        all_differences.is_empty(),
        "{engine:?}: upgraded databases differ from a fresh install: {all_differences:#?}"
    );
}

#[tokio::test]
async fn sqlite_legacy_databases_upgrade_without_losing_data() {
    legacy_upgrade_preserves_data_and_matches_fresh(Engine::Sqlite).await;
}

#[tokio::test]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_legacy_databases_upgrade_without_losing_data() {
    legacy_upgrade_preserves_data_and_matches_fresh(Engine::Postgres).await;
}

/// Break a step half-way through the upgrade: everything must roll back, then succeed once repaired.
async fn failed_upgrade_rolls_back_and_retry_succeeds(engine: Engine) {
    let mut db = Db::open(engine).await;
    db.install_legacy("9f10420").await;
    seed(&mut db).await;
    // Same name as a later step's table but an incompatible shape: `CREATE TABLE IF NOT EXISTS`
    // skips it and the following INSERT fails after earlier steps already ran.
    db.exec("CREATE TABLE task_trigger_migrations(unrelated TEXT)")
        .await;
    db.exec("INSERT INTO task_trigger_migrations VALUES('sabotage')")
        .await;
    let shape = db.shape().await;
    let counts = db.row_counts().await;
    let data = seeded_dump(&mut db).await;

    assert!(
        db.upgrade().await.is_err(),
        "the sabotaged upgrade must fail"
    );

    assert_eq!(
        db.shape().await,
        shape,
        "failed upgrade left schema changes behind"
    );
    assert_eq!(db.row_counts().await, counts);
    assert_eq!(seeded_dump(&mut db).await, data);
    if engine == Engine::Sqlite {
        // Foreign-key enforcement was suspended for the upgrade and must be restored.
        assert_eq!(db.one_i64("PRAGMA foreign_keys").await, 1);
    }
    assert!(
        !db.tables().await.contains(&"schema_migrations".to_owned()),
        "the journal must roll back with the failed upgrade"
    );

    db.exec("DROP TABLE task_trigger_migrations").await;
    db.upgrade().await.unwrap();
    db.integrity().await;
    assert_eq!(seeded_dump(&mut db).await, data);
    db.teardown().await;
}

#[tokio::test]
async fn sqlite_failed_upgrade_rolls_back_and_retry_succeeds() {
    failed_upgrade_rolls_back_and_retry_succeeds(Engine::Sqlite).await;
}

#[tokio::test]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_failed_upgrade_rolls_back_and_retry_succeeds() {
    failed_upgrade_rolls_back_and_retry_succeeds(Engine::Postgres).await;
}

/// The journal records every step, survives restarts untouched, and follows edited idempotent files.
async fn journal_names_every_step_and_tracks_checksums(engine: Engine) {
    let mut db = Db::open(engine).await;
    db.upgrade().await.unwrap();
    let names = db
        .strings("SELECT name || ':' || kind FROM schema_migrations ORDER BY name")
        .await;
    assert!(names.contains(&"baseline:baseline".to_owned()), "{names:?}");
    assert!(
        names.contains(&"baseline_gaps:idempotent".to_owned()),
        "{names:?}"
    );
    assert!(
        names.contains(&"task_triggers:idempotent".to_owned()),
        "{names:?}"
    );
    assert!(
        names.contains(&"compat:task_constraints:compat".to_owned()),
        "{names:?}"
    );
    assert!(names.len() >= 21, "{names:?}");
    let short = db
        .one_i64("SELECT COUNT(*) FROM schema_migrations WHERE length(checksum)<>64")
        .await;
    assert_eq!(short, 0);

    // An idempotent file edited between releases gets its new checksum recorded, not rejected.
    db.exec(&format!(
        "UPDATE schema_migrations SET checksum='{}' WHERE name='task_triggers'",
        "0".repeat(64)
    ))
    .await;
    db.upgrade().await.unwrap();
    let restored = db
        .one_i64(&format!(
            "SELECT COUNT(*) FROM schema_migrations WHERE name='task_triggers' AND checksum<>'{}'",
            "0".repeat(64)
        ))
        .await;
    assert_eq!(restored, 1);
    db.teardown().await;
}

#[tokio::test]
async fn sqlite_journal_names_every_step_and_tracks_checksums() {
    journal_names_every_step_and_tracks_checksums(Engine::Sqlite).await;
}

#[tokio::test]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_journal_names_every_step_and_tracks_checksums() {
    journal_names_every_step_and_tracks_checksums(Engine::Postgres).await;
}

/// A fresh database restarted several times keeps its exact schema and rows.
async fn fresh_database_restarts_are_stable(engine: Engine) {
    let mut db = Db::open(engine).await;
    db.upgrade().await.unwrap();
    seed(&mut db).await;
    let shape = db.shape().await;
    let counts = db.row_counts().await;
    let data = seeded_dump(&mut db).await;
    for _ in 0..3 {
        db.upgrade().await.unwrap();
    }
    assert_eq!(db.shape().await, shape);
    assert_eq!(db.row_counts().await, counts);
    assert_eq!(seeded_dump(&mut db).await, data);
    db.integrity().await;
    db.teardown().await;
}

#[tokio::test]
async fn sqlite_fresh_database_restarts_are_stable() {
    fresh_database_restarts_are_stable(Engine::Sqlite).await;
}

#[tokio::test]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn postgres_fresh_database_restarts_are_stable() {
    fresh_database_restarts_are_stable(Engine::Postgres).await;
}

/// Differences that are intentional (see docs/MIGRATIONS.md): SQLite owns the full-text table and
/// every trigger; PostgreSQL has none. Anything else fails the test.
async fn names(db: &mut Db) -> std::collections::BTreeSet<String> {
    let shape = db.shape().await;
    // Views: PostgreSQL also reports their columns as table columns; implicit PRIMARY KEY/UNIQUE
    // indexes are named in `pg_constraint`, while SQLite hides them as `sqlite_autoindex_*`.
    let views: std::collections::BTreeSet<&str> = shape
        .keys()
        .filter_map(|k| k.strip_prefix("view "))
        .map(|v| v.split(" on ").next().unwrap())
        .collect();
    let constraints: std::collections::BTreeSet<&str> = shape
        .keys()
        .filter_map(|k| k.strip_prefix("constraint "))
        .filter_map(|k| k.split_once('.').map(|(_, n)| n))
        .collect();
    let mut out = std::collections::BTreeSet::new();
    for table in db.tables().await {
        out.insert(format!("table {table}"));
    }
    for key in shape.keys() {
        if let Some(rest) = key.strip_prefix("column ") {
            if !views.contains(rest.split('.').next().unwrap()) {
                out.insert(format!("column {rest}"));
            }
        } else if let Some(rest) = key.strip_prefix("index ") {
            if !constraints.contains(rest) {
                out.insert(format!("index {rest}"));
            }
        } else if let Some(rest) = key.strip_prefix("view ") {
            out.insert(format!("view {}", rest.split(" on ").next().unwrap()));
        } else if let Some(rest) = key.strip_prefix("trigger ") {
            out.insert(format!("trigger {}", rest.split(" on ").next().unwrap()));
        }
    }
    out
}

#[tokio::test]
#[ignore = "requires a temporary PostgreSQL 18 server and MEDIA_TEST_POSTGRES_URL"]
async fn sqlite_and_postgres_expose_the_same_tables_columns_and_indexes() {
    let mut sqlite = Db::open(Engine::Sqlite).await;
    sqlite.upgrade().await.unwrap();
    let mut postgres = Db::open(Engine::Postgres).await;
    postgres.upgrade().await.unwrap();
    let (a, b) = (names(&mut sqlite).await, names(&mut postgres).await);
    let mut lines: Vec<String> = a
        .difference(&b)
        .map(|n| format!("sqlite only: {n}"))
        .collect();
    lines.extend(b.difference(&a).map(|n| format!("postgres only: {n}")));
    // Triggers are summarised: SQLite owns all of them.
    let triggers = lines
        .iter()
        .filter(|l| l.starts_with("sqlite only: trigger "))
        .count();
    lines.retain(|l| !l.starts_with("sqlite only: trigger ") && !l.contains("inventory_works_fts"));
    assert!(lines.is_empty(), "engines diverge: {lines:#?}");
    eprintln!("sqlite-only triggers: {triggers}");
    assert!(
        triggers > 0 && !b.iter().any(|n| n.starts_with("trigger ")),
        "trigger inventory changed: update docs/MIGRATIONS.md"
    );
    sqlite.teardown().await;
    postgres.teardown().await;
}
