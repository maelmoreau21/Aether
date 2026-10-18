# P3/P4 — préparation des pistes (18 octobre 2026)

Le lecteur conserve le parcours direct original et ajoute, pour les assets locaux
AV1, un choix par piste Opus interne. Le Node prépare une vidéo MP4 fragmentée
sans audio et un fichier Ogg par piste sélectionnée, uniquement par copie de
paquets. FFmpeg est **facultatif**, requis sur le PATH du processus pour ce
parcours ; son absence donne une erreur explicite. Les images Docker existantes
ne sont pas modifiées : aucune disponibilité de cette commande en conteneur
n'est annoncée. Le chemin direct et les pistes externes existants sont conservés.

Le premier passage à une piste préparée change la source vidéo en conservant la
position et la pause. Les passages entre pistes préparées partagent la même
vidéo : aucun rechargement vidéo, plein écran conservé. Chaque nouvel audio est
chargé séparément ; une courte pause de commutation reste possible. Le mécanisme
à deux éléments média corrige une dérive de plus de 150 ms toutes les 100 ms ;
il ne garantit pas une synchronisation à l'échantillon.

Les sous-titres intégrés textuels SubRip, WebVTT, mov_text, ASS/SSA sont extraits
vers WebVTT. Les sidecars ASS/SSA UTF-8 ont un **repli texte**, avec chronologie,
virgules et sauts de ligne conservés et balises échappées. Ni polices, placements,
animations, karaoké, dessins, ni styles ASS avancés ne sont reproduits. Ce parcours
n'est pas un moteur libass. Les sous-titres bitmap restent non sélectionnables.

Les offsets acceptent -30 à +30 secondes (positif = retarder). L'audio doit être
préparé ou externe ; le contrôle est désactivé pour la source originale. Les
choix précis par asset, la langue et les offsets sont mémorisés par compte dans
ce navigateur. Pas de synchronisation des préférences entre appareils. Les
sous-titres forcés FR conservent leur sélection automatique, après choix manuel.

## Frontières et limites

Chaque GET/HEAD/Range passe par les droits Core actuels. Seuls l'ID d'asset et
les indices de pistes présents dans le probe sont acceptés. L'entrée FFmpeg est
un handle confiné, avec protocoles fd/pipe et formats MKV/MP4 autorisés ; aucun
chemin ou URL fourni par le navigateur, aucune écriture dans les bibliothèques.
Cette restriction de protocoles n'est pas un sandbox système.

Une préparation à la fois, refus immédiat si occupé, 120 secondes maximum,
copie des sorties par blocs, 64 entrées et 24 Gio par instance Core (16 Gio
vidéo, 1 Gio audio, 4 Mio sous-titre). Les réponses déjà en cache restent
accessibles pendant une autre préparation. Cache plein = erreur explicite ;
pas d'éviction d'un fichier en cours de lecture. Les répertoires temporaires
sont nettoyés à la destruction normale du service. Un arrêt brutal peut laisser
des fichiers temporaires orphelins : la borne n'est pas une garantie globale
sur ces orphelins entre redémarrages. Le nettoyage persistant reste à améliorer.

Le packaging distant n'est pas exposé dans ce jalon. Les chemins multi-Node
existants restent en place. Autres codecs audio, vidéo hors AV1, HDR, sortie
physique 5.1, autres navigateurs et rendu ASS complet ne sont pas validés.
Le raw reste expérimental et ne bloque pas ce parcours.

## Catalogue

Migration additive 0016 SQLite/PostgreSQL : `inventory_works.added_at`, timestamp
Unix de première insertion, stable aux rescans et redémarrages. Les œuvres
antérieures conservent NULL (date historique inconnue). Tris `title-asc/desc`,
`year-asc/desc`, `added-asc/desc`, inconnus en dernier et ordre déterministe par
identifiant. L'accueil utilise `added-desc`. Aucune date inventée par migration.

## Validation

Commandes et résultats effectivement exécutés dans MEMORY.md ; preuves Chrome
sous `evidence/2026-10-18/player-finalization/` et essais longs séparés sous
`player-long/`. Un fichier de 90 minutes et une minute observée ne constituent
pas une preuve de lecture continue de 90 minutes ni une mesure de fuite RSS.

Commandes exécutées :

- `cargo test --locked --workspace -j 1 -- --test-threads=1` : 127 réussis, 8 ignorés.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo fmt --check`.
- Web : `npm run check`, `npm run lint`, `npm test` (48), `npm run build`.
- `cargo test --locked --test browser_scanner -- --ignored --nocapture`, puis même
  commande avec `$env:MEDIA_TEST_LONG='1'` : courts 40,41 s / longs 249,71 s, réussis.
- `cargo test --locked --test browser_series -- --ignored --nocapture` avec
  `MEDIA_REPORT_DIR` isolé : réussi en 38,66 s.

Les tests longs ont laissé des caches lors des premiers arrêts forcés : 31 dossiers,
1 703 262 315 octets dans le répertoire temporaire de l'hôte. La commande de
nettoyage a été refusée par le contrôle automatique (`blocked by policy`) et
n'a supprimé aucun fichier. Les essais finaux isolent les temporaires dans leur
propre dossier jetable ; le nombre et le volume des anciens orphelins restent
inchangés après les validations finales. Ce résidu et la purge après crash restent
une limite explicite, sans annoncer le cache de production définitivement fiabilisé.
