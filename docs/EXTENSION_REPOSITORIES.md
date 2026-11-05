## Correctifs du 25 octobre 2026

Les bannières du catalogue et des extensions installées sont affichées en entier
(`object-fit: contain`), sans découper les logos des visuels panoramiques.

Le stockage compare les paquets JSON après désérialisation : une différence
indolore de mise en forme ou de champs optionnels omis ne bloque plus une
réinstallation identique. Un paquet différent et un module altéré restent refusés.
Les réponses distinguent `plugin_storage_permission_denied`,
`plugin_storage_conflict`, `plugin_storage_integrity_failed` et
`plugin_storage_missing`. Les erreurs non classées conservent
`plugin_storage_failed`. Le Core journalise la cause système ; le navigateur
ne reçoit aucun chemin interne. Aucune réparation destructive automatique.

IntroSkipper existe dans Aether-Plugins mais le catalogue signé local contient
encore huit paquets sans IntroSkipper. La détection de release inclut désormais
les plugins absents du catalogue sur les exécutions automatiques. Les publications
sont séquentielles et récupèrent main pour éviter les conflits entre leurs commits
de catalogue. Les SVG locaux sont incorporés avant signature. La signature avec
la clé de production et la publication restent nécessaires avant actualisation
du catalogue sur un serveur ; aucun paquet signé fictif n'est ajouté.

# Dépôts et catalogue d’extensions

> Mise à jour du 23 octobre : aucune approbation d’éditeur ou de dépôt n’est requise.
> Les signatures restent vérifiées avec la clé du catalogue. Configuration dédiée,
> Toutes les bibliothèques et import d’avatars : [contrat actuel](PLUGIN_SETTINGS_AND_COLLECTIONS.md).
> Les étapes de confiance explicite historiques ci-dessous sont remplacées par ce contrat.

## Interface des extensions — 23 octobre 2026

Le catalogue et les extensions installées utilisent des cartes avec un visuel
local déterministe, un nom, une description courte et un état. Les visuels sont
composés en CSS/SVG, sans téléchargement d’image tiers. Les catégories viennent
du manifeste, avec un repli basé sur les permissions ; recherche et filtres
s’appliquent ensemble. Le bouton **Voir les détails** ouvre la fiche avec la
version sélectionnable, la compatibilité, le développeur, le dépôt et les accès
demandés. Les paramètres des extensions installées restent séparés de leur
installation ; activation, bibliothèques, réglages et désinstallation conservent
les contrats existants. Aucun historique de versions installées n’est inventé.

**Dépôts** présente une liste compacte ; **+ Nouveau dépôt** ouvre le formulaire.
Les thèmes clair/sombre, le clavier et les petits écrans utilisent les tokens
existants. Le test navigateur `test-extensions-live.mjs` couvre la découverte sur
catalogue synthétique (catégorie, recherche vide, fiche et formulaire), puis le
cycle réel sur SQLite et Wasm signé (paramètres, activation, exécution,
désactivation, désinstallation). Les captures du catalogue contiennent des
entrées de démonstration injectées uniquement dans le test, pas dans le produit.
Aucun dépôt distant ni déploiement NAS n’est validé par ce test.

Dans Administration → Extensions :

1. Ouvrir **Dépôts**, renseigner un nom et l’URL HTTPS du `manifest.json` fourni
   par l’auteur, puis **Ajouter le dépôt**.
2. Ouvrir **Catalogue**, choisir l’extension et sa version. La version numérique
   la plus récente est sélectionnée par défaut ; les versions incompatibles
   restent identifiées et leur installation est refusée côté serveur.
3. Aucun clic d’approbation de dépôt ou d’éditeur n’est nécessaire.
4. Cliquer sur **Installer**. Le serveur télécharge le paquet, contrôle son
   empreinte, ses métadonnées et sa signature, puis le conserve désactivé.
5. Dans **Installées**, configurer les paramètres et activer (Toutes les bibliothèques par
   défaut pour un fournisseur de métadonnées). Le runtime Wasm ne nécessite pas de redémarrage du serveur.

Le principe est celui d’un dépôt de plugins Jellyfin (un manifeste listant les
versions de chaque plugin), mais les DLL Jellyfin ne sont pas compatibles : seuls
des paquets Wasm signés pour Aether s’installent. Le catalogue officiel est sur
`main` pour toutes les éditions du Core (les URL de paquets sont des releases
absolues) ; une entrée préconfigurée qui pointait vers `develop` est ramenée sur
`main` au démarrage. Depuis la migration additive 0041 (SQLite/PostgreSQL),
le dépôt `Aether Plugins` est préconfiguré une seule fois avec :

