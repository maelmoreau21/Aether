# Sécurité du banc local P0

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
