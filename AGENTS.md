# AGENTS.md — Consignes obligatoires pour les agents IA

> Projet : **Aether** (nom provisoire). **État initial : conception, pas de code fonctionnel présumé.**
> Ce fichier vaut pour tout le dépôt. La réalité du code et des tests prime sur les annonces.

## 1. Mission et ordre des priorités

Développer une plateforme multimédia **100 % auto-hébergeable et open source**, inspirée de l'ergonomie Netflix, sans dépendre de Netflix, et pouvant coexister sans danger avec Jellyfin.

Ordre **contraignant** :
1. **P0 — Valider AV1 + Opus dans de vrais `.mkv` et `.mp4`.** Scanner, identifier les pistes, lire ou expliquer précisément l'échec sur un navigateur ciblé. Tester seek, pauses, erreurs et HTTP Range. La prise en charge du codec **dans le fichier** n'implique pas la lecture directe **par le navigateur**.
2. **P1 — Mode autonome utilisable**, avec une interface Web, un Core, un Media Node local, SQLite, bibliothèques `ro` et premier administrateur local sécurisé.
3. **P2 — Plusieurs Media Nodes + OIDC** : catalogue commun, appairage sûr, deux machines, SSO Authentik, utilisateurs autorisés et streaming sécurisé.
4. **P3 — Pistes externes et futur format découplé** : plusieurs vidéos/audio/sous-titres, y compris **fichiers sans multiplexage**, puis **flux élémentaires véritablement sans conteneur**, avec index/horodatage explicites, après POC concluant.
5. **P4 — Expérience films/séries** : recherche, profils facultatifs, saisons/épisodes, progression, métadonnées et ergonomie.
6. **P5 — Robustesse et mesure** : sauvegarde/restauration, scans incrémentaux, télémétrie et tests de montée en charge. **Cluster multi-Core hors MVP**.

Ne jamais sacrifier P0 pour une interface spectaculaire, un cluster, un système de plugins ou des optimisations hypothétiques.

## 2. Lecture obligatoire avant chaque intervention

1. Lire `AGENTS.md`, `MEMORY.md`, `ARCHITECTURE.md`, `docs/SPECIFICATION.md`, puis la documentation du domaine concerné.
2. Inspecter les fichiers réellement présents, `git status`, le diff, les tests et dépendances. **Les documents sont des exigences, pas des preuves d'implémentation.**
3. Découper la mission en une étape vérifiable, annoncer les hypothèses, les critères d'acceptation et le périmètre.
4. Implémenter un **parcours vertical fonctionnel** avant de multiplier les couches ou crates vides.
5. Exécuter les tests adaptés ; signaler honnêtement les vérifications impossibles, ainsi que les limites navigateur/client.
6. Terminer par les changements, commandes de tests réellement lancées, résultats, risques restants et prochain petit jalon.
7. Mettre à jour `MEMORY.md` après chaque jalon réel ou changement de décision, sans cocher des fonctions sur la seule base d'un plan.
8. Ne pas détruire des données, changer de licence, pousser sur Git, publier d'images ou modifier des médias sans demande explicite.

## 3. Règles produit non négociables

- **Rôles autorisés exactement : `administrator` et `user`. Aucun rôle `guest`, invité ou administrateur implicite.** Un profil n'est pas un rôle ; un compte peut éventuellement avoir plusieurs profils à l'avenir.
- Pas de compte public auto-inscrit par défaut, pas de mot de passe de démonstration, pas de promotion automatique en administrateur via une simple déclaration OIDC.
- Premier lancement : **création sécurisée d'un compte administrateur local**, sans identifiants par défaut. OIDC activable ensuite ; accès local de récupération protégé et configurable.
- **Mode `standalone` d'abord** : ne nécessite ni PostgreSQL, ni Valkey, ni un service Node.js permanent, ni une grappe, ni un proxy HTTPS externe pour l'usage **local** de base. Documenter clairement les restrictions de cookies et de SSO en HTTP local.
- **Interface, Core et Media Node restent séparés conceptuellement**, même en un seul processus ; une même API versionnée fonctionne en mode autonome et distribué.
- Bibliothèques en **lecture seule (`:ro`)** par défaut. Les caches, miniatures et données applicatives sont dans des volumes séparés. Aucune modification des fichiers Jellyfin.
- **Format prioritaire AV1 vidéo + Opus audio en `.mkv` et `.mp4`**, y compris plusieurs pistes internes. Ne pas modifier la priorité pour de la compatibilité H.264/AAC générique ; ceux-ci peuvent être ajoutés ultérieurement.
- **Aucun transcodage vidéo obligatoire**, surtout sur le Synology DS1522+ sans iGPU. Remux/préparation facultatifs possibles uniquement si justifiés et documentés.
- Aucun téléchargement illégal, contournement de DRM ou contenu protégé fourni comme fixture.
- Les futures sources séparées sont au minimum plusieurs **assets indépendants** ; le besoin plus ambitieux de **flux élémentaires sans aucun conteneur** est distingué explicitement (voir `docs/MEDIA_FORMATS.md`).

