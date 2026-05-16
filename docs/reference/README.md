# Patchloom Reference

This is the reference for Patchloom's meaningful commands, actions, operations, and notable command modes.

- Start with [Quickstart](../getting-started/quickstart.md) if you want a first success.
- Read [Core Concepts](../getting-started/concepts.md) for shared semantics like write modes, exit codes, and transaction behavior.
- Use this file when you need to choose the right feature or mode for a job, or when a pull request adds meaningful CLI surface and the docs coverage test expects it here.

## Global behaviors

Patchloom has a small set of global features that shape how other commands behave.

### Write modes

Patchloom write commands default to preview mode. The canonical semantics live in [Core Concepts](../getting-started/concepts.md#write-modes). The sections below focus on when to choose each mode.

<!-- ref:write-flag:diff -->
### `--diff`

- **What it does:** Prints the unified diff for a write command without mutating files.
- **Use when:** You want a human review step before applying a change, or you want to inspect the exact patch Patchloom would write.
- **Prefer instead:** Use `--check` for CI pass or fail behavior, or `--apply` to actually write files.

<!-- ref:write-flag:apply -->
### `--apply`

- **What it does:** Writes the requested change to disk.
- **Use when:** You have already previewed the change, or you trust the command and want the mutation to happen now.
- **Prefer instead:** Use `--diff` when reviewing, or `--check` when you only need a clean or dirty signal.

<!-- ref:write-flag:check -->
### `--check`

- **What it does:** Calculates whether a write command would change files and returns exit code 2 when changes are pending.
- **Use when:** You are wiring Patchloom into CI, pre-commit validation, or agent workflows that should fail on drift.
- **Prefer instead:** Use `--diff` when you need the actual patch text, or `--apply` when you want the mutation.

### Write policy flags

These flags shape how written content is normalized before it reaches disk.

<!-- ref:write-flag:ensure-final-newline -->
### `--ensure-final-newline`

- **What it does:** Ensures non-empty written files end with `\n`.
- **Use when:** You want simple newline hygiene on every touched file without running a separate cleanup command.
- **Prefer instead:** Use `hygiene fix` when the goal is repo cleanup, not just normalization of files already being edited.

<!-- ref:write-flag:normalize-eol -->
### `--normalize-eol`

- **What it does:** Normalizes written line endings to `keep`, `lf`, or `crlf`.
- **Use when:** A repo or downstream tool expects a specific line ending convention.
- **Prefer instead:** Use `--respect-editorconfig` when the repo already declares the desired convention there.

<!-- ref:write-flag:trim-trailing-whitespace -->
### `--trim-trailing-whitespace`

- **What it does:** Removes trailing spaces and tabs from touched lines before writing.
- **Use when:** You want text cleanup to happen automatically as part of another write command.
- **Prefer instead:** Use `hygiene fix` when the goal is to sweep existing files for whitespace problems.

<!-- ref:write-flag:respect-editorconfig -->
### `--respect-editorconfig`

- **What it does:** Reads `.editorconfig` when present and applies matching write policy.
- **Use when:** The repo already encodes formatting policy in `.editorconfig` and Patchloom should follow it automatically.
- **Prefer instead:** Use explicit write flags, or `tx` `write_policy`, when the command should be self-contained and not depend on repo metadata.

### Output and scope flags

These flags affect how Patchloom reports results or chooses which files to touch.

<!-- ref:global-flag:json -->
### `--json`

- **What it does:** Emits one machine readable JSON document for the command result.
- **Use when:** Another tool, script, or agent needs structured output instead of human oriented text.
- **Prefer instead:** Use `--jsonl` when you want one JSON object per result line for streaming style consumers.

<!-- ref:global-flag:jsonl -->
### `--jsonl`

- **What it does:** Emits one JSON object per result line.
- **Use when:** A read style command may produce many results and you want to stream them incrementally to another tool.
- **Prefer instead:** Use `--json` when you want one aggregate document for the whole command.

<!-- ref:global-flag:quiet -->
### `--quiet`

- **What it does:** Suppresses non-JSON human readable output.
- **Use when:** Only the exit code or the file mutation matters and extra stdout noise would get in the way.
- **Prefer instead:** Use `--json` when another tool still needs structured output.

<!-- ref:global-flag:cwd -->
### `--cwd`

- **What it does:** Sets the working directory used to resolve relative paths.
- **Use when:** You are invoking Patchloom from outside the target repo, or you want scripts to behave predictably regardless of the caller's current directory.
- **Prefer instead:** Use a plan level `cwd` in `tx` when the directory choice should travel with the plan itself.

<!-- ref:global-flag:glob -->
### `--glob`

- **What it does:** Restricts candidate files by one or more glob patterns.
- **Use when:** A command should only see a narrow file type or subtree, even if the input path is broader.
- **Prefer instead:** Use `--files-from` when another tool has already determined the exact file list.

<!-- ref:global-flag:files-from -->
### `--files-from`

- **What it does:** Reads the target file list from a file, or from stdin when passed `-`.
- **Use when:** Another tool already selected the exact paths and Patchloom should operate only on that set.
- **Prefer instead:** Use `--glob` for pattern based scoping, or direct path arguments when the target set is already small and obvious.

### Exit codes

Use [Core Concepts](../getting-started/concepts.md#exit-codes) as the canonical exit code table. When integrating Patchloom into CI or agent workflows, branch on exit codes instead of parsing human readable output.

## Commands

These are the main entry points. If you are deciding between commands, start here.

<!-- ref:command:search -->
## `search`

- **What it does:** Searches files with literal or regex matching, optional context, counts, and file only results.
- **Use when:** You need to locate candidate edits, audit repo state, or narrow inputs before changing files.
- **Prefer instead:** Use `replace` for actual text mutation, or `doc`, `md`, or `patch` when you already know the structured change you want.
- **Related:** `--glob`, `--files-from`, `replace`

<!-- ref:command:replace -->
## `replace`

- **What it does:** Performs mechanical string replacement across one or many files, with literal or regex matching.
- **Use when:** You are doing a rename, version bump, boilerplate rewrite, or another string level change where plain text semantics are enough.
- **Prefer instead:** Use `doc` for structured data, `md` for heading aware markdown, or `patch` when you already have a unified diff.
- **Related:** `search`, `tx`

<!-- ref:command:patch -->
## `patch`

- **What it does:** Checks or applies a unified diff.
- **Use when:** The change already exists as a patch, or you want stale context detection instead of search and replace semantics.
- **Prefer instead:** Use `replace`, `doc`, or `md` when you want to describe the mutation directly instead of carrying a diff artifact.
- **Related:** `patch check`, `patch apply`, `tx patch.apply`

<!-- ref:command:md -->
## `md`

- **What it does:** Performs heading aware markdown edits for sections, bullets, tables, and AGENTS linting.
- **Use when:** Documentation needs semantic markdown edits that should not depend on raw byte offsets.
- **Prefer instead:** Use `replace` for simple line level edits, or `patch` for exact diff application.
- **Related:** `md` actions, `tx` markdown operations

<!-- ref:command:doc -->
## `doc`

- **What it does:** Performs parser backed JSON, YAML, and TOML queries and mutations.
- **Use when:** Config or metadata changes should operate on keys and arrays instead of brittle text matching.
- **Prefer instead:** Use `replace` for plain text, `md` for markdown, or `patch` for existing diffs.
- **Related:** `doc` actions, `tx` document operations

<!-- ref:command:hygiene -->
## `hygiene`

- **What it does:** Checks or fixes trailing whitespace, line endings, and final newlines.
- **Use when:** You need repo text normalization, or a CI guard for basic text hygiene.
- **Prefer instead:** Use write policy flags when the cleanup should only apply to files already being touched by another command.
- **Related:** `hygiene check`, `hygiene fix`, `tx hygiene.fix`

<!-- ref:command:create -->
## `create`

- **What it does:** Creates a new file from literal content or stdin.
- **Use when:** Generating a new tracked file is the whole task, or one step in a larger transaction.
- **Prefer instead:** Use `doc`, `md`, or `replace` when the file already exists and only needs edits.
- **Related:** `delete`, `tx file.create`

<!-- ref:command:delete -->
## `delete`

- **What it does:** Removes a file.
- **Use when:** A file should disappear outright and no other atomic edits are needed.
- **Prefer instead:** Use `tx file.delete` when the removal must be bundled atomically with other changes.
- **Related:** `create`, `tx file.delete`

<!-- ref:command:tx -->
## `tx`

- **What it does:** Runs multiple operations atomically, then optional format and validate steps.
- **Use when:** One logical change spans multiple files or mutation types and partial writes are unacceptable.
- **Prefer instead:** Use standalone commands when one direct operation is enough.
- **Related:** [examples/README.md](../../examples/README.md), `tx` fields, `tx` operations

<!-- ref:command:completions -->
## `completions`

- **What it does:** Generates shell completion scripts for bash, zsh, fish, or elvish.
- **Use when:** You are installing Patchloom into an interactive shell and want faster command discovery.
- **Prefer instead:** Nothing, if Patchloom is only used from scripts or ephemeral CI runners.
- **Related:** [installation guide](../getting-started/installation.md)

## Command modes

These are meaningful command-specific modes that change how a top-level command behaves, even though they are not separate subcommands.

<!-- ref:search-mode:files-with-matches -->
### `search --files-with-matches`

- **What it does:** Emits only file paths that contain at least one match.
- **Use when:** You need a path list to feed into another tool or command instead of the matching lines themselves.
- **Prefer instead:** Use `search --count` when per-file match totals matter, or plain `search` when the matching lines matter.

<!-- ref:search-mode:count -->
### `search --count`

- **What it does:** Emits match counts per file instead of full matching lines.
- **Use when:** You are auditing prevalence, comparing files, or gating on how many matches remain.
- **Prefer instead:** Use plain `search` when you need the matching text, or `search --files-with-matches` when only file membership matters.

<!-- ref:search-mode:invert-match -->
### `search --invert-match`

- **What it does:** Shows lines that do not match the pattern.
- **Use when:** You are looking for non-conforming lines or excluding content that matches a known pattern.
- **Prefer instead:** Use plain `search` when you want the matching lines themselves.

<!-- ref:search-mode:multiline -->
### `search --multiline`

- **What it does:** Lets regex matches span multiple lines by making `.` match newlines.
- **Use when:** The pattern you care about is inherently block-shaped, such as a function body or multi-line stanza.
- **Prefer instead:** Use plain `search` for line-oriented patterns because it is simpler and easier to reason about.

<!-- ref:search-mode:case-insensitive -->
### `search --case-insensitive`

- **What it does:** Matches regardless of case.
- **Use when:** The target text may appear in inconsistent capitalization across files.
- **Prefer instead:** Use case-sensitive search when exact spelling matters and false positives would be noisy.

<!-- ref:replace-mode:regex -->
### `replace --regex`

- **What it does:** Treats `--from` as a regex instead of a literal string.
- **Use when:** The change is pattern-based, or capture groups should shape the replacement.
- **Prefer instead:** Use literal replace for fixed text because it is simpler and less error-prone.

<!-- ref:replace-mode:if-exists -->
### `replace --if-exists`

- **What it does:** Returns success even when no matches are found.
- **Use when:** The replacement is intentionally idempotent and should not fail if the repo is already in the desired state.
- **Prefer instead:** Use default replace behavior when a missing match should be treated as drift or an error.

<!-- ref:replace-mode:nth -->
### `replace --nth`

- **What it does:** Replaces only the Nth occurrence of the target.
- **Use when:** Replacing every occurrence would be too broad and the exact positional match matters.
- **Prefer instead:** Use plain replace when every occurrence should change, or regex when the target can be narrowed semantically.

<!-- ref:replace-mode:multiline -->
### `replace --multiline`

- **What it does:** Lets regex replacement span multiple lines by making `.` match newlines.
- **Use when:** The target pattern is a multi-line block rather than a single line.
- **Prefer instead:** Use line-oriented replace when the match should stay local and easy to inspect.

<!-- ref:replace-mode:case-insensitive -->
### `replace --case-insensitive`

- **What it does:** Matches regardless of case during replacement.
- **Use when:** The target text appears with inconsistent capitalization and should still be updated uniformly.
- **Prefer instead:** Use case-sensitive replace when exact spelling is part of the safety boundary.

<!-- ref:create-mode:stdin -->
### `create --stdin`

- **What it does:** Reads the new file content from stdin instead of `--content`.
- **Use when:** Another tool is generating the content, or shell composition is cleaner than embedding the full text in one argument.
- **Prefer instead:** Use `create --content` for short inline content that should stay visible in the command itself.

<!-- ref:create-mode:force -->
### `create --force`

- **What it does:** Overwrites an existing file instead of failing.
- **Use when:** File recreation is intentional and should replace previous contents deterministically.
- **Prefer instead:** Use default create behavior when accidental overwrite would be dangerous.

<!-- ref:patch-mode:file -->
### `patch --file`

- **What it does:** Reads the unified diff from a file path.
- **Use when:** The patch already exists as a saved artifact that should be reviewed, reused, or passed around directly.
- **Prefer instead:** Use `patch --stdin` when another tool is piping the patch text dynamically.

<!-- ref:patch-mode:stdin -->
### `patch --stdin`

- **What it does:** Reads the unified diff from stdin instead of `--file`.
- **Use when:** Another tool is generating or piping the patch text directly.
- **Prefer instead:** Use `patch --file` when the diff should be stored as a tangible artifact.

<!-- ref:doc-mode:predicate -->
### `doc --predicate`

- **What it does:** Supplies the key-value predicate used by `doc delete-where`.
- **Use when:** Array cleanup should target matching objects instead of deleting by fixed index or selector path alone.
- **Prefer instead:** Use `doc delete` when one direct selector can remove the target without predicate filtering.

<!-- ref:doc-mode:stdin -->
### `doc --stdin`

- **What it does:** Reads merge payload content from stdin for `doc merge`.
- **Use when:** The object being merged is generated by another tool or is awkward to express inline.
- **Prefer instead:** Use `doc merge --value` for short, self-contained object literals.

<!-- ref:md-mode:stdin -->
### `md --stdin`

- **What it does:** Reads replacement or inserted markdown content from stdin for the section-editing commands.
- **Use when:** The markdown payload is generated, large, or easier to stream than to quote inline.
- **Prefer instead:** Use `--content` when the inserted text is small and should stay visible in the command.

<!-- ref:tx-mode:plan-stdin -->
### `tx --plan -`

- **What it does:** Reads the transaction plan JSON from stdin instead of a plan file.
- **Use when:** The plan is generated on the fly or piped from another tool.
- **Prefer instead:** Use `--plan <file>` when the plan should be stored, reviewed, or reused.

## `doc` actions

Use these when the top level `doc` command is right, but you need a specific structured operation.

<!-- ref:doc-action:get -->
### `doc get`

- **What it does:** Reads the value at a selector from a JSON, YAML, or TOML file.
- **Use when:** You need one precise value without mutating the document.
- **Prefer instead:** Use `doc flatten` when you are exploring an unfamiliar file and need a broader map of its contents.

<!-- ref:doc-action:has -->
### `doc has`

- **What it does:** Checks whether a selector exists.
- **Use when:** A script or workflow needs a presence check before choosing a later action.
- **Prefer instead:** Use `doc ensure` when the real goal is to create the value if it is missing.

<!-- ref:doc-action:keys -->
### `doc keys`

- **What it does:** Lists the keys of an object at a selector.
- **Use when:** You want to inspect the shape of a structured object before choosing an edit.
- **Prefer instead:** Use `doc get` when you already know the exact key you want.

<!-- ref:doc-action:len -->
### `doc len`

- **What it does:** Counts items in an array or object.
- **Use when:** You need a quick cardinality check in scripts, CI, or exploratory work.
- **Prefer instead:** Use `doc select` or `doc get` when the actual values matter more than the count.

<!-- ref:doc-action:set -->
### `doc set`

- **What it does:** Sets or creates a value at a selector.
- **Use when:** One exact key path should be updated deterministically.
- **Prefer instead:** Use `doc merge` for multi field updates, or `doc ensure` when existing values should be preserved.

<!-- ref:doc-action:delete -->
### `doc delete`

- **What it does:** Removes the value at a selector.
- **Use when:** A key or node is obsolete and should disappear cleanly.
- **Prefer instead:** Use `doc delete-where` when the target is a subset of array items instead of one direct selector.

<!-- ref:doc-action:delete-where -->
### `doc delete-where`

- **What it does:** Deletes array items that match a predicate.
- **Use when:** You need to remove selected objects from a list without rebuilding the whole array by hand.
- **Prefer instead:** Use `doc delete` when one direct selector can remove the target.

<!-- ref:doc-action:merge -->
### `doc merge`

- **What it does:** Deep merges an object payload into an existing document.
- **Use when:** Several related fields should be added or updated together.
- **Prefer instead:** Use `doc set` when one exact path should change and merge semantics are unnecessary.

<!-- ref:doc-action:append -->
### `doc append`

- **What it does:** Appends a value to an array.
- **Use when:** New items should appear at the end of the list.
- **Prefer instead:** Use `doc prepend` when order or precedence means the new item should come first.

<!-- ref:doc-action:prepend -->
### `doc prepend`

- **What it does:** Inserts a value at the front of an array.
- **Use when:** The new item should win by order, or defaults should be introduced at the front of the list.
- **Prefer instead:** Use `doc append` when simple chronological growth is enough.

<!-- ref:doc-action:select -->
### `doc select`

- **What it does:** Reads only the values that match a selector or predicate.
- **Use when:** You need a filtered read view of a larger structure.
- **Prefer instead:** Use `doc update` or `doc delete-where` when the end goal is mutation rather than inspection.

<!-- ref:doc-action:update -->
### `doc update`

- **What it does:** Updates all matching nodes to the same value.
- **Use when:** A broad but uniform change should apply across many selected elements.
- **Prefer instead:** Use `doc set` when the change only targets one path.

<!-- ref:doc-action:move -->
### `doc move`

- **What it does:** Moves or renames a key path.
- **Use when:** Schema cleanup or key migration should preserve the value while changing the path.
- **Prefer instead:** Use `doc set` plus `doc delete` only when the move semantics are not a clean fit.

<!-- ref:doc-action:ensure -->
### `doc ensure`

- **What it does:** Creates a value only if it is currently missing.
- **Use when:** You need idempotent config bootstrapping and must not overwrite existing values.
- **Prefer instead:** Use `doc set` when the desired value should win even if the key already exists.

<!-- ref:doc-action:flatten -->
### `doc flatten`

- **What it does:** Lists leaf paths and their values.
- **Use when:** You are discovering the shape of an unfamiliar structured file.
- **Prefer instead:** Use `doc get` for one targeted read, or `doc keys` when only the object shape matters.

<!-- ref:doc-action:diff -->
### `doc diff`

- **What it does:** Compares two structured files by their semantic content.
- **Use when:** You care about key and value changes more than raw formatting differences.
- **Prefer instead:** Use `patch` or ordinary diff tooling when the exact textual patch matters.

## `md` actions

Use these when markdown structure matters more than raw text matching.

<!-- ref:md-action:replace-section -->
### `md replace-section`

- **What it does:** Replaces the body of a heading section.
- **Use when:** A section should be treated as authoritative content that can be rewritten in one step.
- **Prefer instead:** Use `md insert-after-heading` when existing section content should stay and you only need to add more text.

<!-- ref:md-action:insert-after-heading -->
### `md insert-after-heading`

- **What it does:** Inserts content immediately after a heading.
- **Use when:** You want to add a note, release entry, or status line while preserving the existing section body.
- **Prefer instead:** Use `md replace-section` when the whole section should be regenerated.

<!-- ref:md-action:insert-before-heading -->
### `md insert-before-heading`

- **What it does:** Inserts content immediately before a heading.
- **Use when:** You want to add a preface or a new section boundary before an existing heading.
- **Prefer instead:** Use `md insert-after-heading` when the addition belongs inside the section that starts at the heading.

<!-- ref:md-action:upsert-bullet -->
### `md upsert-bullet`

- **What it does:** Ensures a bullet exists under a heading, without duplicating it.
- **Use when:** Rules, checklists, or recurring notes should be added idempotently.
- **Prefer instead:** Use `md replace-section` when the entire list should be rewritten.

<!-- ref:md-action:dedupe-headings -->
### `md dedupe-headings`

- **What it does:** Removes duplicate headings.
- **Use when:** Generated markdown or hand edited docs have accumulated repeated sections that should collapse to one.
- **Prefer instead:** Use `md lint-agents` when the goal is diagnosis rather than mutation.

<!-- ref:md-action:lint-agents -->
### `md lint-agents`

- **What it does:** Checks AGENTS style markdown for common problems.
- **Use when:** You want a CI style guard for agent instruction files before they drift into invalid or confusing structure.
- **Prefer instead:** Use `md dedupe-headings` when you already know the file should be auto corrected.

<!-- ref:md-action:table-append -->
### `md table-append`

- **What it does:** Appends a row to the markdown table under a heading.
- **Use when:** A docs table should grow without manually rebuilding its existing rows.
- **Prefer instead:** Use `md replace-section` when the whole table should be regenerated from source data.

## `patch` actions

Use these when the change already exists as a unified diff.

<!-- ref:patch-action:check -->
### `patch check`

- **What it does:** Verifies whether a patch applies cleanly, without writing files.
- **Use when:** CI or review should fail early on stale patch context.
- **Prefer instead:** Use `patch apply` when the patch should be written, or `replace` and `doc` when you do not actually need to carry a diff file.

<!-- ref:patch-action:apply -->
### `patch apply`

- **What it does:** Applies a unified diff.
- **Use when:** The desired change is already available as patch text and should be replayed directly.
- **Prefer instead:** Use `replace`, `md`, or `doc` when you would rather describe the desired mutation at a higher level.

## `hygiene` actions

Use these when newline and whitespace correctness is the main concern.

<!-- ref:hygiene-action:check -->
### `hygiene check`

- **What it does:** Reports missing final newlines, mixed line endings, and trailing whitespace.
- **Use when:** You want a non mutating hygiene audit for CI or local review.
- **Prefer instead:** Use `hygiene fix` when the goal is to normalize the files immediately.

<!-- ref:hygiene-action:fix -->
### `hygiene fix`

- **What it does:** Applies newline and whitespace normalization.
- **Use when:** Existing files already need cleanup and the cleanup itself is the task.
- **Prefer instead:** Use write policy flags when normalization should only apply to files already being touched by another write command.

## `tx` reference

`tx` is the place where Patchloom's features compose. Use [Core Concepts](../getting-started/concepts.md) for the canonical explanation of rollback and exit codes, and [examples/README.md](../../examples/README.md) for plan templates.

### Plan fields

<!-- ref:tx-field:cwd -->
### `cwd`

- **What it does:** Sets the base directory used to resolve relative paths inside the plan.
- **Use when:** The plan should behave the same no matter where it is invoked from.
- **Prefer instead:** Use the CLI `--cwd` flag when the directory choice is a caller concern rather than part of the plan itself.

<!-- ref:tx-field:write_policy -->
### `write_policy`

- **What it does:** Applies newline, EOL, and whitespace normalization across all pending writes in the plan.
- **Use when:** Every write in the transaction should share the same normalization policy.
- **Prefer instead:** Use CLI write flags when one invocation needs defaults, but the plan itself should stay generic.

<!-- ref:tx-field:strict -->
### `strict`

- **What it does:** Rolls back file writes when a format or validation step fails.
- **Use when:** Partial writes are unacceptable and post write failure should behave like a full transaction failure.
- **Prefer instead:** Leave strict mode off when writes may stay on disk even if later validation reports a problem.

<!-- ref:tx-field:operations -->
### `operations`

- **What it does:** Lists the ordered mutations that make up the transaction.
- **Use when:** One logical change spans several steps or several mutation types.
- **Prefer instead:** Use a standalone command when one direct operation is enough.

<!-- ref:tx-field:format -->
### `format`

- **What it does:** Runs shell commands after writes are staged to disk but before validation.
- **Use when:** Generated or edited files should be normalized by tools like `cargo fmt`, `prettier`, or `black` as part of the same workflow.
- **Prefer instead:** Run formatting outside `tx` when it does not need to participate in the transaction's success criteria.

<!-- ref:tx-field:validate -->
### `validate`

- **What it does:** Runs shell commands that decide whether the transaction should be reported as valid.
- **Use when:** Build, test, or policy checks are part of the definition of success for the change.
- **Prefer instead:** Use standalone verification outside `tx` when the mutation and the validation lifecycle should stay separate.

### Transaction operations

The operations below are the building blocks inside `operations`.

<!-- ref:tx-op:replace -->
### `replace`

- **What it does:** Runs text replacement inside a transaction.
- **Use when:** A text rewrite needs to share atomic rollback, formatting, or validation with other operations.
- **Related:** top level `replace`

<!-- ref:tx-op:doc.set -->
### `doc.set`

- **What it does:** Runs a targeted structured set inside a transaction.
- **Use when:** A precise config update must be bundled atomically with other repo changes.
- **Related:** top level `doc set`

<!-- ref:tx-op:doc.delete -->
### `doc.delete`

- **What it does:** Removes a structured value inside a transaction.
- **Use when:** Schema cleanup should happen as one step in a larger atomic change.
- **Related:** top level `doc delete`

<!-- ref:tx-op:doc.merge -->
### `doc.merge`

- **What it does:** Deep merges structured content inside a transaction.
- **Use when:** Several related structured fields should change together as part of one plan.
- **Related:** top level `doc merge`

<!-- ref:tx-op:doc.append -->
### `doc.append`

- **What it does:** Appends to an array inside a transaction.
- **Use when:** List growth must stay atomic with other edits in the same plan.
- **Related:** top level `doc append`

<!-- ref:tx-op:doc.prepend -->
### `doc.prepend`

- **What it does:** Prepends to an array inside a transaction.
- **Use when:** Ordered config precedence should change as part of a larger atomic mutation.
- **Related:** top level `doc prepend`

<!-- ref:tx-op:doc.update -->
### `doc.update`

- **What it does:** Updates all matching structured nodes inside a transaction.
- **Use when:** A broad structured rewrite should be coupled to other edits and validations.
- **Related:** top level `doc update`

<!-- ref:tx-op:doc.move -->
### `doc.move`

- **What it does:** Moves or renames a structured key path inside a transaction.
- **Use when:** Schema migration must stay atomic with related code or docs edits.
- **Related:** top level `doc move`

<!-- ref:tx-op:doc.ensure -->
### `doc.ensure`

- **What it does:** Adds a structured value only if it is missing, inside a transaction.
- **Use when:** Idempotent bootstrapping should happen together with other plan steps.
- **Related:** top level `doc ensure`

<!-- ref:tx-op:doc.delete_where -->
### `doc.delete_where`

- **What it does:** Deletes array items matching a predicate inside a transaction.
- **Use when:** Targeted list cleanup must be coordinated with other transactional edits.
- **Related:** top level `doc delete-where`

<!-- ref:tx-op:md.replace_section -->
### `md.replace_section`

- **What it does:** Replaces a markdown section inside a transaction.
- **Use when:** Docs regeneration should be part of a larger all or nothing repo change.
- **Related:** top level `md replace-section`

<!-- ref:tx-op:md.insert_after_heading -->
### `md.insert_after_heading`

- **What it does:** Inserts markdown content after a heading inside a transaction.
- **Use when:** A release note or docs annotation must be added atomically with code or config changes.
- **Related:** top level `md insert-after-heading`

<!-- ref:tx-op:md.insert_before_heading -->
### `md.insert_before_heading`

- **What it does:** Inserts markdown content before a heading inside a transaction.
- **Use when:** Docs structure must change as one step in a broader plan.
- **Related:** top level `md insert-before-heading`

<!-- ref:tx-op:md.upsert_bullet -->
### `md.upsert_bullet`

- **What it does:** Ensures a markdown bullet exists inside a transaction.
- **Use when:** Idempotent docs or checklist updates should stay coupled to other edits.
- **Related:** top level `md upsert-bullet`

<!-- ref:tx-op:md.table_append -->
### `md.table_append`

- **What it does:** Appends a markdown table row inside a transaction.
- **Use when:** Documentation tables should be updated together with the code or metadata they describe.
- **Related:** top level `md table-append`

<!-- ref:tx-op:md.dedupe_headings -->
### `md.dedupe_headings`

- **What it does:** Removes duplicate markdown headings inside a transaction.
- **Use when:** Cleanup of generated docs should stay atomic with the rest of the plan.
- **Related:** top level `md dedupe-headings`

<!-- ref:tx-op:hygiene.fix -->
### `hygiene.fix`

- **What it does:** Applies hygiene normalization inside a transaction.
- **Use when:** Text cleanup should be part of the same atomic success criteria as other edits.
- **Related:** top level `hygiene fix`

<!-- ref:tx-op:file.create -->
### `file.create`

- **What it does:** Creates a file inside a transaction.
- **Use when:** New files must appear only if the full plan succeeds.
- **Related:** top level `create`

<!-- ref:tx-op:file.delete -->
### `file.delete`

- **What it does:** Deletes a file inside a transaction.
- **Use when:** File removal should roll back if later format or validation steps fail.
- **Related:** top level `delete`

<!-- ref:tx-op:patch.apply -->
### `patch.apply`

- **What it does:** Applies a unified diff inside a transaction.
- **Use when:** Patch replay needs to compose with earlier in plan edits and share the same rollback or validation behavior.
- **Related:** top level `patch apply`
