# Extensions : décision et prochain parcours vertical

> Mise à jour du 20 octobre 2026 : un premier parcours de paquets locaux signés
> et de propositions de métadonnées Wasm est validé localement sur SQLite et Chrome.
> Contrat et limites actuels : [EXTENSIONS_API.md](EXTENSIONS_API.md).
> Les sections ci-dessous conservent le projet initial; elles ne décrivent pas
> l'état courant de ce sous-ensemble. Les dépôts HTTPS et le catalogue sont
> décrits dans [EXTENSION_REPOSITORIES.md](EXTENSION_REPOSITORIES.md) ; extensions
> avancées et catalogue officiel distant publié restent futurs.

Décision du 19 octobre 2026, à la demande explicite de l'utilisateur : préparer
un écosystème d'extensions avec dépôts, catalogue, installation et permissions
accordées par l'administrateur, notamment pour les métadonnées et statistiques.
Cette demande remplace l'exclusion générale des plugins du périmètre exprimée
dans les documents antérieurs. Elle ne change pas la priorité de la lecture,
les frontières Core/Node, les médias en lecture seule ni le MVP à Core unique.

## État réel

La console décrit ce parcours. Aucun téléchargement, registre de dépôts,
installateur, magasin de secrets, moteur d'exécution ou gestion de permissions
pour extensions n'est implémenté à ce jalon. Les capacités ci-dessous sont un
contrat de conception à éprouver avant d'annoncer un système opérationnel.
Le planificateur actuel exécute uniquement les scans connus du Core ; il
n'accepte ni code d'extension ni commandes arbitraires.

## Parcours administrateur retenu

1. Ajouter un dépôt HTTPS et vérifier son identité d'éditeur et sa clé de signature.
2. Afficher un catalogue borné, avec descriptions en texte échappé, versions,
   compatibilité avec le protocole d'extension, provenance et droits demandés.
3. Examiner la version exacte et l'ensemble des capacités demandées. Choisir
   explicitement les bibliothèques, utilisateurs et domaines autorisés.
4. Télécharger en quarantaine dans le stockage applicatif, vérifier l'intégrité,
   la signature et la compatibilité avant installation. Une installation
   n'implique ni activation ni accord implicite pour tous les droits.
5. Activer après accord administrateur enregistré. Afficher les tâches,
   erreurs, appels réseau et changements de métadonnées imputables à l'extension.
6. Désactiver, révoquer les capacités, mettre à jour ou désinstaller. Une mise à
   jour élargissant les permissions attend un nouvel accord ; pas d'activation
   silencieuse avec les anciens consentements.

L'installation de paquets Jellyfin n'est pas promise : le serveur est Rust et
son ABI, son modèle de données et son moteur d'exécution seront distincts.
L'inspiration porte sur le parcours catalogue/dépôts et son administration.

## Capacités proposées, refusées par défaut

Les noms suivants décrivent un futur contrat versionné, pas des endpoints
actuellement utilisables. L'identité d'une extension et sa version doivent
accompagner chaque appel ; les contrôles finaux restent côté Core ou Node.

| Capacité | Portée et contrôle attendu |
| --- | --- |
| `metadata.read` | Fiches et identifiants techniques des bibliothèques explicitement accordées. Aucun chemin NAS ni secret. |
| `metadata.write` | Champs éditoriaux autorisés dans la base applicative ; provenance et journal des modifications. Aucune modification automatique des fichiers NFO, affiches ou médias d'origine. |
| `stats.aggregate.read` | Agrégats minimisés, seuils de regroupement à définir et interdiction de transformer une série de requêtes en historique individuel. |
| `stats.personal.read` | Historique/progression personnelle ; autorisation distincte, périmètre d'utilisateurs nommé et visibilité administrateur explicite. Jamais inclus implicitement dans les statistiques agrégées. |
| `network.request` | Domaines, ports, méthodes et redirections autorisés explicitement ; HTTPS par défaut, limites de taille/délai/débit. Résolution et redirections recontrôlées contre SSRF, y compris IP locales et métadonnées cloud. |
| `tasks.register` | Types de tâches déclarés dans le manifeste ; paramètres typés, fréquence bornée, limites de ressources, annulation, journal et protection contre le chevauchement. Aucun shell fourni par le navigateur. |
| `library.assets.read` | Assets inventoriés appartenant aux bibliothèques accordées, à travers des handles ou des opérations Node contrôlées ; aucune ouverture d'un chemin arbitraire. |
| `library.assets.write` | Option future distincte, désactivée par défaut et limitée à des opérations précises, réversibles lorsque possible. Ne découle jamais de `metadata.write`. |
| `library.assets.delete` | Option destructive future distincte, jamais incluse dans l'installation ou l'écriture. Accord explicite pour l'opération et la bibliothèque, journal et politique de récupération préalables. |

