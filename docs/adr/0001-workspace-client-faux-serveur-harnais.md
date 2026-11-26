---
status: accepted
---

# Workspace en trois crates, client reqwest/rustls, boucle tokio non bloquante, faux serveur HTTP et harnais unique

Le spec #1 impose un client GitHub REST, un binaire `ghu` et un faux serveur, tous testés par un seul point
d'entrée (touches → écran du `TestBackend` de ratatui, contre le faux serveur en HTTP réel). On fige ici ce qui
coûterait cher à défaire : le découpage en crates et le sens des dépendances, la pile HTTP/TLS, le modèle
d'exécution de l'interface, la forme du faux serveur et celle du harnais. Signatures et doc de contrat : `crates/`.

## Décision

1. **Crates** (`crates/<nom>`, édition 2024, MIT) et règle de dépendances :
   - `gh-client` (bibliothèque) : routes REST v3, modèles publics (`Repo`, `RepoName`, `Affiliation`), `Token`,
     `ApiBase`, un seul type d'erreur `Error` (`thiserror`, `#[non_exhaustive]`). Ne dépend d'aucun crate du
     workspace ; ne lit jamais l'environnement et ne lance jamais `gh`.
   - `gh-fake` (bibliothèque + binaire `gh-fake`) : faux GitHub. Ne dépend d'aucun crate du workspace, **pas même
     des modèles de `gh-client`** : son JSON est écrit d'après la doc GitHub, si bien qu'un décodage faux du client se
     voit dans les tests au lieu d'être recopié des deux côtés.
   - `ghu` (bibliothèque + binaire `ghu`) : démarrage, environnement, interface. Dépend de `gh-client` ;
     `gh-fake` seulement en `dev-dependencies`. `main.rs` est une coque mince autour de la bibliothèque.
   - Pas de crate « domaine » séparé : les règles métier de la 0.1 (Own PR, PR non ouverte, Archived repo) sont des
     fonctions pures de `ghu`, sans I/O ; un crate `gh-domain` ne se justifierait qu'avec un second client.
2. **HTTP** : `reqwest` 0.13 sans TLS par défaut, `rustls` 0.23 avec le fournisseur `ring` et le magasin de
   certificats de l'OS (`rustls-platform-verifier`), comme `emu-rust` (`crates/emu-assets/src/http.rs`). Pas
   d'OpenSSL ni d'`aws-lc-rs` (mesuré : `cargo tree -i aws-lc-rs` vide), qui demanderait une chaîne C par cible
   Windows. REST v3 seulement, pagination par l'en-tête `Link` suivie dans le client ; un lien `next` hors de
   l'origine de l'API est refusé (le jeton ne part jamais ailleurs).
3. **Pas de trait d'accès à l'API** : `gh_client::Client` est un type concret. Le seul double de test est le faux
   serveur, atteint en HTTP réel ; un trait `GitHubApi` avec une implémentation en mémoire (forme de `jira-rust`)
   doublerait le travail et testerait un chemin que l'utilisateur ne prend pas.
4. **Exécution** : tokio. `ghu::Ui<B: Backend>` ne fait jamais d'`await` réseau dans la boucle : chaque appel part
   dans une tâche tokio avec un clone du client, et son résultat revient par un canal, appliqué par
   `Ui::recv_background` (annulable, pour `tokio::select!`). Le clavier est lu par un thread OS dédié (crossterm
   bloquant, poll de 200 ms, thread `ghu-keyboard`). Une panique de tâche devient une erreur affichée
   (`TaskFailed`) et ne touche pas au terminal. Messages à l'utilisateur : un par variante d'erreur
   (`error_message`), jamais le `Display` de l'erreur.

   **Terminal et paniques** (correctif de la relecture #2) : `terminal::run` n'appelle pas `ratatui::try_init`,
   dont le hook de panique, global au processus, restaurerait le terminal même pour une panique de tâche tokio
   rattrapée, l'interface continuant alors d'afficher sur un terminal déjà rendu. `run` active elle-même le mode
   brut, l'écran alternatif et le bracketed paste, chaîne son hook devant le hook précédent et remet ce dernier en
   sortant. Le hook ne restaure le terminal (`restore_terminal`, qui coupe aussi le bracketed paste) que si le
   thread qui panique est marqué « thread d'interface » (`thread_local`) : le thread de `run` et le thread
   `ghu-keyboard`. Ailleurs, seul le hook précédent s'exécute. La décision est une fonction pure testée,
   `restore_if_ui(is_ui, restore)` (`crates/ghu/src/terminal.rs`). Contrainte : `run` reste sur un thread fixe
   (`block_on` dans `main`), jamais dans une tâche tokio qui pourrait changer de thread.

5. **Jeton et URL** : lus par `ghu::prepare` au travers du trait `ghu::Environment` (système en production,
   table en mémoire dans les tests : aucun test ne modifie l'environnement du processus, `unsafe` en 2024). Ordre :
   `GH_TOKEN`, `GITHUB_TOKEN`, sortie de `gh auth token` ; une valeur vide compte comme absente. Sans jeton ou
   avec une URL invalide, `ghu` écrit un message sur stderr et sort (code 1) avant d'ouvrir l'interface ; un 401
   s'affiche dans l'interface. `GHU_API_URL` règle l'URL (défaut `https://api.github.com`) ; `http` n'est accepté
   que vers une adresse de bouclage. `gh_client::Token` n'a ni `Display` ni accesseur public, et son `Debug` est
   masqué.
6. **Faux serveur** : axum sur `127.0.0.1` seulement, port libre choisi par l'OS ; état public (`gh_fake::State` :
   utilisateur, repos, latence, échecs injectés, requêtes reçues) lu et modifié pendant que le serveur tourne
   (`FakeGitHub::with_state`). Le binaire `gh-fake` sert les données de démo pour un essai à la main.
7. **Harnais** : un seul point de test, `crates/ghu/tests/support/mod.rs` : `launch` (arguments → code de sortie,
   stdout, stderr) et `Harness` (faux serveur + `Ui<TestBackend>` 100 × 30 ; `press`, `type_text`, `screen`,
   `wait_for`, `settle`) ; `LogCapture` pour vérifier qu'aucun log ne contient le jeton. Des tests unitaires
   restent permis là où le harnais n'atteint pas (analyse de `Link`, validation d'`ApiBase` et de `RepoName`,
   `Debug` de `Token`, routes du faux serveur).
