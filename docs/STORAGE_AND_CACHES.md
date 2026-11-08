# Stockage des médias, caches et renommages

Ce document décrit où Aether écrit (et n'écrit pas), ce qui est durable ou
reconstructible, comment les NFO et les identifiants externes sont lus, et comment
un fichier renommé ou déplacé garde sa fiche. Il complète
[`LIBRARIES_AND_SCANNER.md`](LIBRARIES_AND_SCANNER.md) et
[`LOCAL_FILM_NFO.md`](LOCAL_FILM_NFO.md).

## 1. Bibliothèques en lecture seule

Un Node ne fait que lire une bibliothèque : parcours, `ffprobe`, NFO, affiches,
sous-titres, échantillons d'empreinte. Rien n'est créé à côté des médias, en
particulier aucun dossier `.aether`. Les seuls noms spéciaux reconnus dans un
dossier de médias sont des marqueurs que l'utilisateur écrit lui-même
(`.nomedia`, `.ignore`, `.aetherignore`) et les sidecars `<vidéo>.media.json`.

Audit des écritures du Node (code de production) :

| Écriture | Emplacement | Hors bibliothèque ? |
| --- | --- | --- |
| Caches de préparation / paquets (`cache_dirs.rs`) | `MEDIA_PREPARATION_CACHE_DIR`, sinon `<tmp>/media-preparation-v2` | oui |
| Aperçus d'images (`image_preview.rs`) | `MEDIA_IMAGE_CACHE_DIR`, sinon `<tmp>/media-image-cache` | oui |
| Vignettes de survol et images de chapitres (`trickplay.rs`) | `MEDIA_TRICKPLAY_CACHE_DIR`, sinon `<données>/trickplay` | oui |
| Empreintes audio (`tasks.rs`) | `MEDIA_FINGERPRINT_CACHE_DIR`, sinon `<données>/cache/fingerprints` | oui |
| Identité et état du Node (`server/state.rs`) | répertoire de données | oui |

Garde-fou : `scanning_a_library_never_writes_beside_the_media` (crate `media-node`)
scanne une bibliothèque dont tous les dossiers sont en `0555` et les fichiers en
`0444`, à froid puis à chaud, et vérifie que l'arborescence (types, tailles,
dates, droits) est identique avant/après et qu'aucun nom ne contient `.aether`.

## 2. Données durables, caches de Node, données reconstructibles

| Catégorie | Contenu | Où | Si c'est perdu |
| --- | --- | --- | --- |
| **Durable (Core)** | œuvres, éditions, versions, assets (identifiants stables), corrections manuelles, titres/résumés traduits par compte (`work_translations`), utilisateurs, droits, historique de lecture, tâches | base du Core (SQLite/PostgreSQL), sauvegardes chiffrées | perte réelle : à restaurer depuis une sauvegarde |
| **Cache local de Node** | aperçus d'images, vignettes de survol, images de chapitres, paquets de préparation, empreintes audio | disque du Node, clés = identifiant d'asset + taille + date | rien de perdu : régénéré à la demande |
| **Reconstructible depuis la bibliothèque** | sondes `ffprobe`, NFO lus, affiches locales, sous-titres externes, empreintes de contenu partielles | base du Core (instantanés) et relues à chaque scan | relu au prochain scan |

Règle : un cache de Node ne contient jamais l'unique exemplaire d'une information
de l'utilisateur. Un Node redémarré avec des caches vides sert normalement ; les
vignettes et aperçus se régénèrent, et le Core lui renvoie la liste des fichiers
déjà indexés (taille, date, empreinte) pour qu'il ne relise pas les médias
inchangés.

## 3. Quotas, invalidations, orphelins

| Cache | Quota | Invalidation | Orphelins / crash |
| --- | --- | --- | --- |
| Paquets de préparation | un dossier `entry-*` par préparation, bail verrouillé | bail libéré à la fin | au démarrage suivant, les dossiers dont le bail est libre sont supprimés ; les dossiers inconnus sont laissés |
| Vignettes / chapitres | quota global, éviction LRU jusqu'à 80 % | une source modifiée publie sous une nouvelle empreinte et retire les anciennes grilles | dossiers `.in_progress` périmés balayés (1 h au démarrage, 24 h ensuite) |
| Aperçus d'images (mémoire) | 64 Mio, 2 048 entrées | clé = chemin + taille + date + largeur | n/a |
| Aperçus d'images (disque) | **512 Mio** (`MEDIA_IMAGE_CACHE_MAX_BYTES`), éviction du plus ancien jusqu'à 80 % | clé = chemin + taille + date | `*.tmp` abandonnés > 1 h supprimés au premier écrit puis tous les 64 écrits |
| Empreintes audio | **64 Mio** (`MEDIA_FINGERPRINT_CACHE_MAX_BYTES`) | clé = asset + taille + date | `*.tmp.*` abandonnés > 1 h supprimés à chaque écriture |

