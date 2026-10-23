# Démarrage de lecture et catalogue de plugins — 23 octobre 2026

## Lecture

Le moteur attend une réserve **contiguë de 0,75 seconde** avant de démarrer la
vidéo et l’audio indépendant, avec données décodables (`readyState >= 3`) et
seek terminé. Si le tampon est déjà prêt, cette vérification est immédiate ;
aucun sommeil fixe de 750 ms n’est ajouté. La fin du fichier demande seulement
le contenu restant. Attente bornée à huit secondes, annulation sur fermeture/
pause et aucune lecture tardive après annulation. La limite existante de dix
secondes sur l’opération globale de démarrage est conservée.

L’audio segmenté prépare la fenêtre suivante quand il reste quatre secondes
dans la fenêtre courante de dix secondes. Cela évite de lancer ce deuxième
travail de copie pendant le décodage initial ; un seek près de la frontière
conserve le chargement de la fenêtre suivante. Les bornes mémoire, annulations,
fenêtres et copies sans réencodage existantes restent actives.

Les petites dérives d’horloges indépendantes ne provoquent plus une pause vidéo
et un seek audio toutes les 100 ms : correction audio bornée à ±2 % autour de la
vitesse choisie, avec conservation de hauteur demandée au navigateur. Zone neutre
40 ms, resynchronisation forte au-delà de 250 ms. Les vraies attentes réseau,
erreurs, pauses manuelles et seeks restent contrôlés. Aucun transcodage serveur,
remplacement de langue ou mélange stéréo n’est ajouté.

## Preuves locales et limites

Chrome 155.0.8059.86, Windows, AV1 10 bits 1080p / deux pistes Opus 5.1,
fixture synthétique de seize secondes, HTTP isolé et moteur TypeScript réel :

- Avant la correction douce, réserve déjà ajoutée : cinq événements `play` dans
  les deux premières secondes, sans action utilisateur. Après : un seul, aucun
  `waiting` actif avant deux secondes, 29 images comptabilisées et zéro image perdue.
  Il s’agit d’une observation courte, pas d’un P95 ou d’une preuve sur NAS.
- Après correction douce en mode segmenté : première image 196 ms, lecture prête
  1 025 ms (première préparation audio 974 ms), réouvertures 46/68 ms. Le seuil
  global 200 ms n’est pas atteint à froid ; première image et disponibilité audio
  sont deux mesures distinctes.
- HTTP vidéo volontairement cadencé à 16 Kio/100 ms : lecture prête en environ
  3,1 s, un événement play initial, aucun waiting actif avant deux secondes et
  zéro image perdue. L’empaquetage court de la fixture impose ici une longue
  réception initiale ; aucun gain de démarrage réseau n’est revendiqué.
- PCM FR/EN réellement décodé, changements de langue, pause/seeks, frontière de
  segments, retour à l’ancienne piste après 404 et annulation contrôlés.
  Pas de preuve de sortie 5.1 physique, de 4K, d’endurance ou du stockage réel.

Rapports : [avant correction douce](evidence/2026-10-23/startup-plugins/before-soft-sync.json),
[après](evidence/2026-10-23/startup-plugins/after-soft-sync.json),
[HTTP cadencé](evidence/2026-10-23/startup-plugins/slow-http.json).

## Dépôt de plugins

Lien unique préparé :

```text
https://raw.githubusercontent.com/maelmoreau21/Aether-Plugins/main/repository/manifest.json
```

Le dépôt voisin contient quatre vrais paquets Wasm signés et versionnés : TMDb,
IMDb, fanart.tv et avatars. Le manifeste contient URL, version, clé publique et
empreinte de chaque paquet. `node scripts/test-repository.cjs` vérifie signatures,
empreintes, concordance des manifestes et bornes sans réseau ni clé privée.
`Build-Repository.ps1` détecte les crates de plugins, prépare tous les paquets
et ce manifeste commun dans un nouveau dossier, puis les vérifie. Il ne publie
rien. La CI vérifie également les fichiers de dépôt déjà préparés.

La paire de signature est conservée hors des dépôts dans le dossier privé
`LOCALAPPDATA/MediaExtensionSigning` ; les fichiers publiables ne contiennent
que sa clé publique. Les clés des fournisseurs sont saisies sur chaque serveur.
Ce dépôt de paquets n’est pas un serveur de téléchargement de médias.

Migration additive 0041 dans les deux moteurs : enregistrement du lien une seule
fois, sans requête réseau, approbation d’éditeur ou installation. Le catalogue
initial se charge par une actualisation administrateur. La suppression est
durable après réouverture de la base et conserve les plugins installés.
La confiance de l’éditeur, l’installation désactivée, les paramètres, les accès
aux bibliothèques et l’activation restent des décisions séparées.

