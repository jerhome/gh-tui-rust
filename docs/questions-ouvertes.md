# Questions ouvertes

Seul endroit des questions à l'utilisateur. Chaque question : contexte, recommandation, défaut appliqué en attendant.
Une question répondue est déplacée en bas avec sa réponse et sa date.

## En attente

- Q2 (ADR 0003, § 5, 2026-11-27) — Éditeur par défaut sous Windows : si l'essai réel confirme que le Notepad à onglets
  de Windows 11 rend la main tout de suite, faut-il le retirer (et exiger `VISUAL` ou `EDITOR`) ?
  Recommandation et défaut appliqué : garder notepad avec la notice `ReturnedAtOnce`, trancher après l'essai réel.

## Répondues

- 2026-11-26 — Nom : `gh-tui-rust` ; exécutable court (`gtr` ou autre). Repo public. Jeton de `gh`.
  Agents : ceux d'agents-home seulement.
- 2026-11-26 — Exécutable `ghu`. Repos : les tiens + collaborateur + organisations + saisie libre `owner/repo`.
  Commentaire général d'abord, commentaires de ligne à la version suivante. Diff dans la TUI. Linux/WSL + Windows.
  Plus tard : écran « mes PRs », échelle, fils de discussion.
- 2026-11-26 16:59 — Q1 : interface en anglais (défaut confirmé). Choix de l'ADR 0002 confirmés : filtre ouvert par `/` ;
  `o` + `owner/name` épingle toujours le repo ; dates des PRs en AAAA-MM-JJ UTC.
