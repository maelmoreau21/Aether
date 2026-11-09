# Demandes et Signalements — extensions officielles

Ces applications facultatives utilisent des écrans natifs et les comptes Aether.
Les paquets Wasm `media-requests` et `media-reports` sont développés dans
`Aether-Plugins`. Chacune doit être installée, activée, autorisée sur les
bibliothèques concernées et signée par un éditeur actif. Elles sont indépendantes.
Sans activation, leurs routes métier refusent les opérations et leurs entrées de
navigation sont masquées. Une désactivation conserve les dossiers persistants.

Les sources ne remplacent pas un paquet signé publié : les deux paquets `0.1.0`
sont présents dans le catalogue `develop`, et les changements ultérieurs doivent
passer par la publication signée d'une nouvelle version. La mise à jour d'un
paquet installé reste distincte du déploiement du Core. Aucun serveur existant ni
déploiement NAS n'est modifié par leur développement.

## Frontières

- Core : sessions/CSRF, droits, configuration des connexions, données
  transactionnelles, files persistantes et suivi de l'inventaire.
- Extensions : validation et plan d'acquisition (`requests.validate`,
  `requests.plan`), validation/classification des signalements
  (`reports.validate`, `reports.classify`). Les opérations pures s'exécutent dans
  un worker jetable, sans clé API, réseau, SQL ou accès aux médias.
- Radarr/Sonarr/Lidarr : recherche distante, surveillance, acquisition et import.
- Media Node : scan de son propre emplacement. Le Core ne parcourt pas les
  dossiers distants et ne modifie pas les fichiers des bibliothèques.

Les tables métier sont additives dans SQLite/PostgreSQL et séparées des données
du catalogue. Les rôles restent `administrator` et `user`.

## Demandes

`/requests` réunit recherche externe, création et suivi personnel ;
`/requests/admin` gère les serveurs, destinations, validation et migration.
L'administrateur voit toutes les demandes autorisées par les grants d'extension.

Plusieurs instances de chaque service sont configurables. Une destination lie
un dossier racine distant à un emplacement physique de bibliothèque Aether ;
leurs arborescences relatives doivent correspondre. Les profils et dossiers
proposés proviennent de l'API du serveur. Les chemins du serveur distant ne
deviennent jamais des chemins à scanner par le Core.

La validation manuelle est le défaut. L'administrateur peut activer l'approbation
automatique et régler la limite de demandes en attente par compte. Les demandes
de séries peuvent limiter saisons/épisodes ; les demandes musicales précisent
albums ou surveillance future. Une demande ne doit pas désactiver la surveillance
d'un contenu déjà suivi sur le serveur.

États : `pending`, `approved`, `submitted`, `acquiring`, `imported`, `partial`,
`available`, `rejected`, `cancelled`, `failed` et `suspended` pour l'historique importé.
`available` exige une présence confirmée dans l'inventaire courant ; une réponse
d'import du service distant ne suffit pas. Les scans empruntent le moteur de
tâches existant et ciblent le Node propriétaire de l'emplacement.

Une demande conserve son serveur. Aucun équilibrage de stockage ni basculement
automatique vers un autre serveur. Une création distante dont la réponse a été
perdue est réconciliée par identité avant un nouvel ajout. Un lancement de
recherche non confirmé produit une erreur explicite, sans rejeu aveugle.

Les morceaux isolés ne déclenchent jamais implicitement l'acquisition d'un album
complet. Un contenu absent ou non pris en charge produit un échec visible de
l'administrateur, avec un code distinct d'une panne réseau.
Une demande de morceau peut aboutir lorsque le Core prouve qu'un single ne
contient que cet enregistrement MusicBrainz, vérifie ses éditions dans Lidarr,
épingle une édition et relit cette sélection avant surveillance/recherche.
Le navigateur ne peut fournir cette preuve. Les réponses tronquées, plusieurs
groupes candidats, éditions inconnues ou à plusieurs pistes sont refusés ; les
bornes sont 100 sorties MusicBrainz et quatre éditions Lidarr. Aucun autre
fournisseur de téléchargement de morceaux n'est ajouté.

### Écran Demandes (refonte du 9 novembre 2026)

La recherche choisit le type par un sélecteur segmenté ; la destination n'est
proposée que lorsque plusieurs serveurs servent ce type. Chaque résultat affiche
année, genres, durée, chaîne, nombre de saisons et résumé (bornés par le Core ;
aucune URL d'image distante n'est transmise, seules les affiches servies par
Aether sont chargées). Le Core signale aussi :

