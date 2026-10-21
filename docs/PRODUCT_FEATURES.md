# Fonctions produit — 20 octobre 2026

L’utilisateur a autorisé l’ensemble de ce lot, sans arrêt entre les étapes.
Les bibliothèques et fichiers existants restent en lecture seule. Aucun nouveau rôle.

## Parcours

| Fonction | Parcours réalisé | Limite de portée |
| --- | --- | --- |
| PWA | Installation via le navigateur, manifeste issu de la marque du build, icônes 192/512, affichage autonome | HTTPS ou localhost ; installation sur appareil physique à vérifier |
| Musique | Administration → Bibliothèques → Musique, scan, navigation Musique, recherche titre/artiste/album, lecture, piste suivante | Tags réellement présents ; noms de repli explicites, pas de fournisseur externe ni de pochettes extraites |
| Musique distante | Scan sur le Node propriétaire, inventaire signé et lecture avec les droits de bibliothèque | Relais Core pour la musique ; pas de serveur musical externe requis |
| Compte | Paramètres → Compte : changement du mot de passe actuel, sessions et révocation ciblée/globale | Une entrée est une session et son User-Agent déclaré, pas une identité matérielle vérifiée ; SSO renvoie au fournisseur |
| Restrictions parentales | Administration → Utilisateurs → modifier → restrictions ; âge minimum modifiable dans une fiche | Politique serveur ; médias non classifiés bloqués par défaut, option administrateur pour les autoriser |
| Préférences de pistes | Stockage par compte, rechargement sur un autre appareil, actualisation au focus et protection contre les lectures retardées | Ne change pas brutalement une piste pendant une lecture en cours ; pas de notification réseau poussée |
| Meilleure version | Choix automatique à l’ouverture, puis choix manuel ; résolution et déclarations codec du navigateur, disponibilité, préférence local/direct/relais à qualité égale | Déclaration du navigateur distincte d’un décodage observé ; aucun débit WAN deviné ni basculement transparent promis en plein flux |
| Chapitres | Chapitres ffprobe, timestamps et titre, sélecteur clavier dans le lecteur | Un scan complet est nécessaire pour compléter les anciens probes en cache |
| Filtres | Genre, année, qualité minimum, codec et langue, combinés avant total/pagination | Les langues correspondent aux tags techniques indexés ; les données absentes ne sont pas inventées |
| Identification et métadonnées | Fiche → Listes et gestion du média → titre, année, description, genres, identifiants externes, âge minimum ; retour aux données du scan | Surcharge applicative persistante ; aucun NFO ni média original modifié |
| Affiches | Téléversement PNG/JPEG/WebP normalisé dans la DB, suppression de la surcharge | 2 Mio en entrée, décodeur borné, dimensions maximales 1000×1500 en conservant le rapport |
| Copies multi-Node | Association explicite de deux identifiants de film, identité globale, versions réunies, meilleure copie disponible | Aucun rapprochement automatique par titre ; le premier film choisi définit l’identité et l’état personnel communs, les anciens états par copie ne sont pas supprimés |
| Listes/watchlists | Mes listes : création, ajout depuis une fiche, retrait, suppression ; propriété et droits vivants | Listes privées ; les œuvres devenues interdites disparaissent du contenu présenté |
| Cache après crash | Récupération au démarrage et à la prochaine préparation ; répertoires versionnés avec leases/verrous OS | Seules les entrées identifiées de la nouvelle version sont récupérées ; anciens dossiers temporaires sans marqueur exclus |
| Préparation distante | Copie AV1/Opus et conversion des sous-titres sur le Node propriétaire, stream via transport signé entrant ou canal sortant | FFmpeg facultatif requis pour la préparation ; aucun réencodage vidéo ajouté ; ASS/SSA en texte, pas de rendu stylisé promis |
| Téléchargement | Icône et téléchargement natif du fichier original ; choix uniquement si plusieurs versions | Nom proposé selon titre/format ; progression et espace disque gérés par le navigateur. Aucun mot de passe ou stockage IndexedDB ajouté |
| Anciennes copies hors ligne | Page disponible sans réseau, lecture/seek/Range, récupération des copies à phrase et suppression durable | Compatibilité du navigateur ; copies IndexedDB existantes conservées, aucun remux hors ligne |

## Frontières de sécurité

- Le mot de passe personnel exige le secret actuel, CSRF, un budget de tentatives
  et des workers Argon2id bornés. La mise à jour revalide la session et le hash
  courant dans une transaction ; toutes les sessions sont révoquées.
- Les identifiants de sessions présentés ne sont ni les cookies ni leurs hashes
  utilisables par les mécanismes internes. Révocation limitée au propriétaire.
