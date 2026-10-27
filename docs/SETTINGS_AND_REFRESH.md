# Général, réseau, avatars et actualisation — 19 octobre 2026

## Interface

Console avec barre latérale fixe sur bureau, marque/version, rubriques Général et Réseau,
et grille de médiathèques avec menu Gérer / Renommer / Actualiser / Planifier. Les formulaires
s’ouvrent dans des dialogues clavier natifs. La navigation mobile reste repliable.
Les cartes utilisent une représentation Films/Séries : aucune affiche distante ou image
de médiathèque fictive n’est présentée comme provenant des médias.

## Général et réseau

- `GET/PUT /api/v1/admin/settings` : administrateur. PUT demande origine et CSRF ; objet exact
  `{ "serverName": "...", "welcomeMessage": "..." }`. Nom non vide, 80 caractères maximum ; message
  500 caractères, texte simple. Corps maximal 4 Kio. Persisté dans la base et inclus dans ses backups.
- `GET /api/v1/server` : compte authentifié, nom du serveur et message. La marque produit dans
  `/api/v1/config` reste distincte du nom choisi pour l’installation.
- `GET /api/v1/admin/network` : administrateur ; origine effective du Core, cookies HTTPS, chemins
  Web/API. L’origine vient de `AuthState`, pas d’une supposition tirée des en-têtes de la requête.
- La page Réseau est en lecture seule pour le transport. MEDIA_PUBLIC_URL, les ports d’écoute et
  le proxy restent gérés par le déploiement au démarrage. Aucun port, accès distant ou réglage TLS
  n’est activé automatiquement par l’interface.

## Image de profil

`GET/HEAD/PUT/DELETE /api/v1/account/avatar` : compte authentifié courant uniquement. Aucun userId,
chemin disque ou URL distante dans le contrat. PUT/DELETE demandent origine et CSRF.

PNG/JPEG/WebP, au plus 2 Mio, dimensions d’entrée au plus 4096×4096 et allocation décodée au plus
32 Mio ; deux décodeurs simultanés maximum. Le contenu est décodé, recadré au centre et réencodé
en PNG 256×256. Les octets originaux et leurs métadonnées ne sont pas conservés. SVG et formats
inconnus refusés. L’image normalisée est stockée dans `user_avatars`, avec le compte, et incluse
par les sauvegardes de la base. GET/HEAD répondent en `image/png`, `private, no-store`, `nosniff`.
DELETE est idempotent. L’icône de compte reste le repli lorsque l’image est absente.

## Actualisation des bibliothèques

`POST /api/v1/admin/libraries/{id}/scan` accepte un corps optionnel de 4 Kio maximum :
`{ "mode": "incremental" }`. Sans corps, comportement incrémental historique conservé.
Les champs et valeurs inconnus sont refusés. Les autorisations administrateur/CSRF existantes
restent obligatoires. Le mode est fixé par Core dans le job ; le Node ne choisit pas sa politique.

| Mode | Effet réel |
| --- | --- |
| `incremental` | Découverte et vérification des fichiers/pistes ; probes inchangés réutilisés ; sidecars et NFO relus selon le comportement existant. |
| `new_only` | Nouveaux médias indexés ; métadonnées éditoriales des médias connus conservées. Les fichiers modifiés et leurs pistes restent vérifiés. Ce n’est pas une désactivation des contrôles d’inventaire. |
| `missing_metadata` | Médias sans NFO validé complétés depuis les NFO locaux disponibles ; snapshots NFO connus conservés, y compris films, séries et épisodes. |
| `full` | Caches de probes et d’images ignorés pour ce scan ; tous les fichiers réanalysés et NFO relus. Identifiants conservés lorsque l’identité/source correspond. |

Les pistes/sidecars, l’autorisation, la détection des fichiers disparus et le confinement ne sont
pas court-circuités par les choix éditoriaux. Une racine ou une clé d’identité éditoriale qui ne correspond plus
ne récupère pas silencieusement l’ancien snapshot éditorial. Un NFO corrompu conserve les règles
de repli précédentes. Le scan reste en lecture seule des médias.

En distribué, l’actualisation complète envoie des caches vides dans le contrat Node existant.
La politique éditoriale est appliquée à la publication Core, commune aux modes local et distant.
Les tâches périodiques gardent le mode incrémental ; aucun choix de mode planifié n’est prétendu.

## Préparation des futurs fournisseurs

Les choix de mode séparent déjà découverte technique et politique d’actualisation éditoriale.
Aujourd’hui « métadonnées manquantes » désigne l’absence de snapshot NFO validé, et non l’absence
de chaque champ d’une fiche. Aucun appel TMDB/TVDB/IMDb, clé de fournisseur, téléchargement ou
remplacement d’image à distance n’est implémenté ici. L’interface l’indique.

Un prochain fournisseur devra fournir provenance, identifiant exact et date d’acquisition ;
respecter la conservation/remplacement de métadonnées et les permissions réseau ; conserver les
identités Work/Edition/Source/Track et ne jamais écrire dans les médias par défaut. Les sources
NFO existantes doivent rester utilisables sans connexion externe.

## Vérifications exécutées

- fmt/clippy, suite `cargo test --workspace`, check/lint, 54 tests Web, build et diff-check réussis.
- Tests ciblés `nfo_flow`, `settings_avatars`, `avatars::tests` : modes Films/Séries/Épisodes,
  identifiants/médias préservés, trois formats avatar, limites, droits, CSRF, HEAD et réouverture.
- Test Web simulé sur deux thèmes et cinq largeurs, et `browser_admin` réel avec Core/SQLite
  temporaire : création/renommage, paramètres, accueil, réseau, image décodée/rechargée/supprimée,
  actualisation complète, comptes/groupes/clés/backups/préférences.
- Captures inspectées : `docs/evidence/2026-10-19/settings-expanded/`.

Les vérifications PostgreSQL réelles et les autres parcours navigateur ignorés ne sont pas
revendiqués pour ce jalon. Les tests utilisent des fixtures locales ; pas de validation NAS,
d’appareil physique ou d’un nouveau mode de décodage navigateur, ni de déploiement effectué.
# Navigation du tableau de bord (27 octobre 2026)

L'entrée Administration → Tableau de bord ouvre « Vue d’ensemble » lorsqu'aucune section n'est indiquée dans l'URL. Les liens directs vers une section continuent d'ouvrir cette section.

