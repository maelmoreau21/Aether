# Fiches, saisons et préférences personnelles

Les saisons retournées par le catalogue ont désormais des accès directs sous le titre de la série. Un clic sélectionne la saison et amène à ses épisodes. Le sélecteur détaillé reste disponible. Sans sélection valide, la première saison disponible est affichée, y compris les hors-série.

La navigation conserve une seule recherche, sous forme de bouton avec loupe. Le pied de page du catalogue est supprimé. La pagination affiche la page courante et le nombre de pages (24 éléments par type ; en recherche mixte, le plus grand nombre de pages des deux listes). Les dégradés utilisent le fond du thème actif. Le lien de retour sous la navigation principale est retiré des fiches ; les liens Films et Séries de la navigation restent disponibles.

## Images locales

Le scanner Node conserve les fichiers médias et leurs accompagnements en lecture seule. En plus de l'affiche, il découvre les images jpg/jpeg/png/webp suivantes :

- Arrière-plan : `backdrop`, puis `fanart`.
- Logo : `logo`, puis `clearlogo`.
- Illustration : `clearart`.
- Visuel horizontal : `landscape`, puis `banner`.
- Épisode : nom exact sans extension du fichier vidéo suivi de `-thumb`.
- Saison : `season01-poster`, `season02-poster`, etc. ; `season-specials-poster` pour la saison 0.

Pour les films, les noms partagés ne sont autorisés que dans un sous-dossier sans ambiguïté d’identité ; à la racine ou dans un dossier partagé entre plusieurs œuvres, les noms exacts `nom-video-backdrop`, `nom-video-logo`, etc. sont utilisés. Les collisions de noms restent refusées.

Les visuels généraux d'une série sont cherchés dans son dossier identifié par le scanner, les vignettes dans le dossier de l'épisode. Les variantes numérotées de backdrops ne constituent pas un diaporama. Les images subissent les mêmes validations de format, taille et confinement que les affiches, et les mêmes autorisations GET/HEAD. Les chemins disque ne sont jamais transmis au Web. Le protocole de scan transporte le champ optionnel `artwork` ; les anciens Nodes qui ne le transmettent pas conservent les affiches habituelles. Les nouveaux visuels nécessitent un scan après mise à jour du Node et du Core.

L'API catalogue expose `artwork` : rôles `backdrop`, `logo`, `landscape`, `illustration`, `thumb`, `seasonN` associés à des URL protégées. Les listes exposent l'arrière-plan et le logo ; le détail complète les autres rôles. L'absence d'une image garde le repli existant, sans téléchargement automatique.

## Métadonnées NFO et pistes

Les NFO locaux fournissent maintenant `actors` (nom/rôle, maximum 100), `directors` et `writers` (maximum 40 chacun), ainsi que `ratings` (maximum 20 : fournisseur, valeur et échelle native). Les anciennes limites XML restent applicables. Les notes de fournisseurs restent sur leur propre échelle ; le nom provient du NFO. Un `<thumb>` d'acteur en `https://` est conservé (les chemins locaux, notamment ceux de Jellyfin, sont ignorés). Un scan incrémental ou complet relit les NFO ; les modes conservant les métadonnées connues peuvent conserver les anciens champs.

### Portraits « Distribution & équipe »

Le catalogue accepte `GET /api/v1/catalog?person=<nom>&limit=24&offset=0`,
avec les mêmes permissions de bibliothèques, restrictions parentales, tri et
pagination que l'accueil. Le nom exact est comparé aux acteurs, réalisateurs et
scénaristes de chaque œuvre, avant pagination. Les données historiques ne donnant
pas toujours d'identifiant de personne, des homonymes peuvent être regroupés ;
ce n'est pas une filmographie extérieure au serveur.

TMDb peut transmettre les portraits et identifiants des membres d'équipe dans
`crewCredits`, en conservant les anciennes listes `directors` et `writers`. Le
Core expose ces portraits via le même proxy authentifié et borné que les acteurs,
en vérifiant que le portrait appartient bien aux crédits de l'œuvre consultée.
Ces nouveaux champs nécessitent le plugin TMDb mis à jour puis une actualisation
des métadonnées des œuvres concernées. Si les données ou l'hôte autorisé sont
absents, aucun portrait n'est attribué sur la seule base d'un nom homonyme.

