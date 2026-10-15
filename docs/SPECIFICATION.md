# Périmètre du premier jalon P0

Document créé le 15 octobre 2026 : absent dans le dépôt initial.
Les règles `AGENTS.md`, `ARCHITECTURE.md` et `PROMPT_DEMARRAGE_IA.md` priment.

Parcours livré : générer des fichiers synthétiques AV1/Opus, inspecter leurs
pistes au démarrage, les servir en lecture seule avec une API versionnée,
observer lecture/seek/erreurs dans un navigateur et archiver les résultats.
Le serveur sert quatre fixtures initiales, quatre remux mono-audio optionnels
et quatre fixtures représentatives 10 bits/5.1 optionnelles préparées hors
lecture en P0C, explicitement enregistrés. Il n'est
pas un scanner de bibliothèques ni un serveur multimédia multi-utilisateur.

P0 doit précéder P1 autonome (SQLite/admin local), P2 multi-Node/OIDC,
P3 pistes externes/raw, P4 ergonomie films/séries, puis P5 mesures/robustesse.
Seuls `administrator` et `user` seront des rôles. Aucun de ces rôles ni compte
n'est implémenté dans le banc local. Pas de transcodage pendant la lecture.
Pas de modification des médias personnels, de publication ou de changement de licence.

Critères de ce jalon : fichiers valides, pistes AV1/Opus identifiées, remux sans
modification des paquets, HTTP testé, résultat Chrome mesuré et limites explicites.
Les phases complètes et les autres navigateurs restent ouverts.

## Extension autorisée P1 (15 octobre 2026)

Première intégration verticale Rust/SvelteKit sur localhost, fixtures seulement.
HTML5 natif derrière interface Player légère, sources et pistes exposées par Core.
Pause/lecture/seek/volume/vitesse, FR/EN avec position conservée, WebVTT/off,
erreurs explicites. Aucun nouveau moteur, auth/catalogue réel ou DB anticipé.
Décision et preuves : P1_WEB_INTEGRATION.md. La suite est le bootstrap admin local
sécurisé, puis les bibliothèques ro autorisées.

## Premier jalon catalogue P1 autorisé (15 octobre 2026)

Navigation Accueil / Films / Séries / Recherche / Paramètres, listes séparées,
fiche film et parcours série → saison → épisode → lecteur immersif conservé.
Le catalogue vient d'une API avec recherche, tri et pagination ; seules les
fixtures sont associées explicitement au modèle Work / Edition / MediaVersion /
MediaSource / Asset / Track. Deux copies MKV/MP4 ne créent pas deux films.
Les bibliothèques sont identifiées séparément, sans gestion de dossiers réelle.

Ne pas afficher d'année, d'affiche réelle, d'historique, de récence ou de
recommandation inventée. Les états indisponibles sont expliqués. Ce parcours
ne clôture pas P1 autonome : bootstrap local, sessions/RBAC et SQLite restent
prérequis avant tout dossier personnel en lecture seule. Aucun nouvel essai
codec ou remplacement de lecteur n'est inclus. Voir CATALOGUE_P1.md.

Ce jalon de fixtures a été vérifié localement : 15 tests Rust, check Web 0/0,
lint, 8 tests Vitest, build et parcours navigateur Chrome headless catalogue /
lecteur réussis. États vides/erreurs et pagination simulée bornée vérifiés,
captures desktop/mobile inspectées. Aucun nouveau résultat Edge/Firefox/Safari,
mobile physique ou NAS ; pas de preuve de charge ni de bibliothèque personnelle.
