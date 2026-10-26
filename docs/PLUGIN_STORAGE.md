# Stockage et persistance des extensions (Plugins) Aether

## 1. Emplacement et organisation

Dans tous les modes de déploiement Docker (mode autonome `aether` et mode distribué `aether-core`), les extensions installées sont stockées de façon persistante dans :

```
/data/plugins/
```

Chaque extension dispose d'un sous-dossier dédié nommé d'après son identifiant unique normalisé (ex. `/data/plugins/tmdb/`) contenant exactement trois fichiers vérifiés :
- **`package.json`** : L'archive signée complète sérialisée (manifeste, identité éditeur, bytecode Wasm en base64, empreinte SHA-256 et signature cryptographique Ed25519).
- **`manifest.json`** : Le manifeste de l'extension au format JSON lisible (nom, version, permissions, hôtes HTTP autorisés).
- **`module.wasm`** : Le binaire WebAssembly exécutable compilé pour le guest worker (`media-extension-worker`).

---

## 2. Modèle d'autorité et réconciliation SQL ↔ Disque

1. **Source de vérité** :
   La base de données (`extensions.package_json`) est l'unique autorité durable. Elle contient l'intégralité du paquet signé et son empreinte cryptographique.
2. **Consommation Wasm** :
   Le binaire `module.wasm` sur disque est consommé par les workers isolés `media-extension-worker`. Son intégrité SHA-256 est systématiquement vérifiée avant toute instanciation.
3. **Réconciliation automatique au démarrage (`sync_installed`)** :
   À chaque démarrage de Core (`aether` ou `aether-core`), la fonction `sync_installed` :
   - Crée le dossier `/data/plugins/` s'il n'existe pas encore.
   - Nettoie les dossiers temporaires orphelins (`.install-*`) en cas de coupure brutale pendant une précédente installation.
   - Parcourt tous les paquets enregistrés en base SQL.
   - Restaure automatiquement les fichiers manquants ou reconstruit le dossier s'il a été vidé.
   - Répare automatiquement tout fichier corrompu (ex. `module.wasm` altéré ou tronqué) à partir du paquet officiel validé en base.
4. **Transactions atomiques** :
   L'installation écrit d'abord dans un dossier temporaire de staging `.install-<uuid>` avant d'effectuer un basculement atomique. En cas d'échec d'écriture ou de refus de droits, la transaction SQL est annulée et le dossier temporaire est immédiatement purgé.

---

## 3. Volume Docker nommé vs Dossier monté sur SSD NVMe

### Volume Docker nommé (`aether_data:/data`)
- Géré directement par le moteur Docker sous `/var/lib/docker/volumes/aether_data/_data`.
- Docker initialise automatiquement les permissions du volume pour correspondre à l'utilisateur non-privilégié du conteneur (`aether:aether`, UID `10001`, GID `10001`).
- Ne nécessite pas de réglage manuel de propriétaire sous Linux.

### Dossier monté depuis l'hôte / SSD NVMe (`/mnt/nvme/aether/data:/data` ou Synology `/volume1/docker/aether/data:/data`)
- Lorsque le volume `/data` pointe sur un dossier du système de fichiers de l'hôte (SSD NVMe, NAS Synology) :
  - Le conteneur s'exécute avec l'utilisateur unprivilégié `UID 10001`, `GID 10001`, et toutes les capacités Linux désactivées (`cap_drop: ALL`, `no-new-privileges: true`).
  - Si le dossier sur l'hôte a été créé par `root` (UID `0`) ou par l'administrateur du NAS (UID `1026` ou `1039`) avec des droits restreints (`700` ou `750`), l'utilisateur `10001` ne peut pas créer de sous-dossier `/data/plugins` ni écrire la base SQLite.
  - L'erreur observable dans les logs est alors :
    `Plugin storage error at '/data/plugins' (PermissionDenied): Permission denied (os error 13)`.
- **Commandes obligatoires à exécuter sur l'hôte pour un montage NVMe / NAS** :
  ```bash
  # Aligner le propriétaire du dossier de données Aether sur l'UID 10001
  sudo chown -R 10001:10001 /volume1/docker/aether/data
  sudo chmod 750 /volume1/docker/aether/data
  ```

---

## 4. Vérification et diagnostic sur une installation Docker

### A. Vérifier les fichiers sur le conteneur
```bash
# Vérifier l'existence du dossier des plugins et son propriétaire
docker compose exec aether ls -la /data/plugins

# Vérifier les fichiers d'une extension spécifique (ex. tmdb)
docker compose exec aether ls -la /data/plugins/tmdb

# Doit afficher :
# manifest.json
# module.wasm
# package.json
```

### B. Vérifier l'état de santé du stockage via l'API
Un endpoint de diagnostic sécurisé pour administrateur est exposé :
```http
GET /api/v1/admin/extensions/storage/health
```
Exemple de réponse :
```json
{
  "path": "/data/plugins",
  "exists": true,
  "writable": true,
  "healthy": true,
  "installedCount": 2,
  "onDiskCount": 2,
  "missingCount": 0,
  "corruptedCount": 0,
  "error": null
}
```

### C. Vérifier les journaux de démarrage
Au lancement, le Core consigne explicitement l'état du stockage :
```
Plugin storage initialized at '/data/plugins' (2 plugin(s) verified/synced)
```
En cas de problème de droits ou de volume, un message explicite avec le chemin exact et le code d'erreur OS est produit :
```
Plugin storage error at '/data/plugins' (PermissionDenied): Permission denied (os error 13); affected plugins cannot execute
```
