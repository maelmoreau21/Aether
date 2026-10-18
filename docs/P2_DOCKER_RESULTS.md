# Résultats Docker P2 — 17 octobre 2026

**Résultat : les deux modes passent sur des conteneurs Linux réels.** 86 contrôles du banc final réussis, médias synthétiques uniquement, volumes jetables supprimés après réussite. Aucun commit, push, publication d’image ou accès au NAS.

Preuves brutes : [avant](evidence/2026-10-17/p2-hardening/before.json), [après](evidence/2026-10-17/p2-hardening/after.json), [Chrome distribué](evidence/2026-10-17/p2-hardening/browser-distributed/browser.json), [Chrome standalone](evidence/2026-10-17/p2-hardening/browser-standalone/browser.json). Les IDs des images finales figurent dans le rapport après.

## Vérifications réellement exécutées

- Construction `Dockerfile` : targets `runtime`, `runtime-core`, `runtime-node`, tags locaux `aether:p2-qa`, `aether-core:p2-qa`, `aether-node:p2-qa`.
- Standalone : vrai bootstrap TTY, scan Films et Séries, lecture, HEAD/Range 206/416, abandon de connexion puis nouvelle requête, conservation des UUID après redémarrage.
- Distribué : deux secrets aléatoires distincts, appairages, 24 fichiers par Node (12 MP4 + 12 MKV), catalogue film/épisode, flux directs et relais identiques aux SHA-256 des fixtures, redémarrages, arrêt/reconnexion et rotation persistée.
- Révocation : ancienne capacité refusée après rotation et déconnexion ; arrêt Core refuse le direct, reprise Core restaure la lecture ; Node révoqué hors ligne refuse ses anciennes capacités après retour.
- Tests Rust supplémentaires : bibliothèque fausse/désactivée, droits retirés, utilisateur désactivé, session supprimée, expiration/asset/Node/action/clé ; activité directe traitée par le même worker borné P1.
- HTTPS réel dans Docker : Core et Nodes passent par un terminateur TLS de test avec CA supplémentaire, validation active et SAN adaptés. URL au mauvais nom refusée, URL correcte appairée. Aucun `accept_invalid_certs` ni repli HTTP.
- Rust/TLS Windows : négociation TLS observée, chaîne approuvée acceptée, autorité inconnue/nom erroné/certificat expiré refusés, réponse unsigned/modifiée/redirection refusée, corps de contrôle manquant arrêté après 15 s.
- Chrome 155.0.8059.40, Windows 10.0.26300 headless : 8 parcours UI, film + épisode dans chaque mode, direct et repli forcé, pause/seek 2 s, dimensions 320×180, images vidéo et octets audio décodés observés. Pas d’écoute humaine ni de validation d’un autre navigateur.
- Tous les SHA-256 des médias restent inchangés ; journaux des conteneurs contrôlés sans secret, cookie ou query porteuse. Les URL des rapports Chrome sont expurgées.
- Compose valide ; démarrage refusé sans secrets et ancien jeton court connu refusé. Médias `:ro`, UID 10001, toutes capacités supprimées, no-new-privileges et racine `read-only` dans le banc.

Qualité : `cargo fmt --check` et Clippy workspace/all-features/all-targets avec `-D warnings` passent. `cargo test --locked --workspace --all-features` : **124 réussis, 0 échec, 7 ignorés**. Le test TLS explicite est ensuite exécuté : **4/4 réussis**. Les cinq anciens tests navigateur Rust restent ignorés ; ils ne sont pas présentés comme exécutés. Le helper bootstrap ignoré est exercé par son test parent. Web : check 0 erreur/avertissement, lint, **33/33 tests**, build statique réussi dans Docker.

Logs : [Rust](evidence/2026-10-17/p2-hardening/rust-tests.log), [Clippy](evidence/2026-10-17/p2-hardening/clippy.log), [HTTPS](evidence/2026-10-17/p2-hardening/https-tests.log), [Web](evidence/2026-10-17/p2-hardening/web-checks.log).

La même pile et les mêmes IDs d’images ont également passé une seconde fois les
86 contrôles avec recherche des mots de passe, secrets d’appairage, cookies et
CSRF émis, et queries de lecture, dans **stdout et stderr** :
[répétition et audit des journaux](evidence/2026-10-17/p2-hardening/after-log-audit.json).
Les tableaux gardent les mesures de la première passe finale, sans sélectionner
la meilleure valeur entre les deux essais. Deux captures des lecteurs ont aussi
été inspectées visuellement (film distribué, épisode standalone).

