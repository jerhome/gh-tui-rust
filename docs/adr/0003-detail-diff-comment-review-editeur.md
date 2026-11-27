---
status: accepted
---

# Détail d'une PR, diff, Comment, Review et saisie dans l'éditeur externe

Statut : **acceptée** (2026-11-27 11:45). Proposée le 2026-11-26 17:07, amendée le 2026-11-27 d'après la relecture
`.claude/rv-adr-0003.md` (4 majeurs, 7 mineurs) ; seconde relecture OK avec trois mineurs (n1 mode `keep` du faux
éditeur, n2 départ de la vague 2, n3 seuil `ReturnedAtOnce` réglable), corrigés à l'acceptation.
**Amendée le 2026-11-27 (12:49)** d'après les relectures de #8 (Comment) : § 3 (`Signal::Kept`, `Draft`), § 4 (texte
gardé après un échec, publication incertaine, touches de Confirm et de Publishing, titre `Files (<N>)` du Diff),
lignes tui-compose et tui-review des Lots. Choix réversibles, internes à `ghu`.
Les relectures de #3, #4 et #5 sont faites et commitées (3e7c5f9, cc81eb6) et l'ADR 0002 est acceptée : les
anciens points « à confirmer après #3-#5 » sont tranchés d'après le code de `main` (fefff13) et remplacés ci-dessous
par la décision confirmée.

Les tickets #6 (détail d'une PR), #7 (Diff), #8 (Comment), #9 (saisie dans `$EDITOR`) et #10 (Review Approve /
Request changes) ajoutent sept routes au client et au faux serveur, quatre écrans à `ghu`, la première requête
qui **publie** sur GitHub, et le premier programme externe lancé pendant l'interface. On fige ici ce qui coûterait cher
à défaire ou à refaire en parallèle : les modèles publics de `gh-client` (API entre crates), l'état public de
`gh-fake`, les ajouts au socle `ui.rs` (fichier goulot), le protocole de suspension du terminal, la forme du faux
éditeur, et les règles de publication. Signatures : en blocs de code ci-dessous ; la vague 0 (§ Lots) les recopie
dans `crates/` avec des corps `todo!()`.

Règles de cadrage reprises (`docs/roadmap.md` points 10 à 12, spec #1) : confirmation avant toute publication ;
Own PR = Verdicts masqués ; Pending reviews ignorées ; rechargement après publication ; Markdown rendu simple ; zone
de texte + touche vers l'éditeur externe.

## Décision

### 1. Client : sept routes, modèles publics

Toutes les routes suivent la forme existante (`client.rs`) : URL bâtie depuis `RepoName` et `PrNumber`, en-têtes
communs, pas de redirection suivie, erreurs classées par `status::classify` puis `redact`, listes paginées par
`paginate` (`per_page=100`, dédoublonnage par `id`, ou par `filename` pour les fichiers). Réponses tirées de la
doc REST (<https://docs.github.com/en/rest>, version d'API `2022-11-28`) ; fixtures JSON copiées des exemples de
l'OpenAPI `github/rest-api-description`, comme pour `pulls.json`.

| Méthode | Route | Réponse (champs lus) | Ticket |
|---|---|---|---|
| `current_user` | `GET /user` | objet user : `login` | #6 (Own PR) |
| `pull` | `GET /repos/{o}/{r}/pulls/{n}` | `number`, `title`, `user` (nullable), `state`, `draft`, `merged_at`, `body` (nullable), `head.label`, `head.ref`, `head.sha`, `base.ref`, `base.repo.archived`, `mergeable` (`true` / `false` / `null`), `created_at`, `updated_at`, `additions`, `deletions`, `changed_files` | #6 |
| `reviews` | `GET /repos/{o}/{r}/pulls/{n}/reviews` | tableau, ordre chronologique : `id`, `user` (nullable), `body`, `state` (`APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, `DISMISSED`, `PENDING`), `submitted_at` (absent pour `PENDING`) | #6 |
| `comments` | `GET /repos/{o}/{r}/issues/{n}/comments` | tableau, `id` croissant : `id`, `user` (nullable), `body` (facultatif), `created_at` | #6 |
| `pull_files` | `GET /repos/{o}/{r}/pulls/{n}/files` | tableau (3000 fichiers au plus côté GitHub) : `filename`, `previous_filename` (renommage), `status` (`added`, `removed`, `modified`, `renamed`, `copied`, `changed`, `unchanged`), `additions`, `deletions`, `patch` (absent pour un binaire ou un diff trop gros) | #7 |
| `post_comment` | `POST /repos/{o}/{r}/issues/{n}/comments`, corps `{"body"}` | 201, un issue comment | #8 |
| `post_review` | `POST /repos/{o}/{r}/pulls/{n}/reviews`, corps `{"event", "body"?, "commit_id"}` | 200, une review | #10 |

Une PR est une issue pour GitHub : les Comments de la conversation passent par la route `issues`, pas par
`pulls/{n}/comments` (commentaires de ligne, 0.2).

```rust
// gh-client/src/model.rs (additions). All new structs are #[non_exhaustive] and built only by this crate.

/// The signed-in user (GET /user). Used to tell an Own PR.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct User {
    /// Login, as GitHub writes it (compare ASCII case-insensitively).
    pub login: String,
}

/// A commit id: 40 (SHA-1) or 64 (SHA-256) lowercase hex digits. Invariant checked by parse.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CommitSha(String);
impl CommitSha {
    /// None for anything else (uppercase, other length, non-hex). Pure, never panics.
    pub fn parse(text: &str) -> Option<Self>;
    pub fn as_str(&self) -> &str;
    /// First 7 characters, for display.
    pub fn short(&self) -> &str;
}

/// GitHub's mergeability of an open PR (field `mergeable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mergeable {
    /// `true`: can be merged.
    Yes,
    /// `false`: conflicts with the base branch.
    Conflicts,
    /// `null`: GitHub is still computing it (a refresh may tell).
    Unknown,
}

/// One PR with everything the detail screen shows (GET /repos/{o}/{r}/pulls/{n}).
/// Distinct from PullRequest (list rows), whose doc stops promising detail fields.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PullDetail {
    pub number: PrNumber,
    pub title: String,
    /// `user.login`, `ghost` when `user` is null.
    pub author: String,
    /// Same mapping as PullRequest::state.
    pub state: PrState,
    /// Markdown description; None when null or empty.
    pub body: Option<String>,
    /// `head.label` (`owner:branch`): the source branch, fork owner included.
    pub head_label: String,
    /// `base.ref`: the target branch.
    pub base_ref: String,
    /// `head.sha`: the commit a Review is given on (post_review).
    pub head_sha: CommitSha,
    /// `base.repo.archived`, read at each load (a repo archived during the session is seen after `r`).
    /// false when `base.repo` is null or `archived` is absent (GitHub then refuses with 403, which has its message).
    pub archived: bool,
    pub mergeable: Mergeable,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    /// Lines added, lines deleted, files changed (0 when GitHub omits them).
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
}

/// The state of a submitted review. PENDING reviews never reach the caller (glossary: Pending review, ignored).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    /// A review without verdict (body and/or line comments).
    Commented,
    /// A verdict later dismissed by a maintainer.
    Dismissed,
}

/// A submitted review (GET .../pulls/{n}/reviews, POST .../pulls/{n}/reviews).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Review {
    pub id: u64,
    /// `ghost` when `user` is null.
    pub author: String,
    pub state: ReviewState,
    /// None when empty.
    pub body: Option<String>,
    /// None when GitHub omits it.
    pub submitted_at: Option<Timestamp>,
}

/// A Comment of the PR conversation (issue comment).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Comment {
    pub id: u64,
    /// `ghost` when `user` is null.
    pub author: String,
    /// Markdown; empty string when absent.
    pub body: String,
    pub created_at: Timestamp,
}

/// `status` of a changed file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileStatus { Added, Removed, Modified, Renamed, Copied, Changed, Unchanged }

/// One changed file of a PR (GET .../pulls/{n}/files).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileChange {
    /// `filename`: path in the head commit.
    pub path: String,
    /// `previous_filename`, for Renamed (and Copied).
    pub previous_path: Option<String>,
    pub status: FileStatus,
    pub additions: u64,
    pub deletions: u64,
    /// Unified diff hunks (no file header). None when GitHub sends none or an empty one: binary file, diff too
    /// large, or rename without change.
    pub patch: Option<String>,
}

/// The decision a review carries (glossary: Verdict).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Verdict { Approve, RequestChanges }

/// Most characters GitHub accepts in a Comment or Review body.
pub const MAX_BODY_CHARS: usize = 65_536;

/// Text to publish: not blank, at most MAX_BODY_CHARS characters (Unicode scalar values). Sent as is.
/// Debug shows the length only (the text may be private and must not reach logs).
#[derive(Clone, PartialEq, Eq)]
pub struct MessageBody(String);
impl MessageBody {
    /// Errors: Error::InvalidBody { problem: BodyProblem::Blank } when `text.trim()` is empty,
    /// BodyProblem::TooLong { chars } above MAX_BODY_CHARS.
    pub fn new(text: &str) -> Result<Self, Error>;
    pub fn as_str(&self) -> &str;
}

