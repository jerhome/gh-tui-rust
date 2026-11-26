# Point de reprise (versionné)

Mis à jour le 2026-11-26 17:27.

## État

- Cadrage fini : `GLOSSARY.md`, `docs/roadmap.md`, ADR 0001. Spec 0.1 : issue #1. Tickets #2 à #11 (sous-issues de
  #1, blocages natifs). Méthode : skills mattpocock combinés aux agents d'agents-home (`CLAUDE.md`).
- **#2 (tracer bullet) fermé** : qa, code-review (deux relecteur : Standards OK, Spec À CORRIGER avec 1 majeur — hook
  de panique de ratatui qui restaurait le terminal sur une panique de tâche tokio), corrections (architecte, dev[tui],
  dev[client], dev[fake]), qa final OK avec contre-épreuves. 120 tests.
- Essai à la main possible : `cargo run -p gh-fake -- --latency-ms 800`, puis
  `GHU_API_URL=<url affichée> GH_TOKEN=ghu-fake-token cargo run -p ghu` (?, Esc, q, Ctrl-C).

## Prochaine action

Fermés : #2, #3, #4, #5, #11 (5 sur 10). main = cc81eb6, poussé (correctif macOS : message des Pinned repos avec le
problème avant le chemin ; CI à vérifier). ADR 0003 (#6 à #10) écrite, statut « proposée ».

1. Vérifier la CI de cc81eb6 (run 45858919724).
2. Relecture de l'ADR 0003 (relecteur opus, axe Spec contre #6-#10 + compatibilité avec l'ADR 0002 amendée :
   Shared::push, Flow::Pop retiré), amendements, acceptation.
3. Vague 0 (architecte : signatures dans crates/), puis dev[fake] + dev[client], puis #6 (dev[tui-detail]) ;
   ordre complet dans l'ADR 0003. Règles de la rétro n° 3 : tests vus rouges + contre-épreuves, fmt -p, mutations de
   qa dans un worktree avec son propre target, 3e tour de relecture = bloquants et majeurs seulement.
4. Finir #6, puis écrire deux skills locaux dans ce repo (rétro n° 3) : retro (fusion avec celle de mattpocock) et
   handoff (écrit dans docs/reprise.md au lieu de /tmp).
5. Essai sccache (décision de la rétro n° 3).

## Points ouverts

- `RecordedRequest` du faux serveur sans en-têtes (le client teste ses en-têtes avec un serveur scripté).
- Restauration du terminal (sortie normale, panique de la boucle, panique d'une tâche) : à essayer par l'utilisateur
  dans un vrai terminal ; Windows non vérifié (CI #11). Le câblage du hook (`mark_ui_thread`, pose du hook) n'est
  couvert par aucun test (seule la décision `restore_if_ui` l'est).
- Marques `[private]` / `[archived]` affichées : #3 doit masquer les archivés par défaut (story 6 de #1).
- Q1 (langue de l'interface) : anglais par défaut, à confirmer (`docs/questions-ouvertes.md`).
- Retours sur les agents pour la rétro : `docs/retours-agents.md`.