Les fichiers `repository/` devront être publiés sur **main** dans Aether-Plugins
pour que le lien soit accessible. Ils sont préparés localement, non poussés.
L’actualisation HTTPS publique GitHub n’est donc pas validée. Le parcours réel
Core/SQLite/Chrome utilise des documents HTTPS injectés uniquement dans le
binaire de test ; aucune exception réseau locale n’est livrée en production.
Aucune migration PostgreSQL n’a été exécutée dans cette demande.

## Interface

Cartes compactes, recherche, versions, statut visible, un panneau de configuration
complet ouvert à la fois. Accès/éditeur et détails techniques dépliables ; historique
et journal regroupés. Catalogue, Installées et Dépôts restent accessibles séparément.
La première lecture de l’administration passe de deux séries de sept requêtes à
une seule. Les quatre manifestes signés sont affichés dans une UI de production
à 1440 et 390 pixels, clair/sombre ; API simulée, sans débordement ni erreur JS.
La suppression du dépôt et l’activation explicite sont aussi exercées.

## Commandes exécutées

```powershell
npm test --prefix apps/web
npm run check --prefix apps/web
npm run lint --prefix apps/web
npm run build --prefix apps/web
cargo fmt --check
cargo clippy --locked --all-targets -j 1 -- -D warnings
cargo test --locked -p media-poc --lib extensions::repositories -j 1 -- --include-ignored --test-threads=1
node scripts/browser-dual-opus.cjs
# Audio segmenté : MEDIA_TEST_SEGMENTED=1 ; HTTP cadencé : MEDIA_TEST_SLOW_START=1
node scripts/browser-extension-catalogue-ui.cjs
node ../Aether-Plugins/scripts/test-repository.cjs
```

Aucun push, déploiement ou média personnel modifié. Prochain contrôle utile :
le démarrage et les premières secondes de lecture sur le NAS après installation.


Validation finale : **112 tests Web**, check Svelte zéro erreur/avertissement,
lint/build, format Rust et Clippy all-targets strict réussis. Les quatre tests
Core dépôts, dont le navigateur réel, passent. Le test navigateur de cycle de
vie exécute réellement une fixture Rust/Wasm signée : droits, proposition
persistée après rechargement, désactivation et désinstallation passent aussi.
Il utilise l’import HTTP signé pour préparer sa fixture, sans réintroduire
un écran d’import de fichier dans le produit.

```powershell
cargo test --locked -p media-poc --test extensions browser_extension_installs_and_executes_real_rust_module -j 1 -- --ignored --nocapture --test-threads=1
```

[Aperçu catalogue sombre](evidence/2026-10-23/startup-plugins/catalogue-ui/catalogue-dark-1440.png),
[mobile clair](evidence/2026-10-23/startup-plugins/catalogue-ui/catalogue-light-390.png),
[rapport UI simulée](evidence/2026-10-23/startup-plugins/catalogue-ui/report.json),
[rapport dépôt Core/SQLite réel](evidence/2026-10-23/startup-plugins/core-repository/report.json).

## Administration compacte (23 octobre)

Le menu de navigation est fermé par défaut sur ordinateur et mobile : bouton
Menu, tiroir modal, aucune colonne réservée. Échap, clic extérieur, bouton de
fermeture et choix de section le ferment ; le focus revient au bouton Menu.
Les liens restent disponibles au clavier, avec section active explicite.

La vue d’ensemble conserve serveur, bibliothèques, nœuds et incidents utiles.
Versions et mode sont dans Informations techniques. Les introductions répétées,
badges et compteurs redondants ont été retirés. Catalogue : descriptions courtes,
versions multiples seulement si nécessaires, détails et accès repliés.
Les alertes, permissions, confiance éditeur et confirmations restent visibles
lorsqu’une action les nécessite.

Validation : check Svelte sans erreur/avertissement, lint, 112 tests et build.
Chrome sur build de production : menu fermé sans espace latéral, navigation,
Échap/focus, clic extérieur, clair/sombre 1440/390 et parcours catalogue réussis
avec API simulée. Ce contrôle ne prouve pas un déploiement sur le NAS.

Le scénario historique `apps/web/scripts/test-admin-ui.mjs` n’a pas terminé :
blocage sur le lien des préférences personnelles `#account`, après le parcours
administration clair. Il n’est pas compté comme réussi. Ses captures partielles
sont dans `docs/evidence/2026-10-23/admin-compact/`. Le scénario ciblé
`browser-extension-catalogue-ui.cjs` passe et couvre le nouveau menu, la vue
d’ensemble, le catalogue et ses actions ; APIs simulées explicitement.
