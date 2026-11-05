//! Original, self-contained vector avatars. No network, provider or extension is required.
use crate::{
    assets::AssetError,
    database::{query, query_scalar},
    identity::IdentityStore,
};
use sha2::{Digest, Sha256};

static INITIALIZATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const PACK: &str = "native-orbits";
pub(crate) const FALLBACK_PACK: &str = "native-orbits-fallback";
const PALETTE: [(&str, &str, &str); 12] = [
    ("Aurore", "#272E50", "#D5C8F8"),
    ("Comète", "#34324B", "#E8BCD1"),
    ("Orbite", "#163F45", "#A7DFD5"),
    ("Nova", "#4B3044", "#F3BFA7"),
    ("Éclipse", "#263B56", "#B3D4F5"),
    ("Halo", "#45402A", "#F0D98E"),
    ("Cosmos", "#34304D", "#C3B5EA"),
    ("Brume", "#304145", "#BFDBD8"),
    ("Solstice", "#4B3430", "#E8BE93"),
    ("Lune", "#343E50", "#CDD9EF"),
    ("Zénith", "#343E32", "#CADDA9"),
    ("Astral", "#44384D", "#DDBAE8"),
];

fn avatar_svg(index: usize, background: &str, foreground: &str) -> String {
    let crest = match index % 4 {
        0 => r#"<path d="M104 83 Q94 43 115 46 Q126 45 128 76 Q130 45 143 46 Q164 43 152 83"/>"#,
        1 => r#"<path d="M80 105 L65 63 L111 84 M145 84 L191 63 L176 105"/>"#,
        2 => r#"<path d="M98 85 Q82 55 104 53 Q120 51 128 80 Q138 52 153 56 Q174 64 158 89"/>"#,
        _ => r#"<ellipse cx="128" cy="64" rx="39" ry="12" fill="none" stroke-width="7"/>"#,
    };
    let mouth = if index.is_multiple_of(3) {
        r##"<path d="M112 149 Q128 166 144 149" fill="none" stroke="#242736" stroke-width="6" stroke-linecap="round"/>"##
    } else {
        r##"<path d="M119 153 Q128 158 137 153" fill="none" stroke="#242736" stroke-width="6" stroke-linecap="round"/>"##
    };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256"><rect width="256" height="256" rx="40" fill="{background}"/><circle cx="207" cy="45" r="3" fill="{foreground}" opacity="0.5"/><circle cx="43" cy="177" r="4" fill="{foreground}" opacity="0.35"/><path d="M41 65 L41 77 M35 71 L47 71" stroke="{foreground}" stroke-width="3" stroke-linecap="round" opacity="0.45"/><g fill="{foreground}" stroke="{foreground}">{crest}<path d="M72 254 Q72 190 128 187 Q184 190 184 254" opacity="0.55"/><rect x="64" y="79" width="128" height="112" rx="48"/></g><ellipse cx="100" cy="128" rx="7" ry="10" fill="#242736"/><ellipse cx="156" cy="128" rx="7" ry="10" fill="#242736"/><circle cx="84" cy="148" r="9" fill="{background}" opacity="0.17"/><circle cx="172" cy="148" r="9" fill="{background}" opacity="0.17"/>{mouth}</svg>"##
    )
}

fn user_index(user: &str, count: usize) -> usize {
    let digest = Sha256::digest(user.as_bytes());
    digest
        .iter()
        .fold(0, |value, byte| (value * 256 + usize::from(*byte)) % count)
}

pub(crate) fn fallback_for_user(user: &str) -> Vec<u8> {
    let index = user_index(user, PALETTE.len());
    let (_, background, foreground) = PALETTE[index];
    avatar_svg(index, background, foreground).into_bytes()
}

pub(crate) fn fallback_image(id: &str) -> Option<Vec<u8>> {
    let index = id.parse::<usize>().ok()?;
    let (_, background, foreground) = PALETTE.get(index)?;
    Some(avatar_svg(index, background, foreground).into_bytes())
}

pub(crate) fn fallback_images() -> Vec<serde_json::Value> {
    PALETTE
        .iter()
        .enumerate()
        .map(|(index, (name, _, _))| {
            serde_json::json!({
                "id":index.to_string(), "name":name,
                "url":format!("/api/v1/account/avatar/collections/{FALLBACK_PACK}/{index}")
            })
        })
        .collect()
}

/// Gallery photos are mandatory. Existing choices, including those whose
/// collection was subsequently removed, retain their immutable asset reference.
pub(crate) async fn assign_user(store: &IdentityStore, user: &str) -> Result<u64, AssetError> {
    let existing: i64 = query_scalar("SELECT COUNT(*) FROM user_avatars WHERE user_id=?")
        .bind(user)
        .fetch_one(store.pool())
        .await?;
    if existing != 0 {
        return Ok(0);
    }
    let images: Vec<String> = query_scalar("SELECT sha256 FROM avatar_images UNION ALL SELECT i.sha256 FROM extension_avatar_images i JOIN extension_avatar_packs p ON p.id=i.pack_id JOIN extensions e ON e.id=p.extension_id JOIN extension_publishers pub ON pub.id=e.publisher_id WHERE e.enabled=1 AND pub.active=1 LIMIT 16000")
        .fetch_all(store.pool()).await?;
    let sha256 = if images.is_empty() {
        store
            .assets()
            .put_bytes(&fallback_for_user(user), "image/svg+xml")
            .await?
            .sha256
    } else {
        images[user_index(user, images.len())].clone()
    };
    Ok(query("INSERT INTO user_avatars(user_id,sha256) SELECT id,? FROM users WHERE id=? ON CONFLICT(user_id) DO NOTHING")
        .bind(sha256).bind(user).execute(store.pool()).await?.rows_affected())
}