## Fichiers principaux modifiés

- `crates/node/src/transport.rs`, `server.rs`, binaire Node et `Cargo.lock` : TLS,
  politique de destinations, délais, intégrité des réponses et persistance.
- `crates/core/src/nodes.rs`, `auth.rs`, `permissions.rs`, `inventory.rs`, `lib.rs`
  et protocole partagé : capacités, contrôle vivant et activité P1 commune.
- `Dockerfile`, les deux Compose standalone et les exemples distribués,
  `.env.example` : modes conservés, secrets et image Node allégée.
- Tests Rust ciblés, scripts de génération/banc Docker/Chrome, contrats API,
  sécurité, architecture, guide Synology et `MEMORY.md`.

## Images : avant / après

Octets Docker non compressés, Mo décimaux. Référence standalone : image existante `aether:test` ; l’ancienne `aether:latest` refusait le scan Séries (400), donc ne sert pas de référence fonctionnelle pour ce parcours. Les images de référence ne constituent pas un commit source identifié.

| Image | Avant (Mo) | Après (Mo) | Écart |
|---|---:|---:|---:|
| Node | 774.42 | 499.85 | -35.46 % |
| Core | 172.89 | 173.36 | +0.27 % |
| Standalone | 803.07 | 827.49 | +3.04 % |

Le gain Node est réel : environ **275 Mo / 35,5 %**. Les plus gros paquets initiaux sont LLVM (114 610 Kio), ICU (36 170 Kio), flite (27 554 Kio), MFX (27 346 Kio), Mesa DRI (25 254 Kio), Z3 (22 767 Kio). Le runtime copie `ffprobe` et toute sa fermeture de bibliothèques liées ; les pilotes/données non nécessaires au probe sont omis. Bibliothèques liées conservées même lorsqu’elles restent volumineuses. Notices de copyright et licences conservées.

Le binaire `ffprobe` et la liste de démuxeurs ont exactement les mêmes SHA-256 avant/après : [comparaison](evidence/2026-10-17/p2-hardening/probe-comparison.json). Version 5.1.9 Debian, scans AV1+Opus MKV et MP4 réellement passés. Ce jalon ne teste pas le décodage matériel ou tous les codecs possibles. Inventaire des composants de construction dans `/usr/share/media-probe/source-stage-packages.tsv` ; mise à jour des bibliothèques copiées par reconstruction d’image. Aucun gain de taille standalone annoncé.

## Scans et latences

48 fichiers synthétiques par architecture, 24 par bibliothèque, zéro erreur média. Lectures séquentielles durant au moins 5 s par phase ; ce sont des mesures locales de fichiers courts, pas une capacité maximale ou une charge NAS.

| Parcours | Avant (s) | Après (s) |
|---|---:|---:|
| Scan distribué Films | 20.91 | 3.70 |
| Scan distribué Séries | 4.99 | 3.04 |
| Scan standalone Films | 6.34 | 6.22 |
| Scan standalone Séries | 4.82 | 1.99 |

| Lecture | TTFB p50 avant / après (ms) | TTFB p95 avant / après (ms) |
|---|---:|---:|
| Distribué direct | 11.32 / 82.84 | 63.90 / 161.49 |
| Distribué relais | 12.33 / 87.76 | 34.79 / 154.94 |
| Standalone | 27.66 / 4.96 | 54.66 / 21.81 |

API catalogue : échantillons TTFB après = 4.61, 4.70, 8.66, 4.16 ms. Faible nombre d’échantillons : aucune extrapolation de percentile API.

**Pas de gain de latence démontré.** Les références HTTP et le nouvel HTTPS avec autorisation vivante ne sont pas un essai à transport constant. Hôte partagé avec des vérifications Rust, caches et coûts Docker exec : aucune attribution causale d’un scan plus rapide. Les connexions TLS sont actuellement recréées ; le contrôle vivant coûte une requête supplémentaire au Core. Un futur jalon pourrait mesurer un pool borné sans cache positif d’autorisation ; aucune telle optimisation n’est implémentée ici.

## Mémoire et CPU en activité

