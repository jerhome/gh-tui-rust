# Point de reprise (versionné)

Mis à jour le 2026-11-27 13:22.

## État

- Cadrage fini : `GLOSSARY.md`, `docs/roadmap.md`, ADR 0001 à 0003. Spec 0.1 : issue #1, tickets #2 à #11.
  Méthode : skills mattpocock combinés aux agents d'agents-home (`CLAUDE.md`).
- **Fermés : #2 à #8 et #11 (8 sur 10).** Restent #9 (saisie dans `$EDITOR`) et #10 (Review Approve / Request
  changes), vague 4 de l'ADR 0003.
- ADR 0003 acceptée le 2026-11-27 (deux relectures), § 4 amendé après les relectures de #8 : brouillon rendu après un
  échec de publication (`Signal::Kept`), publication incertaine (réseau, réponse illisible, 5xx) =
  « may have been published », `q` sans effet en Confirm et Publishing.
- sccache activé dans les consignes des agents qui compilent (`RUSTC_WRAPPER=sccache`, `SCCACHE_CACHE_SIZE=3G`,
  binaire dans `~/.local/bin`) : environ -35 % sur le build d'un nouveau worktree (essai du 2026-11-27).
- Skills locaux `.claude/skills/retro` (fusion mattpocock + agents-home) et `.claude/skills/handoff` (vers ce
  fichier), lancés par l'utilisateur seulement.
- Essai à la main possible : `cargo run -p gh-fake -- --latency-ms 800`, puis
  `GHU_API_URL=<url affichée> GH_TOKEN=ghu-fake-token cargo run -p ghu` (détail : Enter sur une PR ; d Diff ;
  c Comment, confirmation, publication sur le faux).

## Prochaine action

Fermés : #2 à #8, #10, #11 (9 sur 10). main = 101805f, poussé.

1. **#9 (saisie dans `$EDITOR`)** : code fait par dev[tui-editor] (opus), commité sur la branche **wip-9-editor**
   (ffc5a86, poussée), partie de main 131bc6b (avant #10). Reste : fusionner main dans wip-9-editor (conflit possible
   dans `crates/ghu/tests/comment.rs` : test `ctrl_e_has_no_effect…` retouché par #9 et #10, nom périmé), puis
   qa, relecteur ×2 (Spec, Standards ; d'office : processus externe, fichier temporaire, terminal), corrections,
   fusion `Closes #9`. La boucle réelle de `terminal.rs` n'est testée par rien : **essai réel par
   l'utilisateur** (fiche artifact, cas listés dans le rapport du dev : vim sous WSL, :cq, notepad sans et avec
   fenêtre ouverte, `code.cmd --wait`, redimensionnement, pas de `ghu-*.md` restant).
2. Puis architecte : retirer les `#[allow(dead_code)]` de `ui.rs`, trancher le 502 du faux (voir Points ouverts),
   et la spec #1 est complète → version 0.1 (essai réel complet, release).
3. Rétro quand l'utilisateur la demande (skill local `retro`, retours dans `docs/retours-agents.md`).

## Points ouverts

- ADR 0003, lignes tui-compose et tui-review des Lots : « 500 et 502 au harnais », mais le faux serveur n'a pas de 502
  (502 couvert par le test unitaire d'`uncertain`) : amender ou ajouter un 502 au faux (architecte).
- `ui.rs` : retirer les `#[allow(dead_code)]` devenus inutiles (`Outcome::Files`, `Signal::Kept`,
  `Target.head`…) après la vague 4 (architecte) ; aide de test pour construire `Shared` (test de `pr_detail.rs`).
- Faux serveur : `commit_id` en majuscules accepté, `commit_id` étranger à la PR accepté (GitHub : 422) ; limites
  notées, sans effet sur le client.
- Guide d'essai de #10 : publier un Verdict sur alice/notes#2 (la #1 porte une Pending d'alice : 422 attendu).
- Q2 (éditeur par défaut sous Windows) dans `docs/questions-ouvertes.md`, à trancher après l'essai réel de #9.
- Largeur : grappes heuristiques (ZWJ, largeur nulle), pas la segmentation Unicode complète.
- `error_message` : le 401 n'a pas le préfixe « Cannot … » des autres cas.
- `RecordedRequest` du faux serveur sans en-têtes.
- Restauration du terminal (et suspension pour l'éditeur) : à essayer par l'utilisateur dans un vrai terminal,
  Windows compris.
- Retours sur les agents pour la rétro : `docs/retours-agents.md`.

## Skills suggérés

- `tdd` et `code-review` (mattpocock) pour finir #9 et #10 ; agents `qa` puis `relecteur` ×2 (Spec, Standards).
- `handoff` (local) en fin de session ; `retro` (local) quand l'utilisateur la demande.