- Les restrictions sont appliquées au catalogue avant pagination, aux fiches,
  assets, affiches, pistes préparées, retours de listes/progression et à la
  validation vivante des jetons directs des Nodes. Les intégrations par clé API
  héritent de la politique du compte. La musique sans classification est exclue
  si ce compte interdit les médias non classifiés.
- L’âge explicite est prioritaire, puis l’âge explicite de la série. La conversion
  limitée des classifications textuelles est définie dans `product::AGE_SQL` et
  peut être remplacée manuellement ; une valeur non reconnue reste inconnue.
- L’association des copies ne donne aucun droit sur les bibliothèques du groupe.
  Les versions interdites ne sont pas ajoutées à la fiche. Favoris et progression
  sont partagés par l’identité choisie ; chaque requête autorise d’abord sa copie.
- Cache de préparation : un répertoire privé contient un verrou d’allocation et
  des entrées louées. Un processus actif garde sa lease. Un arrêt brutal libère le
  verrou OS ; la récupération supprime l’entrée orpheline, sans parcourir les médias.
- Téléchargement : lien de fichier protégé de même origine avec attribut
  `download`, sans charger tout le média dans un Blob en mémoire. Le navigateur
  enregistre le fichier original ; l’API applique toujours les droits serveur.
- Anciennes copies hors ligne : seuls le HTML public de la page Téléchargements et
  les assets du build sont placés dans CacheStorage. Les médias déjà enregistrés
  dans IndexedDB et leurs clés restent lisibles ; ceux à phrase conservent leur
  phrase d’origine. Une copie déjà téléchargée reste lisible après révocation ;
  suppression locale nécessaire sur appareil partagé. La disparition de la carte
  dans l’interface suit la suppression durable des blocs et de sa clé.

## Contrats HTTP

Toutes les mutations de session exigent `Origin` et `X-CSRF-Token` existants.
Les erreurs de droits ne sont jamais remplacées par une réponse de cache média.

- `PUT /api/v1/account/password` : `{currentPassword,newPassword}` ; 204 et
  déconnexion, 403 secret actuel/CSRF incorrect, 409 SSO ou changement concurrent,
  429 budget/workers. La nouvelle valeur suit la règle existante 15–128 caractères.
- `GET /api/v1/account/sessions` : `{sessions:[{id,device,current,createdAt,lastSeenAt,expiresAt}]}`,
  au plus 200 sessions actives récentes. `DELETE .../sessions/{id}` et
  `DELETE .../sessions` : révocation individuelle ou de toutes les sessions.
- `GET /api/v1/music?q=&offset=&limit=` : pistes et total, limite 1–200.
  `GET/HEAD /api/v1/music/tracks/{id}/content` : fichier audio, Range/416 et
  vérification du fichier local ou relais Node.
- `GET/POST /api/v1/lists`, corps de création `{name}` ;
  `GET/DELETE /api/v1/lists/{id}`, `PUT/DELETE /api/v1/lists/{id}/items/{work}`.
- `PUT /api/v1/admin/works/{work}/metadata` :
  `{title,year,plot,genres,externalIds,minimumAge}` ; `DELETE` retire la surcharge.
- `GET /api/v1/works/{work}/poster`, `PUT` octets d’image administrateur,
  `DELETE` administrateur ; la lecture exige session, droits et politique parentale.
- `GET/PUT/DELETE /api/v1/admin/users/{user}/parental`, corps de modification
  `{maximumAge,allowUnrated}` ; entier 0–21.
- `POST /api/v1/admin/media-groups` : `{workIds:[id1,id2,...],label}`,
  2–32 films distincts ; `DELETE /api/v1/admin/media-groups/{work}` dissocie la copie.
- `GET /api/v1/catalog` accepte aussi `genre`, `year`, `quality` (hauteur minimum),
  `codec`, `language`. Totaux et pagination utilisent les mêmes conditions.
- Descripteur média : `chapters:[{start,title}]` ; sources préparées aussi pour les
  Nodes distants. La préparation reste résolue par asset et piste, sans chemin client.
- Interne signé : `POST /internal/v1/core/music/tracks` transporte une piste et
  l’identité du scan actif ; complétion/synchronisation réutilisent le protocole Node.
  `GET/HEAD /internal/v1/node/assets/{id}/prepared/{component}` pour le transport entrant.
  Le relais sortant conserve HEAD et ses longueurs sans téléverser le corps média.

## Vérifications

Les résultats définitifs et limites d’environnement sont consignés dans `MEMORY.md`.
Les tests du lot sont `product_tests`, `account`, `distributed_multi_node`,
`node_enrollment_and_relay`, `cache_dirs`, `browser_series`, les tests Web et
`apps/web/scripts/test-pwa.mjs`. Les scénarios multi-Node locaux ne sont pas une
preuve NAS/WAN ni un essai sur deux machines physiques. L’installation mobile et
PostgreSQL réel nécessitent leurs environnements ; ne pas les déduire des tests SQLite.
