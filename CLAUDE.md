# gh-tui-rust — Instructions projet

Client GitHub en TUI (Rust) : consulter ses repos et leurs pull requests, et y faire des revues (simples au début).
Exécutable court : `ghu`. Repo public `jerhome/gh-tui-rust`.

Conventions communes de mes projets Rust : `../agents-home/docs/rust/CONVENTIONS.md` (Home Repo `../agents-home`,
lis son `AGENTS.md`). En cas de conflit, ce fichier et les ADR de `docs/adr/` priment.

## Cadrage (décisions de l'utilisateur, 2026-11-26)

- **Banc d'essai des agents de base** : ce repo n'a **pas** de `.claude/agents/` et n'en aura pas. Il utilise les cinq
  agents d'agents-home (`architecte`, `dev`, `qa`, `relecteur`, `plateforme`) pour les éprouver ; leurs défauts
  sont notés dans `docs/retours-agents.md` pour la rétro, jamais corrigés par une fiche locale.
- **Authentification** : le jeton de `gh` (`GH_TOKEN`, sinon `GITHUB_TOKEN`, sinon `gh auth token`). Rien n'est
  stocké par l'application ; le jeton n'apparaît ni dans les logs, ni dans les `Debug`, ni dans les erreurs.
- **Tests** : jamais le vrai GitHub. Un faux serveur du repo (ou `wiremock`) ; les essais réels sont faits par
  l'utilisateur.
- **Revues** : simples au début (lire le diff, commenter, approuver, demander des changements). Toute action qui
  publie sur GitHub (commentaire, revue) demande une confirmation dans l'interface.

## Agents

Agents d'agents-home, lancés par leur nom depuis une session `cc`. Consigne de lancement : repo cible
`../gh-tui-rust` (ou son worktree), `CARGO_TARGET_DIR` éventuel, autres agents actifs et disque libre.

| Agent | Modèle | Périmètre dans ce repo |
|---|---|---|
| `architecte` | opus | `docs/adr/`, `docs/roadmap.md`, `docs/questions-ouvertes.md`, signatures publiques des crates |
| `dev` | sonnet | un périmètre par crate, listé ci-dessous (« Périmètre : client », par exemple) |
| `qa` | sonnet | tests (`tests/`, modules `#[cfg(test)]`), données de test (fixtures JSON) |
| `relecteur` | opus | lecture seule ; d'office sur réseau, jeton, parsing des réponses de l'API, nouvelle dépendance |
| `plateforme` | sonnet | `.github/`, `scripts/`, `Cargo.toml` racine (`[profile]`), release |

Périmètres `dev` : **à fixer par l'ADR 0001** (découpage en crates). Proposition de départ, à confirmer :

- `client` : `crates/gh-client` — API GitHub (REST/GraphQL), modèles serde, trait d'accès, erreurs typées.
- `tui` : `crates/ghu` — binaire, interface ratatui/crossterm, écrans, raccourcis, confirmations.
- `fake` : `crates/gh-fake` — faux serveur GitHub pour les tests et la démo.

Ordre type d'une fonctionnalité : `architecte` (contrat, lots) → `dev` en parallèle (un par périmètre, fichiers
disjoints) → `qa` → `relecteur` → session principale (commit avec accord). Boucle d'échec : CONVENTIONS.md § 6.

Référence utile : `../jira-rust` (TUI ratatui + faux serveur, même forme de projet).

## Agent skills

### Issue tracker

GitHub Issues of `jerhome/gh-tui-rust`, through `gh` as the `jerhome` account. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See
`docs/agents/triage-labels.md`.

### Domain docs

Single-context: `GLOSSARY.md` at the root (English) and `docs/adr/`. See `docs/agents/domain.md`.

## Vérification (« terminé »)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Documents

- `docs/roadmap.md` : feuille de route et versions.
- `docs/adr/` : décisions (font autorité).
- `docs/questions-ouvertes.md` : seul endroit des questions à l'utilisateur.
- `docs/retours-agents.md` : défauts et réussites des agents de base, pour la rétro d'agents-home.
- `docs/reprise.md` : point de reprise versionné (commun aux postes).
