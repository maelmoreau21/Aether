# Playlists, musique et installation des extensions

## Utilisation

La page **Playlists** rassemble films, séries et morceaux. Le propriétaire peut
renommer, ajouter, retirer et réordonner les éléments. Les films et séries ouvrent
leur parcours existant ; « Lire les titres musicaux » lance les morceaux dans le
lecteur global. L'enchaînement automatique d'une liste mêlant vidéo et musique
n'est pas fourni.

Les playlists sont privées par défaut. Dans **Administration → Général**,
l'administrateur peut autoriser le partage nominatif et la visibilité publique.
« Publique » signifie uniquement visible par les comptes connectés : aucun accès
anonyme. Le partage ne donne jamais accès à une bibliothèque interdite. Les
destinataires consultent sans modifier. Désactiver une politique retire l'accès
correspondant sans supprimer les listes du propriétaire.

Le lecteur musical reste actif pendant la navigation : file des titres suivants,
précédent/suivant, aléatoire, répétition, volume et commandes Media Session selon
le navigateur. La file en mémoire contient au maximum 500 titres ; elle est
effacée à la déconnexion et n'est pas une sauvegarde persistante. La lecture
depuis Musique utilise la sélection affichée (100 titres maximum par page).
La vidéo et la musique ne sont pas lancées simultanément intentionnellement.
Le décodage reste natif, sans transcodage serveur ni conversion des canaux.

Les paroles proviennent du scan : métadonnées intégrées reconnues, ou fichier
UTF-8 `.lrc` puis `.txt` portant le même nom que le morceau. Limite : 32 Kio.
Un nouveau scan actualise leur copie en base. Les paroles LRC sont synchronisées ;
le texte simple est affiché sans synchronisation. Aucun fournisseur de paroles
en ligne n'est ajouté ; l'absence de paroles est indiquée.

## Contrat HTTP

Toutes ces ressources exigent une session ; les mutations exigent également le
jeton CSRF. Les routes historiques `/api/v1/lists` restent compatibles.

| Route | Contrat |
| --- | --- |
| `GET /api/v1/lists` | Listes possédées ou autorisées, visibilité et propriété, maximum 500 |
| `GET /api/v1/lists/{id}` | `items` historiques et `entries` mixtes autorisées ; partages visibles au propriétaire |
| `GET /api/v1/playlists/policy` | Autorisation globale du partage et des listes publiques |
| `GET/PUT /api/v1/admin/playlists/policy` | Politique ; modification réservée à l'administrateur |
| `GET /api/v1/playlists/recipients?q=…` | Recherche de comptes actifs, minimum 2 caractères, maximum 20 résultats |
| `PUT /api/v1/playlists/{id}/settings` | `name`, `visibility` (`private/shared/public`), `userIds` (maximum 50) |
| `PUT/DELETE /api/v1/playlists/{id}/music/{track}` | Ajout/retrait propriétaire avec contrôle de bibliothèque |
| `PUT /api/v1/playlists/{id}/order` | Réordonnancement transactionnel, identifiants typés, doublons refusés |
| `GET /api/v1/music/tracks/{id}` | Métadonnées autorisées du morceau |
| `GET /api/v1/music/tracks/{id}/lyrics` | `text` et `source` (`library/unavailable`), mêmes autorisations |

Une playlist contient au maximum 1 000 éléments. Les autorisations des médias et
les restrictions du compte sont revérifiées lors de la consultation et du flux.
Les migrations 0039 existent pour SQLite et PostgreSQL ; elles conservent les
listes existantes en visibilité privée.

## Extensions

L'interface propose **Installées**, **Catalogue**, **Dépôts**. Ajouter l'URL HTTPS
du manifeste d'un dépôt, approuver explicitement son éditeur après vérification,
puis installer depuis Catalogue. Le nom du dépôt est facultatif. L'installation
reste désactivée jusqu'à configuration, attribution des permissions et activation.
Voir [le contrat des dépôts](EXTENSION_REPOSITORIES.md).

L'installation manuelle est réservée aux fichiers du serveur, sans formulaire
d'envoi de paquet dans l'interface. Configurer `MEDIA_EXTENSION_IMPORT_DIR` avec
un dossier administré localement, contenant les paquets signés `*.package.json`.
Les éditeurs doivent déjà être approuvés ou figurer explicitement dans
`trusted-publishers.json`, un tableau de `{ "id": "éditeur", "publicKey":
"clé publique Ed25519 base64" }`. Ne jamais y placer de clé privée.

L'import au démarrage vérifie signature, empreinte et ABI : maximum 32 paquets,
32 éditeurs et 400 000 octets par fichier. Il n'accorde aucune bibliothèque,
n'active rien et ne remplace ni une installation ni une clé existante. Un éditeur
révoqué n'est pas réapprouvé par réimport. Une configuration d'import invalide
fait échouer ce démarrage explicitement. Les fichiers restent en place : retirer
aussi le paquet du dossier pour éviter sa réinstallation après désinstallation.
Les DLL et paquets Jellyfin ne sont pas compatibles.

## Vérifications du 22 octobre 2026

- Vitest : 96 tests passés ; Svelte : zéro erreur et avertissement ; lint et build statique passés.
- `cargo clippy --locked --workspace --all-features --all-targets -j 1 -- -D warnings` : passé ; format appliqué.
- Tests produit existants : cinq passés ; test ciblé des playlists mixtes passé avec le parcours navigateur activé.
- `cargo test --locked -p media-poc --test extensions signed_file_import -- --test-threads=1` : import signé vérifié, un test passé.
- `cargo test --locked -p media-poc --lib product_tests::mixed_playlists -- --test-threads=1` avec `MEDIA_TEST_PLAYLIST_BROWSER=1` et `MEDIA_WEB_DIST` absolu : passé.
- Scan musical avec paroles et contrat Range : test ciblé passé. Workflow validé par actionlint.

Chrome réel 155.0.8059.85, Core/SQLite et comptes jetables : lecture Opus
synthétique, passage au titre suivant, paroles, navigation persistante, partage,
filtrage des bibliothèques, refus anonyme, révocation et interface mobile vérifiés.
Le test mesure aussi un signal PCM synthétique ; il ne prouve pas une sortie
audio physique multicanal. Rapport et captures :
[preuves locales](evidence/2026-10-22/playlists/local/report.json).
Le workflow CI reprend ce scénario mais aucune nouvelle CI distante n'a été
lancée ici. Aucun déploiement, push, test NAS, fournisseur distant, nouveau test
PostgreSQL réel ou validation Edge de ces nouveautés n'est revendiqué.