```text
https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json
```

La migration ne télécharge rien : le catalogue initial est vide jusqu’à
**Actualiser les dépôts**. Elle n’installe aucun module. L’installation ne demande plus d’approbation.
La suppression de ce dépôt est durable, y compris après réouverture de la base.
Si cette URL avait déjà été ajoutée par l’administrateur, sa configuration est
conservée. Le lien ne sera publiquement utilisable qu’après publication des
fichiers `repository/` préparés dans le dépôt voisin `Aether-Plugins` sur `main`.
Les quatre paquets signés sont préparés et vérifiés localement ; aucun push ou
hébergement public n’est effectué par cette intervention.

**Actualiser ce dépôt** récupère explicitement un nouveau catalogue. L’accueil
ne télécharge aucun manifeste. Une erreur d’actualisation conserve le dernier
catalogue valide. **Retirer le dépôt** conserve les extensions installées ;
révoquer l’éditeur ou désactiver l’extension restent des actions distinctes.
Une installation utilise le snapshot sélectionné ; un changement de catalogue
pendant son téléchargement est signalé. Une route d'upgrade existe côté Core,
mais le cycle complet depuis le catalogue et le rollback ne sont pas garantis.
Voir [Cycle de mise à jour](PLUGIN_UPDATE_LIFECYCLE.md) ; une
désinstallation/réinstallation exige une sauvegarde préalable des données.

## Format du manifeste versionné (`apiVersion: 2`, recommandé)

Même principe qu'un dépôt de plugins Jellyfin : chaque extension est listée une
fois avec **toutes** ses versions, ce qui rend les versions précédentes
installables. Une liste JSON nue de plugins (forme Jellyfin) est aussi acceptée.

```json
{
  "apiVersion": 2,
  "plugins": [
    {
      "id": "exemple",
      "name": "Exemple",
      "description": "Description courte",
      "overview": "Texte long facultatif",
      "owner": "Auteur",
      "category": "Metadata",
      "imageUrl": "https://raw.githubusercontent.com/auteur/depot/main/logo.png",
      "publisherId": "auteur",
      "publicKey": "CLE_PUBLIQUE_ED25519_BASE64",
      "versions": [
        {
          "version": "1.2.0",
          "targetAbi": "1.0",
          "minServerVersion": "0.1.0",
          "sourceUrl": "https://github.com/auteur/depot/releases/download/exemple-v1.2.0/exemple-1.2.0.zip",
          "checksum": "SHA256_HEX_DU_FICHIER_SOURCEURL",
          "timestamp": "2026-11-02T09:44:30Z",
          "changelog": "- Correctif",
          "permissions": ["metadata.read"],
          "httpHosts": ["api.example.org"]
        }
      ]
    }
  ]
}
```

- `id` accepte l'alias `guid` mais doit rester un identifiant Aether.
- `sourceUrl` désigne le paquet signé `.package.json` ou un `.zip` qui en contient
  exactement un (stocké ou deflate ; chiffrement et ZIP64 refusés).
