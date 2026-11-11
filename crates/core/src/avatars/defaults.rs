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

struct BuiltinAvatar {
    id: &'static str,
    name: &'static str,
    mime: &'static str,
    bytes: &'static [u8],
}

struct BuiltinPack {
    id: &'static str,
    name: &'static str,
    avatars: &'static [BuiltinAvatar],
}

static BUILTIN_PACKS: &[BuiltinPack] = &[
    BuiltinPack {
        id: "native-abysses",
        name: "Abysses",
        avatars: &[
            BuiltinAvatar {
                id: "native-abysses-axolotl",
                name: "Axolotl",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/abysses/Axolotl.png"),
            },
            BuiltinAvatar {
                id: "native-abysses-baleine",
                name: "Baleine",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/abysses/Baleine.png"),
            },
            BuiltinAvatar {
                id: "native-abysses-crabe-corail",
                name: "Crabe Corail",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/abysses/Crabe_Corail.png"),
            },
            BuiltinAvatar {
                id: "native-abysses-crevette-lumineuse",
                name: "Crevette Lumineuse",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/abysses/Crevette_Lumineuse.png"),
            },
            BuiltinAvatar {
                id: "native-abysses-meduse",
                name: "Méduse",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/abysses/Meduse.png"),
            },
            BuiltinAvatar {
                id: "native-abysses-pieuvre-perlee",
                name: "Pieuvre Perlée",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/abysses/Pieuvre_Perlee.png"),
            },
            BuiltinAvatar {
                id: "native-abysses-poisson-lanterne",
                name: "Poisson Lanterne",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/abysses/Poisson_Lanterne.png"),
            },
            BuiltinAvatar {
                id: "native-abysses-raie-manta",
                name: "Raie Manta",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/abysses/Raie_Manta.png"),
            },
        ],
    },
    BuiltinPack {
        id: "native-arcade",
        name: "Arcade",
        avatars: &[
            BuiltinAvatar {
                id: "native-arcade-blob-neon",
                name: "Blob Néon",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/arcade/Blob_Neon.png"),
            },
            BuiltinAvatar {
                id: "native-arcade-chat-arcade",
                name: "Chat Arcade",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/arcade/Chat_Arcade.png"),
            },
            BuiltinAvatar {
                id: "native-arcade-fantome-pixel",
                name: "Fantôme Pixel",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/arcade/Fantome_Pixel.png"),
            },
            BuiltinAvatar {
                id: "native-arcade-hibou-disquette",
                name: "Hibou Disquette",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/arcade/Hibou_Disquette.png"),
            },
            BuiltinAvatar {
                id: "native-arcade-koala-synth",
                name: "Koala Synth",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/arcade/Koala_Synth.png"),
            },
            BuiltinAvatar {
                id: "native-arcade-lapin-joystick",
                name: "Lapin Joystick",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/arcade/Lapin_Joystick.png"),
            },
            BuiltinAvatar {
                id: "native-arcade-manchot-console",
                name: "Manchot Console",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/arcade/Manchot_Console.png"),
            },
            BuiltinAvatar {
                id: "native-arcade-ours-cassette",
                name: "Ours Cassette",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/arcade/Ours_Cassette.png"),
            },
        ],
    },
    BuiltinPack {
        id: "native-botanica",
        name: "Botanica",
        avatars: &[
            BuiltinAvatar {
                id: "native-botanica-cactus",
                name: "Cactus",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/botanica/Cactus.png"),
            },
            BuiltinAvatar {
                id: "native-botanica-champignon",
                name: "Champignon",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/botanica/Champignon.png"),
            },
            BuiltinAvatar {
                id: "native-botanica-chat-tournesol",
                name: "Chat Tournesol",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/botanica/Chat_Tournesol.png"),
            },
            BuiltinAvatar {
                id: "native-botanica-chauve-souris-lierre",
                name: "Chauve-souris Lierre",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/botanica/Chauve_souris_Lierre.png"),
            },
            BuiltinAvatar {
                id: "native-botanica-hibou-bonsai",
                name: "Hibou Bonsaï",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/botanica/Hibou_Bonsai.png"),
            },
            BuiltinAvatar {
                id: "native-botanica-lapin-mousse",
                name: "Lapin Mousse",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/botanica/Lapin_Mousse.png"),
            },
            BuiltinAvatar {
                id: "native-botanica-phoque-lotus",
                name: "Phoque Lotus",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/botanica/Phoque_Lotus.png"),
            },
            BuiltinAvatar {
                id: "native-botanica-renard-fougere",
                name: "Renard Fougère",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/botanica/Renard_Fougere.png"),
            },
        ],
    },
    BuiltinPack {
        id: "native-constellations",
        name: "Constellations",
        avatars: &[
            BuiltinAvatar {
                id: "native-constellations-chat-etoile",
                name: "Chat Étoile",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/constellations/Chat_Etoile.png"),
            },
            BuiltinAvatar {
                id: "native-constellations-esprit-de-lune",
                name: "Esprit de Lune",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/constellations/Esprit_de_Lune.png"),
            },
            BuiltinAvatar {
                id: "native-constellations-hamster-nebuleuse",
                name: "Hamster Nébuleuse",
                mime: "image/png",
                bytes: include_bytes!(
                    "../../embedded_avatars/constellations/Hamster_Nebuleuse.png"
                ),
            },
            BuiltinAvatar {
                id: "native-constellations-hibou-eclipse",
                name: "Hibou Éclipse",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/constellations/Hibou_Eclipse.png"),
            },
            BuiltinAvatar {
                id: "native-constellations-lapin-planete",
                name: "Lapin Planète",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/constellations/Lapin_Planete.png"),
            },
            BuiltinAvatar {
                id: "native-constellations-loutre-satellite",
                name: "Loutre Satellite",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/constellations/Loutre_Satellite.png"),
            },
            BuiltinAvatar {
                id: "native-constellations-ours-aurore",
                name: "Ours Aurore",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/constellations/Ours_Aurore.png"),
            },
            BuiltinAvatar {
                id: "native-constellations-renard-comete",
                name: "Renard Comète",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/constellations/Renard_Comete.png"),
            },
        ],
    },
    BuiltinPack {
        id: "native-galaxie",
        name: "Galaxie",
        avatars: &[
            BuiltinAvatar {
                id: "native-galaxie-cosmo",
                name: "Cosmo",
                mime: "image/svg+xml",
                bytes: include_bytes!("../../embedded_avatars/galaxie/01-cosmo.svg"),
            },
            BuiltinAvatar {
                id: "native-galaxie-orbite",
                name: "Orbite",
                mime: "image/svg+xml",
                bytes: include_bytes!("../../embedded_avatars/galaxie/02-orbite.svg"),
            },
            BuiltinAvatar {
                id: "native-galaxie-nova",
                name: "Nova",
                mime: "image/svg+xml",
                bytes: include_bytes!("../../embedded_avatars/galaxie/03-nova.svg"),
            },
            BuiltinAvatar {
                id: "native-galaxie-saturne",
                name: "Saturne",
                mime: "image/svg+xml",
                bytes: include_bytes!("../../embedded_avatars/galaxie/04-saturne.svg"),
            },
            BuiltinAvatar {
                id: "native-galaxie-luna",
                name: "Luna",
                mime: "image/svg+xml",
                bytes: include_bytes!("../../embedded_avatars/galaxie/05-luna.svg"),
            },
            BuiltinAvatar {
                id: "native-galaxie-meteore",
                name: "Météore",
                mime: "image/svg+xml",
                bytes: include_bytes!("../../embedded_avatars/galaxie/06-meteore.svg"),
            },
            BuiltinAvatar {
                id: "native-galaxie-stella",
                name: "Stella",
                mime: "image/svg+xml",
                bytes: include_bytes!("../../embedded_avatars/galaxie/07-stella.svg"),
            },
            BuiltinAvatar {
                id: "native-galaxie-astro",
                name: "Astro",
                mime: "image/svg+xml",
                bytes: include_bytes!("../../embedded_avatars/galaxie/08-astro.svg"),
            },
            BuiltinAvatar {
                id: "native-galaxie-soleil",
                name: "Soleil",
                mime: "image/svg+xml",
                bytes: include_bytes!("../../embedded_avatars/galaxie/09-soleil.svg"),
            },
            BuiltinAvatar {
                id: "native-galaxie-ovni",
                name: "O.V.N.I",
                mime: "image/svg+xml",
                bytes: include_bytes!("../../embedded_avatars/galaxie/10-o-v-n-i.svg"),
            },
        ],
    },
    BuiltinPack {
        id: "native-mythes",
        name: "Mythes",
        avatars: &[
            BuiltinAvatar {
                id: "native-mythes-cyclope",
                name: "Cyclope",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/mythes/Cyclope.png"),
            },
            BuiltinAvatar {
                id: "native-mythes-dragon",
                name: "Dragon",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/mythes/Dragon.png"),
            },
            BuiltinAvatar {
                id: "native-mythes-griffon",
                name: "Griffon",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/mythes/Griffon.png"),
            },
            BuiltinAvatar {
                id: "native-mythes-kitsune",
                name: "Kitsune",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/mythes/Kitsune.png"),
            },
            BuiltinAvatar {
                id: "native-mythes-licorne-cristal",
                name: "Licorne Cristal",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/mythes/Licorne_Cristal.png"),
            },
            BuiltinAvatar {
                id: "native-mythes-phoenix",
                name: "Phénix",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/mythes/Phoenix.png"),
            },
            BuiltinAvatar {
                id: "native-mythes-slime-mage",
                name: "Slime Mage",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/mythes/Slime_Mage.png"),
            },
            BuiltinAvatar {
                id: "native-mythes-troll-foret",
                name: "Troll Forêt",
                mime: "image/png",
                bytes: include_bytes!("../../embedded_avatars/mythes/Troll_Foret.png"),
            },
        ],
    },
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

/// Assign on creation when enabled or required. Existing choices, including those whose
/// collection was subsequently removed, retain their immutable asset reference.
pub(crate) async fn assign_user(store: &IdentityStore, user: &str) -> Result<u64, AssetError> {
    let policy: Option<(i64, i64)> = crate::database::query_as("SELECT COALESCE((SELECT required FROM avatar_requirements WHERE singleton=1),1), COALESCE((SELECT assign_on_create FROM avatar_policy WHERE singleton=1),1)").fetch_optional(store.pool()).await?;
    if policy.is_some_and(|(required, assign)| required == 0 && assign == 0) {
        return Ok(0);
    }
    assign_user_unconditionally(store, user).await
}
async fn assign_user_unconditionally(store: &IdentityStore, user: &str) -> Result<u64, AssetError> {
    let existing: i64 = query_scalar("SELECT COUNT(*) FROM user_avatars WHERE user_id=?")
        .bind(user)
        .fetch_one(store.pool())
        .await?;
    if existing != 0 {
        return Ok(0);
    }
    let images: Vec<String> = query_scalar(
        "SELECT sha256 \
            FROM avatar_images \
            UNION ALL SELECT i.sha256 \
            FROM extension_avatar_images i \
            JOIN extension_avatar_packs p ON p.id=i.pack_id \
            JOIN extensions e ON e.id=p.extension_id \
            JOIN extension_publishers pub ON pub.id=e.publisher_id \
            WHERE e.enabled=1 AND pub.active=1 \
            LIMIT 16000",
    )
    .fetch_all(store.pool())
    .await?;
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

pub(crate) async fn ensure_required_missing(store: &IdentityStore) -> Result<u64, AssetError> {
    let required: Option<i64> =
        query_scalar("SELECT required FROM avatar_requirements WHERE singleton=1")
            .fetch_optional(store.pool())
            .await?;
    if required == Some(0) {
        return Ok(0);
    }
    assign_missing(store).await
}
pub(crate) async fn assign_missing(store: &IdentityStore) -> Result<u64, AssetError> {
    let users: Vec<String> = query_scalar("SELECT id FROM users WHERE NOT EXISTS(SELECT 1 FROM user_avatars a WHERE a.user_id=users.id)")
        .fetch_all(store.pool()).await?;
    let mut count = 0;
    for user in users {
        count += assign_user_unconditionally(store, &user).await?;
    }
    Ok(count)
}

/// Seed once, including on existing installations: the "Orbites" gallery on the first
/// start, and each built-in collection the first time a server version ships it.
/// Deleting or editing a collection never causes its images to reappear on the next
/// server restart.
pub(crate) async fn initialize(store: &IdentityStore) -> Result<(), AssetError> {
    let _guard = INITIALIZATION.lock().await;
    let initialized: i64 = query_scalar("SELECT COUNT(*) FROM avatar_gallery_initialization")
        .fetch_one(store.pool())
        .await?;
    let seeded: Vec<String> = query_scalar("SELECT pack_id FROM avatar_builtin_seeds")
        .fetch_all(store.pool())
        .await?;
    let pending: Vec<&BuiltinPack> = BUILTIN_PACKS
        .iter()
        .filter(|pack| !seeded.iter().any(|id| id == pack.id))
        .collect();
    if initialized != 0 && pending.is_empty() {
        return Ok(());
    }

    // Publish assets before taking the SQL write transaction; the asset store
    // writes its own metadata. Unreferenced bytes are recoverable by its GC.
    let mut images = Vec::with_capacity(PALETTE.len());
    if initialized == 0 {
        for (index, (name, background, foreground)) in PALETTE.iter().enumerate() {
            let svg = avatar_svg(index, background, foreground);
            let asset = store
                .assets()
                .put_bytes(svg.as_bytes(), "image/svg+xml")
                .await?;
            images.push((format!("native-orbits-{index}"), *name, asset.sha256));
        }
    }
    let mut pack_data = Vec::with_capacity(pending.len());
    for pack in pending {
        let mut pack_images = Vec::with_capacity(pack.avatars.len());
        for avatar in pack.avatars {
            let asset = store.assets().put_bytes(avatar.bytes, avatar.mime).await?;
            pack_images.push((avatar.id, avatar.name, asset.sha256));
        }
        pack_data.push((pack.id, pack.name, pack_images));
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
    for (pack_id, pack_name, pack_images) in pack_data {
        // Claimed in the same transaction: a concurrent start or a pack removed since
        // its first seeding is never filled again.
        let claimed = query(
            "INSERT INTO avatar_builtin_seeds(pack_id) VALUES(?) ON CONFLICT(pack_id) DO NOTHING",
        )
        .bind(pack_id)
        .execute(&mut *tx)
        .await?;
        if claimed.rows_affected() == 0 {
            continue;
        }
        query("INSERT INTO avatar_packs(id,name,source) VALUES(?,?,'builtin') ON CONFLICT(id) DO NOTHING")
            .bind(pack_id)
            .bind(pack_name)
            .execute(&mut *tx)
            .await?;
        for (id, name, sha256) in pack_images {
            query("INSERT INTO avatar_images(id,pack_id,name,sha256) VALUES(?,?,?,?) ON CONFLICT(id) DO NOTHING")
                .bind(id)
                .bind(pack_id)
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
        assert!(bytes.starts_with(b"<svg ") || bytes.starts_with(b"\x89PNG"));
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
        let total_builtin = 12 + BUILTIN_PACKS.iter().map(|p| p.avatars.len()).sum::<usize>();
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM avatar_images")
                .fetch_one(store.pool())
                .await
                .unwrap(),
            (total_builtin - 1) as i64
        );
        query("DELETE FROM avatar_packs")
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

    #[tokio::test]
    async fn existing_installations_receive_new_collections_once() {
        let root = tempfile::tempdir().unwrap();
        let store = IdentityStore::open(&root.path().join("existing.sqlite"))
            .await
            .unwrap();
        // A server seeded by an older version: the gallery is initialized, no built-in
        // collection was ever offered.
        query("INSERT INTO avatar_gallery_initialization(singleton) VALUES(1)")
            .execute(store.pool())
            .await
            .unwrap();
        initialize(&store).await.unwrap();
        async fn count(store: &IdentityStore, sql: &'static str) -> i64 {
            query_scalar::<_, i64>(sql)
                .fetch_one(store.pool())
                .await
                .unwrap()
        }
        assert_eq!(
            count(&store, "SELECT COUNT(*) FROM avatar_packs").await,
            BUILTIN_PACKS.len() as i64
        );
        assert_eq!(
            count(&store, "SELECT COUNT(*) FROM avatar_images").await,
            BUILTIN_PACKS.iter().map(|p| p.avatars.len()).sum::<usize>() as i64
        );
        // The older "Orbites" gallery is not brought back.
        assert_eq!(
            count(
                &store,
                "SELECT COUNT(*) FROM avatar_packs WHERE id='native-orbits'"
            )
            .await,
            0
        );
        // A collection the administrator removes stays removed.
        query("DELETE FROM avatar_packs WHERE id='native-mythes'")
            .execute(store.pool())
            .await
            .unwrap();
        initialize(&store).await.unwrap();
        assert_eq!(
            count(
                &store,
                "SELECT COUNT(*) FROM avatar_packs WHERE id='native-mythes'"
            )
            .await,
            0
        );
        assert_eq!(
            count(&store, "SELECT COUNT(*) FROM avatar_packs").await,
            BUILTIN_PACKS.len() as i64 - 1
        );
    }

    #[test]
    fn builtin_collections_hold_valid_images_with_unique_ids() {
        let mut ids = std::collections::HashSet::new();
        for pack in BUILTIN_PACKS {
            assert!(ids.insert(pack.id), "{}", pack.id);
            assert!(!pack.avatars.is_empty(), "{}", pack.id);
            for avatar in pack.avatars {
                assert!(ids.insert(avatar.id), "{}", avatar.id);
                match avatar.mime {
                    "image/png" => assert!(
                        avatar.bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
                        "{}",
                        avatar.id
                    ),
                    // Shipped with the server, so not held to the extension SVG contract
                    // (`validate_svg` refuses `title`/`desc` and ARIA attributes): it must
                    // still be a small, inert document.
                    "image/svg+xml" => {
                        let svg = std::str::from_utf8(avatar.bytes)
                            .expect("UTF-8 SVG")
                            .to_ascii_lowercase();
                        assert!(
                            svg.starts_with("<svg ") && svg.len() <= 32_768,
                            "{}",
                            avatar.id
                        );
                        for banned in ["<script", "href", "foreignobject", "url(", "<!entity"] {
                            assert!(!svg.contains(banned), "{}: {banned}", avatar.id);
                        }
                        let handler = svg.match_indices(" on").any(|(at, _)| {
                            let rest = &svg[at + 3..];
                            let name = rest.bytes().take_while(|b| b.is_ascii_alphabetic()).count();
                            name > 0 && rest[name..].trim_start().starts_with('=')
                        });
                        assert!(!handler, "{}: event handler", avatar.id);
                    }
                    other => panic!("{}: unexpected type {other}", avatar.id),
                }
            }
        }
    }
}