pub(crate) async fn assign_missing(store: &IdentityStore) -> Result<u64, AssetError> {
    let users: Vec<String> = query_scalar("SELECT id FROM users WHERE NOT EXISTS(SELECT 1 FROM user_avatars a WHERE a.user_id=users.id)")
        .fetch_all(store.pool()).await?;
    let mut count = 0;
    for user in users {
        count += assign_user(store, &user).await?;
    }
    Ok(count)
}

/// Seed once, including on existing installations. Deleting or editing the
/// collection never causes its images to reappear on the next server restart.
pub(crate) async fn initialize(store: &IdentityStore) -> Result<(), AssetError> {
    let _guard = INITIALIZATION.lock().await;
    let initialized: i64 = query_scalar("SELECT COUNT(*) FROM avatar_gallery_initialization")
        .fetch_one(store.pool())
        .await?;
    if initialized != 0 {
        return Ok(());
    }

    // Publish assets before taking the SQL write transaction; the asset store
    // writes its own metadata. Unreferenced bytes are recoverable by its GC.
    let mut images = Vec::with_capacity(PALETTE.len());
    for (index, (name, background, foreground)) in PALETTE.iter().enumerate() {
        let svg = avatar_svg(index, background, foreground);
        let asset = store
            .assets()
            .put_bytes(svg.as_bytes(), "image/svg+xml")
            .await?;
        images.push((format!("native-orbits-{index}"), *name, asset.sha256));
    }
    let mut tx = store.pool().begin().await?;
    let marker = query("INSERT INTO avatar_gallery_initialization(singleton) VALUES(1) ON CONFLICT(singleton) DO NOTHING")
        .execute(&mut *tx).await?;
    if marker.rows_affected() != 0 {
        query("INSERT INTO avatar_packs(id,name,source) VALUES(?,'Orbites','builtin')")
            .bind(PACK)
            .execute(&mut *tx)
            .await?;
        for (id, name, sha256) in images {
            query("INSERT INTO avatar_images(id,pack_id,name,sha256) VALUES(?,?,?,?)")
                .bind(id)
                .bind(PACK)
                .bind(name)
                .bind(sha256)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mandatory_avatars_cover_bootstrap_existing_and_empty_gallery_without_overwriting() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("mandatory.sqlite");
        let store = IdentityStore::open(&path).await.unwrap();
        let admin = store
            .bootstrap(
                "admin",
                zeroize::Zeroizing::new("temporary mandatory avatar password".into()),
            )
            .await
            .unwrap();
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM user_avatars WHERE user_id=?")
                .bind(&admin)
                .fetch_one(store.pool())
                .await
                .unwrap(),
            1
        );
        initialize(&store).await.unwrap();
        query("INSERT INTO users(id,role) VALUES('existing','user')")
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(assign_missing(&store).await.unwrap(), 1);
        let chosen: String =
            query_scalar("SELECT sha256 FROM user_avatars WHERE user_id='existing'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        // Deleting all gallery images retains account-owned copies, and new
        // accounts receive an original local fallback even without any packs.
        query("DELETE FROM avatar_packs")
            .execute(store.pool())
            .await
            .unwrap();
        query("INSERT INTO users(id,role) VALUES('after-empty','user')")
            .execute(store.pool())
            .await
            .unwrap();
        assert_eq!(assign_user(&store, "after-empty").await.unwrap(), 1);
        assert_eq!(assign_missing(&store).await.unwrap(), 0);
        store.close().await;
        let reopened = IdentityStore::open(&path).await.unwrap();
        let _auth = crate::auth::AuthState::new(reopened.clone(), "http://localhost".into())
            .await
            .unwrap();
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM avatar_packs")
                .fetch_one(reopened.pool())
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM user_avatars")
                .fetch_one(reopened.pool())
                .await
                .unwrap(),
            3
        );
        let after_restart: String =
            query_scalar("SELECT sha256 FROM user_avatars WHERE user_id='existing'")
                .fetch_one(reopened.pool())
                .await
                .unwrap();
        assert_eq!(after_restart, chosen);
        let (_, bytes) = reopened.assets().read_all(&after_restart).await.unwrap();
        assert!(bytes.starts_with(b"<svg "));
    }

    #[tokio::test]
    async fn default_gallery_is_local_and_deleted_images_stay_deleted() {
        let root = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&root.path().join("avatars.sqlite"))
            .await
            .unwrap();
        initialize(&store).await.unwrap();
        for (index, (_, background, foreground)) in PALETTE.iter().enumerate() {
            crate::avatars::validate_svg(&avatar_svg(index, background, foreground)).unwrap();
        }
        let images: Vec<(String, String)> =
            crate::database::query_as("SELECT id,sha256 FROM avatar_images WHERE pack_id=?")
                .bind(PACK)
                .fetch_all(store.pool())
                .await
                .unwrap();
        assert_eq!(images.len(), 12);
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM extensions")
                .fetch_one(store.pool())
                .await
                .unwrap(),
            0
        );
        for (_, sha256) in &images {
            let (_, bytes) = store.assets().read_all(sha256).await.unwrap();
            assert!(bytes.starts_with(b"<svg "));
        }
        query("DELETE FROM avatar_images WHERE id=?")
            .bind(&images[0].0)
            .execute(store.pool())
            .await
            .unwrap();
        initialize(&store).await.unwrap();
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM avatar_images")
                .fetch_one(store.pool())
                .await
                .unwrap(),
            11
        );
        query("DELETE FROM avatar_packs WHERE id=?")
            .bind(PACK)
            .execute(store.pool())
            .await
            .unwrap();
        initialize(&store).await.unwrap();
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM avatar_packs")
                .fetch_one(store.pool())
                .await
                .unwrap(),
            0
        );
    }
}