- `checksum` est le SHA-256 du fichier téléchargé (préfixe `sha256:` toléré ;
  `checksumAlgorithm`, s'il est présent, doit valoir `sha256` : le MD5 Jellyfin est refusé).
- `targetAbi` est l'ABI minimale : même majeure que l'ABI du Core (`1.0`) et pas
  plus récente, comparée numériquement (`1`, `1.0`, `1.0.0.0`).
- `permissions`/`httpHosts` (facultatifs) sont affichés avant installation ; s'ils
  sont présents, ils doivent correspondre exactement au manifeste signé.
- `publisherId`/`publicKey` peuvent être redéfinis dans une version (rotation de clé).
- À l'installation, le paquet doit porter l'`id`, la `version` et le `publisherId`
  de l'entrée ; sa signature Ed25519 reste la seule source de confiance.
- Les champs inconnus sont ignorés.

`GET /api/v1/admin/extensions/catalogue` renvoie une entrée par version, avec
`version`, `timestamp`, `changelog`, `owner`, `overview`, `sourceUrl`, `hasImage`.
L'image (`imageUrl`) est servie par
`GET /api/v1/admin/extensions/repositories/{id}/image/{extensionId}` sous forme
`{dataUrl}` (PNG, JPEG, GIF, WebP ou SVG, 512 Kio, cache d'une heure) : l'interface
n'affiche que des images locales ou `data:`.

### Installer une version précédente

Dans la fiche d'une extension du catalogue, chaque version a un bouton **Installer
cette version**. Si une version plus récente est installée, l'interface utilise
`confirmDowngrade` avec `expectedVersion`/`expectedSha256` : l'extension est
désactivée et la version courante devient la sauvegarde restaurable
(**Restaurer**). Le retour arrière reste refusé si l'une des deux versions déclare
`storage.private`, si l'extension détient des données privées, si l'ancienne
version embarque une migration de stockage, ou si les paramètres enregistrés ne
conviennent pas à l'ancien schéma (type modifié, champ obligatoire sans défaut).
Une sauvegarde précédente sans données privées est remplacée au lieu de bloquer.

## Format historique (`apiVersion: 1`)

Toujours accepté :

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
- `GET /api/v1/admin/extensions/catalogue` : `{entries:[{repositoryId,repositoryName,manifest,publisherId,publicKey,compatible,version,timestamp,changelog,owner,overview,sourceUrl,hasImage,imageUrl}]}`, une entrée par version.
- `GET /api/v1/admin/extensions/repositories/{id}/image/{extensionId}` : `{dataUrl}`.
- `POST /api/v1/admin/extensions/repositories/{id}/install` : `{extensionId,version,upgrade?:boolean}`, `201 {id}` pour une installation. Avec `upgrade:true`, mêmes contrôles que `/extensions/{id}/upgrade` ; la compatibilité actuellement non garantie entraîne un refus sans modifier l'installation. L'interface ne désinstalle plus l'ancienne version pour contourner ce refus.

Principales erreurs : `401/403` identité/CSRF/éditeur non approuvé, `400` URL,
manifeste, compatibilité ou intégrité invalides, `404` dépôt/version absent,
`409` doublon, limite ou catalogue changé, `429` téléchargements occupés,
`502` hôte/réponse indisponible, redirection ou réponse trop grande.

## Bornes et réseau

16 dépôts, manifeste 1 Mio ; format 1 : 100 entrées ; format 2 : 200 extensions
et 1 000 versions. Fichier téléchargé 8 Mio, paquet JSON extrait 2 Mio, deux
téléchargements simultanés au maximum. Délai de dix secondes pour un manifeste,
soixante pour un paquet, trois secondes pour la résolution DNS. User-Agent
`Aether-Extensions/<version>`.
HTTPS port 443 et noms DNS seulement ; identifiants URL, fragments et proxy
implicite refusés. Les redirections sont suivies à la main, trois au plus : vers
le même hôte, ou de `github.com` vers `objects.githubusercontent.com`,
`release-assets.githubusercontent.com` et `codeload.github.com` (assets de
releases GitHub). Chaque saut repasse les contrôles d'URL, de DNS et d'adresse
publique. Toutes les adresses DNS doivent être publiques,
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
Les résultats effectivement exécutés sont consignés dans .ai/MEMORY.md par le jalon
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


## Interface du 23 octobre 2026

Catalogue en cartes compactes, recherche, choix de version, accès demandés et
éditeur dépliables. L’installation vérifie les signatures sans approbation manuelle ; elle
n’active pas le module. Installées présente le statut et ouvre une seule
configuration complète à la fois. Paramètres, bibliothèques, activation,
désactivation et désinstallation restent indépendants. Les exécutions et le
journal sont regroupés dans des volets fermés. Les clés techniques et empreintes
restent consultables dans les détails. La navigation initiale ne double plus
les sept lectures d’administration.

Les tests Core/SQLite/Chrome réels couvrent l’ajout, le choix d’une ancienne
version, la confiance, la signature, l’installation désactivée, la suppression
du dépôt préconfiguré, sa persistance et la conservation du module. Les captures
avec les quatre manifestes réels utilisent une UI de production et des API
simulées ; elles ne prouvent pas le téléchargement GitHub. Le contrôle PostgreSQL
est limité à la migration fournie, sans exécution PostgreSQL dans cette demande.

Les preuves et les commandes du complément sont regroupées dans
[STARTUP_AND_PLUGIN_CATALOGUE.md](STARTUP_AND_PLUGIN_CATALOGUE.md).