## 4. Stack de référence

- Rust édition 2024, Axum, Tokio et Tower pour Core et Node.
- Svelte 5 + SvelteKit + TypeScript strict pour le frontend ; build statique lorsque c'est compatible avec les besoins retenus. Assets servis par Core **ou** indépendamment.
- Lecteur : évaluer Video.js 10 / Shaka Player avec le même jeu de fichiers, **choix non figé avant P0**. Isoler le moteur derrière une petite interface.
- SQLx, SQLite par défaut. PostgreSQL plus tard pour usages avancés ; **ne pas prétendre qu'un changement d'URL transfère les données**.
- Cache local minimal ; Valkey/Redis seulement lorsqu'une démonstration de besoin multi-Core existe.
- OIDC Authorization Code + PKCE, sessions côté Core et cookies sécurisés ; le frontend ne stocke pas les jetons OIDC dans `localStorage`.
- Compose pour le déploiement. CI, tests et vérifications de sécurité avant ajout de fonctionnalités majeures.
- Plugins Wasm, Kubernetes, CDN, algorithmes de recommandations, multi-Core actif : **hors MVP**.

Les versions de dépendances doivent être contrôlées au moment de coder ; ne jamais supposer qu'un framework gère une combinaison de codecs/conteneur sans test.

## 5. Identité, nom et configuration

- Aether est un nom temporaire : **aucune référence de marque dans les chemins API, noms SQL, politiques de sécurité, variables d'environnement ou types métier**.
- Variables génériques `MEDIA_*`, ex. `MEDIA_MODE`, `MEDIA_DB_URL`, `MEDIA_PUBLIC_URL`.
- Constante de nom par défaut définie dans `crates/shared/src/brand.rs`. Le frontend a `apps/web/src/lib/config/brand.ts` pour le repli et récupère la configuration publique du Core ; ne pas maintenir plusieurs marques divergentes.
- API publique versionnée : `/api/v1/...` ; protocole interne Node distinct et versionné.

## 6. Contrat média / lecture

- **Ne jamais assimiler** œuvre, édition, encodage, copie physique, asset ou piste ; identifiants uniques et stables (`docs/MEDIA_FORMATS.md`).
- Scanner `.mkv` / `.mp4` sans supposer une seule vidéo, un seul audio ni un seul sous-titre.
- AV1 + Opus est la **priorité des tests d'intégration**, même si leur compatibilité web dépend du client et de l'empaquetage des segments.
- Modéliser chaque piste : source, type, codec, langue BCP 47, libellé, rôle (commentaire, forced, SDH…), résolution, canaux, durée, timebase, timestamps, décalage, éventuel `pre-skip`/codec init et index.
- Gérer `HEAD`, `GET`, requêtes Range valides/invalides (`206`, `416`), annulation, lecture seek et mémoire bornée. Pas de lecture intégrale du film en RAM.
- Si une source n'est pas lisible, renvoyer un **motif technique explicite** ; ne pas simuler un Direct Play réussi à partir d'un simple manifeste DASH.
- P0 concerne les conteneurs actuels. Les pistes externes et le **vrai raw AV1/Opus** ne sont ni un prérequis ni une promesse du P0 ; concevoir leur place dès le modèle de données.
- Les préférences linguistiques du produit peuvent privilégier **audio français** et **sous-titres forcés français** si présents et pertinents ; choix manuel prioritaire.

## 7. Sécurité sans compromis

