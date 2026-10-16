# Scanner Films local P1 — 16 octobre 2026

Extension NFO Films : [LOCAL_FILM_NFO.md](LOCAL_FILM_NFO.md). Les exclusions NFO
ci-dessous décrivent le premier jalon scanner ; les autres exclusions restent.

## Parcours implémenté

Le Node local (`node/scanner.rs`) parcourt une bibliothèque Films active,
récursivement et sans écriture. MKV/MP4 sont analysés par l'adaptateur ffprobe
existant, sans présumer les codecs. Tous les flux et leurs index techniques
sont conservés : format/durée, profil/pixel format, résolution, canaux/layout,
langues source, dispositions, timebases et autres champs réellement disponibles.
Les champs absents restent absents. Aucune analyse pendant la lecture.

Les fichiers cachés, temporaires et caches `.trickplay`/`.metathumb` sont ignorés.
Une corruption est signalée sans bloquer les autres fichiers. Aucun NFO, image,
sous-titre externe, service ARR ou accès réseau de métadonnées n'est utilisé.

L'identité provient d'un identifiant explicitement qualifié (`[tmdb-123]`,
`[imdb-tt123]`, `[tvdb-123]`), sinon du dossier exact avec année, sinon du chemin
exact du fichier. Un nombre ambigu n'acquiert aucun fournisseur. Les conflits
explicites dossier/fichier restent séparés. Il n'existe ni rapprochement flou ni
vérification en ligne des identifiants déclarés. Les rapprochements restent
limités à une bibliothèque et à sa racine ; doublons entre bibliothèques différés.

Migration `0004_inventory.sql` : Work → Edition → MediaVersion → MediaSource →
Asset, plus Track par index réel et état des scans. Une édition locale neutre et
une version/source par fichier : aucun montage éditorial n'est inventé.
UUID persistants ; chemin privé et empreinte taille/mtime réservés au backend.
Un second scan réutilise le probe des fichiers valides inchangés. Un fichier
corrompu est réessayé. Aucun hash complet de film pendant le scan.

Un fichier disparu devient indisponible après un parcours complet. Une erreur
de parcours ne réconcilie pas les disparitions globales ; les erreurs connues
sur un fichier le rendent indisponible sans supprimer ses métadonnées. Une
erreur globale conserve l'inventaire précédent. Publication SQLite atomique,
avec nouvelle vérification de la configuration. Un changement de racine invalide
les anciens descripteurs/cache ; le nouveau scan réassocie les fichiers à la
bonne œuvre. Les anciens graphes sont conservés, sans carte disponible.

## Configuration locale

`MEDIA_LIBRARY_ROOT` désigne une racine absolue existante administrée par
l'opérateur. Dans ce mode, aucun chargement/probe des fixtures expérimentales ni
création de leurs bibliothèques n'est effectué. Créer ensuite une bibliothèque
Films dans Administration → Bibliothèques, accorder les droits puis Scanner.
L'API de configuration accepte aussi les sous-dossiers de la racine autorisée ;
le sélecteur Web propose la racine opérateur. SQLite doit rester dans un dossier
distinct (`MEDIA_DB_PATH`, défaut `data/core.sqlite3`, parent existant).

Exemple PowerShell, après bootstrap local sécurisé décrit dans LOCAL_ADMIN.md :

```powershell
$env:MEDIA_LIBRARY_ROOT = 'D:\Medias\Films'
$env:MEDIA_DB_PATH = 'C:\MediaData\core.sqlite3'
cargo run --locked --bin media-poc
```

Le mode historique `MEDIA_FIXTURE_DIR` reste disponible pour les expériences et
tests P0. Ses bibliothèques réservées ne sont pas scannables. Le processus reste
sur loopback ; aucun NAS, port LAN ou déploiement n'a été configuré.

## Jobs et API

- POST `/api/v1/admin/libraries/{id}/scan` : session administrator, Origin/CSRF ;
  202 `{status:"queued",discovered:0,probed:0,reused:0,errors:[]}`.
- GET même URL : `{status,discovered,probed,reused,errors}`. États `idle`, `queued`,
  `running`, `completed`, `error`. Les compteurs sont publiés en fin de scan,
  sans pourcentage ni progression fichier par fichier. Au plus 100 messages affichés.
- 401 sans session, 403 rôle/CSRF, 400 bibliothèque inéligible/racine invalide,
  409 si un scan de cette bibliothèque est déjà en attente/en cours ; GET ID absent404.
- Une seule analyse locale à la fois, autres bibliothèques en attente. Ctrl+C
  annule les probes et attend les tâches. Après arrêt forcé, les jobs incomplets
  deviennent `error` au démarrage, sans reprise automatique ni effacement.
- Catalogue existant : recherche/tri/pagination et fiches, `mode:"local-library"`,
  `synthetic:false`. Aucun média de fixture n'est présenté comme fichier personnel.
- GET `/api/v1/media/{assetId}` retourne directement le descripteur du lecteur
  `{id,title,sources,subtitles:[]}`. Une source native par fichier, pistes internes
  décrites mais aucune commutation audio/sous-titre native simulée.
- GET/HEAD `/api/v1/assets/{assetId}/content` : mêmes sessions et permissions
  bibliothèque avant toute réponse, dont Range206/416. Aucun chemin HTTP fourni.

Le lecteur natif existant est conservé. Une acceptation du fichier par ffprobe
ne garantit pas sa compatibilité navigateur : MediaError/codecs/conteneur/timeout
restent affichés explicitement, sans transcodage/remux en lecture.

## Limites de sécurité et mesure

Tous les liens symboliques et points de réanalyse Windows, y compris jonctions,
sont refusés au parcours et recontrôlés avant lecture. Les composants relatifs
`..`, racines et préfixes sont rejetés. Canonicalisation et ouverture ne sont
pas atomiques : ACL et stockage contrôlé par l'opérateur requis face à un
écrivain OS hostile. Une révocation bloque la prochaine requête, pas un corps
HTTP déjà ouvert. Les lectures utilisent des chunks de 64Kio, sans chargement
intégral du film. Inventaire/cache du scan en mémoire proportionnels aux fichiers,
aucune preuve de très grande bibliothèque ou de RSS agrégée n'est revendiquée.
ffprobe garde timeout10s/sortie1Mio/protocoles/démuxeurs limités ; ce n'est pas une
sandbox de parseur hostile ni une limite de RSS globale.

Séries, NFO, illustrations, sous-titres externes, métadonnées en ligne, OIDC,
multi-Node, ordonnanceur et déploiement réel restent différés.

Les commandes et résultats exécutés sont consignés dans MEMORY.md et TEST_PLAN.md.
