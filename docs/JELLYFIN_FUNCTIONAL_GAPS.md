# Comparaison fonctionnelle Jellyfin / Aether — 2 novembre 2026

Comparaison ciblée avec les sources locales de `C:/Users/Mael/Downloads/Jellyfin`,
consultées en lecture seule. Ce document ne prétend pas établir une parité complète.
Les parcours et options servent de référence fonctionnelle ; les changements
Aether sont écrits pour son architecture Rust/Core/Media Node/Svelte.

## Lot réalisé dans le code

| Domaine | Référence locale Jellyfin | Changement Aether | Contrat détaillé |
| --- | --- | --- | --- |
| Lecture personnelle | `jellyfin-master/MediaBrowser.Model/Configuration/UserConfiguration.cs` et `jellyfin-web-master/src/components/playbackSettings/` | Enchaînement automatique enregistré par compte ; sauts arrière et avant de 5, 10, 15, 30 ou 60 secondes pour boutons et clavier. Activation de l'enchaînement volontaire, désactivée par défaut ; séances SyncPlay exclues de cet enchaînement individuel. | [Préférences](PREFERENCES.md) |
| Reprise serveur | `jellyfin-master/MediaBrowser.Model/Configuration/ServerConfiguration.cs` et `Emby.Server.Implementations/Library/UserDataManager.cs` | Les seuils existants sont appliqués à l'écriture de progression par le Core, pour tous les clients. Les commandes explicites vu/non-vu restent disponibles. Un contenu trop court ne devient pas automatiquement vu. | [Réglages et reprise](SETTINGS_AND_REFRESH.md) |
| Relance Intro Skipper | `intro-skipper-12.0/IntroSkipper/Configuration/PluginConfiguration.cs` et parcours d'analyse du plugin local | Une analyse manuelle réutilise la tâche d'empreinte terminée et peut réarmer la détection terminée, sans dupliquer une détection déjà active. Les bibliothèques désactivées et Nodes révoqués sont exclus du déclenchement. | [Intro Skipper](INTRO_SKIPPER.md) |
| Accueil personnel | `UserConfiguration.HidePlayedInLatest` | Préférence pour masquer les œuvres explicitement marquées vues dans les nouveautés. Filtre Core avant pagination et sélection des copies regroupées ; requêtes d'accueil réellement triées par date d'ajout. | [Préférences](PREFERENCES.md) |
| Mémoire des pistes | `RememberAudioSelections`, `RememberSubtitleSelections`, `PlayDefaultAudioTrack` | Politiques personnelles appliquées à la sélection initiale et à l'enregistrement des choix réels ; choix manuel prioritaire pendant la séance. | [Préférences](PREFERENCES.md) |
| Journal serveur | `ServerConfiguration.ActivityLogRetentionDays` | Purge bornée du journal durable existant des extensions ; séparation corrigée entre rétention du journal et expiration des sessions. | [Réglages](SETTINGS_AND_REFRESH.md) |
| Réglages Intro Skipper | `intro-skipper-12.0/IntroSkipper/Configuration/PluginConfiguration.cs` | Début de fenêtre acoustique et durée maximale des crédits effectivement consommés ; validation avant analyse et respect de la désactivation des chapitres. Nécessite un paquet plugin reconstruit et installé. | [Intro Skipper](INTRO_SKIPPER.md) |

Les tests exécutés et leurs résultats sont consignés dans `MEMORY.md`.
Un test d'interface avec API simulées ne démontre ni la persistance réelle du
Core ni le décodage d'un média. Les tests HTTP SQLite ne démontrent pas PostgreSQL
ou le fonctionnement sur NAS.

## Prochains lots à construire et vérifier

Ces lignes sont des travaux ouverts, à préciser par inspection de leur parcours
complet avant implémentation. Elles ne représentent pas des options livrées.

| Priorité | Lot | Critère d'acceptation |
| --- | --- | --- |
| 1 | Intro Skipper sur médias représentatifs | Analyse sur le Node propriétaire, extraction réelle, exécution Wasm, persistance, correction manuelle conservée et saut lecteur vérifiés ensemble ; distinguer cold open, recap et scène après générique. Mesurer les erreurs sur un corpus autorisé. |
| 2 | Suite des réglages de détection Intro Skipper | Politiques par bibliothèque/saison, état de la file, corpus acoustique et raccordement du paquet installé ; les nouveaux champs seuls ne démontrent pas ce parcours réel. |
| 3 | Suite de l'accueil personnel | Ordre et exclusion des vues/bibliothèques, calcul de série entièrement vue à partir des épisodes autorisés. Le masquage actuel utilise le statut explicitement enregistré sur l'œuvre. |
| 4 | Collecte durable et rotation des images | Curseur persistant, exécution fournisseur avec lease, reprise après arrêt et statut administratif ; la collecte manuelle actuelle ne suffit pas à démontrer ces propriétés. |
| 5 | Vérification des réglages serveur existants | Suivre chaque champ depuis `server_settings.rs` jusqu'au consommateur : rétention du journal, paramètres trickplay, langues/pays des métadonnées et réseau. Vérifier effet, restauration et limites ; aucun champ uniquement stocké ne doit être présenté comme actif. |
| 6 | Validation des choix de pistes sur bibliothèque réelle | Vérifier les politiques sur différentes éditions, langues, pistes externes et changements de compte ; ne pas confondre sélection logicielle et décodage effectivement audible. |

Les routes Jellyfin `apps/dashboard/routes/playback/`, `tasks/`, `activity/` et
`users/`, ainsi que `components/displaySettings/`, donnent les entrées du prochain
inventaire. L'ajout de transcodage vidéo obligatoire, de DLNA ou de télévision en
direct exige un périmètre distinct et ne fait pas partie de ce lot.
