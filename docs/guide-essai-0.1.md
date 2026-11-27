# Guide d'essai réel — ghu 0.1

Essai complet de la version 0.1 par l'utilisateur, sur Linux/WSL puis Windows : d'abord contre le faux serveur
`gh-fake`, puis, en dernier, contre le vrai GitHub en lecture seule. La fiche d'essai (artifact) reprend ces
étapes ; chaque étape y est notée OK, KO ou non fait, avec un commentaire.

Règles de l'essai :

- Le vrai GitHub n'est touché qu'en partie C, **en lecture seule**. Une publication (Comment, Review) sur le vrai
  GitHub demande l'accord explicite de l'utilisateur, sur un repo d'essai à lui, jamais sur le repo d'autrui.
- Une étape KO est consignée (texte exact du message, capture si possible) ; on ne la corrige pas pendant l'essai.
  Le re-test se fait dans une nouvelle section de fiche.
- Dans le terminal du faux serveur, chaque requête reçue s'affiche (`GET …`, `POST …`) : c'est la preuve qu'une
  publication est partie, ou n'est pas partie.

## 0. Build essayé

À remplir dans la fiche avant de commencer :

| Champ | Valeur |
|---|---|
| Exécution GitHub Actions (lien) | … |
| Commit | … |
| SHA-256 de `ghu` (Linux) | … |
| SHA-256 de `ghu.exe` (Windows) | … |

Préparation :

1. Télécharger les artefacts de l'exécution (archives `ghu-0.1.0-<cible>.*` et `SHA256SUMS`) dans un dossier
   vide.
   Linux/WSL : `sha256sum --ignore-missing --check SHA256SUMS`.
   Attendu : `ghu-0.1.0-x86_64-unknown-linux-gnu.tar.gz: OK`.
2. Windows (PowerShell) : `(Get-FileHash ghu-0.1.0-x86_64-pc-windows-msvc.zip).Hash.ToLower()`.
   Attendu : la même valeur que la ligne de l'archive dans `SHA256SUMS`.
3. Décompresser ; relever l'empreinte de l'exécutable : `sha256sum ghu` (Linux) et `Get-FileHash .\ghu.exe`
   (Windows), la reporter dans l'en-tête de la fiche.
   Attendu : chaque archive contient seulement `ghu` (ou `ghu.exe`), `README.md` et `LICENSE` ; ni `gh-fake`,
   ni `ghu-fake-editor`.

## A. Linux / WSL, faux serveur

Deux terminaux WSL. Le faux serveur se lance depuis le checkout du repo (Rust installé) ; `ghu` est
l'exécutable de l'artefact.

Terminal 1 :

```bash
cargo run -p gh-fake -- --port 8787 --latency-ms 500
```

Attendu : il affiche `GHU_API_URL=http://127.0.0.1:8787` et `GH_TOKEN=ghu-fake-token`, puis attend.

