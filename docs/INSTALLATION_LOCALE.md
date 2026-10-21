# Installer Aether sur son PC Windows

Ce parcours lance un Core, son Node local et SQLite sans Docker. Le frontend est
compilé une fois ; aucun serveur Node.js ne reste lancé. Les extensions restent
facultatives. Prérequis : Rust/Cargo avec l’outillage C++ Windows, Node.js/npm,
`ffprobe` disponible dans le PATH. Le mode HTTP ci-dessous reste sur ce PC.

## 1. Compiler et créer le compte administrateur

Dans PowerShell :

```powershell
Set-Location C:\Users\Mael\Documents\GitHub\Aether
npm --prefix apps/web ci
npm --prefix apps/web run build
cargo build --locked -p media-poc --bin aether-core --bin media-bootstrap --bin media-extension-worker --bin media-extension-pack -j 1
$env:MEDIA_DB_ENGINE = 'sqlite'
$env:MEDIA_DB_PATH = Join-Path (Get-Location) 'data/core.sqlite3'
New-Item -ItemType Directory -Force data | Out-Null
.\target\debug\media-bootstrap.exe
```

Le dernier programme demande le login puis le mot de passe dans le terminal.
Il ne prend pas d’argument `--login`. Si un administrateur existe déjà, utilisez
son compte : ne supprimez pas la base pour recommencer.

## 2. Démarrer

Dans le même terminal (les variables précédentes s’appliquent aussi au Core) :

```powershell
$env:MEDIA_BIND_ADDR = '127.0.0.1'
$env:MEDIA_PORT = '3000'
$env:MEDIA_PUBLIC_URL = 'http://127.0.0.1:3000'
$env:MEDIA_WEB_DIST = Join-Path (Get-Location) 'apps/web/build'
$env:MEDIA_LIBRARY_ROOT = Read-Host 'Chemin complet du dossier de vos médias existants'
if (!(Test-Path -LiteralPath $env:MEDIA_LIBRARY_ROOT -PathType Container)) { throw 'Dossier média introuvable' }
.\target\debug\aether-core.exe
```

Ouvrir <http://127.0.0.1:3000/app/login/>. Dans Administration → Bibliothèques,
ajouter une bibliothèque en choisissant la racine autorisée indiquée au démarrage,
puis lancer un scan. Le Core ne permet pas de parcourir arbitrairement les autres
disques. Les données
applicatives restent dans `data/`; les médias ne sont pas modifiés. Garder ce
terminal ouvert ; `Ctrl+C` arrête proprement le serveur. Pour un lancement
ultérieur, réappliquer les variables et relancer le Core, sans refaire le bootstrap.

L’accès depuis un téléphone ou un autre PC demande une configuration réseau et
HTTPS adaptée, décrite dans [le guide Docker](DEPLOYMENT_DOCKER.md).

## 3. Ajouter les extensions

`Aether-Plugins` contient les sources des fournisseurs. Le dossier du dépôt,
un exécutable `.exe`, un fichier `.wasm` seul ou un manifeste Jellyfin ne
s’importent pas directement. Construire un paquet signé selon
[le guide Aether-Plugins](../../Aether-Plugins/README.md), puis dans
Administration → Extensions :

1. Approuver l’éditeur avec sa clé **publique**.
2. Choisir le fichier `tmdb.package.json`, puis « Vérifier et installer ».
3. Ouvrir « Gérer cette extension » → « Paramètres du plugin » et saisir la
   clé du fournisseur.
4. Cocher les bibliothèques autorisées, enregistrer et activer.
5. Rechercher une œuvre puis exécuter pour obtenir une proposition.

L’installation ne déclenche pas automatiquement un scan, une récupération de
toutes les affiches ou une modification du catalogue. fanart.tv et les avatars
ont des opérations spécifiques qui ne sont pas accessibles depuis ce parcours
manuel de proposition de métadonnées. Les mises à jour en place et paquets
Jellyfin ne sont pas pris en charge. L’onglet Dépôts permet aussi d’ajouter un
[catalogue HTTPS de paquets](EXTENSION_REPOSITORIES.md), lorsqu’un auteur en héberge un.

Une clé API créée dans Aether sert à lire son catalogue depuis une application
externe. Une clé TMDb/fanart.tv est délivrée par ce fournisseur : sa durée,
ses quotas et ses conditions dépendent de lui. Un réglage Aether ne peut pas
rendre un service tiers illimité.

## 4. Activer les sauvegardes chiffrées

Avant le démarrage du Core (ou après l’avoir arrêté avec `Ctrl+C`), créer une
clé privée locale une seule fois, puis la charger dans le même terminal :

```powershell
$secretDirectory = Join-Path $env:LOCALAPPDATA 'MediaServerSecrets'
New-Item -ItemType Directory -Force $secretDirectory | Out-Null
$backupKeyPath = Join-Path $secretDirectory 'backup.key'
if (!(Test-Path -LiteralPath $backupKeyPath)) {
    $backupBytes = New-Object byte[] 32
    $random = [Security.Cryptography.RandomNumberGenerator]::Create()
    $random.GetBytes($backupBytes)
    $random.Dispose()
    [IO.File]::WriteAllText($backupKeyPath, (-join ($backupBytes | ForEach-Object { $_.ToString('x2') })))
}
$env:MEDIA_BACKUP_KEY = [IO.File]::ReadAllText($backupKeyPath).Trim()
.\target\debug\aether-core.exe
```

Administration → Sauvegardes permet alors de créer et télécharger une archive.
Conserver une copie privée de `backup.key` séparément des archives : elle est
nécessaire à la restauration et n’est jamais incluse dans les sauvegardes.
Ne pas la remplacer lors des prochains démarrages. Les médias ne sont pas
copiés dans ces archives ; [la restauration se fait hors ligne](ADMIN_BACKUPS.md).
