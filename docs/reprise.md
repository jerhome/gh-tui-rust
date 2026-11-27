# Point de reprise (versionné)

Mis à jour le 2026-11-27 16:14.

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

main = bad2ced (poussé) : #9 fusionné (3a2a24d, `Closes #9`), L2 (`allow(dead_code)` retirés) et L3 (tests 502
HTML / 503 vide). Branche **wip-0.1-release** (6cf1fdc, poussée) = main + workflow de release (L4) : essai à blanc
au push d'une branche `wip-*-release` (le lancement à la main n'est possible que depuis main), exécution
46022428092 verte (3 archives, SHA256SUMS, publication sautée). Plan de livraison : `docs/version-0.1.md` (L1 à L9).

1. **Essai réel de #9 par l'utilisateur** : fiche <https://claude.ai/artifact/6xH9KZCukSRTjnH6YVkBEt> (db, collection
   `essais` ; Claude la lit sans y écrire). Build : archives de l'exécution 46022428092, `ghu.exe` SHA-256
   91dc5998…f22ec0, `ghu` Linux SHA-256 74e13667…a44fd3. Q2 (éditeur par défaut sous Windows) tranchée après.
2. Résultats de L6 (qa : vérification complète + 5 passages) et L7 (relecteur : L2, workflow de release) ; si OK :
   fusionner wip-0.1-release dans main.
3. Puis L8 : essai réel complet de la 0.1 (`docs/guide-essai-0.1.md`, nouvelle fiche avec le build et son
   empreinte), L9 : critères de #1 relus, tag `v0.1.0` (ou `v0.1.0-rc.1` d'abord pour essayer la publication).
4. Rétro quand l'utilisateur la demande (skill local `retro`, retours dans `docs/retours-agents.md`).

## Points ouverts

- **Aide de la saisie inaccessible** : pendant la saisie d'un Comment ou d'une Review, `?` s'écrit dans le texte
  (`ui.rs`), donc l'aide de cette phase (`Ctrl-S`, `Ctrl-E`, `Esc`) ne s'affiche jamais ; seule la ligne
  d'indication montre ces touches. À trancher (architecte) : autre touche d'aide en saisie, ou retirer la table.
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
