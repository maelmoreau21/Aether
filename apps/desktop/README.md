# Aether Desktop

Application de bureau d'Aether (Windows, macOS, Linux) : **l'interface web du
Core dans une fenêtre native**. Rien n'est réécrit : l'application affiche
`https://votre-serveur/app/` avec le moteur web du système (WebView2 sous
Windows, WKWebView sous macOS, WebKitGTK sous Linux). Elle a donc exactement
l'interface, le lecteur et les réglages du web, et suit chaque mise à jour du
serveur sans nouvelle installation. Aucun navigateur n'est embarqué.

- Premier lancement : une page locale (palette Halo) demande l'adresse du
  serveur et vérifie que c'est bien un serveur Aether (`/api/v1/config`,
  `/api/v1/auth/oidc/status`) avant de l'ouvrir. L'adresse est rangée dans
  `settings.json` (`%APPDATA%\Aether`, `~/Library/Application Support/Aether`,
  `~/.config/aether`) ; les lancements suivants ouvrent directement le serveur.
- Connexion : celle du web (compte local, passkeys, double authentification).
  Le bouton SSO ouvre la page du fournisseur dans **le navigateur de
  l'ordinateur** (ses sessions, son gestionnaire de mots de passe), qui rend
  la main à l'application par un port local (`http://127.0.0.1:<port>/sso`).
  La session est le cookie du web, conservé par le moteur dans le dossier
  `webview` à côté de `settings.json`, et prolongée de 30 jours à chaque jour
  d'utilisation (`POST /api/v1/auth/session/extend`).
- Comportement d'application : pas de menu ni de raccourcis de navigateur
  (recherche, impression, zoom), pas de glisser d'images ou de liens, taille
  de fenêtre mémorisée. `F5` recharge l'interface.
- Hors ligne : si le serveur déjà ouvert ne répond pas, la page locale propose
  « Mes téléchargements hors ligne », qui ouvre la page des téléchargements du
  web (conservée, avec les médias chiffrés, par le moteur web).
- Lecture : le lecteur du web. Le Core lit en direct ce que le moteur sait
  décoder et transcode le reste, comme dans un navigateur.
- Plein écran : celui du lecteur passe la fenêtre en plein écran ; `F11` le
  bascule à tout moment.
- Changer de serveur : `Ctrl + Maj + S`.
- Les liens que l'interface ouvre dans une nouvelle fenêtre partent vers le
  navigateur par défaut.

**HTTPS est nécessaire** hors de `127.0.0.1` : le Core ne délivre son cookie
de session qu'en HTTPS (voir `apps/tv/README.md`, même contrainte).

## Mémoire

Le moteur web est celui du système, partagé avec lui : l'exécutable ne pèse
que quelques mégaoctets et ne charge pas de second navigateur. Sous Windows,
les services d'Edge inutiles à une fenêtre mono-site sont désactivés
(traduction, synchronisation, mises à jour de composants, réseau d'arrière-plan)
et, quand la fenêtre est réduite, le moteur rend sa mémoire au système
(`MemoryUsageTargetLevel` bas) ; la lecture continue.

## Installer (Windows)

Les releases contiennent `Aether-Setup-windows-x64.exe` : un installateur (Inno
Setup, `installer/aether.iss`) qui installe l'application pour l'utilisateur
courant, crée le raccourci du menu Démarrer (et du Bureau, au choix) et un
désinstalleur. Le zip reste disponible pour une utilisation portable.

WebView2 est fourni avec Windows 11 et les Windows 10 à jour. S'il manque,
l'installer depuis <https://developer.microsoft.com/microsoft-edge/webview2/>
(« Evergreen Bootstrapper »).

## Construire

```bash
cargo build --release -p aether-desktop
```

Dépendances système sous Linux (Debian/Ubuntu) : `libwebkit2gtk-4.1-dev
libgtk-3-dev`. Rien à installer sous Windows et macOS.

## Sécurité

Toute page affichée peut envoyer des messages à l'application (c'est ainsi que
le lecteur demande le plein écran). L'application n'accepte que trois demandes
sans effet hors de la fenêtre (plein écran, bascule du plein écran, retour au
choix du serveur) ; le choix du serveur n'est accepté que de la page locale.
Seules les adresses `http(s)` sont suivies ou transmises au navigateur.

## Tests

```bash
cargo test -p aether-desktop
```
