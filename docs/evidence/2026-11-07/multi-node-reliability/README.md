# Preuves — un Core, plusieurs Media Nodes (7 novembre 2026)

Environnement : image Docker `aether-rust-dev` (rustc 1.99.1), compilation debug, boucle locale, SQLite.
Fixtures : générées avec les commandes de `scripts/generate-fixtures.ps1` (`ffmpeg`/`libaom-av1`/`libopus`).

| Fichier | Contenu |
|---|---|
| `reliability-before.txt` | `multi_node_reliability` rejoué sur le **code d'origine** : 4 échecs sur 5 (le scénario de jetons passe) |
| `reliability-after.txt` | même fichier de tests sur le code modifié, scénario de scan et mesure directe/relais inclus : 8 réussis |
| `regression-before.txt` | suites multi-Node existantes avant : mêmes deux échecs `unsupported_preparation` qu'après |
| `regression-after.txt` | suites existantes après, avec le benchmark 1 / 10 / 100 Nodes |
| `media-node-failures-before.txt`, `…-after.txt` | liste des 34 tests `media-node` en échec (encodeurs externes) : identique |

Les lignes `node_media_*` et `media_http_*` (journal par requête) et l'avertissement de sauvegarde sont filtrés.
