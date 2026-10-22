# Dépôts et catalogue d’extensions

Dans Administration → Extensions :

1. Ouvrir **Dépôts**, renseigner un nom et l’URL HTTPS du `manifest.json` fourni
   par l’auteur, puis **Ajouter le dépôt**.
2. Ouvrir **Catalogue**, choisir l’extension et sa version. La version numérique
   la plus récente est sélectionnée par défaut ; les versions incompatibles
   restent identifiées et leur installation est refusée côté serveur.
3. Pour un nouvel éditeur, comparer sa clé publique avec celle de l’auteur sur
   un canal de confiance, puis l’approuver explicitement. Le manifeste ne peut
   pas remplacer une clé déjà approuvée ou réactiver un éditeur révoqué.
4. Cliquer sur **Installer**. Le serveur télécharge le paquet, contrôle son
   empreinte, ses métadonnées et sa signature, puis le conserve désactivé.
5. Dans **Installées**, configurer les paramètres, accorder les bibliothèques et
   activer. Le runtime Wasm ne nécessite pas de redémarrage du serveur.

Le principe est celui d’un catalogue de paquets ; les manifestes et DLL Jellyfin
ne sont pas compatibles. Aucun dépôt officiel distant n’est inventé ni ajouté
par défaut : il faut une URL réellement hébergée par l’auteur. Le dépôt voisin
`Aether-Plugins` fournit un générateur, sans publier ni exposer de serveur.

**Actualiser ce dépôt** récupère explicitement un nouveau catalogue. L’accueil
ne télécharge aucun manifeste. Une erreur d’actualisation conserve le dernier
catalogue valide. **Retirer le dépôt** conserve les extensions installées ;
révoquer l’éditeur ou désactiver l’extension restent des actions distinctes.
Une installation utilise le snapshot sélectionné ; un changement de catalogue
pendant son téléchargement est signalé. Les mises à jour en place et le rollback
ne sont pas fournis : une version déjà installée se gère dans Installées, avec
désinstallation/réinstallation et de nouveaux consentements si nécessaire.

## Format du manifeste

```json
{
  "apiVersion": 1,
  "extensions": [
    {
      "manifest": { "apiVersion": 1, "id": "exemple", "name": "Exemple", "version": "1.2.0", "description": "Description", "author": "Auteur", "license": "MIT", "repository": "https://example.org/source", "permissions": ["metadata.read"] },
      "publisherId": "auteur",
      "publicKey": "CLE_PUBLIQUE_ED25519_BASE64",
      "packageUrl": "https://example.org/packages/exemple-1.2.0.package.json",
      "packageSha256": "EMPREINTE_SHA256_DES_OCTETS_DU_PAQUET_JSON"
    }
  ]
}
```

Une entrée correspond à une version, plusieurs entrées pouvant partager le même
identifiant d’extension. `manifest` doit correspondre exactement à celui du paquet
signé. `packageSha256` concerne le fichier JSON complet ; `sha256` dans le paquet
concerne son module Wasm. Versions : trois entiers numériques. `apiVersion:1`,
ABI `1.0` lorsqu’explicite, bornes `minServerVersion`/`maxServerVersion` inclusives.
Les suffixes de préversion du serveur ne changent pas ses trois nombres de base.

## Contrat HTTP

Session administrateur exigée partout ; CSRF sur chaque mutation. Réponses
`private, no-store`. Aucun chemin disque fourni par le navigateur.

- `GET /api/v1/admin/extensions/repositories` : `{repositories:[{id,name,url,refreshedAt}]}`.
- `POST` à cette même URL : `{name,url}`, `201 {id}` après récupération et validation.
- `POST /api/v1/admin/extensions/repositories/{id}/refresh` : `204` après remplacement du snapshot valide.
- `DELETE /api/v1/admin/extensions/repositories/{id}` : `204`, sans désinstaller de module.
- `GET /api/v1/admin/extensions/catalogue` : `{entries:[{repositoryId,repositoryName,manifest,publisherId,publicKey,compatible}]}`.
- `POST /api/v1/admin/extensions/repositories/{id}/install` : `{extensionId,version}`, `201 {id}` ; mêmes contrôles d’installation que l’import local.

Principales erreurs : `401/403` identité/CSRF/éditeur non approuvé, `400` URL,
manifeste, compatibilité ou intégrité invalides, `404` dépôt/version absent,
`409` doublon, limite ou catalogue changé, `429` téléchargements occupés,
`502` hôte/réponse indisponible, redirection ou réponse trop grande.

## Bornes et réseau

16 dépôts, 100 versions par dépôt, manifeste 256 Kio, paquet 400 000 octets,
deux téléchargements simultanés au maximum. Les téléchargements ont un délai
de dix secondes, et la résolution DNS un délai distinct de trois secondes.
HTTPS port 443 et noms DNS seulement ; identifiants URL, fragments, redirections
et proxy implicite désactivés. Toutes les adresses DNS doivent être publiques,
puis la connexion est épinglée sur ces adresses tout en validant le certificat
TLS du nom demandé. IP locales, privées, link-local, partagées, documentation,
multicast et variantes IPv6/mappées sont refusées.

Les réponses sont lues par fragments avec une borne cumulée, même sans
`Content-Length`. Le catalogue n’accorde aucun accès aux médias, au réseau du
worker ou à SQL. La signature Ed25519, l’approbation vivante de l’éditeur et les
consentements par bibliothèque restent ceux du runtime existant.

## Vérification

`cargo test --locked -p media-poc --lib extensions::repositories -j 1 -- --test-threads=1`
couvre les politiques URL/IP et le parcours HTTP de gestion, confiance,
empreinte altérée, installation désactivée, actualisation et retrait sans perte
de module. Le transport HTTPS de ce test est injecté uniquement sous `cfg(test)` ;
aucune exception réseau locale n’est compilée dans le serveur de production.
Les résultats effectivement exécutés sont consignés dans MEMORY.md par le jalon
principal ; les tests seuls ne prouvent pas un dépôt ou fournisseur public réel.

Après compilation du Web, un parcours Chrome dédié est disponible :

```powershell
cargo test --locked -p media-poc --lib browser_repository_catalogue_version_trust_and_install -j 1 -- --ignored --nocapture --test-threads=1
```

Ce test démarre un vrai Core avec SQLite temporaire et deux versions signées,
ouvre Dépôts/Catalogue/Installées, refuse une URL privée, vérifie le choix de
version et l’approbation explicite, installe puis actualise/retire le dépôt sans
perdre le module installé. Le transport des documents est injecté uniquement
dans le binaire de test ; le navigateur appelle réellement l’API. Captures et
rapport sont écrits sous `docs/evidence/2026-10-21/repositories/` par défaut.

Le nom du dépôt est désormais facultatif. Installation manuelle hors interface : [import signé depuis les fichiers](PLAYLISTS_AND_MUSIC.md#extensions).
