# Aether — instructions de contribution pour les agents IA

Ces règles s'appliquent à l'ensemble du dépôt, pour Claude Code, Codex et les autres agents. Elles définissent des **invariants**, pas une mission automatique. La demande explicite de l'utilisateur détermine la tâche à réaliser, dans le respect des contraintes de sécurité et des instructions de priorité supérieure.

## 1. Démarrer sur l'état réel, pas sur l'histoire

1. Lire ce fichier, inspecter la branche, le commit, git status et les modifications existantes. Ne jamais écraser le travail d'un autre agent.
2. Lire **uniquement** les sources, tests et documents utiles à la demande. Ne pas parcourir systématiquement toute la mémoire, toutes les archives ou tous les rapports.
3. Définir le périmètre, les fichiers concernés et les critères de réussite. Une demande ciblée ne justifie pas une refonte générale.
4. Vérifier l'implémentation avant toute affirmation. Le code et les tests réellement exécutés priment sur les annonces ou plans datés.
5. Travailler par lots cohérents et vérifiables. Après un lot réussi, s'arrêter au périmètre demandé ; ne pas lancer spontanément un autre chantier.

Il n'y a **aucune mission P0/P1 à relancer par défaut**. Le document .ai/PROMPT_DEMARRAGE_IA.md est une archive de la phase initiale, pas une instruction active. Les anciennes roadmaps ne définissent pas les priorités des nouvelles sessions.

## 2. Architecture actuelle et sources utiles

- Backend Rust 2024, Core Axum/Tokio, Media Node et contrats partagés ; les modules ont été découpés en domaines (voir docs/BACKEND_MODULES.md).
- Frontend Svelte 5 / SvelteKit / TypeScript, livré statiquement selon le mode de déploiement.
- Base SQLite en standalone ; PostgreSQL optionnel. Le mode multi-Core actif reste expérimental : ne jamais le présenter comme prêt pour la production sans validation spécifique.
- Bibliothèques de films, séries et musique ; inventaire, administration, extensions et modes standalone/distribué. Vérifier chaque capacité précise dans le code et ses tests.
- API publique /api/v1 ; communications internes Core/Node distinctes.

Lire les documents par domaine, et non en bloc :
- Architecture : ARCHITECTURE.md et docs/BACKEND_MODULES.md.
- Frontend/UI : docs/UI_DIRECTION.md et styles réels dans apps/web/src/lib/styles/.
- Lecture/compatibilité : docs/MEDIA_FORMATS.md et docs/STREAMING_COMPATIBILITY.md.
- Identité/sécurité : docs/SECURITY.md et docs/OIDC.md.
- Déploiement : docs/DEPLOYMENT_DOCKER.md, puis docs/DEPLOYMENT_SYNOLOGY.md si concerné.
- Extensions : docs/EXTENSIONS_API.md et docs/PLUGIN_UPDATE_LIFECYCLE.md.
- Travail multi-agent et validations : docs/AI_WORKFLOW.md.
- Mémoire synthétique : .ai/MEMORY.md, **sur le sujet concerné seulement**. Les archives ne sont pas des tâches à reprendre.

## 3. Invariants produit et sécurité

- Rôles applicatifs : administrator et user. Aucun guest, compte auto-inscrit, promotion admin implicite ou mot de passe de démonstration.
- Bootstrap du premier administrateur local sécurisé. OIDC avec validation des identités, issuer, audience, signature, state/nonce et PKCE lorsqu'il est activé.
- Autorisations côté Core pour les utilisateurs, bibliothèques, ressources et actions. Les contrôles UI ne remplacent pas le RBAC.
- Dossiers de médias en lecture seule par défaut. Ne jamais modifier, supprimer ou réencoder destructivement les fichiers des bibliothèques ni ceux utilisés par Jellyfin.
- Les données, caches et sauvegardes applicatives restent séparés des médias. Aucune suppression de volume, migration de données réelles ou changement de secrets sans autorisation explicite.
- Media Nodes : jamais de chemin arbitraire fourni par le client ; contrôle de racine, identités et capacités de lecture bornées. TLS entre réseaux non fiables. Préserver les contrôles CSRF, CORS, SSRF, IDOR, XSS, traversées de répertoires, liens symboliques et rate limits.
- Priorité média : AV1 vidéo et Opus audio, dans MKV/MP4 et avec plusieurs pistes. Distinguer présence d'un codec, compatibilité du conteneur et **décodage réellement observé**. Aucun transcodage vidéo obligatoire.
- Conserver la distinction œuvre / édition / version / source / asset / piste et leurs identifiants persistants ; préserver les contrats JSON, SQL, HTTP Range/206/416 et protocoles Node lors d'un refactoring.
- Ne jamais inscrire de clés, tokens, données privées ou secrets dans le dépôt, les logs ou les captures.

## 4. Qualité et méthode

- Avant une modification : reproduire le problème ou établir l'état initial avec les tests pertinents. Un ancien échec n'est pas automatiquement un bug actuel.
- Faire la plus petite correction cohérente. Ne pas ajouter de crates, frameworks, abstractions, threads, caches ou modes de fonctionnement sans nécessité démontrée.
- Rust : formatage, Clippy strict et tests ciblés ; contrôle des erreurs, ressources, transactions, verrous et annulations asynchrones.
- Web : vérifications TypeScript/Svelte, lint, tests ciblés et tests navigateur quand une interaction le requiert.
- Séparer tests unitaires, API simulée, navigateur réel, Core/Node réels et déploiement NAS ; ne jamais extrapoler un résultat d'une catégorie à une autre.
- Conserver et renforcer les tests existants. Ne pas désactiver les assertions, augmenter arbitrairement les délais, cacher des échecs ou inventer des gains mesurés.
- En présence d'agents concurrents, utiliser de préférence des branches/worktrees distincts et ne jamais modifier sans coordination les mêmes fichiers.
- Si un outil ou une dépendance manque, utiliser un repli compatible ou déclarer le blocage ; ne pas fabriquer de preuve.
- Ne pas pousser, créer de tag, publier, déployer ou modifier des données distantes sans demande explicite.

## 5. Clôture d'une tâche

Rapport concis : fichiers changés, comportement préservé/modifié, commandes réellement exécutées, réussites/échecs, limites et travail restant **dans le périmètre demandé**.

Mettre à jour la documentation de domaine uniquement si un contrat ou comportement change. Pour une décision durable, ajouter une note courte et sourcée dans .ai/MEMORY.md ; ne pas recopier un journal de session ni archiver automatiquement de gros blocs.

Les documents historiques (docs/archive/ et l'ancien prompt de démarrage) servent de contexte, **pas de backlog implicite ni de consignes actives**.