/// Why a text cannot be published.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyProblem {
    Blank,
    TooLong { chars: usize },
}

/// A Review to publish. A Request changes without text cannot be built (glossary: Review).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewRequest {
    Approve { body: Option<MessageBody> },
    RequestChanges { body: MessageBody },
}
impl ReviewRequest {
    pub fn verdict(&self) -> Verdict;
}
```

```rust
// gh-client/src/error.rs: one variant more (Error stays #[non_exhaustive]).
/// A text refused before sending (MessageBody::new). No request was made.
#[error("invalid text: {problem:?}")]
InvalidBody { problem: BodyProblem },
```

```rust
// gh-client/src/client.rs (additions on Client). Same errors as Client::pulls unless said otherwise.

/// GET /user. Errors: Unauthorized, RateLimited, Forbidden, Network, UnexpectedResponse, Api.
pub async fn current_user(&self) -> Result<User>;
/// GET /repos/{o}/{r}/pulls/{n}. NotFound for a missing PR or repo. Errors also: UnexpectedResponse for an
/// invalid head.sha, number 0, bad timestamp or unknown state.
pub async fn pull(&self, repo: &RepoName, number: PrNumber) -> Result<PullDetail>;
/// GET .../pulls/{n}/reviews, all pages, chronological; PENDING entries dropped; an unknown state is
/// UnexpectedResponse.
pub async fn reviews(&self, repo: &RepoName, number: PrNumber) -> Result<Vec<Review>>;
/// GET /repos/{o}/{r}/issues/{n}/comments, all pages, oldest first.
pub async fn comments(&self, repo: &RepoName, number: PrNumber) -> Result<Vec<Comment>>;
/// GET .../pulls/{n}/files, all pages, GitHub's order; an unknown status is UnexpectedResponse.
pub async fn pull_files(&self, repo: &RepoName, number: PrNumber) -> Result<Vec<FileChange>>;
/// POST /repos/{o}/{r}/issues/{n}/comments with {"body": body}. Expects 201. Never retried. Errors also:
/// Forbidden (no rights, archived repo: 403), Unprocessable (422), NotFound. A Network error does not tell
/// whether GitHub created the comment.
pub async fn post_comment(&self, repo: &RepoName, number: PrNumber, body: &MessageBody) -> Result<Comment>;
/// POST .../pulls/{n}/reviews with {"event": "APPROVE" | "REQUEST_CHANGES", "body" (omitted when None),
/// "commit_id": commit}. Expects 200 and a non-PENDING state (else UnexpectedResponse). Never retried.
/// Unprocessable for a review GitHub refuses (own PR, pending review of this user, closed PR…).
pub async fn post_review(
    &self,
    repo: &RepoName,
    number: PrNumber,
    commit: &CommitSha,
    review: &ReviewRequest,
) -> Result<Review>;
```

Un `post` privé (pendant de `get`) envoie le JSON avec `Content-Type: application/json`, lit la réponse bornée
par `MAX_BODY_BYTES` et la classe comme `get`. `commit_id` est le `head.sha` chargé par l'écran de détail :
la Review porte sur le code que l'utilisateur a vu ; si la branche a bougé, GitHub l'attache à ce commit-là.

### 2. Faux serveur : routes, état, enregistrement des publications

`gh-fake` reste sans dépendance vers le workspace (ADR 0001) ; ses types portent les mêmes noms que la doc, pas
ceux du client.

```rust
// gh-fake/src/state.rs (additions). Public fields, plain data, as today.

pub struct State {
    // ... existing fields ...
    /// Instant written in objects created by POST routes (`created_at`, `submitted_at`, `updated_at` of the
    /// PR). Default "2026-04-01T12:00:00Z"; tests set it to tell old and new entries apart.
    pub now: String,
    /// Next id given to a created comment or review, then incremented. Default 1_000_000.
    pub next_id: u64,
}
impl State {
    /// The PR `number` of `owner/repo` (case-insensitive names), for tests to edit its reviews, comments, files.
    pub fn pull_mut(&mut self, owner: &str, repo: &str, number: u64) -> Option<&mut Pull>;
}

pub struct Pull {
    // ... existing fields ...
    /// `head.sha`. Pull::new: the number as 40 lowercase hex digits (format!("{number:040x}")).
    pub head_sha: String,
    /// `mergeable` (None serves null). Pull::new: Some(true).
    pub mergeable: Option<bool>,
    /// Changed files, in the order served. Pull::new: empty. `additions`, `deletions`, `changed_files` of the
    /// PR are computed from them.
    pub files: Vec<PullFile>,
    /// Reviews, chronological (served in this order, PENDING included as GitHub does for the token's user).
    pub reviews: Vec<PullReview>,
    /// Conversation comments, served sorted by id.
    pub comments: Vec<IssueComment>,
}

/// A changed file (JSON of "List pull requests files").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullFile {
    pub filename: String,
    /// Served as `previous_filename` when Some.
    pub previous_filename: Option<String>,
    pub status: FileStatus,
    pub additions: u64,
    pub deletions: u64,
    /// None: the `patch` key is absent (binary, too large, rename without change).
    pub patch: Option<String>,
}
impl PullFile {
    /// A Modified file whose additions and deletions are counted from `patch` lines starting with `+` / `-`.
    pub fn modified(filename: &str, patch: &str) -> Self;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus { Added, Removed, Modified, Renamed, Copied, Changed, Unchanged }

/// A review (JSON of "List reviews for a pull request").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullReview {
    pub id: u64,
    /// None serves `"user": null`.
    pub author: Option<String>,
    pub state: ReviewState,
    pub body: String,
    /// None: `submitted_at` absent (Pending).
    pub submitted_at: Option<String>,
    pub commit_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewState { Approved, ChangesRequested, Commented, Dismissed, Pending }

/// A conversation comment (JSON of "List issue comments").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueComment {
    pub id: u64,
    pub author: Option<String>,
    pub body: String,
    pub created_at: String,
}
```

Routes ajoutées (même chaîne : latence, enregistrement, authentification, pannes injectées, route) :

- `GET /user` : `{"login", "id", "type": "User"}` de `State::user`.
- `GET /repos/{o}/{r}/pulls/{n}` : 404 si le repo ou la PR manque ; sinon le JSON de la liste plus `head.sha`,
  `mergeable`, `mergeable_state` (`clean` / `dirty` / `unknown`), `merged`, `additions`, `deletions`,
  `changed_files`, `comments` (nombre), `base.repo` (le JSON du repo, avec `archived`).
- `GET .../pulls/{n}/files`, `GET .../pulls/{n}/reviews`, `GET /repos/{o}/{r}/issues/{n}/comments` : 404 si la
  PR manque ; pagination commune (`per_page` 30 par défaut, plafonné par `max_per_page`).
- `POST /repos/{o}/{r}/issues/{n}/comments` : 404 si la PR manque ; repo archivé : 403
  `Repository was archived so is read-only.` ; `body` absent, pas une chaîne ou blanc : 422 `Validation Failed` ;
  sinon ajoute un `IssueComment` (auteur `State::user`, `created_at = now`, `id = next_id`), met
  `updated_at = now` sur la PR, répond 201 avec le commentaire. GitHub accepte un Comment sur une PR fermée : le
  faux aussi (c'est `ghu` qui ne le propose pas).
- `POST .../pulls/{n}/reviews` : 404, 403 archivé comme ci-dessus ; puis 422 `Unprocessable Entity` avec un
  tableau `errors` de chaînes, dans cet ordre : PR non ouverte ; `event` `APPROVE` / `REQUEST_CHANGES` sur une
  PR dont l'auteur est `State::user` (`Can not approve your own pull request` / `Can not request changes on your
  own pull request`) ; `REQUEST_CHANGES` ou `COMMENT` sans `body` non blanc ; une review `Pending` de
  `State::user` existe déjà sur la PR (`User can only have one pending review per pull request`) ; `event`
  inconnu ; `commit_id` présent mais pas 40 ou 64 chiffres hexadécimaux. Sinon ajoute une `PullReview`
  (`event` absent : `Pending`, sans `submitted_at`), répond 200 avec elle.
- **Enregistrement** : rien de nouveau à construire. `RecordedRequest` garde déjà méthode, chemin et corps JSON
  (`State::requests`, `FakeGitHub::requests`) ; un test vérifie la publication par
  `requests().iter().filter(|r| r.method == Method::Post)` et par l'état (le Comment ou la Review ajoutés).
- **Données de démo** (`State::demo`, sans changer le nombre de PRs de `alice/notes`, dont dépendent des tests
  existants) : PR #1 de `alice/notes` avec une description Markdown (titre, gras, liste, bloc de code, image, HTML),
  deux Comments, une Review Approved (carol), une ChangesRequested (dave), une Commented avec texte, une Pending
  d'`alice` (à ignorer), et des fichiers : modifié avec patch, ajouté, supprimé, renommé sans changement, binaire
  sans patch ; une PR ouverte d'`alice` dans `bob/shared-tools` (Own PR) ; une PR ouverte dans
  `acme/legacy-site` (repo archivé) ; une PR avec `mergeable: Some(false)`.

### 3. Socle `ghu/src/ui.rs` : ajouts (vague 0, architecte)

Confirmé d'après le code (fefff13) : `Flow` n'a plus que `Ignored`, `Handled` et `Push` (`ui.rs:161`) ;
`App::key` et `App::apply` empilent tous deux `shared.pushed` après l'appel (`ui.rs:327`, `ui.rs:364`) ; le retour
arrière existant est `App::pop` (`ui.rs:331`). Les ajouts ci-dessous en sont le pendant pour fermer un écran ; ils
réutilisent `App::pop`.

```rust
pub(crate) enum Outcome {
    // ... Repos, Repo, Pulls, TaskFailed ...
    /// The four requests of the PR detail, joined (tokio::try_join!): the first error wins.
    /// Boxed: a PrPage is several times larger than the other variants (clippy large_enum_variant).
    PrPage(gh_client::Result<Box<pr_detail::PrPage>>),
    /// Client::pull_files.
    Files(gh_client::Result<Vec<gh_client::FileChange>>),
    /// Client::post_comment.
    CommentPosted(gh_client::Result<gh_client::Comment>),
    /// Client::post_review.
    ReviewPosted(gh_client::Result<gh_client::Review>),
}