Terminal 2 (dans le dossier de l'exécutable) :

```bash
export GHU_API_URL=http://127.0.0.1:8787 GH_TOKEN=ghu-fake-token
export GHU_LOG_FILE=/tmp/ghu-essai.log GHU_LOG=debug
```

Données du faux serveur : l'utilisateur `alice` ; ses repos `alice/notes`, `alice/secret-garden` (privé),
`alice/dotfiles` (fork), `alice/old-experiments` (archivé) ; `bob/shared-tools` (collaboratrice),
`acme/widgets` et `acme/legacy-site` (archivé) par l'organisation ; `carol/public-lib` (sans lien, à ouvrir par
son nom). PRs de `alice/notes` : #1 ouverte (bob), #2 brouillon (carol), #3 fermée, #4 fusionnée. Autres :
`bob/shared-tools#1` (PR d'alice), `acme/legacy-site#1` (repo archivé), `acme/widgets#1` (conflits).

### A1. Démarrage

1. `./ghu --version`. Attendu : `ghu 0.1.0`, code de sortie 0 (`echo $?`).
2. `./ghu --help`. Attendu : la phrase « Terminal client for GitHub: browse repos and pull requests, and review
   them. » et les options `--help`, `--version` ; code 0.
3. `./ghu --nope`. Attendu : erreur d'usage de clap, code 2.
4. Sans jeton : `env -u GH_TOKEN -u GITHUB_TOKEN PATH=/nonexistent ./ghu; echo $?`.
   Attendu : `ghu: no GitHub token found.` puis une ligne qui cite `GH_TOKEN`, `GITHUB_TOKEN` et
   `gh auth login` ; code 1 ; le terminal n'a pas changé de mode.
5. URL invalide : `GHU_API_URL=pas-une-url ./ghu; echo $?`. Attendu : `ghu: invalid GHU_API_URL: …`, code 1.
6. `./ghu`. Attendu : plein écran, en-tête `ghu · Repo list`, un indicateur `Loading…` puis la liste ; pied
   de page `? help · q quit`. Le terminal 1 affiche `GET /user/repos…`.

### A2. Liste des repos

1. Lire la liste. Attendu : repos d'alice, de bob et d'acme ; `alice/dotfiles` marqué `[fork]`,
   `alice/secret-garden` et `acme/widgets` marqués `[private]` ; aucun repo archivé.
2. `?`. Attendu : aide avec `Enter`, `Up/k`, `Down/j`, `/`, `a`, `o`, `u`, `r`, puis `?`, `Esc`,
   `q`, `Ctrl-C`. `?` ou `Esc` la ferme.
3. `a`. Attendu : titre `Repo list (archived shown)`, `alice/old-experiments` et `acme/legacy-site` marqués
   `[archived]`. `a` à nouveau : ils disparaissent.
4. `/` puis `no`. Attendu : ligne `Filter: no`, seul `alice/notes` reste. `Enter` garde le filtre ; `j` et
   `k` bougent la sélection ; `Esc` efface le filtre. Pendant la saisie, `q` et `?` s'écrivent dans le filtre.
5. `/` puis `zzz`. Attendu : `No repo matches "zzz".` ; `Esc`.
6. `o` puis `pas un nom` et `Enter`. Attendu : notice rouge `Invalid repo name: type owner/name.`, l'invite
   reste ouverte, aucune requête dans le terminal 1. `Esc` ferme l'invite.
7. `o` puis `carol/public-lib` et `Enter`. Attendu : la liste des PRs de `carol/public-lib` s'ouvre
   (`No open pull requests.`). `Esc` : le repo est en tête de liste, marqué `[pinned]`.
8. `o` puis `nobody/nothing` et `Enter`. Attendu : notice `Cannot open nobody/nothing: …`, rien d'épinglé.
9. Quitter (`q`) et relancer `./ghu`. Attendu : `carol/public-lib` toujours épinglé
   (`~/.config/ghu/pinned.toml` existe).
10. `r`. Attendu : `Loading…` puis la même liste ; le terminal 1 montre les requêtes.

### A3. PRs d'un repo

1. `Enter` sur `alice/notes`. Attendu : titre `alice/notes · Pull requests (open)` ; `#1` `open` et `#2`
   `draft`, avec `@auteur` et une date `AAAA-MM-JJ`.
2. `Tab`. Attendu : `(closed / merged)`, `#3` `closed` et `#4` `merged`. `Tab` : retour aux ouvertes.
3. `j` plusieurs fois au-delà de la dernière ligne, puis `k` au-delà de la première. Attendu : la sélection
   s'arrête aux bornes, sans boucler.
4. `Esc`. Attendu : retour à la liste des repos, sélection et filtre inchangés.

### A4. Détail d'une PR

1. Ouvrir `alice/notes#1`. Attendu : titre, état, branches ; description rendue (titre, gras, liste, bloc de
   code ; `<b>Raw HTML</b>` reste du texte) ; `Verdicts: @carol Approve · @dave Request changes` ; la
   conversation (commentaires de carol et bob, revue commentée d'erin) ; la revue en attente d'alice n'apparaît
   pas ; ligne d'actions `c Comment · a Approve · x Request changes · d Diff`.
2. `j`, `k`, `PgDn`, `PgUp`. Attendu : défilement sans dépasser le début ni la fin.
3. `r`. Attendu : rechargement (`Loading…`, requêtes dans le terminal 1).
4. `bob/shared-tools#1` (PR d'alice). Attendu : `c Comment · d Diff · Verdicts are not offered on your own PR.` ;
   `a` et `x` sans effet.
5. `acme/legacy-site#1` (afficher les archivés avec `a`). Attendu : `d Diff · This repo is archived:
   read-only.` ; `c`, `a`, `x` sans effet.
6. `alice/notes#4`. Attendu : `d Diff · This PR is merged: no comment or review.` (et `closed` pour #3).
7. `Esc`. Attendu : retour à la liste des PRs.

### A5. Diff

1. Sur `alice/notes#1`, `d`. Attendu : 5 fichiers à gauche ; diff de `README.md` à droite, lignes ajoutées et
   retirées en couleur.
2. `n` jusqu'au dernier fichier, puis `p`. Attendu : `docs/guide.md` (ajouté), `scripts/export.sh` (supprimé),
   `docs/labels.md → docs/tags.md` avec `Renamed from docs/labels.md, no content change.`, `assets/logo.png`
   avec `No diff from GitHub: binary file, or too large to show.` ; `n` et `p` s'arrêtent aux bornes.
3. `j`, `k`, `PgDn`, `PgUp`, `l`, `h` (et les flèches). Attendu : défilement vertical et horizontal, sans
   retour à la ligne.
4. `r` recharge ; `Esc` revient au détail.

### A6. Comment

1. Sur `alice/notes#1`, `c`, taper `Premier essai` sur deux lignes (`Enter` entre les deux).
   Attendu : zone de saisie ; aide en bas `Ctrl-S publish… · Ctrl-E editor · Esc cancel · F1 help` ; `q` et `?`
   s'écrivent dans le texte. `F1` : aide de la saisie (`Ctrl-S`, `Ctrl-E`, `Esc`), texte intact ; `F1` ou `Esc` la ferme.
2. `Ctrl-S`. Attendu : écran de confirmation qui montre le texte ; aucune requête `POST` dans le terminal 1.
3. `?` dans la confirmation. Attendu : aide `y/Enter` Publish, `n/Esc` Back to the text. Fermer.
4. `n`. Attendu : retour au texte, intact. `Ctrl-S` puis `y`.
   Attendu : `Publishing…`, un `POST …/issues/1/comments` dans le terminal 1, retour au détail rechargé, le
   commentaire en bas de la conversation.
5. `c`, texte vide, `Ctrl-S`. Attendu : `Write a text first.`, rien d'envoyé.
6. `c`, taper `abc`, `Esc`. Attendu : `Press Esc again to discard the text.` ; un second `Esc` ferme sans rien
   envoyer.

### A7. Review

1. `alice/notes#2`, `a`, `Ctrl-S` sans texte, `y`. Attendu : publication (`POST …/pulls/2/reviews`),
   détail rechargé, `@alice Approve` dans les verdicts.
2. `alice/notes#2`, `x`, `Ctrl-S` sans texte. Attendu : `Request changes needs a text.`. Taper un texte,
   `Ctrl-S`, `y`. Attendu : `@alice Request changes` remplace l'Approve.
3. `alice/notes#1` (revue en attente d'alice), `a`, texte `Gardé`, `Ctrl-S`, `y`.
   Attendu : message rouge `GitHub refused the review (422). …`, le texte reste. `Esc`.
   Attendu : `Text kept: open the review again to get it back.` ; `a` à nouveau : le texte `Gardé` revient.
4. Pendant `Publishing…` (latence de 500 ms), `q` et `Esc`. Attendu : ils ne quittent pas.

### A8. Éditeur externe (Linux/WSL)

Avant chaque cas : `ls /tmp/ghu-*.md` ne trouve rien ; après chaque cas, idem. Chaque cas part d'un Comment ouvert
sur `alice/notes#1` avec le texte `base`, et se termine par `Esc` ×2 (rien n'est publié).

1. **vim, `:wq`** : quitter `ghu`, `export VISUAL=vim`, relancer ; `Ctrl-E`. Attendu : vim plein écran sur un
   fichier `/tmp/ghu-….md` qui contient `base`. Ajouter une ligne `depuis vim`, `:wq`. Attendu : retour dans
   `ghu`, écran redessiné, texte `base` + `depuis vim`, curseur à la fin.
2. **vim, `:cq`** : `Ctrl-E`, modifier le texte, `:cq`. Attendu : notice `The editor failed (exit code 1): the
   text is unchanged.`, texte d'avant.
3. **nano** : `export VISUAL=nano`, relancer, `Ctrl-E`, modifier, `Ctrl-O` `Enter` `Ctrl-X`.
   Attendu : texte modifié revenu.
4. **Éditeur par défaut** : `unset VISUAL EDITOR`, relancer, `Ctrl-E`. Attendu : `vi` s'ouvre ; `:q` rend le
   texte inchangé, sans notice.
5. **Éditeur introuvable** : `export VISUAL=ghu-pas-d-editeur`, relancer, `Ctrl-E`. Attendu : `Cannot open the
   editor: ghu-pas-d-editeur was not found. Set VISUAL or EDITOR.`, texte gardé, `ghu` utilisable.
6. **Variable illisible** : `export VISUAL='"vim'`, relancer, `Ctrl-E`. Attendu : `Cannot read VISUAL: unbalanced
   quote or no program.`.
7. **`code --wait`** (si VS Code est installé côté WSL) : `export VISUAL='code --wait'`. Attendu : le fichier
   s'ouvre dans VS Code, `ghu` attend ; fermer l'onglet après enregistrement : le texte revient.
   Puis `export VISUAL=code` (sans `--wait`). Attendu : `The editor returned at once: the text is unchanged. …`.
8. **Redimensionnement** : `VISUAL=vim`, `Ctrl-E`, changer la taille de la fenêtre dans vim, `:wq`. Attendu :
   `ghu` redessiné à la nouvelle taille, sans débris.
9. **Touches tapées pendant l'ouverture** : `Ctrl-E` puis taper aussitôt `zzzz` avant que vim n'apparaisse ;
   `:q!`. Attendu : aucun `z` n'est ajouté au texte de `ghu` après le retour (dans vim, ils ont pu arriver :
   sans importance après `:q!`).
10. **Publication après l'éditeur** : `Ctrl-E`, écrire `publié via vim`, `:wq`, `Ctrl-S`, `y`. Attendu :
   commentaire publié sur le faux serveur.
11. **Fichiers restants** : `ls /tmp/ghu-*.md`. Attendu : aucun fichier.

### A9. Terminal, journal, jeton

1. Quitter par `q`, puis relancer et quitter par `Ctrl-C` (une fois sur la liste, une fois pendant une saisie).
   Attendu : terminal rendu propre (invite normale, écho des touches, curseur visible, pas d'écran alternatif).
2. `grep -c ghu-fake-token /tmp/ghu-essai.log`. Attendu : `0` (le jeton n'est jamais journalisé) ; le journal
   n'est pas vide.
3. `grep -c "Premier essai" /tmp/ghu-essai.log`. Attendu : `0` (le texte publié n'est pas journalisé).
4. Nettoyage : dans `ghu`, `u` sur `carol/public-lib`. Attendu : `Unpinned carol/public-lib.` (sinon il
   apparaîtrait `not found` contre le vrai GitHub).

## B. Windows, faux serveur

`ghu.exe` de l'artefact, dans Windows Terminal (PowerShell). Le faux serveur tourne dans WSL (terminal 1 de la
partie A, `--port 8787`) ; Windows le joint par `127.0.0.1` grâce au renvoi de localhost de WSL2 (à vérifier
au début de l'essai ; si `ghu.exe` ne le joint pas, lancer `cargo run -p gh-fake -- --port 8787` sous Windows si
Rust y est installé, sinon noter « non fait » et passer à la partie C).

```powershell
$env:GHU_API_URL = 'http://127.0.0.1:8787'
$env:GH_TOKEN = 'ghu-fake-token'
$env:GHU_LOG_FILE = "$env:TEMP\ghu-essai.log"; $env:GHU_LOG = 'debug'
```

1. `.\ghu.exe --version` et `.\ghu.exe --help`. Attendu : comme A1.1 et A1.2.
2. Sans jeton : `$env:GH_TOKEN = $null; $p = $env:Path; $env:Path = 'C:\nonexistent'; .\ghu.exe; $LASTEXITCODE;
   $env:Path = $p; $env:GH_TOKEN = 'ghu-fake-token'`. Attendu : `ghu: no GitHub token found.` et `1`.
3. `.\ghu.exe` : refaire rapidement A2.1 à A2.4, A3.1, A4.1, A5.1 et A5.3, A6.1 à A6.4. Attendu : comme sous
   Linux ; flèches, `PgUp` / `PgDn` et `Ctrl-S` reconnus ; couleurs du diff lisibles.
4. Saisie : taper `é à ç €` et `@ # { }` (AltGr) dans un Comment, coller un texte de plusieurs lignes
   (`Ctrl-V` de Windows Terminal). Attendu : caractères et sauts de ligne corrects.
5. Épinglage : `o` `carol/public-lib`, quitter, relancer. Attendu : toujours épinglé
   (`%APPDATA%\ghu\pinned.toml`) ; puis `u` pour le retirer.

### B1. Éditeur externe (Windows)

Avant et après chaque cas : `Get-ChildItem $env:TEMP -Filter 'ghu-*.md'` ne trouve rien. Même départ qu'en A8.

1. **notepad, aucune fenêtre Notepad ouverte** : `Remove-Item Env:VISUAL, Env:EDITOR -ErrorAction
   SilentlyContinue`, relancer, `Ctrl-E`. Attendu : Notepad s'ouvre sur `ghu-….md` ; `ghu` attend ; modifier,
   enregistrer, fermer Notepad : le texte revient.
2. **notepad, une fenêtre Notepad déjà ouverte** (avec un autre fichier) : `Ctrl-E`. Attendu possible (question
   Q2) : le fichier s'ouvre dans un onglet de la fenêtre existante et `ghu` affiche aussitôt `The editor returned
   at once: the text is unchanged. …` ; ou bien `ghu` attend comme au cas 1. Noter exactement ce qui se passe.
   Fermer l'onglet sans enregistrer, puis vérifier le dossier temporaire.
3. **`code.cmd --wait`** : `$env:VISUAL = 'code.cmd --wait'`, relancer, `Ctrl-E`. Attendu : VS Code ouvre le
   fichier, `ghu` attend ; enregistrer et fermer l'onglet : le texte revient.
4. **`code --wait`** (sans `.cmd`) : Attendu : `Cannot open the editor: code was not found. Set VISUAL or
   EDITOR.`.
5. **`code.cmd` sans `--wait`** : Attendu : `The editor returned at once: …`.
6. **Éditeur introuvable** : `$env:VISUAL = 'ghu-pas-d-editeur'`. Attendu : comme A8.5.
7. **Redimensionnement** : avec un éditeur console si disponible (`vim` de Git for Windows, par exemple
   `$env:VISUAL = '"C:\Program Files\Git\usr\bin\vim.exe"'`), redimensionner pendant l'édition ; sinon
   redimensionner Windows Terminal pendant que Notepad est ouvert. Attendu : au retour, `ghu` redessiné à la
   bonne taille.
8. **Touches tapées pendant l'ouverture** : `Ctrl-E` puis taper aussitôt `zzzz` dans Windows Terminal ; fermer
   l'éditeur sans enregistrer. Attendu : aucun `z` dans le texte de `ghu`.
9. **Fichiers restants** : `Get-ChildItem $env:TEMP -Filter 'ghu-*.md'`. Attendu : rien.

### B2. Terminal et journal

1. Quitter par `q`, puis par `Ctrl-C` (liste et saisie). Attendu : Windows Terminal rendu propre (invite
   PowerShell, curseur, pas d'écran alternatif).
2. `Select-String -Path $env:TEMP\ghu-essai.log -Pattern ghu-fake-token`. Attendu : aucune ligne.

## C. Vrai GitHub (en dernier)

**Lecture seule** sauf accord explicite. Avant de lancer : retirer les variables du faux serveur.

- Linux/WSL : `unset GHU_API_URL GH_TOKEN GITHUB_TOKEN` (le jeton viendra de `gh auth token`).
- Windows : `Remove-Item Env:GHU_API_URL, Env:GH_TOKEN, Env:GITHUB_TOKEN -ErrorAction SilentlyContinue`.

1. `ghu`. Attendu : la liste de vos repos (les vôtres, ceux où vous êtes collaborateur, ceux de vos
   organisations) ; forks marqués ; archivés masqués, visibles avec `a`.
2. `o` + un repo public d'autrui que vous suivez (lecture). Attendu : ses PRs ; il est épinglé. `u` ensuite.
3. Une PR ouverte d'un de vos repos : détail, verdicts, conversation, `d` (diff) avec plusieurs fichiers, `n` /
   `p`, défilement ; si possible une PR avec un fichier binaire. Attendu : contenu identique à la page GitHub.
4. `Tab` sur une liste : PRs fermées et fusionnées. Attendu : conformes à GitHub.
5. Sur une PR : `c`, texte, `Ctrl-S`, puis **`n` puis `Esc` ×2** (ne pas publier). Attendu : aucun
   commentaire créé sur GitHub (vérifier sur la page).
6. Faire 1 à 4 aussi sous Windows (`ghu.exe`, jeton de `gh` de Windows).
7. **Avec accord seulement**, sur un repo d'essai à vous : publier un Comment sur une de vos PRs. Attendu :
   visible sur GitHub, détail rechargé. Sur votre propre PR, les verdicts ne sont pas offerts. Un Approve ou un
   Request changes demande une PR ouverte par un autre compte : sinon, noter « non fait ».
8. Fin : `ls /tmp/ghu-*.md` et `Get-ChildItem $env:TEMP -Filter 'ghu-*.md'` vides ; `pinned.toml` ne contient
   que les repos voulus.

## Après l'essai

- Q2 (`docs/questions-ouvertes.md`, éditeur par défaut sous Windows) se tranche d'après B1.2.
- Défauts trouvés : consignés dans la fiche, puis corrigés par le lot du périmètre touché, `qa`, et re-test de
  l'étape seule (nouvelle section de fiche).
- Le tag `v0.1.0` est posé sur le commit essayé ; le SHA-256 de `ghu.exe` publié doit égaler celui de l'en-tête.
