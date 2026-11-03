# Recommandations personnelles locales

Ce lot ajoute un calcul facultatif par compte. Il reste distinct des contenus
similaires d'une fiche, qui rapprochent une œuvre d'autres œuvres sans utiliser
le compte. Aucun appel fournisseur, modèle d'IA, service externe ou profil de
goût persistant n'est requis.

## Parcours

Les réglages personnels proposent l'activation, l'exclusion des contenus déjà
vus et une fenêtre d'historique de 1 à 3 650 jours. L'activation est désactivée
par défaut. Les valeurs sont validées et conservées dans
`recommendation_preferences`, avec un enregistrement par compte et suppression
en cascade lorsque ce compte est supprimé. La fenêtre limite les signaux de
lecture utilisés dans le calcul ; elle ne supprime pas l'historique réel.
Les favoris et évaluations actuels restent utilisables indépendamment de cette
fenêtre. La désactivation vide la section au prochain rafraîchissement.

L'accueil charge les propositions du compte par
`GET /api/v1/account/recommendations`. Le frontend actualise au retour du focus
et après modification des réglages. La réponse est privée, sans cache HTTP ni
cache de résultats calculés côté Core. L'API de préférences est
`GET` / `PUT /api/v1/account/recommendations/preferences` ; `PUT` exige le jeton
CSRF de la session. Le compte ciblé est toujours celui de la session : aucun
`userId` client n'est accepté.

## Calcul borné et explicable

Le Core reprend le catalogue autorisé existant, y compris ses règles parentales,
en contournant le cache partagé de catalogue pour ce calcul dépendant du compte.
Il examine au maximum les 1 000 œuvres accessibles les plus récemment ajoutées,
et emploie au maximum 100 œuvres de ce même ensemble comme sources de signaux.
Une œuvre hors de cet ensemble ne reçoit pas de métadonnées supplémentaires
pour nourrir l'algorithme. Le résultat contient au maximum 16 œuvres, sans
doublons selon l'identité globale consolidée lorsqu'elle existe.

Les signaux sont uniquement les favoris, notes et lectures du compte courant.
Un favori ajoute un poids de 3 ; une note d'au moins 3,5/5 ajoute la moitié de
sa valeur. Une note d'au plus 2/5 exclut son œuvre des propositions et apporte
un signal négatif. Une œuvre vue récemment apporte un poids de 1 lorsqu'elle
n'a pas déjà de préférence explicite. L'exclusion « déjà vus » s'applique aussi
aux lectures plus anciennes que la fenêtre, lorsque ce choix est activé.

L'affinité utilise les genres, réalisateurs et acteurs déjà présents dans le
catalogue. Les poids respectifs sont 2, 4 et 1 par terme partagé, avec un plafond
de quatre termes par catégorie. Le classement retire ensuite 0,8 par genre
déjà représenté dans les choix précédents pour limiter les séries de cartes
trop semblables. Un petit signal de note éditoriale et l'identifiant stable
départagent les propositions. Aucun apprentissage ni estimation implicite de
qualité ou de probabilité de visionnage n'est présenté.

Chaque carte porte une raison courte, par exemple « Selon votre intérêt pour… ».
Le titre source ne peut être exposé que si les droits actuels permettent encore
de consulter cette œuvre. Les candidats sont également revérifiés avant retour
et les champs internes utilisés pour les explications sont retirés de la réponse.
Sans signal positif accessible, le mode `discovery` propose une découverte
locale avec une raison générique. Ce mode ne prétend pas avoir appris les goûts
d'un compte neuf. Les réponses fournissent `mode`, `inspectedCount`,
`candidateLimit` et `truncated` pour rendre visible la borne du calcul.

## Confidentialité, limites et validation

Le calcul ne crée aucune copie persistante de l'historique, des genres préférés
ou du classement. Les seules nouvelles données durables sont les trois
préférences. Les droits et le catalogue sont relus à chaque requête ; une
révocation de bibliothèque ou une modification des métadonnées prend effet au
calcul suivant sans délai de cache de recommandations.

Le plafond des 1 000 œuvres les plus récentes peut omettre des œuvres anciennes
pertinentes et leurs signaux. La diversification reste une pénalité simple de
genres, pas une garantie de représentation. La qualité de pertinence n'est pas
évaluée sur un corpus utilisateur annoté dans ce lot. Aucun gain de latence,
p95 de production ou bénéfice mesuré sur NAS n'est revendiqué.

Les tests unitaires de classement sont dans `recommendations.rs` ; le contrat
HTTP avec compte, CSRF, autorisations, historique et réouverture de SQLite est
dans `recommendation_tests.rs`. Leur présence ne prouve pas une exécution :
les commandes et résultats effectivement obtenus sont consignés dans la
[matrice de mission](MISSION_FEATURES_2026_11_03.md). Les mêmes tables SQL sont
installées de façon additive en SQLite/PostgreSQL ; le parcours PostgreSQL réel
reste à qualifier séparément s'il n'a pas été exécuté pour ce lot.

Le risque principal est un classement peu pertinent sur des métadonnées pauvres
ou un petit catalogue. Retour arrière : désactiver le réglage du compte ou
revenir au binaire précédent ; les tables additives et préférences peuvent être
conservées. Aucun média, favori, note ou historique existant n'est modifié par
le calcul. Prochaine priorité : valider la pertinence et le coût sur un corpus
représentatif avant d'étendre la taille du pool ou l'architecture.
