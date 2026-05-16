# Patchloom Maintainer Workflow

This file is for maintainers of `patchloom/patchloom`.

It describes the default workflow for triaging outside issues, reviewing pull requests, and merging contributions safely.

## Default Policy

| Topic | Default |
|---|---|
| Issues | open to everyone |
| Outside code contributions | via fork + pull request |
| Protected branch | `main` once the repo is public or on a plan that supports private branch protection |
| Merge requirements | DCO, green CI and Security checks, resolved review conversations, 1 approval |
| Merge method | squash merge |
| Direct pushes to `main` | no |

## Issue Triage

When a new outside issue arrives:

1. classify it as bug, docs, feature, or discussion
2. ask for missing repro details if the report is incomplete
3. mark the issue as accepted, needs-info, discussion, or not-planned
4. encourage a pull request only after the scope is clear

### Triage Guide

| Issue type | Maintainer action |
|---|---|
| Bug report | ask for repro, expected behavior, actual behavior, version, and relevant logs |
| Docs or typo | confirm quickly and invite a small PR |
| Feature request | discuss fit, scope, and likely shape before inviting implementation |
| Large design change | move to issue discussion before code |

## Pull Request Intake

| PR shape | Maintainer response |
|---|---|
| Small docs fix | review directly |
| Small bug fix with clear scope | review directly |
| Large feature without prior issue | ask for issue or discussion first |
| Broad mixed-scope PR | ask contributor to split it |
| Incomplete PR | request missing tests, docs, or verification |

## Review Checklist

Before approving, check:

- all commits are signed off
- CI is green
- the change is focused and understandable
- tests were added or updated when behavior changed
- docs or examples were updated when flags, output, or CLI behavior changed
- unresolved review comments are addressed

## Approval Policy

Approve when all of the following are true:

- the scope is acceptable
- the implementation matches the agreed direction
- DCO passes
- CI passes
- review comments are resolved

Do not approve just because the patch is small. Small patches can still change public CLI behavior.

## Merge Policy

Use squash merge by default.

Merge when all of the following are true:

- at least one maintainer approved
- required checks passed
- all review conversations are resolved
- the PR title and description are clear enough for the commit history

After merge:

- auto-delete the source branch if possible
- confirm linked issues are closed when appropriate
- create a follow-up issue immediately if review surfaced real deferred work

## Branch Protection

Protect only `main` once GitHub branch protection is available for the repository.

Recommended settings:

- require pull requests before merge
- require 1 approval
- dismiss stale approvals on new commits
- require resolved conversations
- require DCO, CI, and Security checks
- do not require signed commits
- do not allow force pushes to `main`

Contributor branches on forks do not need branch protection. Until `main` can be protected, maintainers should still avoid direct pushes except for the initial private bootstrap or emergencies.

## Maintainer Notes

- Keep contribution friction low for narrow, high-signal patches.
- Require prior discussion for features that affect CLI shape, output contracts, or project scope.
- Prefer saying "please open an issue first" over reviewing a large unaligned PR for an hour.
- Keep decisions consistent. Outsiders should know what to expect from the process.
