# Morph Fast Apply job matrix vs Patchloom

> **Date:** 2026-07-27
> **Method:** Morph public use cases (docs, Fly blog, HN, OpenCode plugin table) mapped to patchloom CLI/API and re-run as local scenarios against `target/release/patchloom`.
> **Goal:** Cover Morph *jobs* with deterministic tools. Not a Morph clone (no cloud apply model, no `// ... existing code ...` semantic merge without anchors).

## Verdict summary

| Result | Count | Meaning |
|--------|------:|---------|
| **PASS** | 11 | Patchloom already handles this Morph job |
| **PARTIAL** | 2 | Works with flags/host policy; not Morph-default UX |
| **GAP** | 2 | Real product gap if you want Morph users to never need Morph |
| **N/A** | 2 | Morph product metrics (tok/s, network API); not our target |

**Bottom line:** Most Morph reliability jobs (small edit, large file with a matchable span, multi-file, preview, undo, configs, peels) are already covered. Morph still wins on **lazy snippet UX** (no exact `old`) and **semantic freeform placement**. Do not train an apply model; close gaps with AST/multi-hunk routing and optional constrained anchor apply (see open issues).

## Sources (Morph side)

| Source | Jobs extracted |
|--------|----------------|
| [morphllm.com/use-cases](https://www.morphllm.com/use-cases) | Multi-file builder, document structure, batch apply, agent pipeline, editor preview/revert |
| [Fly MorphLLM blog](https://fly.io/blog/build-better-agents-with-morphllm/) | Lazy `//...existing code...` snippets; avoid full rewrite and brittle search-replace |
| [HN Morph launch](https://news.ycombinator.com/item?id=44490863) | AI cannot reliably insert into existing code |
| [OpenCode Morph plugin](https://github.com/JRedeker/opencode-morph-fast-apply) | When to use Morph: large file, scattered edits, whitespace-sensitive |
| Morph MCP docs | Prefer `edit_file` over `str_replace` / full write |

## Matrix (Morph job to Patchloom path)

Legend: **PASS** / **PARTIAL** / **GAP** / **N/A**. Verified 2026-07-27 unless noted.

| ID | Morph job (user want) | Patchloom path | Result | Evidence / notes |
|----|----------------------|----------------|--------|------------------|
| M1 | Small exact code change without full rewrite | `replace OLD PATH --new NEW --apply` | **PASS** | Exact match, `applied: true`, backup session |
| M2 | Whitespace-sensitive / near-miss span | `replace ... --fuzzy` (+ optional `--allow-absent-old`) | **PARTIAL** | Default **fail-closed**: exact absent returns `no_matches` + best `matched_text` (score ~0.97), no write. With `--allow-absent-old` write succeeds; inspect `matched_text` / use `fuzzy_span_suspicious` on hosts. Morph is forgiving by default; we refuse-by-default |
| M3 | Large file, change one site without rewriting file | `replace` on unique span; or `ast replace PATH SYMBOL --old --new` | **PASS** | ~160-line fixture: exact replace PASS; `ast replace ... target` PASS |
| M4 | Multiple identical occurrences in one file | `replace` (all) or `--nth` | **PASS** | Three `const ... = 1` all updated |
| M5 | Multi-file consistency in one step | `batch` / `tx` / `execute_plan` | **PASS** | Two-file batch version bump PASS |
| M6 | Config/YAML without breaking comments | `doc set` | **PASS** | Comment `# keep me` preserved; port updated |
| M7 | Markdown section without full rewrite | `md replace-section` (and siblings) | **PASS** | Commands section replaced; Other kept |
| M8 | Preview before write | Default dry-run (no `--apply`) | **PASS** | Exit **2**, file unchanged |
| M9 | Lazy snippet merge (`// ... existing code ...`, no exact `old`) | None first-class | **GAP** | Expected: no Morph-style apply model. Host must supply matchable `old`, AST symbol, or insert anchors |
| M10 | Place new code after a known anchor | `replace ANCHOR --insert-after TEXT` (or `--insert-before`) | **PASS** | Insert after `a();` PASS |
| M11 | Scattered multi-hunk different sites | Sequential `replace`, `batch` same file, or library `apply_content_edits*` | **PASS** | Batch two different olds same file PASS. Not one Morph lazy multi-hunk blob |
| M12 | Identifier rename across file | `ast rename` | **PASS** | `compute` to `calculate` including call site |
| M13 | Revert / trust after apply | `undo --list` / `undo --apply` (+ library backup APIs) | **PASS** | Restore most recent session restored content |
| M14 | Offline / no API key | Local binary + crate | **PASS** | No network required for apply path |
| M15 | Clear failure kinds for hosts | `--json` `error_kind` (e.g. `binary`) | **PASS** | Sole binary target returns `error_kind: binary` |
| M16 | Semantic freeform "put this method in the right place" with no anchors | Morph model only | **GAP** | Closest: `ast` list/read + insert/replace with user-chosen symbol/anchor. No structure-aware freeform merge |
| M17 | 10k+ tok/s cloud apply latency brand | N/A | **N/A** | Local apply is a different success metric (correctness + peels, not model tok/s) |
| M18 | Host prompt: prefer specialized edit tool over str_replace/full write | `agent-rules` + comparisons + MCP descriptions | **PARTIAL** | Decision tree and Morph section exist; this matrix is the migration checklist. Keep routing sharp in agent-rules |

## OpenCode plugin "when to use Morph" to Patchloom

From [opencode-morph-fast-apply](https://github.com/JRedeker/opencode-morph-fast-apply) decision table:

| Situation | Their tool | Patchloom equivalent | Result |
|-----------|------------|----------------------|--------|
| Small, exact replacement | native `edit` | `replace` exact | **PASS** (prefer us) |
| Large file (500+ lines) | `morph_edit` | Exact/`ast replace` if span or symbol known | **PASS** if agent names span/symbol; **GAP** if only lazy snippet |
| Multiple scattered changes | `morph_edit` | `batch` / `apply_content_edits` / multi `replace` | **PASS** |
| Whitespace-sensitive | `morph_edit` | `--fuzzy` + span policy | **PARTIAL** (fail-closed default) |

## Morph marketing use cases to Patchloom

| Morph use case page | Patchloom | Result |
|---------------------|-----------|--------|
| AI application builder (multi-file, imports/types risk) | `batch`/`tx` + `ast` for code + peels | **PASS** for apply safety; types still need compile/CI |
| Document editor agents (structure) | `md` / `doc` (not DOCX/TipTap product) | **PASS** for md/json/yaml/toml; **N/A** for DOCX/TipTap |
| Batch apply + unified preview | `batch`/`tx` dry-run then `--apply` | **PASS** |
| Agent pipeline + CI validate | CLI/MCP + exit codes + peels | **PASS** |
| Editor embed preview/revert | dry-run + `undo` + library | **PASS** |

## What we will not build (non-goals)

1. Cloud Fast Apply model / paid API dependency for core path.
2. Accepting Morph-format lazy snippets with **no** anchors and guessing placement.
3. Competing on tok/s apply-model benchmarks.

## Follow-up issues

| Issue | Topic |
|-------|--------|
| [#2018](https://github.com/patchloom/patchloom/issues/2018) | Constrained freeform code apply (M9/M16); no Morph clone |
| [#2019](https://github.com/patchloom/patchloom/issues/2019) | Keep matrix / comparisons / agent-rules Morph routing in sync |

## How to re-run smoke scenarios

```bash
# From a temp dir; BIN=path/to/patchloom
BIN=target/release/patchloom
$BIN replace "hello" small.rs --new "world" --apply --json
$BIN replace 'println!("hello")' ws.rs --new 'println!("world")' --fuzzy --json  # fail-closed
$BIN replace 'println!("hello")' ws.rs --new 'println!("world")' --fuzzy --allow-absent-old --apply --json
$BIN ast replace large.rs target --old 'println!("old")' --new 'println!("new")' --apply --json
$BIN batch --apply <<'EOF'
replace a.txt version=1 version=2
replace b.txt version=1 version=2
EOF
$BIN doc set cfg.yaml database.port 9999 --apply
$BIN undo --list
$BIN undo --apply
```

## Related docs

- [Comparisons: vs Morph](../getting-started/comparisons.md)
- [Embedder host checklist](../getting-started/embedder-host.md)
- Competitor research: `~/market-research/cli-tool-patchloom/COMPETITOR-ANALYSIS.md`
