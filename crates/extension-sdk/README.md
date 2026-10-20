# SDK Rust de la plateforme d'extensions

Ce crate fournit les contrats et l'ABI; aucune extension produit n'est fournie.
La cible d'exécution est `wasm32-unknown-unknown`, sans WASI ni imports hôte.

## Interface d'exécution

Le point d'entrée d'un auteur reçoit `ExtensionRequest` et retourne
`ExtensionResponse`. `extension!(handler)` expose `media_alloc` et `media_run`
sans imposer à l'auteur la gestion des pointeurs de l'ABI.

- Requête : `apiVersion`, nom `operation`, `input` JSON.
- Réponse : `apiVersion`, `result` JSON.
- Les opérations sont autorisées et leurs résultats validés par les adaptateurs
  Core. Le runtime générique ne confère aucune permission implicite.
- Ne pas importer des fonctions système : tous les imports sont refusés.
- Les buffers sont limités à 32 Kio et l'instance est détruite après l'appel.

`Manifest`, `Package` et `Package::signing_bytes` définissent l'identité,
la compatibilité et les octets signés. Le binaire `media-extension-pack` signe
hors ligne; le serveur ne reçoit jamais la clé privée d'un auteur.

## Adaptateurs disponibles et contrats futurs

`metadata.propose` est actuellement raccordé : le Core prépare un `MetadataInput`
après contrôle de la bibliothèque et valide un `MetadataProposal`. Cela produit
une proposition tracée, sans modifier les médias ni appliquer automatiquement
un changement éditorial. Le manifeste accepte uniquement `metadata.read` tant
que les autres adaptateurs ne sont pas implémentés.

Les opérations timeline, ressources dérivées, événements, tâches Node,
statistiques et réseau ne sont pas utilisables du seul fait que l'enveloppe
générique peut les transporter. Elles doivent obtenir des contrats et des
contrôles métier avant d'être exposées. Aucune bibliothèque native tierce
n'est chargée dans Core.

## Validation du SDK

Le seul guest Rust présent est une fixture non installée sous
`tests/fixtures/extension-sdk`. Pour vérifier son ABI et le runtime :

```powershell
rustup target add wasm32-unknown-unknown
cargo build -p media-extension-sdk-fixture --target wasm32-unknown-unknown --release -j 1
cargo test -p media-poc --test extensions -j 1 -- --include-ignored --test-threads=1
```

Les tests navigateur nécessitent le build Web et Chrome/Playwright. Les limites
d'isolation, le protocole du paquet et l'administration sont documentés dans
`docs/EXTENSIONS_API.md` à la racine du dépôt.