/// News given to the screen that becomes the top one when the screen above closes itself.
/// Not Copy since the amendment of #8: Kept owns the user's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Signal {
    /// A Comment or Review was published on the PR shown below: reload it.
    Published,
    /// The compose screen closed while the message of a failed publication was shown: the PR detail keeps
    /// the text and gives it back to the next compose screen of the same Kind. Nothing is reloaded.
    Kept(Draft),
}

/// Owned by the PR detail until reopened with the same Kind (moved into the compose screen) or the detail is
/// closed (dropped; never on disk). Debug shows the kind and the length only.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Draft {
    pub(crate) kind: compose::Kind,
    /// Never blank.
    pub(crate) text: String,
}

impl Shared {
    /// Asks to close the calling screen once the current call (key or apply) returns, then gives `signal` to
    /// the screen below, which becomes the top (View::signal). Ignored on the root.
    /// Only the top screen may call it: a request made by a lower screen during apply is a bug
    /// (debug_assert in App; ignored with a tracing::warn in release). A call that asks pop returns
    /// Flow::Handled (key) and calls neither Flow::Push nor Shared::push: debug_assert in App; in release the pop
    /// wins and the screens asked for are dropped (tracing::warn).
    pub(crate) fn pop(&mut self, signal: Option<Signal>);
    /// Asks the loop to open the user's editor on `text` (#9). The latest request wins. The answer comes
    /// back through View::edited on the top screen.
    pub(crate) fn request_edit(&mut self, text: String);
}

pub(crate) trait View {
    // ... existing methods ...
    /// Receives a Signal after the screen above closed with Shared::pop. Default: ignored.
    fn signal(&mut self, signal: Signal, shared: &mut Shared) { let _ = (signal, shared); }
    /// The result of the editor asked for with Shared::request_edit: the new text, or why it failed.
    /// Default: ignored.
    fn edited(&mut self, result: Result<String, crate::EditorError>, shared: &mut Shared) {
        let _ = (result, shared);
    }
}

impl<B: Backend> Ui<B> {
    /// The pending editor request, if a screen asked for one (Shared::request_edit). The caller suspends the
    /// terminal, runs EditRequest::run off the async loop (spawn_blocking, § 5; the test harness may call it
    /// directly), resumes, then calls finish_edit. Never blocks.
    pub fn take_edit(&mut self) -> Option<EditRequest>;
    /// Gives the editor's result to the top screen (View::edited). Ignored once finished.
    pub fn finish_edit(&mut self, result: Result<String, EditorError>);
    /// Clears the terminal so the next draw repaints every cell (after the editor used the screen).
    /// Errors: the backend's.
    pub fn clear(&mut self) -> Result<(), B::Error>;
}
```

`Shared` garde la valeur brute de `VISUAL` et `EDITOR` reçue du `Context` (`editor::EditorVars`), et la
requête d'édition en attente. `App` traite, après chaque `key` et chaque `apply`, juste avant les
`stack.append(&mut self.shared.pushed)` existants : si un `pop` est demandé, il vérifie que l'appelant était le
sommet (dans `apply` : l'écran qui a rendu `None` est le dernier de la pile), appelle `App::pop`, puis `signal`
sur le nouveau sommet ; un `pop` accompagné d'écrans à empiler est le bug décrit dans la doc de `pop`. Sans `pop`
demandé, rien ne change pour les écrans existants.

### 4. Écrans (un module par écran, ADR 0002)

Chaque écran a sa table de touches (`Binding`, aucune touche réservée, test `uses_reserved_key`), sa ligne
d'aide d'au plus 26 caractères, et n'affiche du texte venu du réseau qu'à travers `text::clean` (§ 7). Les
textes entre guillemets sont exacts : les tests au harnais les cherchent à l'écran.

```rust
// ui/pr_detail.rs (#6)
/// Everything the detail shows, loaded together.
pub(crate) struct PrPage {
    pub(crate) pull: gh_client::PullDetail,
    pub(crate) reviews: Vec<gh_client::Review>,
    pub(crate) comments: Vec<gh_client::Comment>,
    pub(crate) viewer: gh_client::User,
}
pub(crate) struct PrDetailScreen { /* repo: RepoName, number: PrNumber, content, scroll, pending… */ }
impl PrDetailScreen {
    /// Opened by Enter on the PR list (which passes repo.name.clone()); starts loading the PrPage at once.
    pub(crate) fn new(repo: gh_client::RepoName, number: gh_client::PrNumber, shared: &mut Shared) -> Self;
}
/// What the user may do on this PR (pure, tested in unit tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Actions {
    /// Open, not archived, not own: Comment and both Verdicts.
    All,
    /// Own PR: Comment only.
    CommentOnly,
    /// Nothing that publishes; the reason is shown.
    None(Refusal),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal { Archived, Closed, Merged }
/// Archived (PullDetail::archived, fresh at each load) beats Closed / Merged, which beat Own PR.
/// Own PR: author == viewer, ASCII case-insensitive. The Repo loaded at start-up is not used (it may be stale).
pub(crate) fn actions(pull: &gh_client::PullDetail, viewer: &gh_client::User) -> Actions;
/// Current verdict per reviewer: the latest Approved / ChangesRequested / Dismissed review of each author,
/// in the order of those latest reviews; an author whose latest is Dismissed is left out; Commented is not a
/// verdict.
pub(crate) fn verdicts(reviews: &[gh_client::Review]) -> Vec<(String, gh_client::Verdict)>;
```

**Détail (#6)**. Titre d'en-tête : `<owner/name> · #<n>`. Corps (défilant, renvoi à la ligne) :