8. **Journaux** : `tracing` ; écriture seulement si `GHU_LOG_FILE` est défini (niveau `GHU_LOG`, défaut `info`),
   jamais sur la sortie du terminal pendant l'interface.

## Options écartées

- **Trait `GitHubApi` + mock en mémoire** (`jira-rust`) : deux implémentations à garder d'accord, contraire au point
  de test unique du spec. Reviendrait si les tests HTTP devenaient trop lents (plus de 30 s pour `cargo test`).
- **`wiremock`** à la place d'un faux serveur : pas d'état modifiable (un Comment publié doit réapparaître à la
  lecture suivante) ni de démo lançable.
- **`reqwest` avec la fonctionnalité `rustls`** (fournisseur `aws-lc-rs`, choix de `jira-rust`) : chaîne C et
  parfois NASM pour Windows ; `ring` est déjà éprouvé en compilation croisée par `emu-rust`.
- **`octocrab`** : grosse API générée, modèles hors de notre contrôle (Debug du jeton, erreurs), pour six routes.
- **Message « pas de jeton » dans l'interface** : rien à afficher d'autre ; stderr est plus simple et lisible par un
  script.

## Conséquences

- **Erreurs et pannes posées dès #2** (relecture Spec du ticket #2) : `gh_client::Error` garde ses variantes
  `InvalidRepoName`, `RateLimited`, `Forbidden`, `NotFound`, `Unprocessable`, et `gh_fake::FailureResponse`
  ses pannes 403, 403 de limite, 404, 422 et 500, bien que #2 ne les affiche pas toutes. Elles servent #4, #8 et
  #10 ; `Error` étant `#[non_exhaustive]`, les ajouter plus tard ne casserait rien, mais les retirer puis les
  remettre coûterait deux passes de client, faux serveur et tests, pour une classification HTTP déjà testée en
  unitaire (`crates/gh-client/src/status.rs`). Option écartée : les retirer jusqu'au ticket qui les affiche
  (surface plus petite, mais travail refait). Reviendrait si une variante restait sans usage après #10.
- **429** : toute réponse 429 est `Error::RateLimited` (`reset_at` lu dans `x-ratelimit-reset` s'il est un
  nombre, sinon `None` ; `retry-after` n'est pas converti, faute d'horloge dans la classification). 429 veut dire
  « trop de requêtes » quel que soit son corps, et l'interface doit proposer d'attendre, pas afficher une erreur
  générique. Le 403 reste classé par son marqueur (`x-ratelimit-remaining: 0` ou message de limite secondaire).
  À appliquer par dev[client] : `classify` de `status.rs` et son test `a_429_without_marker_is_a_plain_api_error`
  (qui devient `RateLimited { reset_at: None }`), avec la doc de `Error::RateLimited` (`error.rs`).
- Existant : aucun code livré avant cette ADR ; rien ne change pour les docs.
- Chaque ticket suivant ajoute une méthode à `Client`, une route et des champs à `gh_fake::State`, un écran à
  `ghu`, et ses tests au harnais, sans changer ces signatures (types `#[non_exhaustive]`, `Context` opaque).
- L'éditeur externe (#9) devra suspendre le terminal depuis l'interface : point de variation à poser dans #9
  (méthode de plus sur `Environment` et sur `Ui`), le faux éditeur étant un programme lancé par `$EDITOR`.
- Réversibilité : le découpage et le harnais sont coûteux à défaire après quelques tickets ; la pile TLS se change
  dans `gh-client` seul.
