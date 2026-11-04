# Notifications — premier socle durable

Le Core réconcilie les faits terminaux de `durable_tasks` et les ajouts durables
de `inventory_works` vers une file de
notifications persistante. Les adaptateurs disponibles sont email SMTP TLS,
ntfy, Gotify et webhook signé. Aucun fournisseur n'est installé ni aucune
destination activée implicitement. Chaque compte doit activer ses préférences ;
l'administrateur configure et active les destinations associées aux comptes.

## Parcours et API

Les paramètres Sécurité contiennent `NotificationSettings.svelte`.
`GET|PUT /api/v1/account/notifications` concerne exclusivement le compte courant.
Les utilisateurs peuvent demander les scans terminés et nouveaux films, séries
et épisodes ; les événements de tâches
système sont réservés aux administrateurs. Une préférence client `tasks:true`
ne confère pas ce droit à un utilisateur.

`GET|POST /api/v1/admin/notifications/channels`,
`PUT|DELETE /api/v1/admin/notifications/channels/{id}` et
`GET /api/v1/admin/notifications/history` sont réservés aux administrateurs.
Les mutations exigent session, origine autorisée et CSRF. Les réponses de
configuration masquent le secret et indiquent seulement `hasSecret`.
Un secret omis lors d'une modification n'est conservé que si l'adaptateur et la
destination restent identiques ; changer de destination exige de le ressaisir.

Les événements raccordés sont `task.completed`, `task.failed`,
`scan.completed` et `content.added`. `node.unavailable` et `provider.error` sont
explicitement déclarés indisponibles. Ils ne sont pas simulés par l'interface.

## Données, audience et reprise

Les tables additives SQLite/PostgreSQL sont `notification_channels`,
`notification_preferences`, `notification_content_preferences`,
`notification_events` et `notification_deliveries`.
Un passage du worker traite au maximum 100 faits terminaux, 100 ajouts et une livraison.
Un identifiant déterministe lie tâche, tentative, état terminal et horodatage ;
l'unicité événement/destination empêche de réinsérer la même livraison.

Les événements contiennent uniquement identifiant, type, tâche, bibliothèque
éventuelle et date. Aucun titre, chemin média, message brut d'erreur fournisseur
ou secret n'est transmis. Avant l'envoi, le Core relit activation de la
destination, génération, compte, préférences et droits de bibliothèque vivants.
Un compte révoqué, désactivé ou ne disposant plus des droits ne reçoit pas le
scan. Une modification de configuration invalide les anciennes livraisons.

La nouvelle préférence `content` est désactivée par défaut. Son activation et
sa date de départ sont persistées de façon atomique avec les autres préférences.
L'ajout utilise `inventory_works.added_at`, sans scanner le disque ni créer un
événement par asset. Il concerne les œuvres dont un asset est disponible ; la
déduplication est par Work et date d'ajout, pas par rescan. Un simple changement
de métadonnées ne crée pas une annonce. L'horodatage est à la seconde : les
œuvres ajoutées pendant la même seconde que l'activation sont omises pour éviter
d'annoncer un ancien catalogue comme nouveau. La réactivation repart de sa date
courante ; le catalogue antérieur n'est pas annoncé rétroactivement.

Avant l'envoi d'un ajout, le Core revérifie l'existence et disponibilité du Work,
ses droits exacts de bibliothèque, ses parents actifs et son contrôle parental
actuel. Le repli vers un autre membre d'un groupe n'autorise pas la fuite d'un
identifiant source interdit. Le payload contient `workId` à la place de `taskId`
pour `content.added`, sans titre, métadonnée ou chemin. Le champ interne SQL
`notification_events.task_id` conserve la référence opaque de la source pour
compatibilité du schéma ; ce nom interne ne change pas le contrat public.
Une réimportation supprimant puis recréant un Work avec une nouvelle identité
peut produire une nouvelle annonce ; la limite de file et la cadence restent
applicables. Les autres préférences ne reçoivent pas les annonces de contenu
sans cet abonnement explicite.

La prise de travail est sérialisée en base et munie d'un jeton et d'un lease
de 30 secondes. La fin d'une tentative doit encore posséder ce jeton et son
lease. Les envois sont bornés à dix secondes, avec cinq essais au maximum,
backoff exponentiel et au moins une minute entre départs d'une destination.
Une livraison ambiguë après panne peut être répétée : c'est un contrat
**au moins une fois**, pas exactement une fois. Le récepteur doit dédupliquer
sur l'identifiant stable. Les webhooks portent aussi `Idempotency-Key`.

