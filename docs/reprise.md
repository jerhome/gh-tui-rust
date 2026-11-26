# Point de reprise (versionné)

Mis à jour le 2026-11-26 01:59.

## État

- Cadrage fini : `GLOSSARY.md`, `docs/roadmap.md`, ADR 0001. Spec 0.1 : issue #1. Tickets #2 à #11 (sous-issues de
  #1, blocages natifs). Méthode : skills mattpocock combinés aux agents d'agents-home (`CLAUDE.md`).
- **#2 (tracer bullet)** : code fait par architecte + dev[fake] + dev[client] + dev[tui], commité sur `main`.
  fmt, clippy et `cargo test --workspace` verts (106 tests). **Pas encore fermé** : restent `qa` puis
  `code-review` (deux `relecteur` : Standards et Spec), corrections, puis commit `Closes #2`.
- Essai à la main possible : `cargo run -p gh-fake -- --latency-ms 800`, puis
  `GHU_API_URL=<url affichée> GH_TOKEN=ghu-fake-token cargo run -p ghu` (?, Esc, q, Ctrl-C).

## Prochaine action (suite de `/mattpocock-skills:implement #2`)

1. `qa` : vérification complète du workspace (signaler `anyhow` inutilisé dans `crates/ghu/Cargo.toml` et le champ
   `repository` inutilisé du `Cargo.toml` racine → architecte).
2. Skill `code-review` depuis `0c852d1` : axe Standards et axe Spec confiés chacun à un `relecteur` (opus).
3. Boucle d'échec, puis commit `Closes #2` et push (accord permanent).
4. Ensuite : #3, #4, #5, #11 débloqués (`/mattpocock-skills:implement-spec` ou `implement` ticket par ticket).

## Points ouverts

- 429 sans en-tête de limite → `Error::Api` plutôt que `RateLimited` : à trancher (architecte).
- `RecordedRequest` du faux serveur sans en-têtes (le client teste ses en-têtes avec un serveur scripté).
- Restauration du terminal après panique et Windows : non vérifiés (essai utilisateur, CI #11).
- Q1 (langue de l'interface) : anglais par défaut, à confirmer (`docs/questions-ouvertes.md`).
- Retours sur les agents pour la rétro : `docs/retours-agents.md`.
