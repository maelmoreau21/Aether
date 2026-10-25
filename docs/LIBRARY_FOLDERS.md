# Bibliothèques avec plusieurs dossiers

L'administration utilise le nom « Bibliothèques ». Une bibliothèque, par exemple
`Films_UHD`, regroupe jusqu'à 32 dossiers, chacun avec son serveur et sa racine
autorisée. Le nom, le type, les exclusions, les options et les permissions sont
communs. L'interface montre les dossiers ajoutés et permet de les retirer.
L'ajout d'un dossier ne déclenche pas automatiquement un scan.

## Éditeur et navigation — 25 octobre 2026

L'éditeur utilise une fenêtre large, adaptée au téléphone, avec les actions
d'enregistrement accessibles pendant le défilement. Le sélecteur propose
« Dossier parent » et « Racines / lecteurs ». Une navigation ne réinitialise
plus le chemin vers le dossier de départ (notamment le dossier temporaire
vide du Core). Un dossier vide peut être sélectionné ; seul un chemin résolu
sans erreur peut être confirmé. Les demandes précédentes sont annulées lors
d'une nouvelle navigation.

Les chemins sont ceux du serveur choisi. Dans Docker, il faut monter les
dossiers médias dans le conteneur ; le navigateur ne donne pas accès aux
dossiers de l'ordinateur client. Les restrictions de racines du Node continuent
de s'appliquer. Aucun droit disque supplémentaire n'est ajouté par l'interface.
Pour un Node distant, l'API actuelle présente ses racines déclarées uniquement :
la navigation dans ses sous-dossiers n'est pas fournie par ce correctif.

## Contrat HTTP

`POST /api/v1/admin/libraries` et `PUT /api/v1/admin/libraries/{id}` acceptent :

```json
{
  "name": "Films_UHD",
  "kind": "films",
  "enabled": true,
  "folders": [
    { "nodeId": "local", "root": "/media/films-uhd" },
    { "nodeId": "node-id", "root": "/media/films-uhd" }
  ],
  "userIds": []
}
```

Session administrateur, Origin et CSRF restent obligatoires. Chaque dossier est
validé selon les racines de son propre nœud. Liste vide, doublon, plus de 32 dossiers,
nœud révoqué ou racine non autorisée : 400. Modification pendant un scan : 409.
La configuration et les rattachements sont enregistrés dans une transaction.

Les anciens champs `nodeId` et `root` désignent le premier dossier. Sans `folders`,
une mise à jour conserve les dossiers supplémentaires déjà configurés. Les réponses
d'administration exposent `folders`. Les listes utilisateur et administrateur
présentent une seule bibliothèque ; les scopes physiques internes ne sont pas
des bibliothèques que l'utilisateur doit configurer ou autoriser séparément.

`POST .../{id}/scan` lance les scans des dossiers. `GET .../{id}/scan` agrège les
compteurs et les erreurs ; une exécution active prime sur l'état final d'erreur.
La planification manuelle et l'actualisation automatique utilisent ce même chemin.
Le filtre `/api/v1/catalog?library={id}` couvre les dossiers autorisés du groupe,
avant pagination ; `libraryId` dans les œuvres renvoie la bibliothèque commune.
La musique peut également être filtrée par la bibliothèque commune.

## Persistance et limites

Migration 0040 disponible pour SQLite et PostgreSQL. Les inventaires physiques
conservent leurs racines, nœuds, pistes et vérifications de lecture. Les permissions
effectives sont héritées du parent, y compris les groupes et le groupe par défaut.
Retirer un dossier désactive son scope et retire son accès effectif ; aucun média
n'est supprimé. Désactiver une bibliothèque désactive tous ses scopes.

Les anciens scopes retirés restent en base pour conserver l'inventaire. Réajouter
le même dossier supplémentaire réutilise son identifiant. Changer le premier
dossier a les mêmes conséquences que changer l'ancienne racine principale :
les anciens instantanés ne sont plus lisibles et un scan est nécessaire.
Deux dossiers contenant la même œuvre ne sont pas fusionnés automatiquement ;
le regroupement de versions existant reste une opération distincte.

Les mesures navigateur et les tests locaux ne prouvent pas un déploiement NAS,
deux machines physiques ou une nouvelle CI GitHub.
