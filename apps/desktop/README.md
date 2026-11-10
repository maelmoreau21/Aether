# Aether Desktop

Application de bureau d'Aether (Windows, macOS, Linux) : interface en GPUI,
lecture par libmpv, API de la Core via le cœur client partagé `aether-client`.

- Connexion en deux pages, même présentation que la page de connexion du web
  (palette Halo). Page 1 : l'adresse du serveur ; « Continuer » vérifie que c'est
  bien un serveur Aether (`/api/v1/config`, `/api/v1/auth/oidc/status`) et refuse
  toute autre adresse. Page 2 : comme le web, le bouton SSO (libellé choisi par
  l'administrateur de la Core ; il ouvre le navigateur sur la page de validation),
  le compte local (si la Core l'autorise) puis Quick Connect (code + QR).
  Le jeton d'appareil est rangé dans le trousseau système (Credential Manager,
  Keychain, Secret Service) ; seuls l'adresse et l'identifiant de télécommande
  vont dans `settings.json` (`%APPDATA%\Aether`, `~/Library/Application Support/Aether`,
  `~/.config/aether`).
- Accueil « Continuer », bibliothèques Films/Séries avec affiches, fiche d'une œuvre.
- Lecteur intégré : lecture directe de l'original (`/api/v1/assets/{id}/content`,
  jeton en en-tête), pistes audio et sous-titres, reprise et sauvegarde de la
  position. Clavier : espace, ←/→ (±10 s), ↑/↓ (volume), F (plein écran), Échap.
- Télécommande : `/api/v1/remote/ws` en `kind: "desktop"` (lecture, pause,
  avance, arrêt, ouverture ; « Reprendre ici » depuis un autre appareil).

## Installer (Windows)

Les releases contiennent `Aether-Setup-windows-x64.exe` : un installateur (Inno
Setup, `installer/aether.iss`) qui installe l'application pour l'utilisateur
courant, crée le raccourci du menu Démarrer (et du Bureau, au choix) et un
désinstalleur. `libmpv-2.dll` y est incluse. Le zip reste disponible pour une
utilisation portable.

## Construire

libmpv est chargée **à l'exécution** : la compilation n'a besoin ni de ses
en-têtes ni d'une bibliothèque d'import.

```bash
cargo build --release -p aether-desktop
```

Dépendances système de GPUI sous Linux (Debian/Ubuntu) :
`libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libxcb1-dev
libfontconfig-dev libfreetype-dev libvulkan1`.

## libmpv à l'exécution

- Linux : `libmpv2` (Debian/Ubuntu) ou `mpv-libs` (Fedora).
- macOS : `brew install mpv`.
- Windows : `libmpv-2.dll` (builds « mpv-dev » de shinchiro) à côté de
  `aether-desktop.exe`.
- Ou un chemin explicite : `AETHER_LIBMPV=/chemin/vers/libmpv.so.2`.

mpv 0.35 ou plus récent. Sans libmpv, l'application fonctionne et le lecteur
explique comment l'installer.

## Limites connues

- Les images sont rendues par le moteur logiciel de mpv (décodage matériel
  avec recopie) puis envoyées à GPUI comme textures, à la résolution de la
  vidéo au plus. Un rendu GPU partagé (D3D11/Metal/Vulkan) reste à faire.
- Le champ de saisie ne gère pas la composition IME.
- `vendor/xattr` : copie de xattr 0.2.3 corrigée pour libc ≥ 0.2.190 (tirée
  par GPUI) ; à retirer quand GPUI passera à xattr 1.x.

## Tests

```bash
cargo test -p aether-desktop
```

Le test `libmpv_plays_renders_and_stops` lit un clip généré par `ffmpeg`
quand libmpv et ffmpeg sont présents, et s'ignore sinon.
