# Point de reprise (versionné)

Mis à jour le 2026-11-26 16:59.

## État

- Cadrage fini : `GLOSSARY.md`, `docs/roadmap.md`, ADR 0001. Spec 0.1 : issue #1. Tickets #2 à #11 (sous-issues de
  #1, blocages natifs). Méthode : skills mattpocock combinés aux agents d'agents-home (`CLAUDE.md`).
- **#2 (tracer bullet) fermé** : qa, code-review (deux relecteur : Standards OK, Spec À CORRIGER avec 1 majeur — hook
  de panique de ratatui qui restaurait le terminal sur une panique de tâche tokio), corrections (architecte, dev[tui],
  dev[client], dev[fake]), qa final OK avec contre-épreuves. 120 tests.
- Essai à la main possible : `cargo run -p gh-fake -- --latency-ms 800`, puis
  `GHU_API_URL=<url affichée> GH_TOKEN=ghu-fake-token cargo run -p ghu` (?, Esc, q, Ctrl-C).

## Prochaine action

Ticket #11 fermé (CI verte, ba6b1c9). #3, #4, #5 : vagues 1 et 2 fusionnées sur main local (6161121, 11 commits
non poussés) + correctif non commité (Shared::push, allows Skeleton retirés). Q1 et choix de l'ADR 0002 confirmés.

1. En cours : qa + relecteur ×2 (Standards, Spec) sur `git diff ba6b1c9`.
2. Corrections ; architecte : phrase Shared::push dans l'ADR 0002, sort de `Flow::Pop` (inutilisé), format du 422
   de gh-fake.
3. Commit `Closes #3 #4 #5` → push → CI.
4. Ensuite : architecte pour le contrat de #6 à #10.

## Points ouverts

- `RecordedRequest` du faux serveur sans en-têtes (le client teste ses en-têtes avec un serveur scripté).
- Restauration du terminal (sortie normale, panique de la boucle, panique d'une tâche) : à essayer par l'utilisateur
  dans un vrai terminal ; Windows non vérifié (CI #11). Le câblage du hook (`mark_ui_thread`, pose du hook) n'est
  couvert par aucun test (seule la décision `restore_if_ui` l'est).
- Marques `[private]` / `[archived]` affichées : #3 doit masquer les archivés par défaut (story 6 de #1).
- Q1 (langue de l'interface) : anglais par défaut, à confirmer (`docs/questions-ouvertes.md`).
- Retours sur les agents pour la rétro : `docs/retours-agents.md`.