L'historique est conservé 30 jours. La réconciliation parcourt les faits des
sept derniers jours ; une tâche supprimée avant sa collecte ou une panne
prolongée au-delà de cette fenêtre peut empêcher sa notification. Ce premier
socle n'est pas une outbox insérée dans chaque transaction métier. La file est
bornée à 100 livraisons en attente par destination ; les livraisons dépassant
cette limite portent l'état `suppressed`, jamais `sent`.

## Réseau et secrets

ntfy, Gotify et webhooks exigent HTTPS sur le port 443, un hôte public,
aucun identifiant, paramètre secret ou fragment dans l'URL et aucune
redirection. La résolution DNS entière est vérifiée puis épinglée ; les proxys
implicites sont désactivés. Les destinations privées/LAN ne sont pas disponibles
dans ce parcours de production. Les tests utilisent un registre de destinations
locales compilé exclusivement avec `cfg(test)` ; aucune option publique ne
permet de l'activer.

SMTP utilise TLS implicite sur le port 465 avec certificat et nom du serveur
vérifiés. L'adresse IP est épinglée après vérification DNS publique ; le nom
d'origine reste utilisé pour TLS. Les adresses et en-têtes refusent les contrôles
et injections de lignes. STARTTLS/587 et relais SMTP privés ne sont pas proposés.

Les webhooks sont signés HMAC-SHA256 avec un secret d'au moins 32 caractères.
La signature porte exactement les octets UTF-8 de
`timestamp.eventId.` suivis du corps JSON transmis. En-têtes :
`X-Media-Timestamp`, `X-Media-Event-Id` et
`X-Media-Signature: sha256=<hex>`. Le récepteur vérifie la signature, une fenêtre
temporelle appropriée et l'absence de traitement antérieur du même événement.

Les secrets résident dans la base applicative et ses sauvegardes, accessibles
au service et aux administrateurs ; ce stockage n'est pas un coffre chiffré
autonome. Les erreurs conservées sont des codes bornés sans réponse distante.

## Validation et retour arrière

Les tests ciblés sont dans `notification_tests.rs`. Ils utilisent exclusivement
des récepteurs HTTP/SMTP locaux et des données synthétiques. Les résultats
réellement exécutés sont consignés dans la matrice de mission et le rapport de
validation Core ; la présence d'un test ne prouve pas son passage. Aucun email,
message ntfy/Gotify ou webhook externe réel n'est envoyé par ces vérifications.

Le lot `content.added` du 4 novembre ajoute un contrat partagé SQLite/PostgreSQL :
catalogue antérieur exclu, déduplication, contrôle parental, droits révoqués,
contenu retiré, opt-out, réouverture et payload minimal sur un récepteur local.
Le filtre notifications a passé **6/6** tests locaux (un test PostgreSQL ignoré
par ce filtre), puis le contrat PostgreSQL enrichi a passé **1/1** sur une base
jetable PostgreSQL 18. Les commandes, durées et limites sont consignées dans le
[rapport Core](MISSION_CORE_VALIDATION_2026_11_03.md). Le nouveau contrôle Web
a également été exercé dans Chrome avec API synthétique, séparément des tests
de persistance Core.

Pour revenir en arrière, désactiver les destinations et préférences puis
arrêter le nouveau worker avant de revenir au binaire précédent. Les tables
additives peuvent être conservées ; aucun média n'est modifié. Ne pas mélanger
anciens et nouveaux workers pour prétendre conserver le fencing. Le multi-Core
global reste expérimental tant que ses autres invariants ne sont pas garantis.

## Prochains événements : audit du 4 novembre, sans implémentation

`media_nodes.status` et `last_seen_at` ne sont pas un journal durable des
transitions. Déduire une panne de l'âge du heartbeat inventerait des événements,
notamment lorsque la détection d'obsolescence est désactivée. Il faut enregistrer
et dédupliquer une transition réelle, initialiser sans alerte rétroactive et
revérifier la panne avant livraison aux seuls administrateurs abonnés.

Pour les fournisseurs, `extension_error` englobe aussi la configuration et les
entrées invalides : ce code seul ne prouve pas une erreur de service. Le prochain
lot doit retenir uniquement des codes fournisseur bornés après vraie invocation,
exclure annulations/refus de droits et produire un fait minimal durable. Attention :
l'automatisation conserve une transaction d'écriture SQLite durant le worker ;
une insertion via un autre pool à cet endroit risquerait un interblocage. Le
point de collecte doit respecter cette transaction, avec un test du vrai worker.
Aucun de ces deux événements n'est activé par cette note.