Avant cette révision, les deux derniers caches disque n'avaient ni quota ni
nettoyage : des fichiers de médias modifiés ou supprimés y laissaient des entrées
orphelines à vie. `disk_cache.rs` applique désormais un quota, ne touche que les
fichiers qu'il reconnaît (extension du cache), ne suit pas les liens symboliques
et ne sort jamais du dossier de cache.

## 4. Import des NFO

`nfo.rs` lit les dialectes de Radarr, Sonarr, Jellyfin et Kodi :

- racines `<movie>`, `<tvshow>`, `<episodedetails>` ; plusieurs `<episodedetails>`
  (fichier multi-épisodes de Sonarr) : le premier décrit le fichier, les suivants
  sont ignorés (32 au maximum) ;
- identifiants : `<uniqueid type="…">`, `<tmdbid>`/`<imdbid>`/`<tvdbid>` (et
  variantes `_id`), attributs `TMDB`/`IMDB`/`TVDB` de `<id>`, alias `themoviedb`
  / `thetvdb`, `<id>tt…` ; la casse est ignorée ;
- fichiers de type Kodi ancien : une URL fournisseur seule, ou avant/après le XML
  (`themoviedb.org/movie/603`, `imdb.com/title/tt0133093`, `thetvdb.com/?id=81189`).
  Le texte est seulement analysé, jamais ouvert. Tout autre texte hors racine reste
  une erreur ;
- `premiered` l'emporte sur `aired` quel que soit l'ordre.

### Identifiants : jamais de confiance aveugle

- Les valeurs sont normalisées (zéros de tête retirés, `TT…` → `tt…`). `0` (valeur
  « inconnue » écrite par Radarr/Sonarr) et `type="unknown"` ne sont pas des
  identifiants.
- Une valeur mal formée (`imdb=12`, `tmdb=abc`) rend toujours le NFO invalide.
- **Contradiction** (deux valeurs différentes pour un même fournisseur dans un
  fichier, ou entre le NFO et le nom du dossier/fichier) : le fournisseur est
  retiré et signalé (`external_id_conflicts`, avertissement de scan), au lieu de
  choisir un gagnant ou d'invalider tout le NFO — le titre, le résumé, etc. restent
  utilisés. Un troisième avis ne « ressuscite » pas un identifiant écarté.
- Séries : les identifiants du nom de dossier et de `tvshow.nfo` sont réconciliés
  (accord ou valeur unilatérale conservés, désaccord écarté) au lieu de laisser le
  NFO remplacer le nom ; les identifiants d'un NFO d'épisode sont conservés pour
  l'épisode.
- Le Core applique la même règle à l'échelle de l'œuvre (`editorial.rs`) et
  affiche l'avertissement ; aucune fusion automatique n'est faite sur un
  identifiant contradictoire.

## 5. Renommages et déplacements

Avant : une œuvre était identifiée par un chemin (`folder:…` / `file:…`) quand le
nom ne portait pas d'identifiant. Renommer un dossier créait une œuvre neuve, sans
corrections manuelles, traductions ni caches.

