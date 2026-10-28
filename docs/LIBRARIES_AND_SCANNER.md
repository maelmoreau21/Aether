# Bibliothèques et permissions P1 — 16 octobre 2026

Les scans vidéo et musicaux ignorent les sous-dossiers contenant `.nomedia`,
`.ignore` ou `.aetherignore`. Ces marqueurs restent des fichiers locaux en
lecture seule ; les exclusions configurées s'appliquent aussi aux deux types.

> Plusieurs dossiers et plusieurs nœuds dans une même bibliothèque :
> [contrat du 22 octobre 2026](LIBRARY_FOLDERS.md). Les descriptions historiques
> d'une seule racine ci-dessous décrivent le premier jalon.

Les métadonnées NFO Films sont maintenant traitées par le Node et persistées
sans modifier les droits ni le cache technique : [LOCAL_FILM_NFO.md](LOCAL_FILM_NFO.md).

## État actuel : scanner Films livré

Le premier scanner local, sa migration SQLite, les jobs et le parcours Web sont
décrits dans [LOCAL_FILM_SCANNER.md](LOCAL_FILM_SCANNER.md). Cette réalisation
succède au jalon de permissions sur fixtures décrit ci-dessous. Le mode
`MEDIA_LIBRARY_ROOT` autorise une bibliothèque locale existante, démarre vide et
ne charge aucune fixture expérimentale. Aucun média personnel n'a été utilisé
pour les validations, qui reposent sur des copies synthétiques jetables.

SQLite conserve Library (ID stable, nom, films/series, node_id local, racine,
enabled) et library_permissions. Interface Administration → Bibliothèques :
liste, création, modification, activation et remplacement atomique des grants.
Session administrator obligatoire ; Origin et CSRF pour POST/PUT. User ne lit
que ses bibliothèques actives. Administrator administre tout mais ne lit pas
les bibliothèques désactivées. Aucun compte ni rôle implicite.

Le Node valide les racines canoniques existantes dans MEDIA_FIXTURE_DIR uniquement
(la sélection Web propose cette racine, API accepte ses sous-dossiers).
Relatifs, composants .., fichiers, sorties de racine et symlinks/junctions vers
l'extérieur refusés. Aucun chemin HTTP arbitraire, aucun original modifié.
Core stocke configuration/droits ; Node conserve inventaire/I/O. Aucun scan
ni accès à un disque distant, aucun dossier NAS connecté.

fixture-films et fixture-series sont initialisées une seule fois sans grants.
Les nouvelles bibliothèques restent vides avant le scanner. Le catalogue
Work/Edition/Source reste une association synthétique explicite Rust ; seul
le registre des bibliothèques et leurs droits est persisté, pas un catalogue
importé complet. Chaque lecture vérifie SQLite, GET/HEAD/Range et URLs directes
inclus. Recherche filtrée avant pagination. Assets demo et sous-titres sont
explicitement partagés par Films et Séries (même mire) : un droit sur l'une
suffit pour ces octets. Le banc P0/MSE/DASH et ses descripteurs relèvent de Films.
Changer racine ou type retire l'association aux fixtures si incompatibles.
Un nouvel ID n'acquiert aucun asset en sélectionnant la même racine.
Illustrations statiques Web publiques ; aucun endpoint d'image privée encore.

Limites : localhost, fixtures seulement, pas de scanner/NFO/métadonnées en ligne,
transcodage ou Node distant. Canonicalisation puis ouverture non atomiques face
à un écrivain local hostile ; ACL opérateur requises. Une révocation bloque
les prochaines requêtes, sans interrompre un corps HTTP déjà ouvert.
Pas d'interface de création de comptes ajoutée à ce jalon.

Prochain jalon après validation : scanner local du Media Node indépendant de
Radarr/Sonarr ; ingestion contrôlée dans SQLite, associations asset/track/image
par bibliothèque, tests de confinement et fichiers inchangés. Dossiers manuels
et conventions ARR acceptables sans service ARR. NFO et images non fiables,
aucune récupération réseau implicite. Multi-Node/appairage en P2 seulement.