- `inLibrary` : l'œuvre existe déjà dans une bibliothèque lisible (identifiant
  TMDb pour un film, TVDB pour une série) ; un film présent n'est plus proposé ;
- `request` : une demande active existe déjà pour ce contenu. Pour celle d'un
  autre compte, seul son état est indiqué, jamais son auteur ni son identifiant ;
- `onServer` : le service distant suit déjà ce contenu.

La demande s'ouvre dans une boîte de dialogue native : saisons cochées à partir
de la liste du serveur (épisodes spéciaux compris), épisodes précis en option,
suivi des épisodes à venir, et récapitulatif avant envoi.

La liste est triée par date de création (curseur `created_at:id`) et filtrable
par groupe d'états (`active`, `pending`, `done`, `attention`, `closed`), par type
et, pour l'administrateur, à ses seules demandes (`mine=true`). Elle renvoie des
compteurs par état, le nom du demandeur et une frise d'avancement. L'administrateur
peut approuver ou refuser plusieurs demandes en attente ; un refus porte un motif
fixe (`already_available`, `not_available`, `duplicate`, `out_of_scope`,
`storage`, `other`) visible du demandeur.

Les résultats externes n'ont pas toujours une classification fiable : les comptes
soumis à une politique parentale sont actuellement refusés sur cette recherche,
plutôt que d'exposer des titres sans classification vérifiée.

## Signalements

`/reports` permet de consulter ses signalements, commenter et suivre leur état.
Un utilisateur peut rouvrir son signalement résolu ou fermé ; l'administration
gère `open`, `in_progress`, `resolved`, `closed`.

Les boutons contextuels des fiches et lecteurs préremplissent l'œuvre, le morceau,
l'asset, la position et les identifiants de pistes connus. Le formulaire affiche
le contexte avant envoi. Le serveur valide la correspondance œuvre/asset/piste.
Les pistes internes utilisent leur identifiant d'inventaire ; les pistes externes
utilisent des références opaques stables, sans chemin privé. Une piste par défaut
ambiguë ne reçoit pas un identifiant inventé.

Catégories : vidéo, audio, sous-titres, métadonnées, fichier manquant, autre.
Les descriptions/commentaires sont du texte échappé. Aucun journal brut, secret
ou fichier média n'est joint automatiquement. Les limites sont 100 signalements
ouverts par auteur, 500 commentaires par signalement et 8 000 octets par texte.
Les faits et transitions sont structurés ; aucune réparation automatique n'est
exécutée dans cette version.

