# Point de reprise (versionné)

Mis à jour le 2026-11-27 16:21.

## État

- Cadrage fini : `GLOSSARY.md`, `docs/roadmap.md`, ADR 0001 à 0003. Spec 0.1 : issue #1, tickets #2 à #11.
  Méthode : skills mattpocock combinés aux agents d'agents-home (`CLAUDE.md`).
- **Fermés : #2 à #11 (10 sur 10)** : la spec #1 est complète ; reste la livraison de la 0.1 (`docs/version-0.1.md`).
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

main = 8561fff (poussé) : #9 fusionné, L2 à L7 faits (L6 qa OK : 6 passages sans échec, CI verte sur les 3 OS ;
L7 relecteur : 2 tours, flux de release corrigé). Workflow de release : un tag `vX.Y.Z-rc.N` construit et publie une
pré-release ; le tag final `vX.Y.Z` sur le même commit **republie les archives de la rc sans recompiler** (job
`promote`) ; essai à blanc au push d'une branche `wip-*-release`. Toolchain fixée (`rust-toolchain.toml`,
1.100.0). Plan de livraison : `docs/version-0.1.md` (L8 et L9 restent).

1. **Essai réel de #9 par l'utilisateur** : fiche <https://claude.ai/artifact/6xH9KZCukSRTjnH6YVkBEt> (db, collection
   `essais` ; Claude la lit sans y écrire). Build : archives de l'exécution 46022428092, `ghu.exe` SHA-256
   91dc5998…f22ec0, `ghu` Linux SHA-256 74e13667…a44fd3. Q2 (éditeur par défaut sous Windows) tranchée après.
2. **L8** (accord de l'utilisateur pour le tag) : tag `v0.1.0-rc.1` sur main → pré-release ; essai réel complet
   (`docs/guide-essai-0.1.md`, nouvelle fiche avec le tag et les SHA-256 des assets de la pré-release). Relire avant
   le tag la phrase « full manual trial » de `docs/release-notes-0.1.md`. Les jobs `publish` et `promote` n'ont
   jamais tourné : la rc.1 est leur premier essai (un tag raté se supprime et se repose, une rc ne se déplace jamais).
3. **L9** : critères de #1 relus, tag `v0.1.0` (promotion), vérification des assets.
4. Rétro quand l'utilisateur la demande (skill local `retro`, retours dans `docs/retours-agents.md`).

## Points ouverts

- **Aide de la saisie inaccessible** : pendant la saisie d'un Comment ou d'une Review, `?` s'écrit dans le texte
  (`ui.rs`), donc l'aide de cette phase (`Ctrl-S`, `Ctrl-E`, `Esc`) ne s'affiche jamais ; seule la ligne
  d'indication montre ces touches. À trancher (architecte) : autre touche d'aide en saisie, ou retirer la table.
- Mineur de relecture L7 (reporté) : `promote` compare le commit actuel du tag rc, pas celui du build ; parade
  retenue : une rc ne se déplace jamais (option : inscrire le SHA du build dans la pré-release).
- Constat mineur de relecture de #9 non corrigé : touches tapées très vite juste après `Ctrl-E` (vidange du canal et
  du tampon crossterm faite ; non testable sans pty : étape W7 de la fiche).

- Aide de test pour construire `Shared` (test de `pr_detail.rs`).
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

- `code-review` (mattpocock) avec `relecteur` pour L7 ; agent `plateforme` si le workflow de release échoue.
- `handoff` (local) en fin de session ; `retro` (local) quand l'utilisateur la demande.