Les portraits viennent des crédits fournis par une extension de métadonnées (TMDb : `profile_path`, taille `w185`) ou d'un `<thumb>` NFO en `https://`. Le navigateur ne charge jamais l'URL du fournisseur : le catalogue expose pour chaque crédit concerné un `imageUrl` servi par le Core ([contrat](API.md#portraits-de-la-distribution-4-novembre-2026)). Le premier affichage télécharge le portrait via un hôte déclaré par une extension autorisée pour la bibliothèque, le réencode en WebP borné (400×600) et le met en cache dans le magasin d'assets, partagé entre les œuvres. Les cartes chargent les portraits en différé, avec dimensions fixées ; des initiales restent affichées pendant le chargement, sans portrait ou en cas d'échec. Les portraits des réalisateurs et scénaristes sont disponibles lorsque les crédits d'équipe les contiennent ; sans identifiant fiable, aucun portrait n'est attribué à partir du seul nom. Les œuvres déjà identifiées avec TMDb peuvent déjà contenir les URL des portraits d'acteurs ; les nouveaux portraits d'équipe nécessitent une actualisation après mise à jour de TMDb. Les œuvres dont la distribution provient seulement d'un NFO doivent être rescannées (et leur NFO contenir des `<thumb>` en `https://`) ; une distribution importée sans portrait nécessite une nouvelle identification ou une actualisation des métadonnées. Les dossiers `.actors/` locaux de Kodi ne sont pas encore lus. Sans extension déclarant l'hôte du portrait (par exemple `image.tmdb.org`), les initiales sont conservées.

Les pistes audio affichent individuellement langue, codec, canaux et titre lorsqu'ils sont connus. Les sources préparées représentant une même piste interne ne la dupliquent plus. Les sources audio externes restent distinctes. Les sous-titres affichent la langue en français, un drapeau indicatif pour les langues prises en charge, le format et les marqueurs forcé/malentendants. Une langue absente est explicitement inconnue. La profondeur 10/12 bits n'est plus assimilée à du HDR.

## Notes et favoris privés

`GET` / `HEAD` et `PUT /api/v1/feedback/{workId}` utilisent exclusivement le compte de la session. PUT exige le jeton CSRF et accepte `{ "favorite": true, "rating": 4 }`. La note va de 0,5 à 5 par pas de 0,5, ou `null` pour la retirer. Le contrôle affiche un cœur et cinq étoiles : cliquer sur la moitié gauche sélectionne une demi-étoile, sur la moitié droite une étoile entière. Recliquer sur la note sélectionnée la retire. Les contrôles sont accessibles au clavier. L’aperçu de la note suit le survol et le focus ; le choix est affiché immédiatement. Les écritures sont sérialisées et les clics rapides sont regroupés pour conserver le dernier choix, sans désactiver toutes les étoiles à chaque aller-retour réseau. Un échec rétablit la dernière valeur confirmée et affiche une erreur. La migration 0030 conserve les anciennes notes entières dans les deux moteurs SQL. Les champs inconnus sont rejetés (422), le corps est limité à 2 Kio. Réponse `{ "favorite": bool, "rating": number|null }`, cache `private, no-store`. L'accès à la bibliothèque est revérifié à chaque requête, y compris après révocation. Sans session : 401 ; sans droit ou œuvre inconnue : 403.

La migration 0029 ajoute `work_feedback`, les rôles d'images et le stockage temporaire des visuels du scan, pour SQLite et PostgreSQL. Les préférences sont persistées par paire compte/œuvre, supprimées avec leur propriétaire ou l'œuvre, et absentes des réponses partagées du catalogue. Deux écritures concurrentes d'un même compte suivent la dernière écriture réussie. Aucun classement public ni moteur de recommandation n'est ajouté.

## Vérification

Voir les tests `series_flow`, `nfo_flow`, `media-node nfo::` et le scénario navigateur `browser_series` / `scripts/browser-series.cjs`. Les fixtures sont synthétiques ; ces résultats ne constituent pas un scan du NAS ou un déploiement sur aether.dfmag.fr. Les preuves exécutées pour ce jalon sont consignées dans .ai/MEMORY.md.

## Réactivité des fiches et du lecteur — 20 octobre 2026

Les images du Node local utilisent `private, no-cache` et un ETag. Un navigateur peut conserver les octets, mais doit revalider chaque réutilisation : session, droits de bibliothèque, confinement et empreinte du fichier sont contrôlés avant une réponse 304 sans corps. Les erreurs restent non stockables. Le relais distant conserve ses limites de transmission des en-têtes conditionnels ; aucun gain 304 distant n'est revendiqué.

Les images de saison et d'épisode ont des dimensions réservées et un décodage asynchrone. Une actualisation rapide après mise à jour du Core/Node découvre les visuels déjà présents (`season01-poster.jpg` dans le dossier série et `<nom exact vidéo>-thumb.jpg` dans le dossier épisode). Leur absence ne provoque aucun téléchargement externe.

La fiche film s'affiche dès réception du catalogue pendant le chargement des descripteurs de lecture. Au lancement, le lecteur natif charge directement la piste audio préférée ; il ne charge plus d'abord le français avant de sélectionner une autre piste. La progression de reprise est récupérée en parallèle du chargement média, puis appliquée avant lecture. La préparation facultative de sources reste susceptible de prendre du temps ; les fichiers nécessitant cette préparation ne deviennent pas du Direct Play.

Les quatre modes de scan restent disponibles pour préserver leurs comportements. Les libellés distinguent désormais l'actualisation rapide recommandée, la conservation des descriptions, le complément des descriptions manquantes et la réanalyse complète plus lente. Les explications précisent les effets sur les fichiers et les NFO locaux.

Les mesures sur fixtures locales ne prouvent pas les temps de chargement NAS, WAN ou de médias arbitraires.

## Actions compactes

Sur les fiches, le bouton « + » ouvre les listes existantes et la création d’une
liste avec ajout immédiat du média. Les outils administrateur de métadonnées
restent dans une action distincte avec une icône d’édition. Le téléchargement
utilise une icône et enregistre le fichier original avec le navigateur, sans
phrase ni nouvelle copie IndexedDB. Un choix n’apparaît que lorsque plusieurs
sources sont disponibles ; voir [PWA et téléchargements](PWA.md).