- `#<n> <titre>` en gras ;
- `<état> · @<auteur> · <head_label> → <base_ref>` puis, pour une PR ouverte, `· mergeable`, `· conflicts`
  ou `· mergeability unknown` (précédé d'une espace) ; état : `open`, `draft`, `closed`, `merged` ;
- `Created <date> · updated <date> · <changed_files> files · +<additions> -<deletions>` ;
- `Verdicts: @carol Approve · @dave Request changes`, ou `Verdicts: none` ;
- ligne d'actions : `Actions.All` → `c Comment · a Approve · x Request changes · d Diff` ; `CommentOnly` →
  `c Comment · d Diff · Verdicts are not offered on your own PR.` ; `None(Archived)` → `d Diff · This repo is
  archived: read-only.` ; `None(Closed)` / `None(Merged)` → `d Diff · This PR is closed: no comment or review.`
  / `… is merged: …` ;
- `Description` puis la description rendue (§ 4, Markdown), ou `No description.` ;
- `Conversation (<k>)` puis, dans l'ordre chronologique, les Comments (`@<auteur> · <date>`) et les Reviews dont
  le texte n'est pas vide (`@<auteur> · <date> · Approve` / `Request changes` / `review` / `dismissed`), texte
  rendu en retrait de deux espaces ; `No comments.` si `k = 0`. Date absente d'une Review : rangée en fin.

Touches : `Up/k` `Down/j` défiler d'une ligne, `PgUp` `PgDn` d'une page, `r` recharger (contenu gardé jusqu'à
la réponse, seule la dernière requête compte), `d` Diff, `c` Comment, `a` Approve, `x` Request changes ; une
touche d'action non permise par `actions` ne fait rien (`Flow::Handled`, aucun écran). Défilement borné : on ne
descend pas sous la dernière ligne, et `Up` réagit dès la première pression après la butée (le maximum calculé au
rendu est gardé dans une `Cell`). Erreur : `Cannot load the pull request: <raison>` en rouge, `r` réessaie.
`signal(Published)` : recharge, puis défile jusqu'à la fin une fois la réponse arrivée.

**Fins de ligne** (descriptions, Comments, Reviews et patchs) : tout texte venu du réseau est découpé en lignes
comme `str::lines()` (un `\r` juste avant `\n` est retiré : `\r\n` et `\n` finissent une ligne) ; seul un `\r`
isolé, au milieu d'une ligne, arrive à `text::clean` et devient `�`. GitHub renvoie `\r\n` pour un texte tapé sur
le site, et les patchs de fichiers CRLF gardent leurs `\r`.

**Markdown rendu simple** (`ui/text.rs`, ligne par ligne après le découpage ci-dessus, sans dépendance) :
`#` à `######` + espace → titre en gras (sans les `#`) ; tiret, astérisque ou plus suivi d'une espace → puce
`•` ; numéro suivi d'un point : gardé ; blocs délimités par trois accents graves ou par `~~~` → lignes en retrait
de deux espaces, style atténué, rien d'interprété dedans ; `**x**` et `__x__` → gras ; `` `x` `` → style code ;
liens, images, HTML, tableaux, citations : laissés tels quels (texte). Aucun cas ne panique (entrée quelconque,
test).

```rust
// ui/text.rs (#6; used by every screen that shows network text)
/// Text safe to draw: tab → 4 spaces; other control characters (C0, DEL, C1), bidi embedding / override /
/// isolate marks (U+202A..=U+202E, U+2066..=U+2069) and U+FEFF → U+FFFD. Newlines are the caller's: it splits
/// with str::lines() first (\n and \r\n end a line), so only a lone \r reaches clean and becomes U+FFFD.
/// Borrowed when nothing changes.
pub(crate) fn clean(text: &str) -> std::borrow::Cow<'_, str>;
/// The Markdown subset above, as styled lines (split with str::lines(), each line cleaned). `indent` spaces
/// before each line.
pub(crate) fn markdown(text: &str, indent: usize) -> Vec<ratatui::text::Line<'static>>;
```

```rust
// ui/diff.rs (#7)
pub(crate) struct DiffScreen { /* repo name, number, files, selected file, scroll… */ }
impl DiffScreen {
    /// Opened by `d` on the detail; starts loading the files at once.
    pub(crate) fn new(repo: gh_client::RepoName, number: gh_client::PrNumber, shared: &mut Shared) -> Self;
}
```

**Diff (#7)**. Titre : `<owner/name> · #<n> · Diff`. À gauche (un tiers de la largeur, au moins 24 colonnes) la
liste des fichiers, une ligne chacun : marque `>` du fichier choisi, lettre d'état (`A`, `D`, `M`, `R`, `C`,
`M` pour Changed, ` ` pour Unchanged), `+<a> -<d>`, chemin coupé avec `…`. À droite le fichier choisi : une
ligne d'en-tête (`<chemin>`, ou `<ancien> → <nouveau>`), puis le patch (découpé comme `str::lines()`, voir
« Fins de ligne ») sans renvoi à la ligne, `+` en vert, `-` en rouge, `@@` en cyan, `\` atténué. Sans patch :
`Renamed from <ancien>, no content change.` (renommé, `+0 -0`) ou `No diff from GitHub: binary file, or too large
to show.` Liste vide : `No changed files.` Touches :
`n` fichier suivant, `p` précédent (bornés ; défilement remis à zéro), `Up/k` `Down/j`, `PgUp` `PgDn`,
`Left/h` `Right/l` (8 colonnes), `r` recharger. Erreur : `Cannot load the changed files: <raison>`. La liste
porte le titre de cadre `Files (<N>)`, `N` = nombre de fichiers chargés (ajout de #7, accepté à sa relecture).

```rust
// ui/text_area.rs (#8, Ctrl-E by #9)
/// A multi-line text field. Text holds no control character but `\n` and `\t` (others dropped on insert,
/// paste and editor result; \r\n and \r become \n). Cursor in characters; no soft wrap: the view scrolls to keep
/// the cursor visible.
pub(crate) struct TextArea { /* lines, cursor, offset, editing request flag */ }
pub(crate) enum TextKey {
    /// The key changed the text or the cursor.
    Handled,
    /// Not a text key: the owner decides (Ctrl-S, Esc…).
    Ignored,
}
impl TextArea {
    pub(crate) fn new() -> Self;
    pub(crate) fn text(&self) -> String;
    /// Characters (with or without Shift, and AltGr = Ctrl+Alt on Windows), Enter (newline), Backspace,
    /// Delete, arrows, Home, End; Ctrl-E (#9): Shared::request_edit with the current text.
    pub(crate) fn key(&mut self, key: crate::KeyEvent, shared: &mut Shared) -> TextKey;
    pub(crate) fn paste(&mut self, text: &str);
    /// Ok: replaces the text (trailing newlines removed, leading U+FEFF removed), cursor at the end.
    /// Err: text unchanged, Notice::Error with the message of § 5.
    pub(crate) fn edited(&mut self, result: Result<String, crate::EditorError>, shared: &mut Shared);
    /// Draws the field and places the cursor.
    pub(crate) fn render(&self, frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect);
}
```

```rust
// ui/compose.rs (#8 Comment, #10 Review)
/// The PR a text is published on.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub(crate) repo: gh_client::RepoName,
    pub(crate) number: gh_client::PrNumber,
    /// head.sha seen on the detail (Review commit_id).
    pub(crate) head: gh_client::CommitSha,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind { Comment, Review(gh_client::Verdict) }
pub(crate) struct ComposeScreen { /* target, kind, text: TextArea, phase: Writing | Confirm | Publishing, error */ }
impl ComposeScreen {
    /// Opened by `c`, `a` or `x` on the detail, only when actions() allows it. No request at opening.
    /// `draft`: the text kept for this Kind (Signal::Kept), put in the field with the cursor at the end; None:
    /// empty field. (Amendment of #8: the detail takes it with PrDetailScreen::take_draft.)
    pub(crate) fn new(target: Target, kind: Kind, draft: Option<String>) -> Self;
}
// View::edited forwards to TextArea::edited (written in wave 0).
```

**Saisie et confirmation (#8, #10)**. Titre : `<owner/name> · #<n> · Comment`, `… · Review: Approve` ou
`… · Review: Request changes`. Trois phases :

1. **Writing** (`editing() = true`) : ligne d'indication `Ctrl-S publish… · Ctrl-E editor · Esc cancel`, puis la
   zone de texte. `Ctrl-S` : texte vérifié par `MessageBody::new` (Approve : texte blanc permis, envoyé sans
   `body`) ; refus en notice : `Write a text first.` (Comment blanc), `Request changes needs a text.`,
   `The text is too long: <n> characters (GitHub allows 65536).` ; sinon phase Confirm. `Esc` : texte blanc →
   écran fermé (`Shared::pop(None)`) ; sinon notice `Press Esc again to discard the text.` et un second `Esc`
   immédiat ferme (toute autre touche annule l'armement). **Exception, après un échec** (message rouge d'une
   publication affiché, voir plus bas) : un seul `Esc` ferme et **garde le texte** :
   `Shared::pop(Some(Signal::Kept(Draft { kind, text })))` et notice `Text kept: open the comment again to get it
   back.` (`… the review again …`) ; texte blanc : `Shared::pop(None)`. Le détail garde ce brouillon (le
   dernier gagne) et le rend au prochain `c` (`a`, `x` pour une Review du même Verdict) ; un brouillon d'une
   autre sorte reste en place ; le détail fermé, il est perdu. Rouvert avec un brouillon, l'écran repart sans
   message : le double `Esc` y vaut de nouveau, et jette le texte.
2. **Confirm** (`editing() = false`) : `Publish this Comment on <owner/name> #<n>?` ou `Publish this Review on
   <owner/name> #<n>?` + `Verdict: Approve` / `Verdict: Request changes`, puis `Text:` et le texte brut nettoyé
   (`(no text)` si vide ; au-delà de la hauteur : `… <k> more lines`), puis `y / Enter publish · n / Esc back to
   the text`. `y` ou `Enter` : publication ; `n` ou `Esc` : retour à Writing, texte intact. Toute autre touche
   est sans effet (`Flow::Handled`), `q` compris (le texte n'est pas perdu sans demander), sauf `?` (aide,
   `Flow::Ignored`).
3. **Publishing** : `Publishing…` ; toute touche est sans effet (`Flow::Handled`), sauf `?` (aide) ; `Esc` et
   `q` donnent la notice `Publishing: wait for GitHub's answer.` ; une seule requête en vol, jamais renvoyée
   seule. `Ctrl-C` quitte toujours (`App`, avant l'écran) : limite connue, la publication en vol peut aboutir
   sans que l'utilisateur le sache ; de même `q` dans l'aide ouverte par `?`.

Succès : `Shared::pop(Some(Signal::Published))` et notice `Comment published.` / `Review published: Approve.` /
`Review published: Request changes.` ; le détail se recharge. Échec : retour à Writing, texte intact, message
rouge au-dessus de la zone : `Cannot publish the comment: <raison>` / `Cannot publish the review: <raison>`
(`error_message`), sauf deux cas propres à la publication :

- **422 d'une Review** → `GitHub refused the review (422). If you started a review on the website (pending
  review), submit or delete it there; or the PR changed: press Esc, then r to refresh; the text comes back when
  you open the review again.` (couvre aussi un force-push après le chargement, `commit_id` hors de la PR, et une
  PR fermée entre-temps) ;
- **Publication incertaine** : GitHub a peut-être créé le Comment ou la Review. Cas : `Error::Network` ;
  `Error::UnexpectedResponse` (statut 2xx autre que celui attendu, réponse trop grosse, JSON illisible : GitHub a
  souvent déjà répondu 2xx, et la variante ne dit pas le statut) ; `Error::Api` de statut 500 à 599 (une
  passerelle 502 / 503 / 504, ou un 500 après l'écriture) ; `Outcome::TaskFailed` reçu pendant Publishing (tâche
  paniquée). Message (`<what>` = `comment` ou `review`) : `Cannot publish the <what>: no clear answer from
  GitHub. It may have been published anyway: press Esc, then r to check the PR; the text comes back when you open
  the <what> again.` La règle est une fonction pure de `compose.rs` (`fn uncertain(&gh_client::Error) -> bool`,
  testée par variante), commune à Comment et Review.

Le message rouge reste affiché pendant la saisie (frappe, collage, éditeur, refus de `Ctrl-S`) jusqu'à la
prochaine publication (entrée en Publishing) ou la fermeture de l'écran : c'est lui qui donne le conseil, et sa
présence décide du `Esc` qui garde le texte. Le texte publié n'est jamais construit ni complété par `ghu` : c'est
celui de la zone, tel quel.

### 5. Éditeur externe (#9)

```rust
// ghu/src/editor.rs (public items re-exported by lib.rs: EditRequest, EditorError)

