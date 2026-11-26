# Feuille de route

## Décisions de cadrage (2026-11-26)

1. Client GitHub en TUI, en Rust ; usage personnel ; repo public `jerhome/gh-tui-rust`, licence MIT.
2. Exécutable `ghu` (`gtr` est pris sur crates.io).
3. Authentification par le jeton de `gh` (`GH_TOKEN` / `GITHUB_TOKEN` / `gh auth token`).
4. Tests contre un faux serveur, jamais contre le vrai GitHub.
5. Banc d'essai des cinq agents de base d'agents-home (aucun agent propre au repo).
6. Périmètre (2026-11-26) : les repos, leurs PRs, ajouter des commentaires, approuver ou demander des changements.
7. Repos listés : les tiens, ceux où tu es collaborateur, ceux de tes organisations, plus une saisie libre
   `owner/repo`.
8. Diff affiché dans la TUI (liste des fichiers, diff unifié coloré, défilement).
9. Plateformes : Linux/WSL et Windows dès le début ; macOS construit par la CI sans effort particulier.
10. Comment et Review distincts (voir `GLOSSARY.md`) ; verdicts Approve / Request changes ; sur une Own PR, verdicts
    masqués ; revues en attente (pending) ignorées ; repos archivés masqués (filtre), forks marqués.
11. Saisie du texte : zone de texte dans la TUI, avec une touche pour passer à `$EDITOR` ; confirmation avant toute
    publication. Markdown : rendu simple (titres, gras, listes, code).
12. Rafraîchissement manuel, et automatique après une publication ; pas d'interrogation périodique.

## Versions

| Version | Contenu | Processus |
|---|---|---|
| 0.1 | Outil suffisant pour faire une revue : liste des repos (repos épinglés compris) ; PRs d'un repo (ouvertes, filtre fermées / fusionnées) ; détail d'une PR (description, état, verdicts et conversation en lecture) ; diff dans la TUI ; Comment ; Review (Approve / Request changes), avec confirmation ; rafraîchissement manuel et après publication | À découper (to-spec, to-tickets) ; essai réel par l'utilisateur |
| 0.2 | Commentaires sur une ligne du diff, publiés en une seule revue | — |
| plus tard | Checks (CI) et commits dans le détail ; écran « mes PRs » ; grosses PRs (échelle) ; fils de discussion (répondre, résoudre) ; revues en attente (pending) | À décider |
