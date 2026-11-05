# Mise à jour et retour arrière des extensions

## Installation volontaire et dépôts — 5 novembre 2026

Le démarrage du Core n'installe plus TMDb ou IMDb. Une désinstallation reste
effective après redémarrage ; les installations existantes et leurs réglages sont
conservés. Le dépôt officiel préconfiguré fournit un catalogue, sans installer ni
activer les extensions qu'il annonce.

À chaque ouverture d'Administration → Extensions, les dépôts sont actualisés
automatiquement, par lots de deux téléchargements. Un dépôt indisponible produit
un avertissement et conserve son dernier catalogue ; les autres sont actualisés.
Le bouton d'actualisation reste disponible.

L'ajout d'un dépôt propose les adresses officielles `main` et `develop`. Les liens
GitHub racine, `/tree/develop` ou `/blob/develop/repository/manifest.json` sont
convertis en adresse JSON `raw.githubusercontent.com` en conservant la branche.
Un dépôt `develop` choisi par l'administrateur n'est plus remplacé par `main`
au démarrage. Les paquets proposés restent uniquement ceux effectivement publiés
dans le manifeste de cette branche et soumis aux mêmes contrôles de signature,
compatibilité et confiance éditeur. Ajouter ce dépôt n'installe aucune extension.

## Contrat implémenté — 2 novembre 2026

Les routes `POST /api/v1/admin/extensions/{id}/upgrade` et installation avec
`upgrade:true` conservent paramètres, activation et bibliothèques. Elles exigent
le même ID, éditeur et clé de signature, une version numérique strictement
supérieure, un ABI/module compatible et aucun nouveau droit ou domaine réseau.
Une extension ne peut donc pas s'accorder de nouvelles permissions par mise à jour.
Un namespace conservé sous une autre identité d'éditeur ou clé bloque également
la migration : la mise à jour ne modifie jamais ces données inaccessibles.
Les champs de configuration facultatifs sont additifs ; les changements de type
et nouvelles valeurs obligatoires sans défaut restent refusés.

L'ancien paquet signé est aussi conservé dans `plugins/.revisions/<id>/`.
La source de vérité est la base : le cache disque est reconstructible et son
échec après commit ne défait pas la version installée.

## Migrations privées signées et bornées

Une extension ayant `storage.private` ou des données privées doit inclure dans
le manifeste du paquet cible :

```json
"storageMigration": {
  "fromVersion": "0.1.0",
  "keyRenames": [{"from":"ancienne-cle","to":"nouvelle-cle"}]
}
```

`fromVersion` doit correspondre exactement à la version installée. Une liste
vide déclare explicitement que le format privé reste compatible. Ce champ est
couvert par la signature ; les paquets historiques sans ce champ conservent
exactement leur entrée de signature existante.

Le Core accepte au plus 32 renommages de clés de 256 octets au maximum. Il les
applique dans toutes les portées du plugin, sans modifier valeur, empreinte,
expiration ou durabilité. Il renouvelle la version CAS des entrées renommées.
Doublons, cycles et destinations déjà occupées dans une même portée sont refusés.
Les transformations arbitraires de valeurs, SQL guest et migrations irréversibles
ne sont pas exposés : les changements de format au-delà de ces renommages
nécessitent un autre contrat explicite et restent refusés.

Tout paquet déclarant une migration doit répondre à l'opération
`lifecycle.verify` avec `{"apiVersion":1,"result":{"healthy":true}}`.
Ce contrôle s'exécute **avant mutation** dans un worker jetable, sans hôtes HTTP
ni broker de données privées, avec les limites Wasm existantes et un délai de
10 secondes. Un échec laisse l'installation intacte. Ce contrôle synthétique ne
valide pas un fournisseur externe ou la qualité de détection d'Intro Skipper.

Sous le verrou d'écriture partagé SQLite/PostgreSQL, le Core sauvegarde la
révision précédente, les paramètres, grants et tout le namespace privé
(KV, compteurs, objets et blocs) dans les tables `extension_update_*`, applique
la migration, change paquet et génération et écrit l'audit dans **une seule
transaction**. Une erreur abandonne aussi la sauvegarde préparée. Les anciens
workers ne peuvent plus écrire avec leur génération périmée.
Une seule sauvegarde, celle de la dernière mise à jour, est retenue par plugin.
La copie reste dans la base, sans charger les objets complets en mémoire Core ;
elle peut ajouter jusqu'au quota privé du plugin à la taille de la base et des
sauvegardes générales. Elle contient des secrets au même titre que les tables
actives et n'est jamais envoyée dans les réponses API.

