---
name: retro
description: Run a retrospective that improves the agents' environment and the base agents of agents-home. Use when the user asks for a retro ("rétro"), never on your own.
disable-model-invocation: true
---

# Rétro

Une rétro améliore l'**environnement** des agents (repo, skills, fiches), pas le code du produit. Elle fusionne la
méthode de mattpocock (candidats d'amélioration, du plus mécanique au plus subjectif) et la rétro d'agents-home
(section « Agents » de `../agents-home/AGENTS.md`, point « Amélioration (en rétro) »).

Appelle d'abord le skill `writing-for-agents` : toute règle écrite pendant la rétro suit son guide.

## Étapes

1. **Cycle.** Lis la date de la dernière rétro (`CLAUDE.local.md` d'agents-home, ligne « Dernière rétro »), puis
   `date`. Le cycle va de l'une à l'autre. Relève les projets touchés : `git log --since=<date> --all` dans chaque
   repo de `../agents-home/AGENTS.md`, plus les agents lancés notés dans `docs/reprise.md`. Ce repo est le banc
   d'essai des agents de base : ne traite que ce repo et agents-home, sauf demande contraire.
2. **Sources.** Lis `docs/retours-agents.md` (lignes du cycle), `docs/reprise.md`, `git log` du cycle et les
   échecs de CI. Critère de fin : chaque ligne du journal du cycle est classée dans une catégorie de l'étape 3 ou
   écartée avec sa raison.
3. **Candidats**, par catégorie (la première qui s'applique) :
   - **Faute mécanique** (motif de syntaxe, API interdite, forme d'import, emplacement de fichier, commande
     dangereuse) : un **contrôle déterministe**, tranché sans débat : hook (`.claude/settings.json`,
     `scripts/`), lint clippy ou `deny.toml`, étape de CI. Lis d'abord les contrôles qui existent (CI, hooks) :
     un contrôle présent mais non branché ou cassé est le constat.
   - **Jugement** (cohérence entre fichiers, « comme le style voisin ») : une règle pour `relecteur`, dans le
     `CLAUDE.md` du repo ou la fiche. Une règle qui ne change pas le comportement est retirée.
   - **Défaut d'un agent de base** (fiche de `../agents-home/.claude/agents/`) : consigne-le comme proposition pour
     la rétro d'agents-home (étape 4). Une fiche ne se corrige jamais ici ; les propositions se règlent là-bas.
   - **Navigation** : un pointeur vers le fichier que l'agent a mis longtemps à trouver.
   - **Information** : une source qui manquait (journaux, accès en lecture, exemples téléchargés plutôt que écrits de
     mémoire).
   - **Économie d'outils** : appels coûteux, build lourd lancé en parallèle, cible de build partagée.
   - **Mémoire** : les consignes `CLAUDE.md` trop longues sont déplacées vers un contrôle ou une doc pointée.
4. **Défauts des agents de base.** Regroupe les lignes de `docs/retours-agents.md` par fiche (architecte, dev, qa,
   relecteur, plateforme) avec la piste de chaque ligne. Ces propositions partent à la rétro d'agents-home, où elles
   se décident.
5. **Questionnaire unique.** Présente les candidats par gravité, en un seul questionnaire (AskUserQuestion), une
   question par candidat avec une réponse par défaut marquée. Les questions d'une conception posées plus tôt sont
   regroupées ici. Critère de fin : chaque candidat a une réponse de l'utilisateur.
6. **Consigner.** Écris les décisions dans `docs/reprise.md` (rubrique « Rétro n° N — AAAA-MM-JJ HH:MM », heure lue
   avec `date`). Une décision ne s'applique qu'avec l'accord de l'utilisateur ; les mesures acceptées sont ensuite
   appliquées, chacune vérifiée (un hook se teste, un contrôle se voit rouge puis vert).
7. **Clôture.** Rappelle à l'utilisateur de noter « Dernière rétro : AAAA-MM-JJ HH:MM » dans `CLAUDE.local.md`
   d'agents-home.

## Principes (de mattpocock)

- Les agents d'implémentation ont la plus forte charge de contexte : les normes sont imposées par la relecture et
  par des contrôles, pas par une longue liste de consignes.
- Les fichiers chargés à chaque tour (`CLAUDE.md`) servent surtout de pointeurs ; le reste vit dans des docs ou des
  skills.
