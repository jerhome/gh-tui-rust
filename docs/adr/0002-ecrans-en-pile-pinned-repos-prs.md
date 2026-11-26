---
status: accepted
---

# Écrans en pile, fichier des Pinned repos en TOML, routes repos et PRs

Les tickets #3 (Repo list complète), #4 (Pinned repos) et #5 (PRs d'un repo) touchent tous `crates/ghu/src/ui`,
`gh-client` et `gh-fake`. On fige ici ce qui coûterait cher à défaire ou à refaire en parallèle : la navigation
entre écrans et le découpage de `ui/` en un module par écran, le format et l'emplacement du fichier des Pinned repos
(données de l'utilisateur sur disque), les routes et modèles publics ajoutés, et deux dépendances. Signatures et doc
de contrat : `crates/` (`ghu/src/ui.rs`, `ghu/src/ui/*.rs`, `ghu/src/pins.rs`, `gh-client/src/model.rs`,
`gh-client/src/client.rs`, `gh-fake/src/lib.rs`, `gh-fake/src/state.rs`).

## Décision

1. **Pile d'écrans** (`ghu/src/ui.rs`) : `App.stack: Vec<Box<dyn View>>`, racine = Repo list, jamais dépilée.
   Chaque écran implémente le trait privé `View` (`title`, `help`, `editing`, `key`, `paste`, `apply`,
   `render`) dans son propre module (`ui/repo_list.rs`, `ui/pulls.rs`, puis `ui/pr_detail.rs` en #6…) ; `key`
   rend un `Flow` (`Ignored`, `Handled`, `Push(écran)`). Le retour à l'écran précédent passe par la table
   globale (`Esc`), seul chemin de dépilement ; `Flow::Pop`, sans appelant, est retiré (il reviendra avec son
   premier usage, par exemple un écran qui se ferme après une action). Un écran ouvert en réponse à un résultat
   d'arrière-plan (`apply`) ou à une touche est demandé par `Shared::push` ; `App` l'empile après le traitement
   (`apply` comme `key`), dans l'ordre des appels, au-dessus de l'écran qui l'a demandé. `Flow::Push` reste la forme
   courte pour une touche. Un nouvel écran ne modifie pas `ui.rs`.
2. **Requêtes de fond routées par identifiant** : `Shared::spawn` rend un `RequestId` unique ; le résultat
   (`Outcome`, une variante par route du client) est offert aux écrans du haut vers le bas, et seul l'écran qui
   attend cet identifiant le prend (`apply` rend `None`). Une réponse périmée (écran quitté, rechargement plus
   récent, bascule ouvertes / fermées) n'est prise par personne et est jetée ; elle compte dans l'indicateur
   `Loading` jusqu'à son arrivée. Ajouter une route = ajouter une variante d'`Outcome` (architecte).
3. **Touches** (`ui/keys.rs`) : une table par écran (`Binding<A>`) et une table globale ; l'aide `?` montre la table
   de l'écran du dessus puis la globale. Ordre de traitement : `Ctrl-C` quitte toujours ; toute touche efface la
   notice ; aide ouverte : seuls `?`, `Esc` (fermer) et `q` agissent ; écran en saisie (`editing`) : il reçoit
   tout (`q` et `?` sont du texte) ; sinon l'écran d'abord, puis la table globale s'il rend `Ignored` (`?` aide,
   `Esc` retour, `q` quitter). `?`, `q` et `Ctrl-C` sont réservés (aucune table d'écran ne les lie, testé).
   Repo list : `Enter` ouvrir les PRs, `Up/k` `Down/j`, `/` filtre par nom, `a` archivés montrés / masqués, `o`
   ouvrir `owner/name`, `u` désépingler, `r` rafraîchir. PR list : `Tab` ouvertes ↔ fermées et fusionnées, `r`,
   `Up/k` `Down/j` ; `Enter` réservé au détail (#6).
4. **Notice** : une ligne de pied de page (`Notice::Info` / `Notice::Error`, rouge) qui remplace les indications de
   touches jusqu'à la touche suivante ; c'est le canal des messages qui ne sont pas l'état d'un écran (repo épinglé,
   404 à l'ouverture, fichier des Pinned repos illisible).
5. **Fichier des Pinned repos** (`ghu/src/pins.rs`) : `<dossier de config de l'OS>/ghu/pinned.toml`, dossier donné
   par `Environment::config_dir` (`dirs::config_dir()` : `$XDG_CONFIG_HOME` ou `~/.config` sous Linux,
   `%APPDATA%` sous Windows, `~/Library/Application Support` sous macOS). Contenu : un commentaire d'en-tête et
   `pinned = ["owner/name", …]`, dans l'ordre d'épinglage ; rien d'autre. Le fichier ne contient que des noms
   canoniques (casse rendue par GitHub dans `full_name`) : on épingle le nom rendu par `GET /repos/{owner}/{repo}`,
   jamais la saisie ; un nom écrit à la main dans une autre casse est remplacé, à sa place dans l'ordre, quand
   GitHub renvoie le repo au chargement (`pins.pin(repo.name)`, `same_repo` insensible à la casse), et le fichier
   n'est réécrit que si l'écriture a changé. Un repo renommé ou transféré (autre nom, pas seulement une autre casse)
   n'est pas rapproché en 0.1 : l'ancien nom reste épinglé. Écriture
   atomique (fichier `.tmp` puis `rename`) à chaque épinglage ou désépinglage, depuis le fil de l'interface (fichier
   minuscule). Lecture : absent = liste vide ; clés inconnues ignorées ; doublons (insensibles à la casse) retirés ;
   fichier illisible, invalide ou de plus de 1 Mio refusé en bloc : liste vide, épinglage désactivé pour la session,
   fichier jamais écrasé, message en notice ; `ghu` démarre quand même. Sans dossier de config : épinglage en
   mémoire seulement. Les tests utilisent un dossier temporaire (`Harness`), jamais le vrai. **Limite connue de la
   0.1** : aucun verrou entre deux `ghu` lancés en même temps ; chacun réécrit le fichier entier depuis sa liste en
   mémoire, donc le dernier qui écrit efface les épinglages faits entre-temps par l'autre (le fichier reste valide
   grâce au `rename` atomique). Parade si besoin : relire et fusionner avant d'écrire, ou un verrou de fichier.
6. **Repo list** (#3, #4) : `user_repos(&[])` (les trois affiliations), plus chaque Pinned repo chargé par
   `GET /repos/{owner}/{repo}` (une requête par repo, en parallèle). Pinned repos d'abord, dans l'ordre d'épinglage,
   puis les autres dans l'ordre de GitHub ; un repo n'apparaît qu'une fois (`RepoName::same_repo`). Archived repos
   masqués par défaut sauf s'ils sont épinglés (choix explicite de l'utilisateur). Filtre : sous-chaîne de
   `owner/name`, insensible à la casse ASCII, après `/`. Ouvrir `owner/name` (`o`) : nom validé localement
   (`RepoName`), puis `GET /repos/{owner}/{repo}` ; succès = épinglé et liste des PRs ouverte ; échec (404…) = rien
   d'épinglé, la saisie reste ouverte avec son texte et un message.
7. **PRs** (#5) : `Client::pulls(repo, PullFilter)` = `GET /repos/{owner}/{repo}/pulls?state=open|closed`
   `&sort=updated&direction=desc&per_page=100`, toutes les pages. Modèle public : `PullRequest` (`number:
   PrNumber`, `title`, `author` — `ghost` si `user` est `null` —, `state: PrState`, `updated_at: Timestamp`),
   `PrState::{Open { draft }, Closed, Merged}` (fusionnée = `closed` avec `merged_at` non nul). Date affichée :
   `YYYY-MM-DD` en UTC.
8. **`Timestamp` sans crate de dates** : chaîne validée `YYYY-MM-DDTHH:MM:SSZ` (forme de l'API REST), ordre
   chronologique par ordre lexical, `date()` = 10 premiers caractères. Pas de fuseau local en 0.1.
9. **Dépendances** : `dirs` 7 (MIT OR Apache-2.0 ; dossier de config de l'OS) et `toml` 1.2 (MIT OR Apache-2.0 ;
   fonctionnalités `std`, `serde`, `parse`, `display`) pour `ghu` ; `tempfile` 3 (MIT OR Apache-2.0) en
   `dev-dependencies` de `ghu`. `thiserror`, déjà au workspace, entre dans `ghu` (`PinsError`) ; `serde`
   n'y entre pas (`pins.rs` lit un `toml::Table`, sans dérive). Aucune ne demande de chaîne C.
   **Licences transitives** : `dirs` tire `dirs-sys` puis `option-ext` 0.2.0, sous **MPL-2.0** (seul crate du
   `Cargo.lock` sans option MIT ni Apache). Accepté : copyleft au niveau du fichier, qui n'oblige qu'à publier les
   fichiers d'`option-ext` modifiés ; on ne le modifie pas et sa source est publique sur crates.io. Option écartée :
   calculer le dossier de config à la main (`XDG_CONFIG_HOME`, `APPDATA`, `HOME`), trois cas d'OS à maintenir et
   à tester pour éviter un fichier de 20 lignes. Reviendrait si une distribution binaire demandait une liste de
   licences sans copyleft (remplacer `dirs` par ce calcul). Le reste du `Cargo.lock` (relevé par
   `cargo metadata` le 2026-11-26, toutes cibles) est permissif, déjà là avant cette ADR pour l'essentiel (rustls,
   ratatui et ses dépendances) : ISC, BSD-3-Clause, Zlib, BSL-1.0, Unicode-3.0 / Unicode-DFS-2016,
   CDLA-Permissive-2.0 (`webpki-root-certs`), WTFPL (`terminfo`), Apache-2.0 WITH LLVM-exception ; `termina`
   (MIT OR MPL-2.0) et `r-efi` (… OR LGPL-2.1-or-later) sont pris sous leur option MIT.
10. **Faux serveur** : `Access::NoAffiliation` (repo public d'un autre, servi par `GET /repos/{owner}/{repo}` mais
    jamais listé), `State.pulls: Vec<Pull>` et `PullState` propres à `gh-fake` (JSON écrit d'après la doc GitHub,
    toujours sans dépendre de `gh-client`, ADR 0001) ; pagination commune à toutes les listes ; routes et requêtes
    acceptées décrites dans `gh-fake/src/lib.rs`.

## Options écartées

- **Un `enum Screen` dans `ui.rs`** (dispatch par `match`) au lieu du trait : chaque écran ajouterait des bras dans
  le même fichier, point de conflit des `dev` en parallèle (#6 à #10). Reviendrait si un écran devait être
  inspecté par un autre (aujourd'hui aucun cas).
- **Un écran unique avec des champs par fonction** (forme de #2) : #3, #4 et #5 auraient édité les mêmes fonctions.
- **Résultats de fond sans identifiant** (le dernier arrivé gagne) : une réponse lente à une bascule ou un
  rafraîchissement écraserait une plus récente ; un `Esc` avant la réponse l'appliquerait au mauvais écran.
- **Fichier en texte brut** (une ligne `owner/name`) : aucune dépendance, mais aucun moyen d'évoluer sans changer de
  format. **JSON** (`serde_json` déjà au workspace) : moins lisible et pénible à éditer à la main. TOML est la norme
  des fichiers de config Rust et s'édite à la main. Reviendrait si `toml` pesait trop sur la compilation Windows.
- **`directories`** (`ProjectDirs`) : chemins plus profonds sous Windows (`%APPDATA%\ghu\config`) pour un seul
  fichier ; `dirs` suffit. **Variable `GHU_CONFIG_DIR`** : inutile aux tests (`Environment::config_dir` est déjà
  un point de variation) ; à ajouter si l'utilisateur la demande.
- **`jiff` ou `chrono`** pour les dates : utiles pour l'heure locale ou « il y a 3 jours », hors 0.1 ; `Timestamp`
  garde son champ privé, on peut changer de représentation sans toucher aux appelants.
- **Taper directement pour filtrer** (sans `/`) : entre en conflit avec `q`, `j`, `k`, `a`, `o`, `u`, `r`.
- **Archived repos épinglés masqués comme les autres** : l'utilisateur les a choisis ; les cacher surprendrait.

## Conséquences

- Existant (#2) :
  - `ui.rs` : réorganisé en pile d'écrans, comportement de #2 inchangé (les 15 tests de `tests/repo_list.rs` et
    les autres passent sur le squelette). Le message 404 devient
    `… not found on GitHub (404): it does not exist or this token cannot see it.`
  - Repo list : collaborateur et organisations listés ; le test `only_the_users_own_repos_are_listed` de
    `tests/repo_list.rs` est remplacé par son contraire (dev[tui-repos]). Le marqueur `[archived]` ne se voit
    plus que filtre ouvert ou sur un Pinned repo. Une marque de sélection `>` précède la ligne choisie.
  - `Environment` gagne `config_dir` (méthode requise) ; `TestEnv` rend `None` par défaut, `Harness` un dossier
    temporaire (`Harness::restart`, `pinned_file`, `paste` ajoutés).
  - `Context` porte le `PinStore` ; `Launch` garde sa forme.
  - Faux serveur : `Access` gagne `NoAffiliation` (`match` exhaustifs à compléter, un seul dans `routes.rs`).
- CI (#11) : rien ; les dépendances nouvelles n'ont pas de code C.
- Données de l'utilisateur : premier fichier écrit par `ghu` ; aucune migration (rien n'existait).
- Réversibilité : la pile et le trait sont internes à `ghu` (moyen à défaire). Le format du fichier est le seul
  point difficile : un changement ultérieur devra relire l'ancien (clé `pinned`), d'où le choix d'un format à clés.
  Les routes et modèles publics de `gh-client` sont `#[non_exhaustive]` et s'étendent sans casse.
