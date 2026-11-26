# Point de reprise (versionné)

Mis à jour le 2026-11-26 17:12.

## État

- Cadrage fini : `GLOSSARY.md`, `docs/roadmap.md`, ADR 0001. Spec 0.1 : issue #1. Tickets #2 à #11 (sous-issues de
  #1, blocages natifs). Méthode : skills mattpocock combinés aux agents d'agents-home (`CLAUDE.md`).
- **#2 (tracer bullet) fermé** : qa, code-review (deux relecteur : Standards OK, Spec À CORRIGER avec 1 majeur — hook
  de panique de ratatui qui restaurait le terminal sur une panique de tâche tokio), corrections (architecte, dev[tui],
  dev[client], dev[fake]), qa final OK avec contre-épreuves. 120 tests.
- Essai à la main possible : `cargo run -p gh-fake -- --latency-ms 800`, puis
  `GHU_API_URL=<url affichée> GH_TOKEN=ghu-fake-token cargo run -p ghu` (?, Esc, q, Ctrl-C).

## Prochaine action

Tickets #3, #4, #5 fermés (3e7c5f9, poussé). **CI rouge sur macOS** : test
`an_invalid_file_at_start_up_shows_a_notice_and_refuses_pinning` (pinned_repos.rs:354), écran sans « is invalid » ;
hypothèse : chemin temporaire macOS long qui pousse le texte hors des 100 colonnes du harnais (test fragile).

1. Corriger ce test (dev[tui] ou qa), CI verte.
2. ADR 0003 (#6 à #10) écrite, statut « proposée » : relecture (relecteur), acceptation, puis vague 0 (architecte),
   dev[fake] + dev[client], dev[tui-detail], etc. (ordre dans l'ADR).
3. Rétro n° 3 d'agents-home le 2026-11-26 vers 17:20 (questionnaire : <https://claude.ai/artifact/T3kijTUKyZ8wNZfNckNzHA>).

## Points ouverts

- `RecordedRequest` du faux serveur sans en-têtes (le client teste ses en-têtes avec un serveur scripté).
- Restauration du terminal (sortie normale, panique de la boucle, panique d'une tâche) : à essayer par l'utilisateur
  dans un vrai terminal ; Windows non vérifié (CI #11). Le câblage du hook (`mark_ui_thread`, pose du hook) n'est
  couvert par aucun test (seule la décision `restore_if_ui` l'est).
- Marques `[private]` / `[archived]` affichées : #3 doit masquer les archivés par défaut (story 6 de #1).
- Q1 (langue de l'interface) : anglais par défaut, à confirmer (`docs/questions-ouvertes.md`).
- Retours sur les agents pour la rétro : `docs/retours-agents.md`.