- RBAC côté Core : `administrator` / `user` seulement. Vérifier chaque ressource, bibliothèque, piste et action. Aucun contrôle d'accès basé uniquement sur l'UI.
- Core : mot de passe local haché avec un algorithme robuste et paramètres actuels ; bootstrap sûr, rate limiting, session sûre, rotation/révocation. OIDC : issuer, audience, signature, nonce, state, code flow + PKCE, URLs de redirection autorisées.
- Node : **aucun chemin disque arbitraire accepté depuis le navigateur** ; résolution des assets depuis son inventaire, autorisations de lecture limitées (ressource, expiration, droits). Appairage administrateur et secrets par nœud.
- TLS sur réseaux non fiables ; ne pas publier automatiquement les ports NAS. Tester CORS, CSRF, SSRF, IDOR, traversées de dossiers/symlinks, lecture hors bibliothèque, injection, XSS, fuites dans les logs et abus de débit.
- Ne pas inscrire de clés, jetons ou mots de passe réels dans Git, les fixtures, les captures ou les logs.
- Voir `docs/SECURITY.md` pour le modèle de menaces et la checklist.

## 8. Évolutivité préparée, non implémentée

- Multi-Node pris en charge à partir de P2 ; Core unique pendant le MVP.
- Identifiants indépendants des instances, services sans état durable uniquement en mémoire, DB comme source de vérité et tâches rejouables/idempotentes.
- Prévoir interfaces adaptables pour cache, sessions et tâches mais **ne pas écrire 3 backends abstraits « au cas où »**.
- Plus tard seulement : plusieurs Core derrière proxy, PostgreSQL partagé, coordination des tâches, propagation de révocations, caches partagés, reprise de panne et tests de charge.
- Ne pas router tous les flux vidéos via Core : direct Node si joignable et autorisé ; relais en dernier recours. Mesurer la consommation réseau.

## 9. Qualité et CI

- Rust : `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` ; `Result` et erreurs typées ; éviter `unwrap` / `expect` dans les chemins de production soumis à des données externes.
- Web : TypeScript strict, lint, vérifications Svelte, tests unitaires et tests end-to-end ciblés. États d'erreur/chargement explicites et accessibilité clavier.
- Tests automatisés sur fixtures **légalement créées** : AV1 + Opus `.mkv`, AV1 + Opus `.mp4` (si mux valide généré), pistes multiples, Range/seek, fichiers incomplets, variations de métadonnées.
- Tests réels navigateur : documenter les résultats, versions, OS, profils codecs et méthodes de lecture. Les tests unitaires ne prouvent pas la lecture visuelle/audio.
- Mesurer séparément RSS mémoire, CPU, démarrage lecture, scan, taux d'erreur, débit et simultanéité ; pas de promesse arbitraire « 30 Mo » / « 1 000 personnes ».
- Scans incrémentaux hors des requêtes HTTP, limite du parallélisme, backpressure et arrêt propre.
- Une API créée → contrat documenté et tests incluant échecs ; une fonctionnalité modifiée → mise à jour documentaire concernée.

## 10. Contrôle de périmètre

- Ne **pas** écrire tous les dossiers/crates vides du diagramme. Créer chaque module seulement lorsqu'un parcours le requiert.
- Ne pas remplacer Rust/SvelteKit, introduire FFmpeg en dépendance permanente, inventer un format propriétaire brut, créer un cluster ou un système de plugin sans validation technique et besoin explicite.
- En cas de blocage codec/navigateur, documenter le résultat et proposer le plus petit compromis démontré : Direct Play si compatible, remux sans réencodage, préparation hors lecture, ou échec explicite.
- Le premier développement demandé à l'IA est décrit dans **`PROMPT_DEMARRAGE_IA.md`**. Il ne faut pas commencer par tout le projet à la fois.

## 11. Sources de vérité

- `AGENTS.md` : règles de contribution.
- `ARCHITECTURE.md` : composants et frontières.
- `MEMORY.md` : état et décisions.
- `docs/SPECIFICATION.md` : exigences et périmètre.
- `docs/MEDIA_FORMATS.md` : modèle des pistes et formats.
- `docs/STREAMING_COMPATIBILITY.md` : **preuves** de lecture observées.
- `docs/SECURITY.md` : modèle de menaces et règles d'authentification.
- `docs/API.md` : contrats provisoires à stabiliser par tests.
- `docs/TEST_PLAN.md` : vérifications et jeux d'essai.
- `docs/DEVELOPMENT_PLAN.md` : phases et critères de passage.
- **Code + tests effectivement exécutés** : seuls justificatifs du comportement réel.