/// The raw values captured by prepare (Environment::var("VISUAL"), var("EDITOR")); parsed only when used.
#[derive(Clone, Debug, Default)]
pub(crate) struct EditorVars { pub(crate) visual: Option<String>, pub(crate) editor: Option<String> }

/// A request to edit `text` in the user's editor, taken from Ui::take_edit. Send; owns its data.
pub struct EditRequest { /* vars: EditorVars, text: String */ }

impl EditRequest {
    /// Blocking; call with the terminal suspended. Command: the first non-blank of VISUAL, EDITOR, else `vi`
    /// (`notepad` on Windows, cfg!(windows)); split on whitespace, double quotes group (no other escape, no
    /// shell). Writes `text` to a new temporary file `ghu-*.md` (tempfile, owner-only on Unix, removed after),
    /// runs `<program> <args…> <path>` with the terminal's stdin, stdout and stderr, waits without timeout,
    /// then reads the file again by path (editors that replace the file are fine), bounded:
    /// File::open + Read::take(MAX_EDITED_BYTES + 1) + read_to_end, TooLarge when more than MAX_EDITED_BYTES
    /// came (never fs::read then a check). Program resolution is std's: on Windows only `.exe` is added to a
    /// bare name, so a `.cmd` launcher must be named with its extension (`code.cmd --wait`).
    /// ReturnedAtOnce when the editor exits with success less than the request's threshold (RETURNED_AT_ONCE
    /// unless with_returned_at_once changed it) after the spawn and the file holds exactly the bytes written (an
    /// editor that handed the file to another window and returned).
    /// Errors: see EditorError. Never panics.
    pub fn run(self) -> Result<String, EditorError>;
    /// The same request with another ReturnedAtOnce threshold. For tests: a slow CI runner may take more than
    /// 1 s to spawn the fake editor, so the positive test sets a large one (30 s); Duration::ZERO disables the
    /// check. Production code never calls it.
    pub fn with_returned_at_once(self, threshold: std::time::Duration) -> Self;
}

/// Most bytes read back from the edited file: 1 MiB.
pub const MAX_EDITED_BYTES: u64 = 1_048_576;

/// Default threshold of EditRequest: below this time between spawn and exit, an unchanged file means
/// ReturnedAtOnce.
pub const RETURNED_AT_ONCE: std::time::Duration = std::time::Duration::from_secs(1);

