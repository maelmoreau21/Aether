# Aether pour téléviseurs Samsung (Tizen) et LG (webOS)

Les applications TV ne réécrivent pas l'interface : elles ouvrent l'interface web du Core (`apps/web`) en
**mode TV**. Ce dossier contient la coquille qui demande l'adresse du serveur, les fichiers de chaque
plateforme et les scripts d'empaquetage.

```
apps/tv/
  shell/            coquille commune : index.html, shell.js, shell.css, logo.svg
  tizen/            config.xml + icon.png (512 × 423)
  webos/            appinfo.json + icon.png (80 × 80) + largeIcon.png (130 × 130)
  scripts/
    package.sh      assemble build/<plateforme>/ puis empaquette dans dist/
    validate.mjs    vérifie config.xml, appinfo.json, icônes et coquille sans les SDK
    icons.mjs       régénère les icônes depuis shell/logo.svg
```

## Fonctionnement

1. Au premier lancement, la coquille demande l'adresse du Core (`aether.maison.lan` devient
   `https://aether.maison.lan`). Elle vérifie qu'un Aether répond en chargeant
   `/app/images/brand-mark.svg`, mémorise l'adresse puis remplace sa page par `https://…/app/?tv=1`.
2. Aux lancements suivants, elle se reconnecte seule après 2,5 s ; une touche pendant ce délai permet de
   changer de serveur.
3. L'interface web reconnaît Tizen et webOS (et `?tv=1`) : mise en page TV, navigation aux flèches,
   touche Retour, touches média dans le lecteur. Sur la télécommande des autres appareils, le téléviseur
   apparaît comme `Samsung TV · Tizen` ou `LG TV · webOS` (`kind: "tv"`).

La page est chargée directement, pas dans un cadre : le Core interdit l'affichage en `iframe`
(`frame-ancestors 'none'`) et son cookie de session est `SameSite=Strict`.

**HTTPS est nécessaire** : le Core refuse le HTTP hors de `127.0.0.1`. Le téléviseur doit faire confiance au
certificat ; un certificat auto-signé est généralement refusé par les TV, préférez un certificat public
(par exemple via un proxy inverse et Let's Encrypt).

## Mode TV dans l'interface web

| Touche | Effet |
| --- | --- |
| Flèches | focus vers le contrôle le plus proche dans cette direction (rangées, grilles, menus) |
| OK (Entrée) | active le contrôle ; coche les cases |
| Retour (Tizen 10009, webOS 461, Échap, Retour arrière) | ferme le menu ou la fenêtre ouverte, sinon page précédente ; sur l'accueil, quitte l'application |
| Lecture/Pause, Lecture, Pause | dans le lecteur |
| Avance / Retour rapide | ± le pas de saut des préférences de lecture |
| Stop | ferme le lecteur |
| Suivant / Précédent | épisode suivant / retour au début |

Dans le lecteur, sur l'image : gauche/droite avancent ou reculent, haut/bas affichent les commandes avec le
focus sur Lecture, OK met en pause. Les commandes se masquent seules après 3 s.

Le mode s'active automatiquement (agent utilisateur Tizen, webOS et autres TV), via le réglage
« Mode TV sur cet appareil » des paramètres, ou via l'adresse : `?tv=1` (forcé), `?tv=0` (désactivé),
`?tv=auto`. Le choix est mémorisé sur l'appareil. Code : `apps/web/src/lib/tv/`.

## Empaqueter

Toutes les commandes partent de la racine du dépôt.

### Assembler seulement

```bash
sh apps/tv/scripts/package.sh stage
```

### LG webOS (`.ipk`)

Prérequis : Node.js et le CLI webOS (`npm install -g @webos-tools/cli`).

```bash
sh apps/tv/scripts/package.sh webos
```

Installation sur un téléviseur en mode développeur (application *Developer Mode* de LG Content Store) :

```bash
ares-setup-device
```

```bash
ares-install --device tv apps/tv/dist/io.github.maelmoreau21.aether_1.0.0_all.ipk
```

```bash
ares-launch --device tv io.github.maelmoreau21.aether
```

### Samsung Tizen (`.wgt`)

Prérequis : Tizen Studio avec le CLI (`tizen-studio/tools/ide/bin` dans le `PATH`), les extensions
*TV Extensions* et un profil de certificat Samsung créé dans le *Certificate Manager*.

```bash
sh apps/tv/scripts/package.sh tizen MonProfil
```

Installation sur un téléviseur en mode développeur (Apps → saisir `12345` → Developer mode, IP du PC) :

```bash
sdb connect 192.168.1.50
```

```bash
tizen install -n apps/tv/dist/Aether.wgt -t <nom-du-téléviseur-dans-sdb-devices>
```

```bash
tizen run -p AetherTV01.Aether -t <nom-du-téléviseur-dans-sdb-devices>
```

Les touches média sont réservées par la coquille (`tizen.tvinputdevice`, privilège
`tv.inputdevice`) avant d'ouvrir le Core ; l'interface web les réserve à nouveau quand l'API est exposée à
la page distante.

## Vérifier

Après `npm ci` dans `apps/web` (Playwright y est installé) :

```bash
node apps/tv/scripts/validate.mjs
```

Le parcours au clavier de l'interface en mode TV (accueil → fiche → lecture → touches média → Retour),
qui démarre lui-même `vite preview` sur le port 5189 :

```bash
cd apps/web && npm run build && npm run test:tv-ui
```

Pour modifier le logo, remplacer `shell/logo.svg` puis lancer `node apps/tv/scripts/icons.mjs`.

## Versions

`version` de `config.xml` et d'`appinfo.json` restent identiques (vérifié par `validate.mjs`). Les deux
plateformes exigent un format `x.y.z`.