À la création, le Core appelle `reports.classify` et conserve, dans
`media_report_triage`, une priorité (`high`, `normal`, `low`) et des vérifications
suggérées (codes bornés, par exemple `file_presence` ou `subtitle_timing`). Le
texte libre n'est jamais interprété. Une extension occupée ou antérieure à 0.2.0
laisse la priorité `normal`. Seul l'administrateur voit ce tri, peut changer la
priorité (`POST /reports/{id}/priority`, sans notification à l'auteur) et filtrer
ou trier la file par priorité. La liste renvoie le média concerné (titre, année,
affiche Aether), le nom de l'auteur et des compteurs par état, plus `urgent`
(priorité haute non résolue) pour l'administrateur. Une transition peut porter
un message, enregistré comme commentaire dans la même transaction ; les réponses
de l'administration sont marquées dans le fil.

## Connexions et API

Le broker des connexions administrées est distinct du broker HTTPS public des
fournisseurs. Seuls l'hôte, le port et le préfixe configurés explicitement sont
joignables ; les adresses DNS sont épinglées et revérifiées, sans proxy implicite
ni redirection. HTTP est possible sur une connexion locale explicitement
configurée. Les adresses de métadonnées/link-local sont refusées.
Les clés restent côté Core, masquées dans les réponses, sans coffre chiffré
autonome supplémentaire. Sauvegardes et volume applicatif contiennent donc des
données sensibles, comme pour les autres connexions d'Aether.

Principales routes, sous `/api/v1` :

| Route | Usage |
|---|---|
| `GET /extensions/features` | Disponibilité des deux applications pour la session |
| `GET /requests/search` | Recherche par type et serveur |
| `GET, POST /requests` | Liste et création de demandes |
| `GET /requests/{id}` | Détail, sélection et historique |
| `POST /requests/{id}/action` | Valider, refuser, reprendre ou annuler |
| `GET, PUT /admin/acquisition/settings` | Validation et limites |
| `GET /admin/acquisition/locations` | Emplacements physiques des bibliothèques |
| `GET, POST /admin/acquisition/servers` | Configuration multi-serveurs |
| `PUT, DELETE /admin/acquisition/servers/{id}` | Modification/suppression de connexion |
| `GET /admin/acquisition/servers/{id}/options` | Profils et dossiers distants |
| `POST /admin/acquisition/servers/{id}/test` | Vérification de connexion |
| `GET, POST /reports` | Liste et création de signalements |
| `GET /reports/{id}` | Contexte, commentaires et historique |
| `POST /reports/{id}/comments` | Commenter |
| `POST /reports/{id}/status` | Transition d'état, avec message facultatif |
| `POST /reports/{id}/priority` | Priorité de traitement (administrateur) |
| `GET /admin/imports` | Progression des imports |
| `POST /admin/imports/preview` | Analyse d'une source, sans mutation de celle-ci |
| `POST /admin/imports/{id}/start` | Correspondances et démarrage/reprise |
| `POST /admin/imports/{id}/run` | Traitement d'une page bornée |
| `POST /admin/imports/{id}/pause` | Suspension persistante |

Les mutations exigent session, origine autorisée et CSRF. Les listes sont paginées.
Un accès individuel ne doit pas révéler les dossiers d'un autre utilisateur.

## Migration et bascule

L'importateur lit les API de demandes, utilisateurs, services et signalements
d'une installation source compatible, dont Seerr. Il ne reprend ni mots de passe
ni privilèges et ne modifie jamais cette installation. L'aperçu n'expose pas les
clés des services retournées par la source.

Les correspondances de comptes sont explicites. Les serveurs source utilisent
des clés telles que `radarr:1` et `sonarr:1`, car leurs identifiants numériques
peuvent coïncider. Les éléments non associés restent historiques ; un auteur
inconnu devient une étiquette historique, jamais un nouveau compte privilégié.

Les imports avancent par pages, avec curseur et lease persistants. Une pause est
revérifiée après les lectures réseau. Identité de source et identifiants d'origine
dédupliquent les relances. Les deltas ne remplacent pas les traitements natifs
déjà repris dans Aether. Les commentaires conservent auteurs, dates et portée
saison/épisode disponibles dans la source.

Toute demande importée reste `suspended` : aucun job ni notification n'est créé.
Son statut et son identifiant distant d'origine sont conservés séparément.
La reprise administrative exige une destination et un auteur valide, puis
réconcilie l'objet distant avant toute commande. Une disponibilité historique
n'est jamais présentée comme une preuve de présence dans Aether.

Pour basculer : suspendre les nouvelles demandes sur la source, importer un
dernier delta, vérifier associations/erreurs, puis reprendre les dossiers voulus
dans Aether. Conserver l'ancienne installation pendant cette vérification.

## Notifications et validation

Les préférences `requests` et `reports` de `/account/notifications` sont désactivées
par défaut. Les livraisons reprennent le pipeline existant et revérifient auteur,
droits, contrôle parental, grants et activation des extensions. Leur contenu ne
comprend que des références opaques et types d'événements, sans titre privé ni
description. Les événements d'import sont exclus.

Les tests ciblent permissions/CSRF, reprise et idempotence, serveurs simulés,
correspondances d'inventaire, import et interface. Ils ne prouvent pas à eux seuls
le comportement d'une installation réelle de Radarr/Sonarr/Lidarr ou d'un NAS.

### Vérifications locales du 4 novembre 2026

- Core : 14 tests `acquisition::` réussis, dont brokers, reprise après réponse
  perdue, inventaire courant et preuve de single. Deux tests d'import et un test
  d'audience/notifications réussis dans le même binaire de tests compilé.
- Signalements : quatre contrats HTTP réussis ; exécution répétée avec les vrais
  modules Wasm des extensions, signés par une clé de test temporaire. Les tests
  utilisant le worker partagent un verrou de test, car son admission est commune
  au processus même lorsque les bases SQLite sont indépendantes.
- `cargo clippy -p media-poc --locked --lib -- -D warnings`, `cargo fmt --all --
  --check` et `git diff --check` réussis.
- Web : Svelte sans erreur/avertissement, ESLint ciblé, cinq tests des helpers,
  build statique et `scripts/test-applications-ui.mjs` réussis. Chrome utilise des
  API simulées ; le scénario couvre configuration de destination physique,
  demande/sélection, reprise d'historique, signalement, migration et format mobile.
- Extensions : six tests natifs Demandes réussis, Clippy ciblé et compilation
  Wasm. Les deux modules passent les vérifications d'import/export et le smoke
  ABI ; tailles brutes de 155 887 et 125 663 octets sous le plafond du Core.

Pas de suite globale relancée, de fournisseur réel, de PostgreSQL réel, de
déploiement NAS ou de téléchargement réel validé dans ce lot.
