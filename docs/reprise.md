# Point de reprise (versionné)

Mis à jour le 2026-11-26 16:44.

## État

- Cadrage fini : `GLOSSARY.md`, `docs/roadmap.md`, ADR 0001. Spec 0.1 : issue #1. Tickets #2 à #11 (sous-issues de
  #1, blocages natifs). Méthode : skills mattpocock combinés aux agents d'agents-home (`CLAUDE.md`).
- **#2 (tracer bullet) fermé** : qa, code-review (deux relecteur : Standards OK, Spec À CORRIGER avec 1 majeur — hook
  de panique de ratatui qui restaurait le terminal sur une panique de tâche tokio), corrections (architecte, dev[tui],
  dev[client], dev[fake]), qa final OK avec contre-épreuves. 120 tests.
- Essai à la main possible : `cargo run -p gh-fake -- --latency-ms 800`, puis
  `GHU_API_URL=<url affichée> GH_TOKEN=ghu-fake-token cargo run -p ghu` (?, Esc, q, Ctrl-C).

## Prochaine action

Ticket #11 fermé (CI verte Linux/Windows/macOS, ba6b1c9). #3, #4, #5 en cours selon l'ADR 0002 (contrat et squelette de
l'architecte commités) :

1. Vague 1 (worktrees `../gh-tui-rust-wt-{fake,client,pins}`, branches `wip-345-*`, target commun) : dev[fake],
   dev[client], dev[pins].
2. Fusion vague 1 → vérification → commit. Vague 2 : dev[tui-repos] (opus, #3 + #4), dev[tui-pulls] (#5).
3. Fusion vague 2, retrait des `#[allow(dead_code)]` « Skeleton » de ui.rs → qa → relecteur ×2 → commit
   `Closes #3 #4 #5` → push.
4. Ensuite : architecte pour le contrat de #6 à #10.

## Points ouverts

- `RecordedRequest` du faux serveur sans en-têtes (le client teste ses en-têtes avec un serveur scripté).
- Restauration du terminal (sortie normale, panique de la boucle, panique d'une tâche) : à essayer par l'utilisateur
  dans un vrai terminal ; Windows non vérifié (CI #11). Le câblage du hook (`mark_ui_thread`, pose du hook) n'est
  couvert par aucun test (seule la décision `restore_if_ui` l'est).
- Marques `[private]` / `[archived]` affichées : #3 doit masquer les archivés par défaut (story 6 de #1).
- Q1 (langue de l'interface) : anglais par défaut, à confirmer (`docs/questions-ouvertes.md`).
- Retours sur les agents pour la rétro : `docs/retours-agents.md`.