Maintenant (bibliothèques **Films** et **Séries**, scan **complet** seulement — un dossier
illisible n'est pas un dossier disparu), le Core compare les fichiers apparus aux
assets disparus de **la même bibliothèque** et, s'ils correspondent, déplace
l'asset (même identifiant) vers le nouveau chemin et suit la clé de l'œuvre. Les
corrections manuelles, traductions et caches indexés par asset restent attachés.

Deux preuves seulement, toutes deux sans ambiguïté :

1. **Contenu** : même taille + même empreinte partielle (§6), unique des deux côtés
   (un seul asset disparu, un seul fichier apparu, aucun autre fichier du scan avec
   la même empreinte), identifiants non contradictoires, **et** un signal
   indépendant (même date de modification, même nom de fichier ou identifiant
   fiable commun). L'empreinte seule ne suffit jamais.
2. **Identifiant fiable** (tmdb/imdb/tvdb), pour un ré-encodage (mise à niveau
   Radarr : autres octets, mêmes identifiants NFO) : un seul asset disparu et un
   seul fichier apparu le portent, sans contradiction, et aucune œuvre dont les
   fichiers sont encore présents ne le porte (sinon c'est une seconde copie).

Tout doute (doublons, deux candidats, identifiants contradictoires, scan
incomplet) ne déplace rien : le fichier devient une œuvre neuve, le comportement
d'avant. Les bibliothèques ne sont **jamais** fusionnées : la comparaison est
bornée à une bibliothèque (donc à ses droits).

Séries : mêmes preuves, avec deux garde-fous en plus. Un fichier ne change jamais
d'épisode en se déplaçant (même saison et même numéro d'épisode). La clé de la série
(donc son œuvre, ses saisons et leurs liens) ne suit un renommage de dossier que si
plus aucun fichier du scan n'utilise l'ancienne clé et si la nouvelle n'est pas déjà celle
d'une autre œuvre. Un déplacement partiel (une partie des épisodes vers un autre
dossier), une série éclatée en deux emplacements ou une clé déjà prise ne déplacent
rien.

## 6. Empreinte de contenu partielle

`fingerprint.rs` : BLAKE3 de la taille et d'échantillons de 64 Kio (début, milieu,
fin ; plus un quart et trois quarts au-delà de 1 Gio), soit au plus 320 Kio lus
quelle que soit la taille du fichier. Un fichier d'au plus 192 Kio est haché en
entier (étiquette `b3f1`, sinon `b3s1`). Lecture par la poignée confinée en lecture
seule ; résultat jeté si le fichier change pendant la lecture.

**Ce n'est pas une identité cryptographique.** Des octets non échantillonnés
peuvent différer sans changer l'empreinte (un test le documente). Elle n'est
jamais utilisée seule, jamais pour dédupliquer ou fusionner, et sa perte ne fait
perdre qu'un indice. Elle est calculée seulement pour les fichiers nouveaux ou
modifiés ; pour les autres, la valeur enregistrée par le Core est réutilisée (un
Node redémarré à froid ne relit rien). Elle est stockée dans
`inventory_asset_fingerprints` (migration additive `content_fingerprints.sql`,
sans `ALTER`, SQLite et PostgreSQL).

### Coût

Aucun budget de latence n'est imposé : le coût est borné en **octets lus** (au plus
5 lectures de 64 Kio, soit autant de positionnements), pas en millisecondes, car
la latence dépend du support. Estimation de modèle (non mesurée ici), pour un
fichier froid : SSD ≈ 5 positionnements × ~0,1 ms ; HDD ≈ 5 × ~8–12 ms ; partage
réseau (SMB/NFS) ≈ 5 allers-retours × ~1–5 ms. La première analyse d'une grande
bibliothèque existante calcule les empreintes manquantes une seule fois.

Seule mesure réalisée à ce jour : 24 fichiers de 96 Mio (2,2 Gio) sur le système
de fichiers d'un conteneur Docker Desktop, **cache disque chaud** (il n'a pas pu
être vidé) : médiane ≈ 0,11 ms, p90 ≈ 0,13–0,17 ms, maximum ≈ 0,24 ms par fichier.
Ce chiffre ne dit rien du SSD, du HDD ou du NAS à froid : ils restent à mesurer
avec la commande ci-dessous sur le matériel réel.

Pour mesurer sur votre matériel (rien n'est écrit, seuls des fichiers ≥ 1 Mio du
dossier sont lus ; videz le cache disque avant pour un chiffre à froid) :

```bash
MEDIA_FINGERPRINT_BENCH_DIR=/chemin/vers/le/support \
  cargo test -p media-node fingerprint::tests::bench -- --ignored --nocapture
```

## 7. Export portable `.aether`

Il n'existe aucun export `.aether` automatique et le fonctionnement en lecture
seule n'en dépend pas. Un éventuel export portable devra être déclenché
explicitement par un administrateur, écrit hors des bibliothèques d'origine
(destination choisie), ne contenir ni secrets (clés, jetons, mots de passe, clés
de sauvegarde) ni données privées (comptes, historique, droits), et rester
ignorable : sans lui, tout continue de fonctionner. Les sauvegardes chiffrées du
Core restent le mécanisme de reprise.
