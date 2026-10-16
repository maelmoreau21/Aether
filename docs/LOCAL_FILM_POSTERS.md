# Affiches locales Films — P1

Le Node détecte les affiches sans écrire dans les bibliothèques ni appeler une API
externe. Le Core conserve les associations et contrôle les accès. Aucun changement
de lecteur, miniature, conversion ou illustration autre qu’une affiche.

## Sélection

Ordre des noms : `poster`, `folder`, `<base vidéo>-poster`, `<base vidéo>`.
Pour chaque nom : `.jpg`, `.jpeg`, `.png`, `.webp`. Casse selon le système de
fichiers. La première image présente est déterminante ; invalide, elle produit
un avertissement et un repli, sans basculer silencieusement vers une autre.
`poster`/`folder` ne sont partagés que dans un sous-dossier où toutes les vidéos
ont la même identité locale, sans conflit explicite avec le dossier. Aucun
partage à la racine de bibliothèque. Les noms exacts restent propres à la vidéo.
Au niveau Work, priorité puis chemin relatif départagent plusieurs candidats.
Aucun rapprochement entre bibliothèques ou à partir des titres NFO.

## Inventaire et API

Migration additive `0006_local_posters.sql` : table `inventory_images` distincte
des assets vidéo, UUID stable par Work/racine/chemin, taille, mtime, MIME, priorité
et disponibilité. Migrations0004/0005 et identifiants du graphe vidéo inchangés.
Un scan complet réconcilie ajout/modification/disparition ; il ne supprime jamais
une œuvre pour une affiche manquante. Les images inchangées sont réutilisées
après confinement et comparaison taille/mtime. Aucun ffprobe supplémentaire.

Le catalogue et la fiche exposent `posterUrl` ou null, jamais le chemin disque.
GET/HEAD `/api/v1/assets/{id}/image` : session et permission de bibliothèque
réévaluées avant toute réponse, y compris conditionnelle/Range. Bibliothèque
désactivée, grant ou session révoqué : accès refusé. UUID inconnu refusé sans
révéler son existence ; fichier disparu/invalide/modifié hors scan :404.
URL révisée par mtime/taille et `Cache-Control: private, no-store` ; MIME réel,
`X-Content-Type-Options: nosniff`, CSP inchangée. Les erreurs gardent les chemins
privés. Une rescan actualise les données, puis une nouvelle requête catalogue
ou un rechargement de page actualise l’image ; aucun push temps réel.

## Bornes et limites

JPEG/PNG/WebP uniquement, extension cohérente avec le format décodé, maximum
10Mio compressés et4096 pixels par dimension. Décodeur image0.25.10, budget
allocation32Mio de meilleur effort ; un seul décodeur bloquant simultané, permis
conservé jusqu’à sa fin même après annulation HTTP. Validation complète avant
service puis flux ServeFile64Kio ; pas de chargement intégral du film, aucune
conversion ni copie persistante des images. Le décodage de validation implique
un tampon compressé et une image décodée bornés ; ce n’est pas une garantie de
RSS globale. Les demandes attendent le permis : pas de quota agrégé ou de preuve
sous forte charge. GET et HEAD revalident le fichier, coût CPU assumé ici.

Liens symboliques et points de réanalyse/jonctions refusés à découverte et lecture.
Confinement, validation et ouverture du flux ne sont pas atomiques : TOCTOU
résiduel face à un écrivain OS hostile, ACL opérateur requises. Taille/mtime ne
sont pas un hash cryptographique. Une révocation bloque la prochaine requête,
pas un corps déjà ouvert ni les pixels déjà affichés par le navigateur.

## Web et preuves

Le composant Poster existant couvre Accueil, Films, recherche et fiches : ratio
2:3, object-fit cover, cartes lazy/decoding async, fiche eager, repli discret
si absent ou erreur. Les originaux restent en lecture seule. Les tests utilisent
des copies synthétiques jetables, avec mutations explicites des fixtures pour
simuler les changements de l’opérateur. Résultats exécutés : MEMORY.md et TEST_PLAN.md.

Fichiers principaux : `node/images.rs`, `node/scanner.rs`, `node.rs`,
`inventory.rs`, `permissions.rs`, `lib.rs`, `scans.rs`, migration0006,
`Cargo.toml`/`Cargo.lock`, `Poster.svelte`, `Catalog.svelte`, `catalog/api.ts`.
Tests : `posters_flow.rs`, `browser_posters.rs`, `browser-posters.cjs`, extensions
unitaires inventaire/scanner/Node ; preuves dans `evidence/2026-10-16/posters/`.
Architecture/API/sécurité/scanner/NFO/spécification/plan de tests et mémoire
signalent ce jalon ; les jalons historiques sont conservés.
