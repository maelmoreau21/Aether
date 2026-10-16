> Extension P1 affiches locales Films : inventaire séparé, route image avec
> session/droits et repli Web. État actuel : [LOCAL_FILM_POSTERS.md](LOCAL_FILM_POSTERS.md).
> Les mentions historiques « images différées » ci-dessous sont remplacées pour les affiches Films.

> Mise à jour scanner Films P1 : inventaire SQLite et bibliothèques locales
> autorisées par MEDIA_LIBRARY_ROOT. Jobs session/rôle/CSRF, contrôle des droits
> avant GET/HEAD/Range, refus liens et points de réanalyse Windows. Confinement
> non atomique : risque TOCTOU résiduel et ACL opérateur nécessaires ; pas de
> publication LAN/NAS. Voir [LOCAL_FILM_SCANNER.md](LOCAL_FILM_SCANNER.md).
>
> Mise à jour P1 bibliothèques : droits explicites SQLite et administration
> session/rôle/CSRF livrés sur fixtures. Les restrictions administrator seule
> ci-dessous sont historiques. Voir [LIBRARIES_AND_SCANNER.md](LIBRARIES_AND_SCANNER.md).

# Sécurité du Core local et historique du banc P0

NFO Films : XML local non fiable, quick-xml sans résolveur, DTD/entités externes
rejetées, UTF-8 strict, limites1Mio/profondeur16/champs16Kio/64genres, confinement
symlink/reparse existant et rendu texte échappé. Aucun champ ne devient chemin
ou URL consulté ; aucune modification des médias/NFO. Le risque TOCTOU reste
explicite. Détails et tests : [LOCAL_FILM_NFO.md](LOCAL_FILM_NFO.md).

## État actuel — 16 octobre 2026

Le binaire exige désormais une session SQLite et une permission backend pour
les routes catalogue et média, y compris HEAD, Range, sous-titres et expériences
MSE/DASH. Connexion Argon2id, sessions hachées/révocables, expirations, CSRF et
limitation persistante des tentatives : [LOCAL_SESSIONS.md](LOCAL_SESSIONS.md).
Seul l'administrateur accède à l'inventaire synthétique ; `user` est refusé
jusqu'au prochain jalon de permissions explicites par bibliothèque.
Le texte ci-dessous décrit les jalons historiques et leurs limites persistantes
(notamment fixtures uniquement et confinement non atomique). Ses mentions
« aucune authentification » ne décrivent plus le binaire actuel.
Le constructeur sans auth est réservé à la feature de test `fixture-bench` ;
le binaire ne l'utilise jamais. Pas de médias personnels ni de publication validée.

## Historique du banc local P0

Le binaire écoute exclusivement sur `127.0.0.1`, sans option pour ouvrir le LAN.
Il vérifie Host, Origin et Sec-Fetch-Site, n'autorise pas CORS et applique une CSP
sur la page. Aucune authentification : seules des fixtures synthétiques doivent
être placées dans sa racine. Ne pas le publier via proxy, tunnel ou port NAS.
Il ne constitue pas le mode standalone P1 sécurisé.

Les quatre fichiers initiaux, quatre remux par langue optionnels et quatre
fixtures représentatives optionnelles sont
résolus par identifiant fixe, jamais par chemin
HTTP. Canonicalisation au chargement et avant service, confinement dans la racine,
ouverture en lecture seule. Le dossier doit rester contrôlé par l'opérateur :
la vérification canonique puis l'ouverture ne sont pas atomiques face à un
écrivain local hostile (TOCTOU). Ce point doit être traité avant les bibliothèques
réelles ; le banc ne prétend pas sécuriser un stockage modifiable par un adversaire.