## Restauration explicite

La fiche administrateur affiche la version restaurable et une confirmation
expliquant la perte des changements postérieurs. Elle appelle :

```http
POST /api/v1/admin/extensions/{id}/rollback
Content-Type: application/json

{"expectedSha256":"<digest actif>","expectedVersion":"0.2.0","confirmDataRestore":true}
```

RBAC administrateur, session et CSRF sont obligatoires. Le Core revérifie la
signature précédente et l'éditeur actif ; version/digest périmés et absence de
snapshot sont refusés. Paquet, paramètres, grants, compteurs, KV, objets et blocs
sont restaurés atomiquement avec une nouvelle génération. L'extension revient
**désactivée**, pour inspection et réactivation par l'administrateur. Le snapshot
consommé est supprimé. Une désinstallation nettoie aussi ces copies privées,
y compris `deleteData=false` ; les règles de conservation des données actives
restent celles du stockage privé.

Les deux schémas et la création idempotente sur réouverture sont alignés.
Les nouvelles tables sont ajoutées sans réécriture des tables médias.
Ce mécanisme n'est pas une sauvegarde générale du serveur ni un historique
illimité des versions.

## Versions depuis le catalogue et rétrogradation

La fiche présente la version active, les versions réellement annoncées par les
catalogues et la version restaurable du snapshot même si elle n’est plus publiée.
La date d’actualisation du dépôt ne prouve pas la date de publication d’une version.

Une version antérieure publiée peut être installée après confirmation explicite
via la route d’installation du dépôt avec `upgrade:true`, `confirmDowngrade:true`,
`expectedVersion` et `expectedSha256`. Le paquet reste signé, vérifié, compatible,
et lié au même éditeur et à la même clé ; aucun nouveau droit n’est accordé.
Le Core refuse une rétrogradation si un snapshot existe déjà, si une des versions
utilise le stockage privé ou si des données privées subsistent, si une migration
est déclarée ou si les schémas de paramètres ne sont pas identiques. La vérification
du snapshot et des données est répétée sous verrou de transaction. L’ancienne
version installée et les paramètres sont sauvegardés atomiquement pour un retour ;
la version choisie reste désactivée. Sans publication historique et sans snapshot,
aucune ancienne version n’est inventée.

## Installation réelle : constat en lecture seule

Le 2 novembre 2026, SSH et requêtes PostgreSQL `BEGIN READ ONLY` sur le NAS
montrent TMDb, IMDb, OMDb, fanart.tv et Intro Skipper installés en `0.1.1`.
Les quatre fournisseurs sont activés ; Intro Skipper est désactivé.
Dernières exécutions à 20 h 53 (Paris) : TMDb et fanart.tv `completed`, IMDb et
OMDb `error` avec code générique `extension_error`. Le détail fonctionnel des
résultats et la précision d'Intro ne sont donc pas validés par cette observation.
Aucun réglage, plugin, conteneur ni média du NAS n'a été modifié. Le nouveau
cycle de mise à jour/rollback n'a pas été exécuté sur cette installation.

## Vérification ciblée

Le scénario HTTP SQLite
`private_extension_update_migrates_atomically_and_rollback_restores_snapshot`
couvre santé refusée, conflit de destination sans changement de génération,
migration signée, paramètres conservés, snapshot présent après réouverture,
restauration des KV/objets/blocs, confirmation obligatoire, version périmée,
désactivation après rollback et refus de réutiliser un snapshot consommé.
Le test de refus historique couvre droits/domaines, signature/éditeur, ABI,
version égale ou inférieure et absence de déclaration de compatibilité privée.
Le dépôt Aether-Plugins conserve désormais `storageMigration` lors de la
projection du manifeste avant signature ; un test de contrat vérifie ce point.
Son modèle `templates/persistent-storage` répond à `lifecycle.verify` sans
incrémenter le compteur privé, et documente la déclaration de compatibilité.
Les résultats réellement exécutés sont consignés dans `.ai/MEMORY.md` ; le schéma
PostgreSQL aligné ne constitue pas une preuve d'exécution du nouveau rollback
sur PostgreSQL ni sur NAS.