Pics échantillonnés par phase, en Mio cgroup (cache fichier et enfants ffprobe inclus). CPU moyen en % d’un cœur, instrumentation incluse. Le pic `memory.peak` cumulatif comprend aussi le bootstrap Argon2 : il est conservé dans les JSON, pas assimilé au pic de lecture.

| Phase / conteneur | Pic phase avant / après (Mio) | CPU avant / après (%) | Pic cumulatif après (Mio) |
|---|---:|---:|---:|
| Scan distribué / core | 11.70 / 15.57 | 8.4 / 10.0 | 70.57 |
| Scan distribué / films | 24.59 / 12.85 | 23.2 / 38.1 | 16.61 |
| Scan distribué / series | 7.25 / 8.18 | 18.1 / 35.4 | 19.35 |
| Scan distribué / tls | — / 14.41 | — / 3.6 | 18.41 |
| Direct / core | 14.18 / 17.93 | 9.1 / 5.2 | 70.57 |
| Direct / films | 15.53 / 10.38 | 14.9 / 38.2 | 16.61 |
| Direct / series | 5.02 / 10.57 | 15.0 / 37.9 | 19.35 |
| Direct / tls | — / 15.05 | — / 7.2 | 18.81 |
| Relais / core | 14.62 / 20.25 | 30.1 / 72.3 | 70.57 |
| Relais / films | 15.46 / 10.36 | 14.0 / 5.5 | 16.61 |
| Relais / series | 4.68 / 10.54 | 17.0 / 4.4 | 19.35 |
| Relais / tls | — / 14.70 | — / 9.1 | 18.81 |
| Scan standalone / standalone | 22.17 / 19.83 | 88.1 / 77.9 | 69.63 |
| Lecture standalone / standalone | 16.11 / 11.81 | 39.6 / 29.3 | 69.63 |

## Réseau interne Core ↔ Nodes

Le proxy TLS du banc ne reçoit que les communications internes Core/Nodes. Ses RX/TX mesurent contrôle, callbacks et données du relais ; les clients directs passent par les ports loopback des Nodes. Ce sont des compteurs d’interface, pas une capture dédupliquée par pair TCP ni un débit maximal.

| Phase après | Proxy RX / TX (Mo) | Proxy RX / TX observés (Mo/s) | Core RX / TX (Mo) |
|---|---:|---:|---:|
| Scan | 0.210 / 0.219 | 0.031 / 0.032 | 0.237 / 0.085 |
| Direct | 0.220 / 0.315 | 0.036 / 0.051 | 0.096 / 0.102 |
| Relais | 13.572 / 13.619 | 2.219 / 2.227 | 13.632 / 13.509 |

## Limites et prochain jalon

- Les résultats HTTPS portent sur des conteneurs du même hôte, avec terminateur TLS jetable Python. Ils ne qualifient ni deux machines physiques, ni le réseau, DNS, pare-feu, certificats clients ou proxy DSM du NAS.
- Chrome accède au Web et aux URLs directes en HTTP loopback ; le transport interne Core/Nodes est HTTPS. Aucun certificat de test ajouté au magasin Windows, aucune validation navigateur HTTPS public/NAS annoncée.
- Révocation à la requête suivante ; un corps déjà ouvert reste ouvert comme en P1. Core indisponible : nouvelles requêtes directes refusées.
- Une panne pendant l’échange de rotation peut demander une récupération opérateur ; aucun protocole transactionnel distribué de rotation ni test de panne à chaque instruction n’est revendiqué.
- Linux x86_64 Docker/WSL2 seulement ; pas de preuve ARM64, de longue vidéo, de montée en concurrence, de FFmpeg matériel ou d’écoute audio humaine.
- Essais intermédiaires corrigés : ancien standalone sans Séries, attentes fixes du bootstrap, noms de champs historiques, assertion logout 200 au lieu de 204. Une compilation Windows simultanée à un exécutable de test a échoué avec accès refusé ; la suite sérialisée a ensuite passé. Les problèmes de placement de tests/lecture partielle relevés par Clippy ont été corrigés.

Prochain petit jalon : exécuter le [guide Synology](DEPLOYMENT_SYNOLOGY.md) sur les machines cibles **après autorisation**, avec certificats et médias de test, sans OIDC ni P3. Le [contrat et les commandes reproductibles](P2_DOCKER_VALIDATION.md) restent la référence technique.
