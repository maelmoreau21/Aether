# Guide de contribution à Aether

Merci de votre intérêt pour **Aether** ! Ce projet vise à concevoir une plateforme multimédia **100 % auto-hébergeable et open source**, inspirée de l'ergonomie de streaming moderne, sans dépendance externe et capable de coexister en lecture seule avec d'autres serveurs comme Jellyfin.

Pour préserver la cohérence, la sécurité et la qualité du code, chaque contribution doit respecter les principes ci-dessous.

---

## 1. Principes fondamentaux et règles non négociables

Avant de proposer des modifications, prenez connaissance des règles d'architecture et de produit :

1. **Ordre strict des priorités :**
   - **P0** : Validation d'AV1 + Opus dans de vrais conteneurs `.mkv` et `.mp4` avec preuve de compatibilité en navigateur (Direct Play, remux audio sans réencodage vidéo).
   - **P1** : Mode autonome utilisable (Core + Media Node local + SQLite + admin local sécurisé).
   - **P2** : Multi-Nodes + SSO OIDC (Authentik).
   - **P3** : Pistes externes (audio/sous-titres) et exploration de flux élémentaires.
   - **P4** : Expérience films/séries, navigation, profils et ergonomie.
   - **P5** : Observabilité, tests de charge, robustesse.
   *Ne pas proposer de fonctionnalités avancées (P4/P5, cluster multi-Core, plugins) tant que les jalons P0/P1 ne sont pas consolidés.*

2. **Rôles stricts :**
   - Seuls deux rôles sont autorisés : `administrator` et `user`.
   - **Aucun rôle `guest` ou invité implicite.**

3. **Intégrité des bibliothèques médias :**
   - Les médias sont montés en **lecture seule (`:ro`)**.
   - Aether ne modifie jamais les fichiers sources, tags ou structures de dossiers existants.

4. **Pas de transcodage vidéo obligatoire :**
   - Le système doit fonctionner sur du matériel économe sans iGPU (ex. Synology DS1522+). La priorité est le Direct Play ou le remux léger sans réencodage vidéo.

5. **Neutralité du nom de marque :**
   - "Aether" est un nom de code temporaire. Aucune référence de marque ne doit figurer dans les chemins API, schémas de base de données, variables d'environnement (`MEDIA_*`) ou types métier.

6. **Fixtures et droits :**
   - Aucun contenu sous droits d'auteur, fichier protégé ou contournement de DRM n'est accepté dans le dépôt. Seuls les médias synthétiques générés légalement sont admis.

---

## 2. Déroulement d'une contribution

### Workflow Git

1. **Forkez** le dépôt et clonez votre copie locale.
2. Créez une branche thématique dédiée :
   ```bash
   git checkout -b feature/ma-fonctionnalite
   # ou
   git checkout -b fix/mon-correctif
   ```
3. Effectuez vos modifications de manière atomique et documentée.
4. Assurez-vous que l'ensemble des vérifications locales réussissent (voir ci-dessous).
5. Ouvrez une **Pull Request** vers la branche `main` en remplissant soigneusement le template de PR.

---

## 3. Exigences de qualité et commandes de test

Toute modification Rust doit satisfaire sans avertissement les vérifications suivantes :

```bash
# Vérification du formatage
cargo fmt --check

# Analyse statique stricte (aucun warning toléré)
cargo clippy --locked --all-targets -- -D warnings

# Exécution des tests unitaires et d'intégration
cargo test --locked

# Audit de sécurité des dépendances
cargo audit
```

### Frontend (SvelteKit) — lorsqu'actif
Lorsque le frontend dans `apps/web` sera initialisé :
```bash
cd apps/web
npm ci
npm run check
npm run lint
npm test
npm audit
```

---

## 4. Documentation

- Toute modification de contrat d'API doit être reflétée dans [`docs/API.md`](docs/API.md).
- Tout nouveau test ou jalon doit être documenté avec ses preuves d'exécution dans [`MEMORY.md`](MEMORY.md) et [`docs/TEST_PLAN.md`](docs/TEST_PLAN.md).
- Consultez [`ARCHITECTURE.md`](ARCHITECTURE.md) et [`AGENTS.md`](AGENTS.md) pour les détails exhaustifs de conception.
