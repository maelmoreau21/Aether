## Description des modifications

<!-- Décrivez clairement les modifications apportées et leur motivation technique. -->

## Jalon concerné

- [ ] P0 — Codecs & Conteneurs (AV1 + Opus, MKV/MP4, streaming navigateur)
- [ ] P1 — Mode autonome (Core + Media Node local + SQLite)
- [ ] P2 — Multi-Node & Authentik SSO
- [ ] P3 — Pistes externes / formats élémentaires
- [ ] P4 — Films & Séries
- [ ] P5 — Robustesse & Observabilité
- [ ] CI / Outillage / Documentation

Issue liée : <!-- ex: Closes #123 -->

---

## Garde-fous d'architecture et de sécurité (AGENTS.md)

Veuillez cocher pour attester de la conformité aux règles non négociables :
- [ ] **Accès lecture seule** : Les bibliothèques et médias restent strictement montés en `:ro` (aucune écriture, aucun tag altéré).
- [ ] **Rôles stricts** : Seuls les rôles `administrator` et `user` sont utilisés (aucun rôle `guest` ou invité implicite).
- [ ] **Pas de transcodage vidéo forcé** : Le streaming direct ou le remux léger est privilégié sans exiger d'iGPU.
- [ ] **Neutralité de marque** : Aucune référence de marque en dur dans les chemins API, DB, types ou variables (`MEDIA_*` utilisé).
- [ ] **Sécurité & Secrets** : Aucun jeton, clé secrète, mot de passe ou chemin NAS absolu n'est introduit dans le code ou les commits.
- [ ] **Fixtures légales** : Aucun média sous droits d'auteur n'est introduit (uniquement des flux synthétiques générés).

---

## Vérifications locales obligatoires

- [ ] `cargo fmt --check` : exécuté avec succès.
- [ ] `cargo clippy --locked --all-targets -- -D warnings` : 0 warning.
- [ ] `cargo test --locked` : tous les tests passent.
- [ ] `cargo audit` : validé sans faille sur les dépendances.
- [ ] Si applicable, tests de lecture navigateur documentés.
- [ ] Documentation associée et `MEMORY.md` mis à jour si un jalon réel est franchi.
