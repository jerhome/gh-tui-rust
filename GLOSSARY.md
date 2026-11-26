# ghu

A terminal client for GitHub: browse repos and their pull requests, and review them.

## Repos

**Repo**:
A GitHub repository the user can browse in ghu, identified by `owner/name`.
_Avoid_: Project, repository (in UI text)

**Repo list**:
The repos shown at startup: those the user owns, collaborates on, or reaches through an organization, plus pinned
repos.
Archived repos are hidden unless a filter shows them; forks are shown and marked.
_Avoid_: Dashboard, favorites

**Pinned repo**:
A repo the user opened by typing `owner/name`, kept in the repo list across sessions until unpinned.
_Avoid_: Favorite, bookmark

**Archived repo**:
A read-only repo; its pull requests can be browsed but not reviewed.

## Pull requests

**Pull request (PR)**:
A proposed change to a repo, with a conversation, commits and a diff.
_Avoid_: Merge request, MR

**Draft PR**:
An open PR its author marked as not ready; listed with open PRs and marked as draft.

**PR state**:
Open, closed or merged. Only an open PR can be reviewed or commented on.

**Own PR**:
A PR whose author is the signed-in user; GitHub forbids giving it a verdict.

**Diff**:
The changes of a PR, file by file, as a unified diff.
_Avoid_: Patch, changeset

## Feedback

**Comment**:
A message added to a PR's conversation, carrying no verdict.
_Avoid_: General comment, note, reply

**Review**:
A verdict on a PR, published at once, with an optional body (mandatory for Request changes).
_Avoid_: Approval (for the whole act), feedback

**Verdict**:
The decision a review carries: Approve or Request changes.
_Avoid_: Status, vote, decision

**Pending review**:
A review started on the GitHub website and not yet published; ghu ignores it.
_Avoid_: Draft review (confusable with Draft PR)