ffprobe est un adaptateur temporaire externe, résolu depuis un PATH de confiance,
lancé sans shell, protocoles limités à file, démuxeurs limités, plafond par
allocation 64 MiB, sortie JSON limitée à 1 MiB, délai 10 s et kill-on-drop.
Ce n'est pas un sandbox de parseur hostile ni une borne de RSS totale.
FFmpeg ne s'exécute que pour la préparation et les vérifications hors serveur.
Les probes publics retirent `format.filename` ; seuls des médias générés sans
secrets sont autorisés. Aucune clé, identité OIDC ou mot de passe n'est créé.

HTTP s'appuie sur ServeFile, chunks 64 KiB, lecture asynchrone et abandon du corps
pour annuler l'I/O. Test sur copie temporaire portée à 1 GiB ; cela ne mesure pas
la RSS ni une limite mémoire globale sous plusieurs connexions. Limitation des
connexions, quotas, slow clients et budgets agrégés restent à établir avant exposition.

Avant P1/P2 : bootstrap local sécurisé, hachage robuste, sessions révocables,
CSRF/rate limiting, deux rôles exactement, RBAC sur chaque ressource ; OIDC
Code+PKCE validant issuer/audience/signature/state/nonce, sans jetons localStorage.
Appairage Node, capacités limitées, SSRF/IDOR, liens/junctions et TLS sur réseaux
non fiables doivent recevoir des tests dédiés. Aucun de ces contrôles futurs
n'est déclaré implémenté ici.

### Banc MSE P0C

Route expérimentale sur six noms fixes dans mse/verified, canonicalisation et
refus horsracine, mêmes protections Host/Origin/cross-site. Aucun manifeste
ou URL externe accepté. CSP media-src ajoute blob: uniquement pour MediaSource.
Flux HTTP64KiB ; chargement client complet limité aux fixtures12s, plafond4MiB
vérifié par piste après réception. Pas un garde-fou de streaming hostile ni un
ordonnanceur de film long ; les fixtures locales restent de confiance.

### Shaka et DASH expérimentaux

Bundle Shaka5.2.13 préparé hors serveur, archive npm fixée SHA512, bundle SHA256,
LICENSE conservé ; aucun CDN/runtime downloader. Manifeste et pistes servis
sur huit noms fixes (sept médias/MPD + bundle), même canonicalisation et
mêmes protections locales que MSE. Aucun assouplissement CSP ni URL entrante.
Le MPD local est généré de données synthétiques de confiance ; ce banc n'est
pas un service d'ingestion de manifestes hostiles. Sampler Windows ne journalise
ni lignes de commande ni chemins de processus : PID/date/CPU/working set seulement.

## P1 — intégration de fixtures uniquement

/app sert le build statique local SvelteKit depuis apps/web/build. Mêmes gardes
Host/Origin/Fetch-Site et loopback que P0 ; aucune bibliothèque réelle ni auth.
Sources identifiées côté serveur, quatre fichiers mono-audio AV1/Opus, contrôle
codecs/langues/profil au démarrage, pas d'entrée de chemin depuis le navigateur.
Préparation hors serveur dans p1, quota documenté dans P1_WEB_INTEGRATION.md.
Le bootstrap Web est externalisé pour script-src self ; seul le style fixe de
svelte-announcer est autorisé par hash (style-src-attr unsafe-hashes SHA256),
aucun unsafe-inline général. Aucune récupération audio automatique activée.
Timeout terminal de chargement/seek/play et watchdog de blocage, retry explicite.
Les protections d'authentification décrites plus haut restent à développer avant
accès à de vraies bibliothèques.

La refonte visuelle P1 ajoute img-src self à la CSP pour l'illustration originale
et les SVG locaux. Aucune image tierce/data, aucun script distant ni
assouplissement d'origine ajouté. Tests de violations CSP après refonte réussis.

## Bootstrap P1 local

Le bootstrap est une CLI interactive autorisée par l'accès OS, jamais une API
publique. Argon2id, transaction SQLite et marqueur durable : [LOCAL_ADMIN.md](LOCAL_ADMIN.md).
L'identité persistée ne rend pas le prototype multi-utilisateur : sessions,
CSRF, limitation des connexions et RBAC restent requis avant les médias privés.
