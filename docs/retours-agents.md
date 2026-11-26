# Retours sur les agents de base

Ce repo éprouve les agents d'agents-home. Chaque lancement notable laisse une ligne ; la rétro d'agents-home s'en sert
pour améliorer les fiches.

Format : `AAAA-MM-JJ HH:MM — agent (modèle) — tâche — ce qui a marché / ce qui a manqué — piste pour la fiche`.

## Journal

- 2026-11-26 01:46 — architecte (opus) — #2 : ADR 0001, workspace, signatures, lots — a marché : contrat complet (3 crates,
  signatures documentées, cargo check vert), options écartées chiffrées, lots avec tests TDD précis, ordre et
  goulot nommés, tranche seul les choix réversibles (Q1 langue, défaut appliqué) ; ~10 min — a manqué : rien de
  bloquant ; propose opus pour dev[tui] avec raison.
- 2026-11-26 01:49 — dev (sonnet) — #2 périmètre fake : faux serveur — a marché : 20 tests rouges puis verts (tdd suivi),
  signatures intactes, binaire vérifié, signale proprement le fournisseur rustls manquant et le reformatage d'un
  autre crate par `cargo fmt --all` ; ~3 min — a manqué : `cargo fmt --all` touche les fichiers d'un autre dev en
  parallèle (piste pour la fiche dev : `cargo fmt -p <crate>` quand plusieurs dev travaillent ensemble).
- 2026-11-26 01:51 — dev (sonnet) — #2 périmètre client : gh-client — a marché : 63 tests, contre-épreuve faite en
  cassant le code quand les tests d'intégration étaient verts d'emblée, fixture tirée de l'OpenAPI GitHub, rapport
  précis (vu / non vérifié) ; ~5 min — a manqué : ajouts non demandés (troncature, plafonds, masquage), utiles et
  signalés mais hors demande ; même `cargo fmt --all` sur les fichiers de l'autre dev.
