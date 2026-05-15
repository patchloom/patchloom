# Contributing to Patchloom

Thanks for your interest in Patchloom.

Patchloom aims to be a low-friction, high-trust open-source CLI. Keep contributions small, reviewable, and well-explained.

## Before You Open a Pull Request

- Check whether an issue or discussion already covers the work.
- Prefer one logical change per pull request.
- If the change affects CLI behavior, flags, output, or docs examples, update the relevant docs in the same pull request.
- Run the relevant tests, lint, and formatting steps for the files you changed.

## Issues and Proposal Workflow

- Anyone can open an issue.
- Small docs fixes, typo fixes, and narrow bug fixes may be opened as pull requests directly.
- For non-trivial features, CLI surface changes, or design changes, open an issue or discussion first and wait for maintainer alignment before writing a large pull request.
- Maintainers may close or redirect proposals that do not fit the current scope.

## Development Expectations

- Keep pull requests focused.
- Add or update tests when behavior changes.
- Call out breaking changes clearly.
- Include enough context in the PR description for a reviewer to understand the change quickly.
- Run `make check` before requesting review.
- Use `make help` to see the available local development commands.

## Commit Sign-off Requirement (DCO)

Patchloom uses the [Developer Certificate of Origin 1.1](https://developercertificate.org/).

Every commit must include a `Signed-off-by:` trailer that matches the commit author identity.

Use this for new commits:

```bash
git commit -s -m "<message>"
```

Use this to fix the most recent commit:

```bash
git commit --amend -s
```

If you use a GUI or another tool, make sure the final commit message still contains a valid sign-off line.

## Security Reporting

Do not post sensitive security details in public channels.

While the repository is private, follow [SECURITY.md](./SECURITY.md) for the temporary reporting policy. After the repository is public, maintainers should enable GitHub private vulnerability reporting.

## Licensing

By contributing to Patchloom, you agree that your contributions are licensed under the repository license: `MIT OR Apache-2.0`.

## Pull Request Checklist

Before requesting review, make sure that:

- all commits in the pull request are signed off
- relevant tests, lint, and formatting checks were run
- docs and examples were updated when behavior changed
- the pull request description explains what changed, why, and how it was verified

## Review and Merge Policy

- External contributors should open pull requests from a fork. The target policy is to protect `main` and merge changes through pull requests. While the repo remains private on GitHub Free, maintainers should follow that policy by convention because GitHub cannot enforce private branch protection there.
- DCO and required status checks should pass before merge.
- At least one maintainer approval is required before merge.
- Unresolved review comments should be addressed before merge.
- Maintainers may ask for a smaller or more focused pull request if the scope is too broad.
- Squash merge is the default unless a maintainer asks for a different history shape.
