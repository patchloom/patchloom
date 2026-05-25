# Patchloom Day 1 Repo Setup

This file captures the recommended day 1 policy setup for `patchloom/patchloom`, including the private bootstrap phase and the later public launch phase.

## Required Integrations

| Item | Recommendation | Notes |
|---|---|---|
| DCO enforcement | Install the [DCO-2 GitHub App](https://github.com/apps/dco-2) | make its status check required once the repo is public, or on a plan that supports private branch protection |
| CI | Have at least one required CI workflow | use a single `CI` workflow at the start, and point it at the repo's self-hosted runner once that runner exists |
| Release workflow | Commit `dist-workspace.toml`, `.github/workflows/release.yml`, and `.github/workflows/publish-crates.yml` together | keep cargo-dist config and release automation in sync, keep release jobs on GitHub-hosted runners at first, and skip crates.io and Homebrew publishing while the repo is private |
| Homebrew tap repo | create `patchloom/homebrew-tap` before the first stable release | the release workflow pushes formula updates there |
| Release secrets | add `HOMEBREW_TAP_TOKEN` and `CARGO_REGISTRY_TOKEN` | needed before the first public release |
| Security reporting | commit `SECURITY.md` now, then enable GitHub private vulnerability reporting after the repo becomes public | private Free org repos do not expose GitHub private vulnerability reporting yet |
| Branch protection | Protect `main` once the repo is public or on a plan that supports private branch protection | required before accepting outside patches |
| Public repo metadata | commit `LICENSE`, `README.md`, `CONTRIBUTING.md`, and issue templates in the bootstrap | keep the repo legible from day one |

## Recommended Branch Protection For `main`

| Setting | Recommendation |
|---|---|
| Require a pull request before merging | yes |
| Required approvals | 1 |
| Dismiss stale approvals when new commits are pushed | yes |
| Require review from code owners | no, not on day 1 |
| Require conversation resolution before merge | yes |
| Require status checks before merge | yes |
| Required status checks | the DCO-2 check, the main CI check, and the Security workflow |
| Require signed commits | no, not on day 1 |
| Require linear history | yes, if squash merge is the default |
| Allow force pushes | no |
| Allow branch deletion | no |

Protect only `main`. Outside contributors should work from forks and open pull requests back to `main`. Contributor branches on forks do not need branch protection.

## Recommended Repository Settings

| Setting | Recommendation |
|---|---|
| Default branch | `main` |
| Allow squash merges | yes |
| Allow merge commits | no |
| Allow rebase merges | optional |
| Auto-delete head branches | yes |
| Enable auto-merge | yes, after the repo is public or on a paid plan that supports it |
| Enable dependency graph and vulnerability alerts | yes, during the private phase |
| Enable secret scanning | yes, after the repo is public or on a paid plan that supports private-repo scanning |

## Maintainer Workflow For Outside Contributions

| Situation | Recommendation |
|---|---|
| New issue | allow anyone to open it, then triage it as bug, docs, feature, or discussion |
| Small docs or typo PR | allow direct PR and review quickly |
| Narrow bug fix PR | allow direct PR if the scope is clear |
| Non-trivial feature PR | ask for an issue or discussion first |
| Merge gate | require DCO, green CI and Security checks, resolved conversations, and 1 approval |
| Merge method | use squash merge and auto-delete the merged branch |

## DCO-2 Setup Steps

1. Install [DCO-2](https://github.com/apps/dco-2) on the repo or org.
2. Commit `.github/dco.yml` with the initial low-friction policy.
3. Add DCO instructions to `CONTRIBUTING.md`.
4. Open a small test PR with signed-off commits.
5. Push the bootstrap branch and let the self-hosted `CI` workflow pass once.
6. Once the repo is public, or on a paid plan that supports private branch protection, mark the DCO-2 check, CI, and Security workflows as required on `main`.

## Self-Hosted Runner Recovery

If `CI` stays queued on maintainer PRs or pushes to `main`, check the self-hosted runner before retrying workflows. Patchloom currently relies on one runner, `patchloom-mac-1`, for trusted `CI` jobs. The `Security` workflow also uses the self-hosted runner for trusted events, and falls back to GitHub-hosted `ubuntu-latest` only for fork PRs.

1. Check runner status in GitHub:
   ```bash
   gh api repos/patchloom/patchloom/actions/runners \
     --jq '.runners[] | {name,status,busy,labels:[.labels[].name]}'
   ```
2. Inspect the queued run and its jobs:
   ```bash
   gh run list --repo patchloom/patchloom --limit 5 \
     --json databaseId,workflowName,status,conclusion,displayTitle,headBranch,event
   gh api repos/patchloom/patchloom/actions/runs/RUN_ID/jobs \
     --jq '.jobs[] | {name,status,conclusion,runner_name,labels,started_at}'
   ```
3. If `patchloom-mac-1` is offline, restart the service on the runner host:
   ```bash
   cd ~/actions-runner-patchloom
   ./svc.sh start
   ./svc.sh status
   ```
4. Re-check the `CI` or `Security` workflow. The queued self-hosted jobs should move to `in_progress` once the runner is back online.
5. Before pushing another commit, confirm the previously queued run finishes green.

This recovery path is intentionally lightweight. It does not replace a second runner or a GitHub-hosted fallback, but it keeps maintainer workflows unblocked when the single runner goes offline.

## Initial `.github/dco.yml`

```yaml
allowRemediationCommits:
  individual: true
allowOverrideAction: false
```

## Why This Setup

- DCO keeps contribution friction low.
- Required checks keep provenance and CI policy explicit.
- Keeping the day 1 CI job on the self-hosted runner avoids private-repo minute pressure while still keeping release publishing isolated on GitHub-hosted runners.
- One approval is enough for day 1, while still preventing direct unreviewed merges.
- Avoiding mandatory signed commits on day 1 reduces contributor friction without giving up provenance, because DCO already records authorship intent.
