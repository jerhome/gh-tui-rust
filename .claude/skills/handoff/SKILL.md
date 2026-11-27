---
name: handoff
description: End-of-session handoff that updates docs/reprise.md and CLAUDE.local.md instead of /tmp. Use when the user asks for a handoff ("passation", "fin de session", "point de reprise"), never on your own.
argument-hint: "What will the next session be used for?"
disable-model-invocation: true
---

# Passation

Mets à jour le point de reprise pour qu'un agent frais, sur ce poste ou l'autre, continue le travail. Écris dans le
repo, pas dans `/tmp` (perdu à l'extinction) : `docs/reprise.md` fait foi, `CLAUDE.local.md` ne garde que le
local.

## Étapes

1. **Heure.** Lance `date` ; « Mis à jour le AAAA-MM-JJ HH:MM » prend cette valeur, jamais une heure estimée.
2. **`docs/reprise.md`** (versionné, commun aux postes) garde la forme existante : *État*, *Prochaine action*
   (liste ordonnée), *Points ouverts*. Réécris ces sections d'après la session ; si l'utilisateur a passé un
   argument, adapte la « Prochaine action » à ce qu'il décrit.
   - Renvoie aux artefacts par numéro, chemin ou URL (issues, ADR, commits, run de CI) sans les recopier.
   - Termine par une rubrique **Skills suggérés** : les skills que le prochain agent appelle avec l'outil Skill, et
     les agents de base à lancer (voir `CLAUDE.md`).
   - Masque les secrets et données personnelles (jetons, clés, adresses).
3. **`CLAUDE.local.md`** (non versionné) : worktrees ouverts, agents en cours, chemins et variables du poste.
   Rien de ce qui concerne l'autre poste n'y va.
4. **Pousse.** Fin de session = `docs/reprise.md` à jour **et poussé**, et les branches en cours poussées, pour que
   l'autre poste les voie. Le push utilise le compte `jerhome` : `GH_TOKEN=$(gh auth token -u jerhome)` (jeton
   fourni au credential helper), jamais `gh auth switch`. Accord permanent de commit et de push dans ce repo
   (`CLAUDE.md`) ; groupe tout dans un seul push.
5. **Vérifie.** `git status` propre ; `git log origin/main..` vide ; `git fetch` ne montre aucune avance de
   `origin`. Critère de fin : l'état distant porte la même « Prochaine action » que ta dernière écriture.
