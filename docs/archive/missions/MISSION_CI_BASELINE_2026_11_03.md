# Mission fonctionnalités — état CI du 3 novembre 2026

Audit effectué après `git fetch origin develop`, sans changement de branche,
push, publication ni relance CI. Les deux branches locales `develop` égalent
`origin/develop` : divergence `0/0` dans chaque dépôt.

| Dépôt | SHA de référence | Dernière CI develop |
|---|---|---|
| Aether | `699345fc17fff56b8528b3d836fdab67359f019b` | [41622840802](https://github.com/maelmoreau21/Aether/actions/runs/41622840802), échec |
| Aether-Plugins | `0943099a6f0586a92c2f1d348f83e13c33df769f` | [41622841072](https://github.com/maelmoreau21/Aether-Plugins/actions/runs/41622841072), échec |

## Causes distantes et corrections locales préexistantes

- Aether, job Chrome `125872641630` : timeout à
  `scripts/browser-scanner.cjs:255`, ancienne assertion ASS dans
  `video.textTracks`. Les jobs Rust, PostgreSQL, frontend, audits et images AMD64
  réussissent ; la publication est ignorée par le gate. Le worktree contient
  déjà la vérification du canvas libass et la désactivation des anciennes pistes
  natives avant détachement, avec son test de régression.
- Plugins, job `125872642103` : Clippy échoue avec E0422/E0560 sur
  `MetadataCollection` et `StructuredMetadata.collection`. Le worktree épingle
  déjà le SDK `133896830c609a177e4c4c611084154d7e4aab5d`, dans le manifeste et
  le lockfile, et comporte les tests avec/sans collection. Les étapes tests et
  Wasm du run distant sont ignorées après cet échec.
- L'erreur Docker `refs/pull/35/merge` appartient au
  [run historique 41511991820](https://github.com/maelmoreau21/Aether/actions/runs/41511991820),
  job `125505890792`. Le worktree contient déjà `source: .` pour les appels
  Bake CI/publication et le checkout explicite de `github.sha`.

Ces corrections et les autres modifications locales existaient avant cette
mission. Elles ne sont ni attribuées à ce nouvel audit, ni présumées intégrées,
publiées ou déployées. Voir les bilans
[P0](CI_P0_REPAIRS_2026_11_03.md), [Docker](CI_DOCKER_A01_2026_11_03.md) et
[SDK](../../Aether-Plugins/docs/SDK_COLLECTION_CI_2026_11_03.md).

## Contrôles essentiels rejoués pendant cet audit

Environnement local Windows ; tests sur les worktrees courants :

| Commande | Résultat |
|---|---|
| `python scripts/test-release-policy.py` | 15 tests réussis |
| `npm test --prefix apps/web -- src/lib/player/native.test.ts src/lib/player/subtitle-preference.test.ts src/lib/player/subtitle-style.test.ts` | 52 tests réussis |
| Plugins : `cargo test --locked -p aether-plugin-api --test collection_contract` | 2 tests réussis |

Pas de nouveau défaut CI confirmé imposant une correction supplémentaire avant
le parcours vertical OpenSubtitles. Préserver les correctifs existants et leur
découpage. Les CI distantes restent rouges : seuls de nouveaux runs sur les
corrections intégrées pourront valider Linux. Le navigateur complet et Docker
n'ont pas été reconstruits ici ; les preuves locales récentes restent celles
des rapports P0. Aucun test NAS, fournisseur réel, matériel ARM ou déploiement.
