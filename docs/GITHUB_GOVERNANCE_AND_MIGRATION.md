# Gouvernance, Sécurité des Branches & Guide de Migration Organisation GitHub

Ce document détaille les paramètres de sécurité et la stratégie de migration vers une organisation GitHub dédiée (ex. `AetherMedia`), sans impacter les utilisateurs existants ni rompre l'intégrité de la plateforme.

---

## 1. Politique de Protection des Branches

Conformément aux exigences de professionnalisation, les branches `main` et `develop` doivent disposer de règles de protection strictes configurées dans GitHub (**Settings > Branches > Branch protection rules**).

### 1.1 Branche `main` (Branche Stable & Releases)

| Règle | Valeur requise | Justification |
| :--- | :--- | :--- |
| **Require a pull request before merging** | **Activé** | Aucun push direct autorisé sur `main`. |
| **Require approvals** | **1 minimum** | Revue de code obligatoire avant fusion. |
| **Dismiss stale pull request approvals when new commits are pushed** | **Activé** | Évite qu'une approbation reste valide si le code change. |
| **Require status checks to pass before merging** | **Activé** | La fusion est bloquée tant que la CI n'est pas verte. |
| *Status checks requis* : | `workflows`, `rust-tests`, `security-audit`, `frontend-tests`, `browser-tests`, `images` | Tous les contrôles de qualité, de sécurité et d'intégrité Docker. |
| **Require branches to be up to date before merging** | **Activé** | Garantit que le test en CI a été exécuté sur la base exacte de `main`. |
| **Require conversation resolution before merging** | **Activé** | Tous les commentaires de revue doivent être résolus. |
| **Do not allow bypassing the above settings** | **Activé** | S'applique également aux administrateurs. |
| **Restrict who can push to matching branches** | **Activé** (aucun utilisateur) | Seules les PRs via l'interface ou les bots autorisés peuvent fusionner. |
| **Allow force pushes** | **Désactivé** (strict) | Empêche toute réécriture d'historique sur la branche stable. |
| **Allow deletions** | **Désactivé** (strict) | Protège la branche contre les suppressions accidentelles. |

### 1.2 Branche `develop` (Branche d'Intégration Continue)

| Règle | Valeur requise | Justification |
| :--- | :--- | :--- |
| **Require a pull request before merging** | **Recommandé** | Canalise les contributions via des PRs thématiques. |
| **Require status checks to pass before merging** | **Activé** (`CI`) | Bloque l'introduction de régressions dans l'arbre commun. |
| **Allow force pushes** | **Désactivé** | Préserve l'intégrité des historiques d'équipe. |
| **Allow deletions** | **Désactivé** | Maintient la branche de développement active. |

---

## 2. Permissions Minimales des GitHub Actions

Dans **Settings > Actions > General > Workflow permissions** :
* Sélectionner : **Read and write permissions** (nécessaire pour la publication des releases et le tagging par le bot GitHub Actions dans `release.yml`).
* Cocher : **Allow GitHub Actions to create and approve pull requests** (si souhaité pour des automations de dépendances comme Dependabot).

---

## 3. Guide de Migration vers une Organisation GitHub (`AetherMedia`)

> [!IMPORTANT]
> **Ne pas exécuter la migration sans autorisation explicite du propriétaire.**
> GitHub assure des redirections automatiques pour les dépôts et les paquets, mais les étapes suivantes garantissent une transition sans friction.

### 3.1 Architecture Cible

```text
AetherMedia/
├── aether                      (Dépôt principal : Core, Node, Web, SDK)
├── aether-plugins              (Catalogue central, vérification & signatures)
├── aether-plugin-template      (Dépôt modèle pour créer de nouvelles extensions)
├── plugin-tmdb                 (Dépôts indépendants par extension)
├── plugin-imdb
├── plugin-omdb
├── plugin-fanart
├── plugin-musicbrainz
├── plugin-discogs
├── plugin-lrclib
├── plugin-avatar
└── plugin-intro-skipper
```

### 3.2 Étapes de Transfert des Dépôts

1. **Création de l'Organisation GitHub** :
   * Créer l'organisation (ex: `AetherMedia` ou nom final retenu).
   * Configurer l'authentification 2FA obligatoire pour tous les membres.
2. **Transfert du Dépôt `Aether`** :
   * Dans `maelmoreau21/Aether` > Settings > Danger Zone > **Transfer ownership**.
   * Transférer vers la nouvelle organisation.
   * *Conséquence* : L'URL `https://github.com/maelmoreau21/Aether` est automatiquement redirigée vers la nouvelle organisation. L'historique Git, les issues, les PRs et les releases sont 100 % préservés.
3. **Paquets GHCR & Compatibilité Docker** :
   * Les images existantes publiées sous `ghcr.io/maelmoreau21/aether*` restent téléchargeables grâce aux redirections GitHub Packages.
   * Les nouveaux workflows utilisent désormais dynamiquement `${{ github.repository_owner }}` : dès le transfert effectué, les nouvelles images seront automatiquement publiées sous `ghcr.io/<nouvelle-orga>/aether*`.
   * Les fichiers `docker-compose.yml` et `compose.*.yaml` supportent la variable `AETHER_IMAGE_PREFIX` :
     ```bash
     # Exemple pour pointer explicitement sur la nouvelle orga si besoin :
     AETHER_IMAGE_PREFIX=ghcr.io/AetherMedia docker compose up -d
     ```
4. **Catalogue d'Extensions (`Aether-Plugins`)** :
   * Le catalogue central valide les clés publiques de signature Ed25519 des éditeurs.
   * La signature cryptographique des paquets porte sur le binaire WebAssembly et ses métadonnées intrinsèques : **le transfert du dépôt n'invalide en rien les signatures Ed25519 existantes**.
   * Les URLs de téléchargement dans le catalogue devront être mises à jour lors de la publication de la révision suivante du manifest.
