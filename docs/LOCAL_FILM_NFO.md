> Extension P1 affiches locales Films : inventaire séparé, route image avec
> session/droits et repli Web. État actuel : [LOCAL_FILM_POSTERS.md](LOCAL_FILM_POSTERS.md).
> Les mentions historiques « images différées » ci-dessous sont remplacées pour les affiches Films.

# Métadonnées NFO Films locales — P1

Ce jalon enrichit le scanner Films existant sans changer le lecteur, les droits
ou les probes techniques. Aucun appel à un service de métadonnées.

## Découverte et priorités

Le Node recherche `<nom vidéo sans extension>.nfo` puis `movie.nfo`. Le premier
présent est déterminant : un NFO exact invalide ne provoque pas de basculement
silencieux sur un autre fichier. `movie.nfo` est accepté uniquement dans un
sous-dossier dont les vidéos correspondent à une seule identité locale, sans
contradiction explicite avec le dossier. Il est refusé à la racine de la
bibliothèque. Les noms suivent la sensibilité à la casse du système de fichiers.

Les métadonnées NFO ne servent jamais de clé de regroupement. Les Work, Editions,
MediaVersions et MediaSources existants gardent leurs identifiants. Deux versions
déjà associées partagent une fiche ; en cas de plusieurs NFO valides, sélection
déterministe par priorité exacte/partagée, chemin NFO, puis chemin vidéo.

## Données et actualisation

Champs : title, originaltitle, plot, outline, year, premiered, genres répétés,
rating valide entre0 et10, mpaa, uniqueid avec fournisseur explicite, balises
TMDb/IMDb reconnues (et id uniquement s'il porte un identifiant IMDb `tt…`).
Une date valide peut fournir l'année si year manque. Synopsis : plot, sinon
outline, sinon texte de repli. Aucune information absente n'est inventée.

Les titres/années suivent NFO valide → dossier fiable → nom de fichier. Les tags
techniques d'un NFO sont ignorés : codec, durée, langues et pistes viennent
exclusivement du probe. Les identifiants contradictoires entre NFO et repli local,
ou entre versions, sont signalés ; le fournisseur contradictoire est retiré des
identifiants publics. Aucun rapprochement automatique supplémentaire n'est fait.
Une contradiction à l'intérieur d'un NFO rend ce NFO invalide.

Migration additive `0005_film_editorial.sql`, sans modification de0004 : snapshot
des candidats éditoriaux par asset et JSON éditorial/provenance au niveau Work.
Les chemins des sidecars restent privés. La sélection au niveau Work intervient
dans la même transaction que l'inventaire, après toutes les vidéos découvertes.

Les petits NFO sont relus à chaque scan, indépendamment du cache vidéo : une
modification ou suppression est détectée même si taille/mtime vidéo ne changent
pas. Aucun nouvel appel ffprobe sur une vidéo valide inchangée.

- Valide : actualisation de l'instantané et de la fiche.
- Absent : suppression de l'instantané NFO et retour au repli local.
- Invalide/illisible : avertissement non bloquant et conservation du dernier
  instantané valide pour cet asset ; à défaut, repli local.
- Changement de racine ou d'identité locale : l'ancien instantané n'est pas repris.
- Média indisponible : métadonnées conservées ; aucun retour artificiel en lecture.

Les avertissements NFO sont séparés des erreurs média dans le rapport Node et
présentés dans les messages non bloquants du scan. Ils ne marquent pas la vidéo
indisponible. L'API catalogue ajoute originalTitle, plot, outline, premiered,
genres, rating, mpaa, externalIds, metadataProvenance (`nfo`/`local`) et
metadataWarnings. Aucun nouvel endpoint nécessaire.

## Sécurité

Parseur événementiel quick-xml0.42.0, sans résolveur externe : DTD rejetée,
entités inconnues/externes rejetées, aucune requête réseau ni ouverture de chemin
déduit du XML. Seules les entités XML standard et références numériques valides
sont décodées. Limites : fichier1Mio, profondeur16, champs16Kio,64 genres ; lecture
au plus1Mio+1 octet avant rejet. UTF-8 strict, BOM UTF-8 accepté ; autres encodages,
XML malformé et caractères interdits signalés sans arrêt de bibliothèque.

Le NFO passe par le confinement existant qui refuse symlinks et points de
réanalyse Windows. Les médias/NFO sont ouverts en lecture seule. La course
TOCTOU entre vérification et ouverture reste une limite : stockage et ACL sous
contrôle opérateur requis. Aucune écriture dans les bibliothèques.

Svelte affiche le contenu comme texte échappé, y compris les balises HTML
hostiles ; aucun `{@html}`, script ou chargement d'URL provenant du NFO.

## Vérifications et périmètre

Les tests utilisent exclusivement des copies synthétiques jetables. Les mutations
de NFO sont réalisées explicitement par les tests pour simuler l'opérateur ; les
octets sont comparés avant/après chaque scan. Le test navigateur dédié réalise
un vrai redémarrage du Core et conserve ses preuves dans `evidence/2026-10-16/nfo/`.
Résultats réellement exécutés : MEMORY.md et TEST_PLAN.md.

Séries/épisodes, affiches, sous-titres externes, métadonnées en ligne, multi-Node,
PostgreSQL, Docker/Synology et refonte UI restent hors de ce jalon.

Voir [Fiches, saisons, visuels et préférences privées](CATALOGUE_PRESENTATION.md) pour les extensions du 19 octobre 2026.