/// Why the editor gave no text. The text being edited is kept in every case.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EditorError {
    /// VISUAL or EDITOR has an unbalanced quote or no program.
    #[error("cannot parse {var}")]
    Unparsable { var: &'static str },
    /// The program was not found (spawn: NotFound).
    #[error("editor not found: {program}")]
    NotFound { program: String },
    /// Exited with a failure status (None: killed by a signal).
    #[error("editor failed: {code:?}")]
    Failed { code: Option<i32> },
    /// More than MAX_EDITED_BYTES written.
    #[error("edited text too large")]
    TooLarge,
    /// Exited with success within the threshold (RETURNED_AT_ONCE by default), file unchanged: probably an editor that does not wait.
    #[error("editor returned at once")]
    ReturnedAtOnce,
    /// Not valid UTF-8.
    #[error("edited text is not UTF-8")]
    NotUtf8,
    /// Temporary file or spawn failure other than NotFound.
    #[error("editor I/O error: {message}")]
    Io { message: String },
}
```

Messages (notice rouge, `TextArea::edited`) : `Cannot read VISUAL: unbalanced quote or no program.` (ou
`EDITOR`) ; `Cannot open the editor: <program> was not found. Set VISUAL or EDITOR.` ; `The editor failed (exit
code <n>): the text is unchanged.` (`(stopped)` sans code) ; `The edited text is larger than 1 MiB: the text is
unchanged.` ; `The edited text is not UTF-8: the text is unchanged.` ; `The editor returned at once: the text is
unchanged. Set VISUAL to an editor that waits (vim, nano, "code.cmd --wait").` ; `Cannot use the editor:
<message>.`

**Éditeur qui n'attend pas** (Windows surtout) : le Notepad à onglets de Windows 11 peut confier le fichier à une
fenêtre déjà ouverte et rendre la main tout de suite (à vérifier à l'essai réel), comme `code` sans `--wait`.
`ReturnedAtOnce` détecte le cas courant (moins d'1 s, contenu identique) et le dit, au lieu de rendre en silence
un texte inchangé. Limites connues, notées pour la 0.1 : un éditeur qui rend la main après plus d'1 s sans
attendre n'est pas détecté (texte inchangé, sans message) ; si l'utilisateur enregistre ensuite dans l'autre
fenêtre, le fichier supprimé peut être recréé avec son texte dans le dossier temporaire ; si `ghu` est tué
pendant l'édition (`Ctrl-C` dans la console pendant qu'un éditeur graphique est ouvert : le terminal n'est plus en
mode brut), `TempPath` n'est pas supprimé. Parade possible plus tard : ignorer `SIGINT` / `CTRL_C_EVENT` pendant
l'attente, comme git (pas dans ce lot).

Éditeurs documentés (aide de #9 et guide d'essai) : `vim`, `nano`, `"code --wait"` sous Unix ;
`"code.cmd --wait"` sous Windows (`code` seul n'est pas trouvé : `Command::new` n'ajoute que `.exe`), notepad
par défaut.

**Boucle réelle** (`terminal.rs`) : après chaque `ui.input`, si `ui.take_edit()` rend une requête : arrêter le
fil clavier (`stop` + `join`, 200 ms au plus : il ne doit pas lire les touches destinées à l'éditeur), quitter le
mode TUI (`restore_terminal`), `tokio::task::spawn_blocking(move || req.run()).await` (`EditRequest` est `Send` et
possède ses données ; la boucle reste correcte sur un runtime current-thread, comme le promet la doc de `Ui` ; un
`JoinError` devient `EditorError::Io`), rentrer en mode TUI (mode brut,
écran alternatif, collage), relancer un fil clavier neuf, `ui.clear()`, `ui.finish_edit(result)`. Une erreur
d'entrée / sortie en rentrant dans le mode TUI termine `run` comme aujourd'hui (terminal restauré).

**Harnais** (`tests/support/mod.rs`) : après chaque touche envoyée (`press_with`, donc `press` et `type_text`),
si `take_edit` rend une requête, le harnais la lance (`run()`) et rend le résultat par `finish_edit` ; aucun
terminal à suspendre. Le chemin `run()` du code de production est donc celui des tests. Le harnais applique à chaque
requête un seuil réglable (`EditRequest::with_returned_at_once`, défaut `RETURNED_AT_ONCE`) : le test positif de
`ReturnedAtOnce` le fixe à 30 s, pour ne pas dépendre de la vitesse de lancement d'un processus sur la CI Windows.

**Faux éditeur** : binaire Rust `ghu-fake-editor` du crate `ghu` (`src/bin/fake_editor.rs`, `test = false`,
`doc = false`), trouvé par les tests via `env!("CARGO_BIN_EXE_ghu-fake-editor")`. Le test le désigne par
`EDITOR` dans `TestEnv` (jamais l'environnement du processus) : `"<chemin>" write "Hello\nfrom the editor"`
(remplace le contenu ; `\n` littéral → saut de ligne), `"<chemin>" upper` (met le contenu reçu en majuscules :
prouve l'aller-retour), `"<chemin>" fail 3` (sort avec le code 3), `"<chemin>" bytes` (écrit des octets non
UTF-8), `"<chemin>" fill <n>` (écrit `n` octets `a` : passe la borne d'1 Mio sans argument géant, qui
dépasserait 128 Kio par argument sous Linux et 32 767 caractères de ligne de commande sous Windows),
`"<chemin>" keep` (sort tout de suite avec le code 0 sans toucher au fichier : l'éditeur qui n'attend pas). Éditeur
introuvable : `EDITOR=/nonexistent/ghu-editor`. Le binaire n'écrit rien sur la sortie et ne lit
rien d'autre que son fichier.

### 6. Règles de publication (résumé)

- Aucune requête `POST` sans passage par la phase Confirm et une touche `y` / `Enter` dans cette phase.
- Comment et Reviews ne sont offerts que selon `actions` (archivé, fermé, fusionné : rien ; Own PR : Comment
  seulement) ; le faux serveur, comme GitHub, refuse de lui-même les Verdicts sur une Own PR (défense en profondeur
  testée côté client).
- Pending reviews : jamais affichées, jamais créées (`event` toujours envoyé) ; le 422 qu'elles provoquent a son
  message.
- Après une publication réussie : retour au détail, rechargé.

### 7. Sécurité

- **Jeton** : les nouvelles routes passent par les mêmes en-têtes, `redact` et `truncate` ; le `post` ne suit
  pas les redirections. Tests : le jeton n'apparaît ni à l'écran, ni dans les logs, ni dans les erreurs, pour une
  publication réussie et pour un 422 (extension de `tests/secrecy.rs` par le lot tui-review).
- **Contenu publié** : uniquement le texte de la zone, après confirmation. Jamais journalisé : les logs donnent la
  longueur (`MessageBody` a un `Debug` sans contenu). Le faux éditeur et le fichier temporaire ne contiennent que
  ce texte ; le fichier est supprimé après lecture (`TempPath`), même en cas d'erreur.
- **Échappement du terminal** : mesuré dans `ratatui-core` 0.1.2 (`buffer/buffer.rs:351`, `text/span.rs:314`) :
  les graphèmes qui contiennent un caractère de contrôle ne sont pas écrits, si bien qu'un `ESC` venu du réseau
  n'atteint pas le terminal. On ne s'en remet pas à ce détail interne : tout texte venu du réseau (titres, logins,
  branches, descriptions, Comments, Reviews, chemins, patchs) passe par `text::clean`, qui rend visibles (U+FFFD)
  les contrôles et les marques bidi (« Trojan Source »). Test : un patch et un Comment contenant
  `\u{1b}[31m`, `\u{202e}` et un `\r` isolé (`a\rb`) s'affichent avec `�`, sans séquence brute dans le tampon ;
  un Comment et un patch en `a\r\nb` s'affichent sur deux lignes sans `�` (« Fins de ligne », § 4).
- **Éditeur** : lancé sans shell (aucune interprétation de `VISUAL` / `EDITOR` au-delà des guillemets) ; fichier
  temporaire créé par `tempfile` (création exclusive, droits 0600 sous Unix).

### 8. Dépendances et manifestes

- `tempfile` 3 (déjà au workspace, MIT OR Apache-2.0, sans C) passe des `dev-dependencies` aux `dependencies` de
  `ghu` (fichier de l'éditeur). Aucune autre dépendance ; `Cargo.lock` inchangé.
- `crates/ghu/Cargo.toml` : `[[bin]] name = "ghu-fake-editor"`, `path = "src/bin/fake_editor.rs"`, `test =
  false`, `doc = false`. Une future release ne doit empaqueter que `ghu`.
- Aucun langage ajouté : le faux éditeur est en Rust.

## Options écartées

- **Étendre `PullRequest`** avec des champs `Option` pour le détail : la même structure serait tantôt complète,
  tantôt non ; `PullDetail` dit ce qu'on a. Coût : un type de plus.
- **Quatre `RequestId` pour le détail** (une variante d'`Outcome` par route, affichage progressif) : états
  partiels à gérer (verdicts sans PR…) pour quatre requêtes rapides ; `try_join!` dans une tâche donne un tout ou
  rien simple. Reviendrait si une route devenait lente au point de gêner.
- **`/user` chargé une fois au démarrage** (dans la Repo list ou `Shared`) : toucherait `repo_list.rs` en
  relecture et un état global de plus ; une requête de plus par ouverture du détail coûte peu (5000 requêtes / h).
- **`pulldown-cmark`** : rendu CommonMark exact mais une dépendance, une correspondance événements → lignes
  aussi longue que le sous-ensemble écrit à la main, et un comportement moins prévisible sur une entrée hostile.
  Reviendrait si l'utilisateur veut liens cliquables, tableaux ou citations.
- **Détail, saisie et confirmation dans un seul écran** : `pr_detail.rs` serait modifié par #7, #8 et #10 en même
  temps. Un écran `compose` empilé, fermé par `Shared::pop`, garde des fichiers disjoints.
- **Rechargement du détail à chaque retour au sommet** (`resumed`) : rechargerait aussi en revenant du Diff,
  contraire au rafraîchissement manuel (roadmap point 12) ; le `Signal::Published` ne recharge qu'après une
  publication.
- **`Ui` qui suspend lui-même le terminal** (trait `Suspend` passé à `Ui::new`) : change la signature de
  `Ui::new` et mêle le terminal réel à l'application testée ; `take_edit` / `finish_edit` laissent la suspension
  à la boucle et le lancement au code partagé.
- **Faux éditeur par réexécution du binaire de test** (filtre libtest + variable) : fragile et illisible ; **script
  shell** : pas de Windows, pas Rust ; **double en mémoire** (trait `EditorRunner`) : ne teste pas le lancement
  réel du programme, que le spec demande.
- **Lancer l'éditeur par `sh -c`** (comme git) : accepte `VISUAL="code --wait"` et plus, mais pas de `sh` sous
  Windows et une surface d'interprétation de plus ; le découpage avec guillemets couvre les cas courants
  (`"C:\Program Files\Notepad++\notepad++.exe" -multiInst`).
- **Fichier temporaire à la main** (`std::env::temp_dir` + `create_new`) : droits et nettoyage à refaire par OS ;
  `tempfile` est déjà dans le `Cargo.lock`.
- **Envoyer la Review sans `commit_id`** : GitHub prendrait le dernier commit, peut-être poussé après la lecture
  du diff ; on approuverait du code non vu.
- **Brouillon dans `Shared`** (table par PR et sorte, vivant toute la session) : survivrait à la sortie du
  détail, mais un champ de plus dans `Shared` casse chaque construction littérale de `Shared` (tests de
  `text_area.rs`, `pr_detail.rs`, `ui.rs`) et demande une borne mémoire ; le détail est le seul lieu où le
  conseil « Esc, r » ramène. `Signal::Kept` reste local. Reviendrait si l'on veut retrouver un texte après être
  sorti du détail.
- **Changer seulement le conseil** (« vérifiez sur le site ») : aucun moyen, dans `ghu`, de vérifier sans perdre
  le texte ; le double `Esc` armé par le conseil actuel le jetterait.
- **`Error::may_have_succeeded()` dans `gh-client`** : API publique entre crates pour une règle d'affichage d'un
  seul appelant ; une fonction de `compose.rs` suffit. Reviendrait si un second client de `gh-client` publiait.
- **5xx de passerelle seulement (502 / 503 / 504) incertains** : un 500 peut suivre l'écriture ; dire « may have
  been published » à tort coûte un `r`, une double publication coûte un Comment en double visible de tous.
- **Confirmation avant `q` en Confirm** : un écran de plus pour un cas que « `q` sans effet » règle ; on quitte
  par `n` puis la fermeture, ou `Ctrl-C`.
- **Message 422 tiré du texte de GitHub** (« pending review ») : contraire à la règle de `error_message` (jamais de
  texte libre du réseau) ; un message qui couvre la cause fréquente suffit.

## Conséquences

Effet sur l'existant :

- `ui/pulls.rs` : `Enter` ouvre le détail de la PR choisie (aujourd'hui sans effet) ; aide `Enter Open the PR`.
  Tests existants : `ghu/tests/pull_list.rs:252` (`selection_is_bounded_and_enter_does_nothing_yet`) appuie sur
  `Enter` et vérifie que la liste reste affichée ; il casse en vague 2. Le lot dev[tui-detail] le reprend
  (renommé `selection_is_bounded_and_enter_opens_the_detail` : la sélection reste bornée, `Enter` ouvre le
  détail de la PR choisie).
- Amendement de #8 : `Signal` perd `Copy` et gagne `Kept(Draft)` (`ui.rs`) ; `PrDetailScreen` gagne un champ
  `draft`, l'arm `Signal::Kept` et `take_draft` (`pr_detail.rs`, faits par l'architecte avec leurs tests) ;
  `ComposeScreen::new` gagne `draft` (un seul appelant, `open_compose`) ; `tests/comment.rs` : le 500 devient une
  publication incertaine (test `a_401_a_404_and_a_500…` repris par dev[tui-compose]).
- `ui.rs` : variantes d'`Outcome`, `Signal`, `Shared::pop`, `Shared::request_edit`, `View::signal`,
  `View::edited`, `Ui::take_edit`, `Ui::finish_edit`, `Ui::clear` ; écrans existants : rien, confirmé d'après le
  code (leurs `apply` finissent par `other => return Some(other)` : `pulls.rs:208`, `repo_list.rs:506`, `:534`,
  `:559` ; nouvelles méthodes de `View` à corps par défaut). Constructions à compléter, toutes en vague 0 :
  `Shared {..}` dans `Ui::new` et dans le test de `ui.rs` (l. 445), `Context {..}` dans `launch.rs:122`.
- `launch.rs` : `Context` gagne `editor: EditorVars` (lus par `prepare`, sans effet si absents) ; `--help` et
  les codes de sortie : rien.
- `terminal.rs` : la boucle sait suspendre le terminal ; sans édition, rien ne change. `spawn_blocking` est dans
  la fonction `rt` de tokio, déjà tirée par `rt-multi-thread` : `ghu/Cargo.toml` inchangé pour tokio.
- `gh-client` : `PullDetail` lit `base.repo.archived` ; `Error` gagne `InvalidBody` (`error_message` tombe sur
  son cas générique, jamais atteint : la   saisie vérifie avant) ; la doc de `PullRequest` ne promet plus de champs de détail.
- `gh-fake` : `Pull` gagne cinq champs (remplis par `Pull::new` : aucun test existant ne construit `Pull` par
  littéral), `State` deux ; démo enrichie sans changer les PRs de `alice/notes` en nombre ni en état.
- Fichiers de l'utilisateur : aucun nouveau (le fichier temporaire est supprimé).
- CI (#11) : un binaire de plus construit par `--all-targets` ; rien d'autre.
- Réversibilité : modèles et méthodes du client `#[non_exhaustive]` / additifs, faciles à étendre, plus coûteux à
  retirer (appelants). Le reste est interne à `ghu` (moyen). Le protocole d'édition (fil clavier arrêté, terminal
  suspendu) est le point délicat : s'il échoue sous Windows, la parade reste locale à `terminal.rs`.

