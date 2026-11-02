# Architecture et Écosystème Futur des Extensions Aether

Ce document décrit la transition de l'écosystème d'extensions Aether vers des dépôts découplés, un SDK autonome et un catalogue centralisé sécurisé.

---

## 1. Principes d'Isolation & Cycle de Vie Indépendant

Chaque extension officielle ou communautaire doit pouvoir être maintenue, testée, versionnée et distribuée de manière autonome sans recompilation ni synchronisation de version avec Aether Core.

```
                      +-----------------------------+
                      |       Aether Core / Node    |
                      |   (Runtime WebAssembly WASI)|
                      +--------------+--------------+
                                     |
                           Charge les paquets signés
                                     |
                                     v
                      +-----------------------------+
                      |       Catalogue Central     |
                      |      (Aether-Plugins)       |
                      |  - Metadonnées vérifiées    |
                      |  - Validation Ed25519       |
                      |  - SHA256 & Intégrité       |
                      +--------------+--------------+
                                     |
         +---------------------------+---------------------------+
         |                           |                           |
         v                           v                           v
+-----------------+         +-----------------+         +-----------------+
|   plugin-tmdb   |         |   plugin-imdb   |         |  plugin-avatar  |
|  - Dépôt dédié  |         |  - Dépôt dédié  |         |  - Dépôt dédié  |
|  - Releases vX  |         |  - Releases vX  |         |  - Releases vX  |
|  - CI & Wasm    |         |  - CI & Wasm    |         |  - CI & Wasm    |
+-----------------+         +-----------------+         +-----------------+
         |                           |                           |
         +---------------------------+---------------------------+
                                     |
                                 Consomment
                                     |
                                     v
                      +-----------------------------+
                      |      media-extension-sdk    |
                      |  - Contrats d'ABI Wasm      |
                      |  - Types sérialisables      |
                      |  - Versionné indépendamment |
                      +-----------------------------+
```

---

## 2. Découplage du SDK (`media-extension-sdk`)

### 2.1 État Initial (Couplage fort)
Actuellement, les plugins consomment le SDK via un chemin relatif local :
```toml
media-extension-sdk = { path = "../../Aether/crates/extension-sdk" }
```
Cela impose de cloner le dépôt `Aether` complet dans la CI d'`Aether-Plugins`.

### 2.2 Stratégie de Transition Recommandée
1. **Étape A : Dépendance Git taggée ou Crates.io**
   * Dès qu'une version stable de l'ABI est figée, `media-extension-sdk` peut être soit :
     * Publié sur crates.io (`media-extension-sdk = "0.1.0"`).
     * Référencé via le dépôt Git officiel avec un tag spécifique :
       ```toml
       media-extension-sdk = { git = "https://github.com/AetherMedia/aether", tag = "sdk-v0.1.0" }
       ```
   * En développement local, les contributeurs peuvent continuer à utiliser `[patch]` dans leur `Cargo.toml` ou `.cargo/config.toml` pour pointer sur un dossier local.
2. **Garantie de Compatibilité ABI** :
   * Chaque plugin déclare :
     * `target-abi = "1.0"`
     * `min-server-version = "0.1.0"`
     * `max-server-version = "1.0.0"`
   * Le Core Aether vérifie ces bornes avant l'instanciation du module Wasm (`wasmi`). Si l'ABI est incompatible, l'extension est désactivée proprement avec un message explicite dans les logs d'administration.

---

## 3. Rôle du Catalogue Centralisé (`Aether-Plugins`)

Le dépôt `Aether-Plugins` conserve son rôle de catalogue officiel et de registre sécurisé :
1. **Manifeste Centralisé (`manifest.json`)** :
   * Contient la liste des paquets officiels vérifiés.
   * Référence pour chaque plugin : ID, version, auteur, URL de téléchargement de l'archive/binaire Wasm, empreinte SHA-256, signature Ed25519 de l'éditeur et compatibilité ABI.
2. **Workflow d'Enregistrement Automatisé** :
   * Lorsqu'une release d'un dépôt d'extension (ex. `plugin-tmdb`) est publiée, un workflow ou une PR automatisée soumet le manifest du plugin au catalogue central.
   * Le catalogue exécute une batterie de tests cryptographiques (`test-manifests.cjs`, `test-repository.cjs`) :
     * Vérification de la clé publique de l'éditeur.
     * Vérification que la signature correspond au contenu du binaire.
     * Contrôle que le SHA256 déclaré correspond au téléchargement.
   * Aucune extension n'est ajoutée au catalogue si sa signature Ed25519 est invalide.

---

## 4. Dépôt Modèle (`aether-plugin-template`)

Un dépôt modèle sera fourni pour permettre aux développeurs tiers de créer rapidement des plugins conformes :
* Structure minimale d'une extension Rust Wasm.
* Workflow GitHub Actions préconfiguré (lint, tests unitaires, build `wasm32-unknown-unknown`, signature locale pour tests).
* Scripts de génération de package (`.package.json`).
* Documentation sur les permissions Wasm (accès réseau, stockage, hooks de métadonnées).
