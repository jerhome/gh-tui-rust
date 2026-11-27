# Version 0.1 : ce qui reste après #9

Préparé le 2026-11-27 (16:01) par `architecte`, d'après `docs/roadmap.md` (ligne 0.1), `docs/reprise.md` (Points
ouverts), l'ADR 0003 (amendement du 2026-11-27 16:01, § 9) et le code de `wip-9-editor` (e19f5f2).

La 0.1 de la feuille de route (repos, PRs, détail, diff, Comment, Review avec confirmation, rafraîchissement) est
couverte par les tickets #2 à #11. Fermés : tous sauf #9. Ce fichier liste ce qui sépare la fermeture de #9 de la
release `v0.1.0` : lots, périmètres, fichiers, ordre, critère de « terminé » de chacun.

## Décisions prises ici (choix techniques réversibles, tranchés)

- **502** : couvert en unitaire, le harnais s'en tient au 500 ; pas de 502 au faux serveur (ADR 0003 § 9). Un
  seul test de plus, côté client (corps HTML d'une passerelle) : lot L3.
- **`Debug` d'`EditRequest`** : écrit à la main, sans le texte (ADR 0003 § 9) ; fait dans les corrections de #9
  (lot L1), pas dans un lot à part : `editor.rs` est dans le périmètre de dev[tui-editor], en cours.
- **Release** : sur un tag `v0.1.0`, un workflow GitHub Actions construit **seulement `ghu`** (ni `gh-fake`, ni
  `ghu-fake-editor` : ADR 0003 § 8) pour `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc` et
  `aarch64-apple-darwin` (macOS construit sans essai réel, dit dans les notes de release, roadmap point 9), avec
  `SHA256SUMS`. Pas de publication sur crates.io pour la 0.1. On revient sur ce choix si l'utilisateur veut un
  autre canal (crates.io, winget) ou une autre cible.
- **Empreinte du build essayé** : la fiche d'essai porte le SHA-256 de l'exécutable (règle d'agents-home) ;
  `ghu --version` reste tel qu'aujourd'hui pour la 0.1.

## Lots

| Lot | Agent (modèle) | Tâche | Fichiers | Dépend de | Terminé quand |
|---|---|---|---|---|---|
| L1 | dev[tui-editor] (opus, en cours) | Corrections de #9 d'après qa et les relectures, dont le `Debug` d'`EditRequest` (ADR 0003 § 9) | `crates/ghu/src/{editor,terminal}.rs`, `crates/ghu/src/ui/{compose,text_area}.rs`, `crates/ghu/tests/{editor,comment}.rs` (fichiers de la branche en cours) | — | vérification complète verte (`CLAUDE.md`) ; test unitaire : le `Debug` d'une requête sur `"secret draft"` ne contient pas `secret` et contient `chars: 12` ; relecteur ×2 OK ; fusion dans `main` avec `Closes #9` (session principale) |
| L2 | architecte (opus) | Retirer les `#[allow(dead_code, …)]` de `ui.rs` : variantes `Outcome::PrPage`, `Files`, `CommentPosted`, `ReviewPosted`, `Signal::Published`, `Signal::Kept`, `Shared::pop`, `Shared::request_edit` (8 attributs, l. 99-140, 250, 257 de e19f5f2). Un élément encore mort après le retrait est supprimé, ou garde un `#[expect(dead_code, reason = …)]` justifié | `crates/ghu/src/ui.rs` seul | L1 fusionné (`request_edit` est appelé par #9) | `git grep -n dead_code crates/ghu/src` vide ; vérification complète verte ; `crates/ghu/tests/support/mod.rs` garde son `#![allow(dead_code)]` (module commun aux binaires de test, chacun n'en utilise qu'une partie) |
| L3 | qa (sonnet) | Test du 502 de passerelle (ADR 0003 § 9) : dans le module `tests` de `status.rs`, `run(502, &[], "<html>…</html>")` donne `Error::Api { status: 502, message: "" }` ; un 503 au corps vide, pareil ; contre-épreuve : le même test avec 500 et un `message` JSON garde le message | `crates/gh-client/src/status.rs` (module `#[cfg(test)]` seulement) | L1 fusionné | `cargo test -p gh-client` vert ; la contre-épreuve échoue si l'on casse `unwrap_or_default` (essai noté dans le rapport, puis annulé) |
| L4 | plateforme (sonnet) | Workflow de release : `on: push: tags: ['v*']` et `workflow_dispatch` (entrée `publish`, défaut `false` : construit et téléverse les artefacts sans créer de release) ; `cargo build --release --locked -p ghu --bin ghu --target <cible>` pour les 3 cibles ; archives `ghu-<version>-<cible>.tar.gz` (`.zip` sous Windows) ; `SHA256SUMS` ; release GitHub créée seulement sur un tag, notes = section 0.1 de la page de release | `.github/workflows/release.yml` (nouveau) ; `Cargo.toml` racine (`[profile.release]`) seulement si besoin | — (indépendant du code) | `actionlint` sans erreur ; exécuté une fois par `workflow_dispatch` (`publish: false`) sur une branche `wip-*` poussée (règle CI d'agents-home) : 3 archives et `SHA256SUMS` téléversés, aucune archive ne contient `gh-fake` ni `ghu-fake-editor` (liste du contenu dans le rapport) |
| L5 | architecte (opus) | README (installation depuis une release, jeton de `gh`, écrans et touches, `VISUAL` / `EDITOR` et `code.cmd --wait` sous Windows, limites connues ci-dessous) ; guide de l'essai réel complet `docs/guide-essai-0.1.md` (cas listés plus bas) | `README.md`, `docs/guide-essai-0.1.md` (nouveau) | L1 fusionné (touches et messages définitifs) | markdownlint sans erreur, lignes ≤ 120 ; chaque touche citée existe dans l'aide `?` de la branche fusionnée |
| L6 | qa (sonnet) | Vérification complète de `main` après L1 à L5, puis CI verte sur les 3 OS | lecture ; tests seulement si un trou apparaît | L1 à L5 | les 3 commandes de `CLAUDE.md` vertes ; CI de `main` verte (Linux, macOS, Windows) ; `cargo test --workspace` 5 fois de suite sans échec (test instable) |
| L7 | relecteur (opus) | Relecture courte de L2 et L4 (L4 d'office : release, ce qui est publié) | lecture seule | L6 | OK, ou constats renvoyés à L2 / L4 |
| L8 | utilisateur | **Essai réel complet** sur l'artefact du `workflow_dispatch` de `main` (L4), Linux/WSL et Windows ; fiche en artifact (build, commit, SHA-256 de `ghu`) préparée par la session principale d'après `docs/guide-essai-0.1.md` | — | L6, L7 | toutes les étapes OK, ou défauts consignés et corrigés (boucle : lot du périmètre touché, puis `qa`, puis re-test de l'étape seule) ; Q2 (`docs/questions-ouvertes.md`) tranchée d'après l'étape notepad |
| L9 | session principale | Tag `v0.1.0` sur le commit essayé, release publiée par L4, vérification des assets et de `SHA256SUMS`, `docs/reprise.md` | — | L8 | release « Latest » avec 3 archives et `SHA256SUMS` ; le SHA-256 de l'exécutable Windows publié égale celui de la fiche d'essai (même commit) |

Avant le tag (L9), la session principale relit les critères d'acceptation de la spec #1 et confirme qu'aucun n'est
resté sans ticket fermé (à vérifier : non relu ici, l'architecte n'a pas lancé `gh`).

## Ordre

1. L1 (en cours) → `qa` → relecteur ×2 (Spec, Standards) → fusion `Closes #9`. L4 peut partir dès maintenant :
   il ne touche aucun fichier de L1.
2. Après la fusion de #9 : **L2 ‖ L3 ‖ L5** (fichiers disjoints : `ui.rs` / `status.rs` / `README.md` et le
   guide), avec L4 s'il n'est pas fini.
3. L6 (vérification complète et CI) → L7 (relecture) → L8 (essai réel) → corrections éventuelles → L9 (tag, release).

**Goulot** : la fusion de #9 dans `main` (L2, L3, L5 l'attendent) ; ensuite l'essai réel de l'utilisateur (L8),
qui ne se parallélise pas.

Fichiers partagés : `crates/ghu/src/ui.rs` → L2 seul ; `Cargo.toml` racine → L4 seul, et seulement
`[profile.release]` ; `README.md` → L5 seul ; `docs/reprise.md` → session principale ; `Cargo.lock` → personne
(aucune dépendance nouvelle).

## Essai réel complet (contenu du guide, L5)

Sur un vrai compte GitHub de l'utilisateur, avec un repo d'essai à lui (aucune action sur un repo d'autrui) :

1. Démarrage : jeton de `gh` trouvé ; sans jeton, message clair ; `ghu --help`, `ghu --version`.
2. Liste des repos (les siens, collaborateur, organisations), filtre `/`, saisie libre `o` + `owner/name`,
   épinglage ; repo archivé masqué, fork marqué.
3. PRs d'un repo : ouvertes, filtre fermées / fusionnées, sélection bornée.
4. Détail : description Markdown, état, branches, verdicts, conversation, `r`, `Esc`.
5. Diff : liste des fichiers, couleurs, `n` / `p`, défilement, fichier binaire.
6. Comment : saisie, confirmation, publication, détail rechargé.
7. Review : Approve sans texte, Request changes avec texte ; sur une Own PR, verdicts absents.
8. Éditeur (#9) : vim sous WSL (texte revenu, `:cq` sans effet sur le texte), notepad sans puis avec une fenêtre
   déjà ouverte (notice `ReturnedAtOnce` attendue), `code.cmd --wait`, redimensionnement pendant l'édition,
   aucun `ghu-*.md` restant dans le dossier temporaire.
9. Terminal : sortie par `q` et par Ctrl-C, terminal rendu propre (Windows Terminal et terminal WSL).

## Limites connues de la 0.1 (pour les notes de release)

Reprises de `docs/reprise.md` (Points ouverts), sans effet bloquant :

- Largeur du texte : grappes heuristiques (ZWJ, largeur nulle), pas la segmentation Unicode complète ; curseur
  possiblement décalé avec des caractères larges.
- Le message du 401 n'a pas le préfixe « Cannot … » des autres erreurs.
- Faux serveur (tests seulement) : `commit_id` en majuscules ou étranger à la PR accepté ; `RecordedRequest` sans
  en-têtes ; un seul code 5xx (500). Sans effet sur l'exécutable livré.
- macOS : construit, jamais essayé.