Les montages médias existants restent `:ro`. Autoriser une capacité dans le Core
ne modifie pas les montages du NAS et ne contourne pas leur protection. Toute
évolution vers des écritures de médias demanderait un jalon séparé, une
configuration opérateur explicite et des tests de sécurité et récupération.

## Frontière d'exécution

Le format de manifeste peut être conçu avant le choix du runtime, mais aucune
bibliothèque native arbitraire ne sera chargée dans le Core sur la seule foi
d'un catalogue. Évaluer un runtime Wasm/WASI avec accès hôte minimaux ou un
processus isolé, en mesurant la faisabilité des premiers fournisseurs de
métadonnées. Le choix reste à valider par un POC ; ne pas prétendre que Wasm
seul résout l'isolation, les limites de ressources ou les appels réseau.

Le runtime devra imposer quotas mémoire/CPU, temps maximal, taille d'entrée et
de sortie, contrôle des appels hôte et arrêt effectif. Les secrets fournisseurs
restent dans un stockage Core protégé et ne sont remis qu'à l'extension et au
domaine autorisés ; jamais au frontend, au manifeste ou aux logs. Pas d'accès
SQL brut, de session administrateur héritée ni de clé API générale injectée.

Une révocation doit prendre effet avant chaque nouvel appel hôte et annuler
les tâches concernées ; les traitements déjà partis et leurs garanties d'arrêt
doivent être testés et documentés. Une désinstallation conserve ou retire les
données applicatives de l'extension selon un choix explicite, sans toucher aux
médias. Aucun rollback annoncé sans démonstration des migrations de données.

## Dépôts, intégrité et confiance

- HTTPS protège le transport ; il ne prouve ni l'innocuité d'un paquet ni
  l'identité de son auteur au-delà du domaine contacté.
- Un checksum vérifie que l'archive correspond aux octets annoncés. Si dépôt
  et checksum sont compromis ensemble, il ne fournit aucune authenticité.
- Une signature vérifiée avec une clé d'éditeur préalablement approuvée atteste
  la provenance des octets ; elle ne garantit pas l'absence de vulnérabilité.
- Une clé publiée uniquement dans le même dépôt inconnu ne crée pas à elle
  seule une confiance indépendante. L'ajout, la rotation et la révocation de
  clés nécessitent une politique d'approbation explicite et un journal.
- Les noms et descriptions d'un dépôt restent des données non fiables. Le
  Core doit borner les catalogues/archives et bloquer traversées de chemins,
  symlinks, décompression abusive, redirections non autorisées et installations
  concurrentes d'une même extension.

Le journal d'audit doit relier administrateur, extension/version/digest, dépôt,
capacités accordées ou révoquées, activation et résultat de chaque opération.
Il ne doit pas contenir de secret ni d'historique personnel en clair par défaut.

## Premier petit jalon et critères d'acceptation

Commencer par un fournisseur de métadonnées synthétique, local et légal :
manifeste versionné, vérification d'une archive signée par une clé de test,
consentement `metadata.read` sur une seule bibliothèque et exécution bornée
renvoyant une proposition de fiche sans écrire dans les médias. Le catalogue
et l'installateur devront être reliés à ce parcours avant de multiplier les
catégories d'extensions.

Tests requis : mauvaise signature, mauvais digest, archive piégée, extension
incompatible, refus sans consentement, accès à une autre bibliothèque, révocation
pendant une tâche, dépassement de quota, mise à jour demandant de nouveaux
droits et absence de secrets dans les réponses/logs. Un test navigateur devra
prouver la lecture et l'accord des capacités par un administrateur. Les
statistiques personnelles et écritures destructives attendent leurs propres
preuves avant activation.
