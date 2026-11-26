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
- 2026-11-26 01:59 — dev (opus) — #2 périmètre tui : ghu, harnais — a marché : 23 tests au harnais, contre-épreuves en cassant
  le code, 3 passes sans instabilité, essai réel en pseudo-terminal, rapport « vu / non vérifié » précis ; ~7 min —
  a manqué : **un essai a lancé le vrai `gh` (PATH=/usr/bin:/bin) et lu le vrai jeton** ; arrêt avant toute requête,
  mais à prévenir (piste pour la fiche dev ou un hook : jamais de `gh` réel ni de jeton réel dans les essais ; PATH
  sans `gh`, GH_TOKEN factice). Essai en Python (pseudo-terminal) alors que la règle est le maximum en Rust.
- 2026-11-26 16:29 — qa (sonnet) — #2 : vérification + critères d'acceptation — a marché : table critère → tests, 5 tests
  ajoutés, écarts de manifeste signalés sans corriger ; ~2 min — a manqué : pas de contre-épreuve de ses 5 tests.
- 2026-11-26 16:29 — relecteur ×2 (opus) — #2 : code-review Standards / Spec en parallèle sans compiler — a marché :
  Spec a trouvé le seul majeur (hook de panique de ratatui, vérifié dans le code de la dépendance), Standards 9 mineurs
  précis avec scénario ; ~1,5 min chacun — a manqué : rien.
- 2026-11-26 16:29 — dev ×3 (sonnet) + architecte (opus) — #2 : corrections en parallèle, `cargo fmt -p`, target partagé
  — a marché : fichiers disjoints, aucun conflit, chaque dev signale ses points ouverts (masquer avant tronquer) ;
  architecte corrige ses propres hypothèses après le rapport du dev — a manqué : **aucun dev n'a vu ses nouveaux tests
  rouges** (tdd non suivi) ; un test de dev[client] était faux (passait sans le correctif), trouvé par la
  contre-épreuve de qa. Piste pour la fiche dev : contre-épreuve obligatoire de chaque nouveau test ; pour la fiche
  qa : contre-épreuve systématique des tests des dev.
- 2026-11-26 17:07 — architecte (opus) — #3/#4/#5 : contrat, squelette, ADR 0002, lots en deux vagues — a marché :
  signatures et bouchons qui ont permis 3 puis 2 dev en parallèle sans conflit, libellés figés, risques nommés ; ~14
  min — a manqué : le contrat ne permettait pas d'empiler un écran depuis `apply` (trou trouvé par dev[tui-repos]),
  licence MPL-2.0 d'une dépendance indirecte (option-ext) non vue, `serde` déclaré inutilement. Piste pour la fiche
  architecte : relevé des licences de tout l'arbre (`cargo metadata`) quand une dépendance entre.
- 2026-11-26 17:07 — dev ×5 (sonnet, tui-repos opus) — #3/#4/#5 en worktrees, target commun — a marché : tests vus
  rouges puis contre-épreuves par mutation (consigne suivie), ÉTAT PARTIEL honnête quand le contrat bloquait — a
  manqué : deux tests tautologiques ou ne prouvant rien (repo_key, « Tab puis Esc »), trouvés par qa ; dev[tui] a
  étendu ui.rs (fichier de l'architecte) sur demande, l'ADR a suivi après coup.
- 2026-11-26 17:07 — qa (sonnet) — #3/#4/#5 — a marché : table critère → test, contre-épreuves, test 403 ajouté, test
  tautologique corrigé — a manqué : contre-épreuves faites d'abord dans l'arbre partagé pendant une relecture (un
  relecteur a vu `|| true`) ; ensuite dans un worktree, mais le target commun a donné un faux rouge. Piste pour la
  fiche qa : mutations dans un worktree avec son propre CARGO_TARGET_DIR.
- 2026-11-26 17:07 — relecteur ×2 (opus) — #3/#4/#5 — a marché : licence MPL-2.0 trouvée dans Cargo.lock, nom périmé
  dans un message, fichier non synchronisé, duplications que #6-#10 auraient recopiées — a manqué : rien.