## Lots

Même règles que #3-#5 : un `dev` par périmètre, fichiers disjoints, `tdd` au point de test (harnais clavier contre
`gh-fake`), contre-épreuve de chaque nouveau test (le voir rouge), `rustfmt --edition 2024 <ses fichiers>` et
jamais `cargo fmt --all` ni `-p ghu` quand plusieurs lots touchent `ghu`, `CARGO_TARGET_DIR` commun, jamais de
`gh` ni de jeton réels (PATH sans `gh`, `GH_TOKEN` factice). Commandes de « terminé » de chaque lot :
`cargo clippy -p <crate> --all-targets -- -D warnings` et `cargo test -p <crate>`, plus la vérification complète
du `CLAUDE.md` par `qa`.

**Aucun `todo!()` atteignable au clavier dans un commit** : chaque vague est commitée seule (`Closes #6`
avant #7). La vague 0 donne donc à `DiffScreen` et `ComposeScreen` un corps provisoire qui ne panique pas
(titre de l'écran, corps `Not available yet.`, `Esc` revient) ; tui-diff et tui-compose le remplacent. De même,
tui-compose laisse la publication d'une Review sur ce message (notice `Not available yet.`) jusqu'à tui-review, et
`Ctrl-E` sans effet jusqu'à tui-editor. Les `todo!()` restants ne sont atteints que par des tests.

**Vague 0 faite** (2026-11-27) : les éléments pas encore appelés portent `#![expect(dead_code, reason = …)]` en tête
de leur module (`editor.rs`, `ui/{pr_detail,diff,compose,text_area,text}.rs`) : le lot qui les branche retire
l'attribut de son fichier quand le compilateur le signale (`unfulfilled_lint_expectations`). Dans `ui.rs`, gelé, ce
sont des `#[allow(dead_code, reason = …)]` par élément (variantes d'`Outcome`, `Signal::Published`, `Shared::pop`,
`Shared::request_edit`), à retirer par l'architecte après la vague 4. Écrits en vague 0 en plus de la plomberie :
`TextArea::new`, `EditRequest::new` (`pub(crate)`) et `with_returned_at_once`, le `Debug` de `MessageBody`, les
titres des écrans provisoires ; `Outcome::PrPage` porte un `Box<PrPage>` (clippy `large_enum_variant`) ; le faux
éditeur sort avec le code 64 sur un usage invalide.

| Vague | Lot (modèle) | Fichiers | Dépend de | Terminé quand |
|---|---|---|---|---|
| 0 | architecte (opus) : signatures | `gh-client/src/{model,error,client,lib}.rs` ; `gh-fake/src/{state,lib}.rs` ; `ghu/src/{ui.rs,launch.rs,lib.rs,editor.rs,bin/fake_editor.rs}`, `ghu/src/ui/{pr_detail,diff,compose,text_area,text}.rs` ; `ghu/Cargo.toml` | ADR acceptée | `cargo check --workspace --all-targets` vert ; corps `todo!()` sauf plomberie de `ui.rs` (`pop`, `signal`, `request_edit`, `take_edit`, `finish_edit`, `clear`), capture des variables dans `prepare`, valeurs par défaut de `Pull::new` / `State::new` ; tests existants verts |
| 1 | dev[fake] (sonnet) | `gh-fake/src/routes.rs`, `gh-fake/src/state.rs` (corps : `pull_mut`, `PullFile::modified`, `demo`), `gh-fake/tests/routes.rs` | 0 | 7 routes ; tests par route : succès, 404, pagination des 3 listes, 403 archivé ×2, chaque 422 de `POST reviews`, 422 Comment blanc, Pending créée sans `event`, publication visible au `GET` suivant, `now` / `next_id` appliqués, `requests()` garde le corps ; démo : tests existants verts |
| 1 | dev[client] (sonnet) | `gh-client/src/{client,wire,model}.rs` (corps), `gh-client/tests/http.rs`, `gh-client/tests/fixtures/*.json` (OpenAPI : pull, files, reviews, issue comments, user, review créée, comment créé) | 0 | teste **seul**, sans les routes de dev[fake] (voir « Vague 1 » sous le tableau) ; décodage des exemples documentés ; `archived` lu dans `base.repo` (`true`, `false`, `base.repo` null) ; `mergeable` 3 cas ; PENDING retirée ; état ou statut inconnu = `UnexpectedResponse` ; `patch` absent ou vide = `None` ; `MessageBody` (blanc, 65 536, 65 537, `Debug` sans texte) ; corps JSON des POST lus dans `requests()` (`event`, `body` omis si `None`, `commit_id`) ; 201 / 200 ; 403, 422, 404 classés ; jeton absent des erreurs |
| 2 | dev[tui-detail] (sonnet) | `ghu/src/ui/{pr_detail,text}.rs`, `ghu/src/ui/pulls.rs` (`Enter`), `ghu/tests/pr_detail.rs`, `ghu/tests/pull_list.rs` (test `Enter` repris) | 1 (fusionnée) et `qa` bout-en-bout client ↔ faux | #6 : `selection_is_bounded_and_enter_opens_the_detail` vert ; chaque critère au harnais (titre, auteur, branches, état, draft, mergeable / conflicts, Markdown, verdicts par reviewer, Pending ignorée, Dismissed, conversation ordonnée, défilement borné, `r`, `Esc` retour, chargement lent, 404 et 500 lisibles) ; lignes d'actions des 4 cas, dont un repo archivé après le chargement puis `r` (actions retirées) ; unitaires `actions`, `verdicts`, `clean`, `markdown` (sans panique sur entrée quelconque) ; fins de ligne `a\r\nb` sans `�`, `a\rb` avec ; test d'échappement du § 7 |
| 3 | dev[tui-diff] (sonnet) | `ghu/src/ui/diff.rs`, `ghu/tests/diff.rs` | 2 | #7 : liste et compteurs, couleurs (style des cellules lu dans le tampon), `n` / `p` bornés, défilement vertical et horizontal, renommé sans changement, binaire, liste vide, pagination (> 100 fichiers avec `max_per_page` bas), erreur, échappement d'un patch hostile, patch CRLF sans `�` |
| 3 | dev[tui-compose] (sonnet) | `ghu/src/ui/{compose,text_area}.rs` (Comment ; branches `Kind::Review` laissées en `todo!()`), `ghu/tests/comment.rs` | 2 | #8 : saisie multi-ligne et collage, annulation (`Esc` blanc, double `Esc`), refus du texte blanc, écran de confirmation exact, `n` revient au texte, publication reçue par le faux (corps exact), détail rechargé avec le Comment, Comment absent si archivé / fermé / fusionné, 403, limite de débit, 422 et réseau coupé : message, texte conservé, aucune seconde requête ; `TaskFailed` pendant Publishing (unitaire) : retour à Writing, message « may have been published ». **Amendement (relectures de #8)** : `ComposeScreen::new(target, kind, draft)` et `open_compose` de `pr_detail.rs` ; `uncertain` (unitaire par variante : Network, UnexpectedResponse, Api 500 / 502 / 599 oui ; Api 499 / 600, 403, 404, 422, 429, 401 non) ; au harnais : 500 et 502 → message incertain, `Esc` unique après un échec puis `r` puis `c` → texte revenu, puis double `Esc` le jette ; message gardé après frappe ; `q` sans effet en Confirm et en Publishing (aucun POST de plus, écran resté) |
| 4 | dev[tui-editor] (**opus**) | `ghu/src/editor.rs`, `ghu/src/bin/fake_editor.rs`, `ghu/src/ui/text_area.rs` (`Ctrl-E`, `edited`), `ghu/src/terminal.rs`, `ghu/tests/support/mod.rs`, `ghu/tests/editor.rs` | 3 (tui-compose) | #9 : ordre VISUAL > EDITOR > défaut (unitaire, défaut par `cfg`), découpage avec guillemets (unitaire), aller-retour (`upper`), `write` multi-ligne, introuvable, code 3, non UTF-8, `fill 1048576` accepté et `fill 1048577` = `TooLarge`, `ReturnedAtOnce` (faux éditeur `keep`, seuil du harnais fixé à 30 s) et ses contre-épreuves (`write` rapide : pas de `ReturnedAtOnce` ; `keep` avec un seuil `Duration::ZERO` : texte rendu inchangé, sans erreur), fichier temporaire supprimé ; boucle réelle par `spawn_blocking` ; au harnais : texte revenu dans la zone puis publié ; essai réel par l'utilisateur (vim sous WSL, notepad sous Windows avec une fenêtre Notepad déjà ouverte, `code.cmd --wait`) |
| 4 | dev[tui-review] (sonnet) | `ghu/src/ui/compose.rs` (branches Review), `ghu/tests/review.rs`, `ghu/tests/secrecy.rs` (cas de publication) | 3 (tui-compose) | #10 : Approve sans texte et avec texte, Request changes refusé blanc, confirmation avec Verdict, `event` et `commit_id` reçus par le faux, détail rechargé avec le Verdict, Own PR (pas de `a` / `x`, ligne d'explication, `c` marche), PR non ouverte et repo archivé, 422 (Pending ou PR changée) : message exact du § 4, texte conservé ; publication incertaine (`uncertain`, réseau et 502) : message `review` ; après un 422, `Esc` puis `r` puis la même touche (`a` / `x`) rend le texte, l'autre Verdict ouvre une zone vide |

Modèles : `sonnet` (tableau « Agents ») sauf **tui-editor en opus** : protocole fil clavier / terminal / processus
externe, propre à chaque OS, que les tests au harnais ne couvrent pas (seul l'essai réel le fera) ; une erreur y
laisse le terminal inutilisable.

**Vague 1 : dev[fake] et dev[client] en parallèle, sans s'attendre.** En vague 0, `gh-fake/src/routes.rs` n'a pas
les nouvelles routes (une requête vers elles reçoit le 404 générique) ; dev[client] ne teste donc pas ses succès
contre le faux :

- **Succès** (200 / 201, décodage, pagination) : le `scripted_server` de `gh-client/tests/http.rs`
  (`http.rs:48`) sert les fixtures. Pour un `POST`, il doit lire le corps annoncé par `Content-Length` avant de
  répondre (aujourd'hui il ne lit que l'en-tête : une socket fermée avec des octets non lus peut finir en RST
  côté client) ; dev[client] l'étend dans `http.rs`.
- **Erreurs 403 / 422 / 404 / limite de débit** : l'injection `Failure` du faux (`state.rs:247`), appliquée
  avant le routage, sur le chemin et la méthode `POST` des nouvelles routes.
- **Corps des `POST`** (`event`, `body` omis si `None`, `commit_id`) : `FakeGitHub::requests()`, qui
  enregistre le corps JSON avant l'authentification et le routage (`routes.rs:135-148`), donc même sans route.
- **Bout-en-bout client ↔ faux** (vraies routes de dev[fake], un test par méthode : succès et une erreur
  propre au faux, 403 archivé ou 422 Own PR) : `qa` après la fusion des deux lots, dans un fichier neuf
  `gh-client/tests/fake_roundtrip.rs` ; il conditionne le départ de la vague 2.

Option écartée : enchaîner fake puis client (tests de succès contre le faux) : une session de plus sur le chemin
critique, pour un gain que le passage de `qa` donne aussi.

Ordre : vague 0 → vague 1 (fake ‖ client, indépendants) → `qa` (vérification complète + bout-en-bout client ↔
faux) + `relecteur` ×2 (réseau, parsing, jeton : relecture d'office) → vague 2 (tui-detail) → `qa` + relecteur →
commit `Closes #6` → vague 3 (tui-diff ‖ tui-compose) → `qa` + relecteur → commit `Closes #7 #8` → vague 4
(tui-editor ‖ tui-review) → `qa` + relecteur ×2 (Standards, Spec) → commit `Closes #9 #10` → essai réel de
l'utilisateur (éditeur, terminal, Windows). La vague 1 est fusionnée avant le passage de `qa`, qui écrit
`fake_roundtrip.rs` sur la fusion ; la relecture peut porter sur les deux lots fusionnés. La vague 2 ne démarre
jamais avant le bout-en-bout client ↔ faux de `qa`.

**Goulots** : `ghu/src/ui.rs` (vague 0 seulement, puis gelé) ; `ghu/src/ui/text_area.rs` (tui-compose puis
tui-editor, en séquence) ; `ghu/src/ui/compose.rs` (tui-compose puis tui-review, en séquence) ;
`ghu/tests/support/mod.rs` (tui-editor seul ; un autre lot qui en aurait besoin le demande dans son rapport).

Fichiers partagés : `Cargo.toml` racine et `Cargo.lock` → personne (aucun changement) ; `ghu/Cargo.toml`,
`ui.rs`, `lib.rs` (les trois crates), `launch.rs` → vague 0 ; `ghu/tests/pull_list.rs` → dev[tui-detail] ;
`gh-client/tests/fake_roundtrip.rs` (nouveau) → `qa` après la vague 1 ; `gh-fake/src/state.rs` → vague 0 (types) puis
dev[fake] (corps) ; `docs/` → session principale et architecte.

## Risques

| Risque | Déclencheur | Détection | Parade | Retour arrière |
|---|---|---|---|---|
| Le fil clavier vole des touches à l'éditeur | `poll` de crossterm pendant l'édition | essai réel (taper dans vim) | fil arrêté et joint avant le lancement | revenir à `take_edit` sans éditeur (zone seule) |
| Terminal mal restauré après l'éditeur (Windows) | console Windows, notepad | essai réel de l'utilisateur ; CI Windows ne le voit pas | `ui.clear()` + mode TUI refait ; erreur → sortie propre | désactiver `Ctrl-E` sous Windows (`cfg`) |
| Double publication | réponse perdue (réseau) après création | test « réseau coupé » | message « may have been published: refresh » ; pas de renvoi automatique | — |
| Éditeur qui rend la main sans attendre (Notepad à onglets, `code` sans `--wait`) | Windows 11, fenêtre Notepad déjà ouverte | test `ReturnedAtOnce` au harnais ; essai réel avec une fenêtre Notepad ouverte | notice `The editor returned at once…` ; `code.cmd --wait` documenté | si l'essai le confirme pour notepad : plus de défaut sous Windows (`VISUAL` / `EDITOR` requis, message `NotFound` adapté), décision à noter ici |
| Test de `POST` instable contre `scripted_server` | corps non lu avant la réponse (RST) | 20 passages de `cargo test -p gh-client` par dev[client] | lire `Content-Length` octets avant de répondre | tests de succès des `POST` reportés au bout-en-bout de `qa` |
| Écart entre les fixtures du client et les routes du faux | vague 1 en parallèle | bout-en-bout `qa` après fusion (`fake_roundtrip.rs`) | même source (exemples OpenAPI) des deux côtés | corriger le faux ou le client avant la vague 2 |
| Faux serveur trop différent de GitHub (messages 422, règles) | doc incomplète | essai réel de l'utilisateur | `ghu` ne lit pas les messages 422 | corriger le faux |
| Démo modifiée qui casse des tests existants | ajout de PRs à `alice/notes` | `cargo test --workspace` de dev[fake] | PRs nouvelles dans d'autres repos | — |
| Conflits entre lots parallèles de `ghu` | `cargo fmt` ou fichier hors périmètre | `git diff --stat` par lot avant fusion | `rustfmt` sur ses fichiers ; listes de fichiers ci-dessus | rejouer le lot |
| Curseur décalé avec des caractères larges (CJK, emoji) | positions en caractères | essai réel | limite connue de la 0.1, notée | largeur `unicode-width` plus tard |

## À vérifier

- Libellés exacts des 422 de GitHub pour une Review (Own PR, pending, PR fermée) : sans effet sur `ghu` (non lus),
  utiles à la fidélité du faux.
- Comptage de `MAX_BODY_CHARS` par GitHub (caractères ou unités UTF-16) : au pire un 422 au lieu du refus local.
- notepad et le fichier temporaire sous Windows (fichier fermé avant le lancement : `TempPath`) ; Notepad à
  onglets de Windows 11 qui rend la main tout de suite quand une fenêtre est déjà ouverte (supposé, à l'essai
  réel : la notice `ReturnedAtOnce` doit apparaître) ; éditeurs qui forkent (`code` sans `--wait`) : même notice.
- `EDITOR="code.cmd --wait"` sous Windows : trouvé et attendu par `Command::new` (extension explicite), à l'essai
  réel ; `code --wait` y donne `NotFound` (documenté).
- Seuil de `RETURNED_AT_ONCE` (1 s) : assez haut pour le lancement d'un Notepad déjà ouvert, assez bas pour ne pas
  gêner un `:q` rapide dans vim (qui ne verrait que la notice, texte inchangé de toute façon).
- Comportement de GitHub pour une Review sur une PR fermée ou fusionnée (le faux répond 422).
