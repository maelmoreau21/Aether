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

Le principe est celui d’un catalogue de paquets ; les manifestes et DLL Jellyfin
ne sont pas compatibles. Depuis la migration additive 0041 (SQLite/PostgreSQL),
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
