# Changelog

All notable changes to Patchloom are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.3.1](https://github.com/patchloom/patchloom/compare/patchloom-v0.3.0...patchloom-v0.3.1) (2026-06-21)


### Bug Fixes

* gate AstRename/AstReplace match arm behind cfg(feature = "ast") ([#680](https://github.com/patchloom/patchloom/issues/680)) ([ceea1f9](https://github.com/patchloom/patchloom/commit/ceea1f9e6669713d9697699d8dbd001296c242b9)), closes [#679](https://github.com/patchloom/patchloom/issues/679)
* MCP validation parity, config boundary, empty file, cross-file md_move ([#712](https://github.com/patchloom/patchloom/issues/712)) ([9c37f01](https://github.com/patchloom/patchloom/commit/9c37f0180f578ab2ef809c3fa556446269ca82bf))
* rename misleading concurrent MCP test and clean cosmetic GlobalFlags shadows ([#721](https://github.com/patchloom/patchloom/issues/721)) ([3a6918e](https://github.com/patchloom/patchloom/commit/3a6918e7c8e6fc25917503d9838a0d2d736b6cce))
* resolve tech-debt [#691](https://github.com/patchloom/patchloom/issues/691), [#694](https://github.com/patchloom/patchloom/issues/694), [#708](https://github.com/patchloom/patchloom/issues/708) (clap optional, AST tests, fuzz) ([#714](https://github.com/patchloom/patchloom/issues/714)) ([e116430](https://github.com/patchloom/patchloom/commit/e1164304334e50e0971bce8a085229559d04ebbc))
* text and path matching bugs ([#685](https://github.com/patchloom/patchloom/issues/685), [#700](https://github.com/patchloom/patchloom/issues/700), [#701](https://github.com/patchloom/patchloom/issues/701)) ([#710](https://github.com/patchloom/patchloom/issues/710)) ([2f587d0](https://github.com/patchloom/patchloom/commit/2f587d0dbf94f184469675e98d134161cc6aeea9))
* transaction engine atomicity and rename-after-delete ([#696](https://github.com/patchloom/patchloom/issues/696), [#697](https://github.com/patchloom/patchloom/issues/697), [#698](https://github.com/patchloom/patchloom/issues/698)) ([#711](https://github.com/patchloom/patchloom/issues/711)) ([fe51c17](https://github.com/patchloom/patchloom/commit/fe51c17dd2200b9a245e97d71e5027b6ccb1bfe8))
* unique OPERATION_FAILED exit code, timestamp-based pruning, AST improvements ([#713](https://github.com/patchloom/patchloom/issues/713)) ([572fb08](https://github.com/patchloom/patchloom/commit/572fb0886465d289bca6987f1fb3fdd4cce83d80))

## [0.3.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.2.0...patchloom-v0.3.0) (2026-06-21)


### Features

* add --word-boundary flag to prevent partial-word matches ([#657](https://github.com/patchloom/patchloom/issues/657)) ([91893e2](https://github.com/patchloom/patchloom/commit/91893e2cafc1f615904665e83cf7dabd12d6e8ba))
* add AST-aware operations using tree-sitter (20 languages) ([#658](https://github.com/patchloom/patchloom/issues/658)) ([17223e5](https://github.com/patchloom/patchloom/commit/17223e515a9e625999bc5dbff454b2ea2ec2b0df)), closes [#647](https://github.com/patchloom/patchloom/issues/647)
* add file.append operation and --format flag on write commands ([#667](https://github.com/patchloom/patchloom/issues/667)) ([9a57f06](https://github.com/patchloom/patchloom/commit/9a57f06d58ac979701a3bfda9b3a8c7c514cf814)), closes [#661](https://github.com/patchloom/patchloom/issues/661) [#662](https://github.com/patchloom/patchloom/issues/662)
* **ast:** add map, replace, impact, and diff subcommands ([#650](https://github.com/patchloom/patchloom/issues/650), [#653](https://github.com/patchloom/patchloom/issues/653), [#654](https://github.com/patchloom/patchloom/issues/654), [#655](https://github.com/patchloom/patchloom/issues/655)) ([#660](https://github.com/patchloom/patchloom/issues/660)) ([d4eef5a](https://github.com/patchloom/patchloom/commit/d4eef5a17db9891cabdfbd265cbabdfa2534c588))
* **ast:** add search, refs, and deps subcommands ([#649](https://github.com/patchloom/patchloom/issues/649), [#651](https://github.com/patchloom/patchloom/issues/651), [#652](https://github.com/patchloom/patchloom/issues/652)) ([#659](https://github.com/patchloom/patchloom/issues/659)) ([72eccb8](https://github.com/patchloom/patchloom/commit/72eccb83405e54ce92676c4be485ad1e0779c8b6))


### Bug Fixes

* add AST and word_boundary to all operation surfaces ([#666](https://github.com/patchloom/patchloom/issues/666)) ([ff85bb7](https://github.com/patchloom/patchloom/commit/ff85bb756654dd2504b78242685c8b9b7658905f)), closes [#663](https://github.com/patchloom/patchloom/issues/663) [#664](https://github.com/patchloom/patchloom/issues/664) [#665](https://github.com/patchloom/patchloom/issues/665)
* **ci:** remove dependabot[bot] from auto-approve actor list ([#648](https://github.com/patchloom/patchloom/issues/648)) ([677982d](https://github.com/patchloom/patchloom/commit/677982dae876dd9c3f1b2682af38d1657027381a))
* improvement cycle 19 - PTY flush, MCP tests, doc updates ([#670](https://github.com/patchloom/patchloom/issues/670)) ([af289e9](https://github.com/patchloom/patchloom/commit/af289e9c76a9d119b1c12876d76400a99339d7b5))
* move VS Code badge to distribution row to prevent 4-row wrap ([#643](https://github.com/patchloom/patchloom/issues/643)) ([3461e2d](https://github.com/patchloom/patchloom/commit/3461e2d67e0c4d3ab65a98c12fdac2c7a2156a56))
* re-expose fallback and AST internals as public API for library consumers ([#677](https://github.com/patchloom/patchloom/issues/677)) ([c25ce76](https://github.com/patchloom/patchloom/commit/c25ce766b4d5605d2284136a964be27bb07cbc08))
* rebalance badge rows to 5-5-3 to prevent row 2 wrap ([#644](https://github.com/patchloom/patchloom/issues/644)) ([bdc5da0](https://github.com/patchloom/patchloom/commit/bdc5da03aaedc429dd7a257a210c68f16b0cabb4))
* remove bump-patch-for-minor-pre-major to align with semver-checks ([#672](https://github.com/patchloom/patchloom/issues/672)) ([f930c9b](https://github.com/patchloom/patchloom/commit/f930c9bc90d53a9ba466ebedf81bd9546c190305))
* replace retired VS Code Marketplace badge with gist endpoint ([#641](https://github.com/patchloom/patchloom/issues/641)) ([2b30ab6](https://github.com/patchloom/patchloom/commit/2b30ab6195377510f59c1bc6f4676218d27606b9))
* run --format command in --confirm paths for all write commands ([#668](https://github.com/patchloom/patchloom/issues/668)) ([c884f75](https://github.com/patchloom/patchloom/commit/c884f753e29dfd3686ab4c394869ecdfab1bc4ee))

## [0.2.0](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.7...patchloom-v0.2.0) (2026-06-18)


### ⚠ BREAKING CHANGES

* All public structs and enums in the API surface are now marked #[non_exhaustive]. External code that constructs these types via struct literals must use ..Default::default() or equivalent patterns. Serde deserialization (the primary construction path) is unaffected.

### Features

* add cargo-semver-checks CI and fix all rustdoc warnings ([#615](https://github.com/patchloom/patchloom/issues/615)) ([250dcb5](https://github.com/patchloom/patchloom/commit/250dcb52191b22ca3beda879715ac7000dd306a7)), closes [#612](https://github.com/patchloom/patchloom/issues/612) [#613](https://github.com/patchloom/patchloom/issues/613)
* extract path containment into public module ([#609](https://github.com/patchloom/patchloom/issues/609)) ([d3d9ae2](https://github.com/patchloom/patchloom/commit/d3d9ae26e2b07c9867a3ac38461c637d84e6bd44))
* harden tx rollback and add three-way patch merge ([#587](https://github.com/patchloom/patchloom/issues/587)) ([db21982](https://github.com/patchloom/patchloom/commit/db2198222aa159d4b8b4874e14c4dbd399569909))
* make files module public and extract exec module ([#610](https://github.com/patchloom/patchloom/issues/610)) ([746701a](https://github.com/patchloom/patchloom/commit/746701a9030ac6dc81ce78ef1594e33bcfb8fe6f))
* mark all public types as #[non_exhaustive] for semver safety ([#624](https://github.com/patchloom/patchloom/issues/624)) ([b3592e2](https://github.com/patchloom/patchloom/commit/b3592e20ead71c62f6bc302bee432182447f0fed))
* re-export EolMode from write module ([#611](https://github.com/patchloom/patchloom/issues/611)) ([fc604d9](https://github.com/patchloom/patchloom/commit/fc604d9b60963e73d1cef461c1cb1899648b0564))
* support RELEASE_NOTES.md override for curated release descriptions ([#627](https://github.com/patchloom/patchloom/issues/627)) ([f0f92be](https://github.com/patchloom/patchloom/commit/f0f92be348c2371ad625770cd260092a077c12b8))


### Bug Fixes

* remove dead test code in containment path guard ([#628](https://github.com/patchloom/patchloom/issues/628)) ([5e5b9e5](https://github.com/patchloom/patchloom/commit/5e5b9e5fa459dc60bcc124565471dd9debd4afb2))
* resolve tech-debt issues [#620](https://github.com/patchloom/patchloom/issues/620)-[#623](https://github.com/patchloom/patchloom/issues/623) ([#625](https://github.com/patchloom/patchloom/issues/625)) ([b031686](https://github.com/patchloom/patchloom/commit/b03168619872d77cf5963892aac728c725b93768)), closes [#621](https://github.com/patchloom/patchloom/issues/621) [#622](https://github.com/patchloom/patchloom/issues/622)
* **schema:** add op field to md.move_section examples ([#600](https://github.com/patchloom/patchloom/issues/600)) ([9a816fe](https://github.com/patchloom/patchloom/commit/9a816fe955207ecfe322b9eb96232f474dee8d35))
* use platform-appropriate absolute paths in containment tests ([#616](https://github.com/patchloom/patchloom/issues/616)) ([6a0bb6a](https://github.com/patchloom/patchloom/commit/6a0bb6ae7904de09ac2a3e64042c5c6495c20270))
* use thread-local FORCE_RESTORE_FAIL for parallel tests ([#594](https://github.com/patchloom/patchloom/issues/594)) ([57b6f9b](https://github.com/patchloom/patchloom/commit/57b6f9bd00f123b8308ba2f81c90ee2a8ec33930))
* warn on invalid config values and clarify batch quoting ([#585](https://github.com/patchloom/patchloom/issues/585)) ([7803291](https://github.com/patchloom/patchloom/commit/7803291f7523c2d1dc684b73cd4148a3d6c74286))

## [Unreleased]

### Changed

- **tx** - **Breaking:** `strict` now defaults to `true` in plans, MCP write tools, and `batch`. Use `"strict": false`, `[tx] strict = false` in `.patchloom.toml`, or `patchloom tx --no-strict` to opt out. Operation staging failures now exit 4 with `error_kind: operation_failed` (was exit 7). Mid-commit write failures roll back via the backup session (exit 7 `rollback`, or exit 1 `rollback_failed` if rollback is incomplete).

## [0.1.7](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.6...patchloom-v0.1.7) (2026-06-16)


### Features

* add --whole-line, --range, and --collapse-blanks to replace ([#564](https://github.com/patchloom/patchloom/issues/564)) ([5651320](https://github.com/patchloom/patchloom/commit/56513207a0c7f7c18b3745825fc369eb04cc1271)), closes [#563](https://github.com/patchloom/patchloom/issues/563)
* close [#573](https://github.com/patchloom/patchloom/issues/573) and [#574](https://github.com/patchloom/patchloom/issues/574) - complete API parity and edge case tests ([#576](https://github.com/patchloom/patchloom/issues/576)) ([d6fc1a9](https://github.com/patchloom/patchloom/commit/d6fc1a99fb1f36045bd309a4707d8f4a84919bb5))
* md.move-section -- move a heading section between files ([#554](https://github.com/patchloom/patchloom/issues/554)) ([d6f42e7](https://github.com/patchloom/patchloom/commit/d6f42e7e97db115d3506ab8295c4e261aee2f67e)), closes [#553](https://github.com/patchloom/patchloom/issues/553)


### Bug Fixes

* improvement cycle 11 — config, schema, MCP tests, docs ([#568](https://github.com/patchloom/patchloom/issues/568)) ([ea4967b](https://github.com/patchloom/patchloom/commit/ea4967bc53f0d123fbdb6c9336a53f66638ab3be))
* improvement cycle 11b - docs, CI hardening ([#569](https://github.com/patchloom/patchloom/issues/569)) ([5041287](https://github.com/patchloom/patchloom/commit/5041287207d695f45e82200b063b39ae3e6f4159))
* improvement cycle 12 - Windows CI, fuzz CI matrix ([#572](https://github.com/patchloom/patchloom/issues/572)) ([c24792f](https://github.com/patchloom/patchloom/commit/c24792fe51a540c6afb2e8f66cf2f54648b561fe))
* improvement cycle 13 - tests, inline refactor, error context ([#575](https://github.com/patchloom/patchloom/issues/575)) ([6208177](https://github.com/patchloom/patchloom/commit/6208177ad64228b4278310f39a4f23ccab50068b))
* improvement cycle 14 - strengthen weak test assertions ([#577](https://github.com/patchloom/patchloom/issues/577)) ([2ba2396](https://github.com/patchloom/patchloom/commit/2ba2396ea310c7ccf78913dcfe1e82ca5610e311))
* make unit tests portable in Docker and pseudo-TTY environments ([#579](https://github.com/patchloom/patchloom/issues/579)) ([591b4d8](https://github.com/patchloom/patchloom/commit/591b4d83db426ff7cea6c69926698e5bd3182d15))
* md move-section same-file path detection and cross-file --check mode ([#556](https://github.com/patchloom/patchloom/issues/556)) ([da76cc5](https://github.com/patchloom/patchloom/commit/da76cc5cb0ce1ecfee8027ba7b7d1c3d6a577bdf))
* rename same-file detection via path canonicalization ([#557](https://github.com/patchloom/patchloom/issues/557)) ([a1b5573](https://github.com/patchloom/patchloom/commit/a1b5573a573744ebcd5806beae187e8e232ec5aa))
* replace broken shields.io badges with gist endpoints ([#578](https://github.com/patchloom/patchloom/issues/578)) ([23b14f3](https://github.com/patchloom/patchloom/commit/23b14f389a12c8d044cc79cb29ff6eb1b751f3de))
* update MCP bench to use individual tool calls ([#570](https://github.com/patchloom/patchloom/issues/570)) ([655a1d2](https://github.com/patchloom/patchloom/commit/655a1d24b7d9e89c73d9f91a852957a2a8327681))

## [0.1.6](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.5...patchloom-v0.1.6) (2026-06-08)


### Features

* public Rust library API with thread safety, intent format, and fallback chain ([#530](https://github.com/patchloom/patchloom/issues/530)) ([093eb8b](https://github.com/patchloom/patchloom/commit/093eb8bc0abf4d567027fd9a726934943823e1e2))


### Bug Fixes

* add error context to backup restore and rename cross-device paths ([#543](https://github.com/patchloom/patchloom/issues/543)) ([69018e7](https://github.com/patchloom/patchloom/commit/69018e784e9a5594b70000275167d15d67a1a0a0))
* **ci:** use App token in update-branches to trigger CI on updated PRs ([#523](https://github.com/patchloom/patchloom/issues/523)) ([e51cdae](https://github.com/patchloom/patchloom/commit/e51cdae6ac200ac443ec1bc923b3c9c27c02a3e3))
* correct pinned action SHAs in docs workflow ([#549](https://github.com/patchloom/patchloom/issues/549)) ([b1fabf6](https://github.com/patchloom/patchloom/commit/b1fabf6895ec73560d7d380c6bc6a5f82469741c))
* improvement cycle (UTF-8 truncate, doc_set double-parse, docs freshness) ([#531](https://github.com/patchloom/patchloom/issues/531)) ([a8dffb9](https://github.com/patchloom/patchloom/commit/a8dffb9c8a5c1588dfa7b9a0f6d003772e41b6d4))
* md silent default mode, search empty-pattern guard, strengthen assertions ([#542](https://github.com/patchloom/patchloom/issues/542)) ([45d3239](https://github.com/patchloom/patchloom/commit/45d323976bdc19e4bb9d37f23ba60566f0dc43a9))
* md/doc --check produce stdout output and doc --json errors use structured JSON ([#546](https://github.com/patchloom/patchloom/issues/546)) ([819fb7c](https://github.com/patchloom/patchloom/commit/819fb7c1a2190e74445672a1dbb3c77f09496e9a)), closes [#544](https://github.com/patchloom/patchloom/issues/544) [#545](https://github.com/patchloom/patchloom/issues/545)
* propagate read errors in file_create and extract inline conditional ([#533](https://github.com/patchloom/patchloom/issues/533)) ([26ab09c](https://github.com/patchloom/patchloom/commit/26ab09cca8c5a3229a4de6350137aded69e4ec1a))
* propagate YAML serialization error and remove unnecessary borrows in ops.rs ([#537](https://github.com/patchloom/patchloom/issues/537)) ([24e67f4](https://github.com/patchloom/patchloom/commit/24e67f40755606863add7d83468a28583a42f7d5))
* remove documentation field so crates.io auto-links to docs.rs ([#547](https://github.com/patchloom/patchloom/issues/547)) ([f6bbd10](https://github.com/patchloom/patchloom/commit/f6bbd10d30d60c6964d68a8d45d2c72ed14aaa1a))

## [0.1.5](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.4...patchloom-v0.1.5) (2026-06-07)


### Bug Fixes

* improvement cycle 6 (doc_query validation, troubleshooting docs) ([#520](https://github.com/patchloom/patchloom/issues/520)) ([93d3fdf](https://github.com/patchloom/patchloom/commit/93d3fdf77957d0fa14dc9f358c39a402a2f0af6c))

## [0.1.4](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.3...patchloom-v0.1.4) (2026-06-07)


### Bug Fixes

* auto-sync PATCHLOOM.md on release-please version bumps ([#513](https://github.com/patchloom/patchloom/issues/513)) ([cb6cb1c](https://github.com/patchloom/patchloom/commit/cb6cb1c3dca42974c2230485d3a712dd3ac05b75)), closes [#512](https://github.com/patchloom/patchloom/issues/512)
* parse release-please pr output as JSON ([#515](https://github.com/patchloom/patchloom/issues/515)) ([3215fcd](https://github.com/patchloom/patchloom/commit/3215fcdf2137ccf6a2243b7a8373d58a0f0ad94b))

## [0.1.3](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.2...patchloom-v0.1.3) (2026-06-07)


### Bug Fixes

* add wasi crate to FOSSA false positive filter ([#510](https://github.com/patchloom/patchloom/issues/510)) ([1882060](https://github.com/patchloom/patchloom/commit/18820609dcd0fea3062e70a7e173f10836682464))
* improvement cycle 5 (tx.rs refactoring, error path tests) ([#508](https://github.com/patchloom/patchloom/issues/508)) ([680b18b](https://github.com/patchloom/patchloom/commit/680b18bbd78f00a1eccaff7026b5292a178ebea9))
* make release host job idempotent for release-please ([#511](https://github.com/patchloom/patchloom/issues/511)) ([2b6ae3b](https://github.com/patchloom/patchloom/commit/2b6ae3b2507282d2257906bc5c35a542ceb2e4dc))

## [0.1.2](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.1...patchloom-v0.1.2) (2026-06-07)


### Features

* enable MCP feature by default ([#502](https://github.com/patchloom/patchloom/issues/502)) ([7eb8750](https://github.com/patchloom/patchloom/commit/7eb87507e5b686a1d80391e50a97ce50abdd51a0))


### Bug Fixes

* add benchmark result directories to .gitignore ([#500](https://github.com/patchloom/patchloom/issues/500)) ([9bdd383](https://github.com/patchloom/patchloom/commit/9bdd38319f7fe2713e4b22eea24fa99244a9d392))
* improvement cycle 4 (MCP tests, doc dedup, error messages) ([#507](https://github.com/patchloom/patchloom/issues/507)) ([764c355](https://github.com/patchloom/patchloom/commit/764c3554c219ee7d5ca9c9098c73b0621ff90ad9))

## [0.1.1](https://github.com/patchloom/patchloom/compare/patchloom-v0.1.0...patchloom-v0.1.1) (2026-06-06)


### Features

* add --confirm flag for interactive preview-then-apply workflow ([c7c0796](https://github.com/patchloom/patchloom/commit/c7c07966fb2331e1ec1bffcf5469341af4fde040)), closes [#354](https://github.com/patchloom/patchloom/issues/354)
* add 7 missing batch operations and 2 MCP tools ([20c6ce5](https://github.com/patchloom/patchloom/commit/20c6ce542753a00f468572fc05fc4030dcec39a7)), closes [#219](https://github.com/patchloom/patchloom/issues/219) [#223](https://github.com/patchloom/patchloom/issues/223)
* add batch command for line-oriented multi-file edits ([0c446b9](https://github.com/patchloom/patchloom/commit/0c446b94b15a3222620dbfa508dcb60d491ba4df))
* add Claude Code and Aider agent drivers ([75a33c3](https://github.com/patchloom/patchloom/commit/75a33c3f0c6c990ac58d6c1e0111fba001ce2424))
* add Codex CLI and Cline agent drivers ([97281d7](https://github.com/patchloom/patchloom/commit/97281d72100a69c84ff44ce3b591633434a68683))
* add inline examples to MCP tool descriptions and simplify bench prompts ([#479](https://github.com/patchloom/patchloom/issues/479)) ([fbd8336](https://github.com/patchloom/patchloom/commit/fbd83363b6a990b13034fa0929a545470693ae75))
* add MCP benchmark suite (make bench-mcp) ([#470](https://github.com/patchloom/patchloom/issues/470)) ([dc81fdd](https://github.com/patchloom/patchloom/commit/dc81fdd2d32f91e93abfa602d3a9d7bcf7206c0a))
* add MCP server for structured tool calls ([d039043](https://github.com/patchloom/patchloom/commit/d0390431dfc8e0debf5a99c2347ee6b88244aa6a))
* add patchloom explain command for human-readable plan descriptions ([f1a0056](https://github.com/patchloom/patchloom/commit/f1a00568cd19c90f7c800dbf627799a865d59b6d)), closes [#356](https://github.com/patchloom/patchloom/issues/356)
* add usage examples to --help output for all commands ([85bc1a6](https://github.com/patchloom/patchloom/commit/85bc1a6b655d76061c1b7e5750476a5c47a6ab33)), closes [#352](https://github.com/patchloom/patchloom/issues/352)
* add usage examples to MCP agent-rules output ([#471](https://github.com/patchloom/patchloom/issues/471)) ([e6eb618](https://github.com/patchloom/patchloom/commit/e6eb618a3b2b27837da3abe481344dec2a30d706))
* **agent-rules:** add --mode and --platform flags, fix Windows quoting ([4743892](https://github.com/patchloom/patchloom/commit/47438921dc936b9a6d0d310226ff439357f79828)), closes [#256](https://github.com/patchloom/patchloom/issues/256) [#257](https://github.com/patchloom/patchloom/issues/257) [#258](https://github.com/patchloom/patchloom/issues/258)
* auto-install shell completions in patchloom init ([019e793](https://github.com/patchloom/patchloom/commit/019e7938ee64ac47c4fc31464cc95e69dd592bc6)), closes [#353](https://github.com/patchloom/patchloom/issues/353)
* benchmark reproducibility (README, dry-run, report, CI) ([fa96a98](https://github.com/patchloom/patchloom/commit/fa96a982973d1941b29a023325020077656a7f88)), closes [#346](https://github.com/patchloom/patchloom/issues/346)
* comment preservation for sequence-rooted YAML files ([b5ca605](https://github.com/patchloom/patchloom/commit/b5ca6058a35879df6903578054aedd78b642f78e)), closes [#208](https://github.com/patchloom/patchloom/issues/208)
* diff summary line after preview output ([3952718](https://github.com/patchloom/patchloom/commit/3952718285deb778fd7b3e71f4275d596d214822)), closes [#359](https://github.com/patchloom/patchloom/issues/359)
* MCP benchmark 11/11 via anti-CLI instructions and diagnostic logging ([#478](https://github.com/patchloom/patchloom/issues/478)) ([d2e776d](https://github.com/patchloom/patchloom/commit/d2e776d4d2abe4b42fb60ec464bc91976db1ac59))
* **mcp:** add batch_replace and batch_tidy homogeneous batch tools ([#486](https://github.com/patchloom/patchloom/issues/486)) ([73981b4](https://github.com/patchloom/patchloom/commit/73981b4a588766be1399f258c46692a46553e6bd))
* op name aliases, consolidate doc_query, dynamic bench timeout ([#480](https://github.com/patchloom/patchloom/issues/480)) ([ffd0e3c](https://github.com/patchloom/patchloom/commit/ffd0e3c6fbf113b8ac712f32a4e3a2b0ac7a34cd))
* preserve TOML comments and formatting during doc operations ([3b245e3](https://github.com/patchloom/patchloom/commit/3b245e30960224fab5d8c6991268477e540daa5b)), closes [#202](https://github.com/patchloom/patchloom/issues/202)
* project config file (.patchloom.toml) for per-project defaults ([a02f71f](https://github.com/patchloom/patchloom/commit/a02f71fb1b051065803a127876d3f35098ab11ff)), closes [#355](https://github.com/patchloom/patchloom/issues/355)
* smart error recovery hints for no-match results ([fc3e7f3](https://github.com/patchloom/patchloom/commit/fc3e7f3510fd89c41c05708312621ac483a805f4)), closes [#357](https://github.com/patchloom/patchloom/issues/357)
* strengthen MCP agent-rules with tool selection guide ([#472](https://github.com/patchloom/patchloom/issues/472)) ([813d30f](https://github.com/patchloom/patchloom/commit/813d30f2862cc89371be795abe2d44eb9658a917))
* structured JSON APIs for batch and transaction MCP tools ([#473](https://github.com/patchloom/patchloom/issues/473)) ([84bed9f](https://github.com/patchloom/patchloom/commit/84bed9f89aa55c6e75c5c6143a4a9570e87b827b))
* tx search directory support, MCP lint-agents tool, example 08 smoke test ([6cf582b](https://github.com/patchloom/patchloom/commit/6cf582bf0fba079ecbf14b0a640d6110b8b6f32e))
* undo safety net with backup sessions ([4119e9a](https://github.com/patchloom/patchloom/commit/4119e9a02da3789aaf23f65db990b3836e98fd6a)), closes [#358](https://github.com/patchloom/patchloom/issues/358)


### Bug Fixes

* 4 Windows integration test failures ([3035e2a](https://github.com/patchloom/patchloom/commit/3035e2ae2e271c5d60a79faf3ba26f1df8feb24d))
* add backup support to delete and rename commands ([cc8e8c2](https://github.com/patchloom/patchloom/commit/cc8e8c29827d86c37d0275b87cab685c457724cc))
* add command prefix to md and read error messages ([6a5fd88](https://github.com/patchloom/patchloom/commit/6a5fd8869e6bf5e44866486c4c61db1bb072c368))
* add missing 'rename' to subcommand set, deduplicate driver helpers ([f0f7a37](https://github.com/patchloom/patchloom/commit/f0f7a37ca32ac87a95d6ab1bf7de3a30db34933f))
* add missing subcommands to agent driver subcommand set ([#444](https://github.com/patchloom/patchloom/issues/444)) ([4804b2d](https://github.com/patchloom/patchloom/commit/4804b2dbe1c76dd25eb8a55f52bead2927be855d))
* add test coverage and update install instructions for v0.1.0 ([#465](https://github.com/patchloom/patchloom/issues/465)) ([c42c7c1](https://github.com/patchloom/patchloom/commit/c42c7c1c2b726e95d64478f0c6656c79a5f46f18))
* add tilde fence support to lint-agents code block detection ([4ad6256](https://github.com/patchloom/patchloom/commit/4ad62563c31a094f9b80c1e656d8e6018292674f))
* address AI code quality findings in GrokDriver ([#453](https://github.com/patchloom/patchloom/issues/453)) ([1ecf4dc](https://github.com/patchloom/patchloom/commit/1ecf4dc79b45a1dbaa70e99df4c83bb3c0f8e1e5))
* agent bench file_ops collision and use focused agent-rules modes ([f86e02b](https://github.com/patchloom/patchloom/commit/f86e02b2ba1b3cab3eefe54e5453ce48c0432696))
* batch tokenizer silently drops empty quoted strings ([2da3d3f](https://github.com/patchloom/patchloom/commit/2da3d3ffa76c4607ab7ed86a4195d1e83dc2b398))
* bench CI replace uses wrong --from flag syntax ([cedaea0](https://github.com/patchloom/patchloom/commit/cedaea0691cc478d02bc30bfa8d043266a8dcc42)), closes [#343](https://github.com/patchloom/patchloom/issues/343)
* **bench:** prefer newest binary, add per-tool MCP log reporting ([#489](https://github.com/patchloom/patchloom/issues/489)) ([3868408](https://github.com/patchloom/patchloom/commit/3868408960a3385ca3ec28709c7e05d73f26ce99))
* **bench:** use neutral tidy prompt so agents discover batch_tidy ([#490](https://github.com/patchloom/patchloom/issues/490)) ([9fb5e90](https://github.com/patchloom/patchloom/commit/9fb5e90ad8276a65710adc2b92c9d249a355372a))
* **ci:** correct SBOM upload path for cargo-cyclonedx ([#462](https://github.com/patchloom/patchloom/issues/462)) ([07e7bd1](https://github.com/patchloom/patchloom/commit/07e7bd17054cb997f924efc43678c470d4b6149e))
* **ci:** disable fossa test until false positives are filtered ([#437](https://github.com/patchloom/patchloom/issues/437)) ([46e0e04](https://github.com/patchloom/patchloom/commit/46e0e04109f75cebd575117f2244b9169dae365e))
* **ci:** exclude securityscorecards.dev from lychee link checks ([#464](https://github.com/patchloom/patchloom/issues/464)) ([04ceac8](https://github.com/patchloom/patchloom/commit/04ceac8a98dee83ccabefc40bf879d5643c3e400))
* **ci:** make coverage badge step non-fatal when GIST_TOKEN missing ([#445](https://github.com/patchloom/patchloom/issues/445)) ([73c2e1e](https://github.com/patchloom/patchloom/commit/73c2e1e1b60ed141bbd0fff651bf0cdb93caa3de))
* **ci:** move FOSSA secret check from job-level to step-level ([#436](https://github.com/patchloom/patchloom/issues/436)) ([3f3be6f](https://github.com/patchloom/patchloom/commit/3f3be6fa1c19f35751f217877cb1cdf08b285ce4))
* **ci:** resolve Scorecard findings for token permissions and pinned deps ([#438](https://github.com/patchloom/patchloom/issues/438)) ([877f1fe](https://github.com/patchloom/patchloom/commit/877f1fe5c46738f7eef329436dfcab8b6e5f1a39))
* **ci:** upload Sigstore attestation bundles to GitHub Releases ([#466](https://github.com/patchloom/patchloom/issues/466)) ([2496409](https://github.com/patchloom/patchloom/commit/2496409382227576dcb997ed0c5a0c995b571f4d))
* colored diff output, edge-case tests, and clearer error messages ([#468](https://github.com/patchloom/patchloom/issues/468)) ([2eb5e39](https://github.com/patchloom/patchloom/commit/2eb5e394c0124446f7ae796ac59de4872cdebfee))
* complete driver refactoring (2 missed call sites, restore Path imports) ([7f389f8](https://github.com/patchloom/patchloom/commit/7f389f876df79cadca03bec00ea86077fb2d7cca))
* cross-platform backup paths for Windows drive letters ([334ecb4](https://github.com/patchloom/patchloom/commit/334ecb4a32db4e8bf21ced38c2e2f6acf6665062))
* doc append/prepend respect --quiet flag; add batch example ([3aade73](https://github.com/patchloom/patchloom/commit/3aade731c9d95abb8386796bf3073c543e78fc3c))
* **doc:** correct predicate syntax in doc select help text ([84471b9](https://github.com/patchloom/patchloom/commit/84471b96d5986a95a2d96d3f1c8532ce79d1b815))
* eliminate flaky validation tests caused by timestamp collision ([f112d97](https://github.com/patchloom/patchloom/commit/f112d97974e16797dec5cdc6ce7de608b61a807d))
* enable all integration tests on Windows ([74c4350](https://github.com/patchloom/patchloom/commit/74c4350ed0de45f2a0818b01240cee13c58dc9de)), closes [#218](https://github.com/patchloom/patchloom/issues/218)
* enable serde_json preserve_order to maintain JSON key ordering ([f5727a8](https://github.com/patchloom/patchloom/commit/f5727a826a97dd99cfc386d01039c6e51ce5955c))
* gate shell helpers with cfg(not(windows)) ([4c666ca](https://github.com/patchloom/patchloom/commit/4c666ca22a31d572e9c1dfcb1998b9799490bd58))
* improvement cycle 1 (create backup, tidy JSON, finalize ordering) ([#428](https://github.com/patchloom/patchloom/issues/428)) ([cabd164](https://github.com/patchloom/patchloom/commit/cabd164f38af7ab5f1ae5992edb0d98eca8cdc9e))
* improvement cycle 2 (delete backup, tidy exit code tests) ([#429](https://github.com/patchloom/patchloom/issues/429)) ([50f58c5](https://github.com/patchloom/patchloom/commit/50f58c5fa9bbedf1059d1ee493267d15631c600a))
* improvement cycle 3 (backup consistency, test coverage) ([#430](https://github.com/patchloom/patchloom/issues/430)) ([8486726](https://github.com/patchloom/patchloom/commit/8486726b4e4a05a8f23f97250fce3794863f5552))
* install lychee from GitHub releases on ubuntu-latest ([6c2d3bc](https://github.com/patchloom/patchloom/commit/6c2d3bc3361d2c44f3a923af4c417d6bd62f8a50))
* isolate Trivy from runner's broken Docker credential helper ([14734d2](https://github.com/patchloom/patchloom/commit/14734d26a81b5f1329f7fa5321d497d5ca7effc1))
* lint-agents skips dangerous commands inside code fences and inline code ([21ba873](https://github.com/patchloom/patchloom/commit/21ba873a633f5d1db40917865c1f34f225b44cc4))
* make update-readme portable across BSD and GNU sed ([3d60525](https://github.com/patchloom/patchloom/commit/3d60525ae2542aa8a276a84c598976842586e460)), closes [#360](https://github.com/patchloom/patchloom/issues/360)
* MCP batch test and enable MCP integration tests on all platforms ([ddff787](https://github.com/patchloom/patchloom/commit/ddff787670084014247965075dcbb3cc49a14a29))
* **mcp:** remove batch/transaction tools for zero-failure agent benchmarks ([#481](https://github.com/patchloom/patchloom/issues/481)) ([1ea3849](https://github.com/patchloom/patchloom/commit/1ea3849df96b0f11a110e3b17c8abfd66fcfaeba))
* md command errors now include the file path ([41b4533](https://github.com/patchloom/patchloom/commit/41b453390a635b9e11ead35349efe3e5a637a2b2))
* plan schema version, batch op limit, test hardening ([1c9b1a5](https://github.com/patchloom/patchloom/commit/1c9b1a573f02d1122fec210b94130fc843383b4e))
* preserve single-file text format in tx search, add path-prefix assertions ([54f420f](https://github.com/patchloom/patchloom/commit/54f420f5d30e259027b88590a08eecba74ea85b1))
* preserve YAML comments on array-resizing doc operations ([e8e1bfb](https://github.com/patchloom/patchloom/commit/e8e1bfbd7a71eb86fe4d980c0a055a01500c0c4b))
* prevent data corruption when backing up files outside project root ([be4bf78](https://github.com/patchloom/patchloom/commit/be4bf78606ab1281d427c7f1b411f46c7dda636e)), closes [#373](https://github.com/patchloom/patchloom/issues/373)
* propagate backup finalize errors instead of discarding them ([35d65f8](https://github.com/patchloom/patchloom/commit/35d65f8542f2e54c786ef22c0e20fe965d01b4e8))
* reject --nth 0 in replace and tx replace operations ([e025eaf](https://github.com/patchloom/patchloom/commit/e025eafd1369f51cf805b310654dc558f27f713b))
* reject invalid normalize_eol values and surface doc append/prepend errors ([b17bce8](https://github.com/patchloom/patchloom/commit/b17bce82759bc65627648175eb8dd79f4c9a05ae))
* remove unused import and write bench summary to step summary ([bed246c](https://github.com/patchloom/patchloom/commit/bed246cea8c4a289956810fef3a77f54a9090e54))
* rename command read_to_string now includes file path in errors ([9664b29](https://github.com/patchloom/patchloom/commit/9664b29c5b02d0ee652d9771090c3f2499d3fa1e))
* replace stale 'key' with 'selector' in all descriptions and docs ([ede2e1d](https://github.com/patchloom/patchloom/commit/ede2e1d903a8d463ce225f06194dd8c1134955b9))
* replace unwrap() with proper error in batch temp path ([fe2ed28](https://github.com/patchloom/patchloom/commit/fe2ed286f9eb4d4b2d886246ebc3c0f1613526f0))
* **replace:** include search path in no-match stderr message ([37f919a](https://github.com/patchloom/patchloom/commit/37f919a8b2254193b1d5872f9cfaf4514a5f4ec7))
* resolve 26 CodeQL Python quality findings ([#439](https://github.com/patchloom/patchloom/issues/439)) ([31a9d5d](https://github.com/patchloom/patchloom/commit/31a9d5d9745bae7b8027449ddc9e7951ff31ef89))
* resolve 5 open issues ([#409](https://github.com/patchloom/patchloom/issues/409)-[#413](https://github.com/patchloom/patchloom/issues/413)) ([75c8e82](https://github.com/patchloom/patchloom/commit/75c8e82afa9df5c38062571507cb1ee0ebdbdcb5)), closes [#410](https://github.com/patchloom/patchloom/issues/410) [#411](https://github.com/patchloom/patchloom/issues/411) [#412](https://github.com/patchloom/patchloom/issues/412)
* resolve cyclic imports and restore dynamic coverage badge ([#442](https://github.com/patchloom/patchloom/issues/442)) ([251287c](https://github.com/patchloom/patchloom/commit/251287c277f071af0eb8303de78901c511dc7cb2))
* resolve GitHub AI code quality findings ([#469](https://github.com/patchloom/patchloom/issues/469)) ([abfdbdb](https://github.com/patchloom/patchloom/commit/abfdbdbeec59cbb36ec8f5af5edb5497321a488d))
* resolve issues [#364](https://github.com/patchloom/patchloom/issues/364)-367 from Cycle 3 ([31e546b](https://github.com/patchloom/patchloom/commit/31e546b99cdb0ada383fbe976eea3afa6c067a87)), closes [#365](https://github.com/patchloom/patchloom/issues/365) [#366](https://github.com/patchloom/patchloom/issues/366) [#367](https://github.com/patchloom/patchloom/issues/367)
* resolve YAML merge keys (&lt;&lt;) during doc operations ([85f3e32](https://github.com/patchloom/patchloom/commit/85f3e32670b19fd50e015483c98fae62f97425f9)), closes [#203](https://github.com/patchloom/patchloom/issues/203)
* **selector:** reject ? prefix in predicate keys with helpful message ([723cae5](https://github.com/patchloom/patchloom/commit/723cae51a7c2fcdfc3e82435586476240fe9bf04)), closes [#403](https://github.com/patchloom/patchloom/issues/403)
* spec compliance fixes and test coverage for MPI Cycle 5 ([52ac410](https://github.com/patchloom/patchloom/commit/52ac410898fc08d74e44d24e442899f038bfdb62))
* undo correctly restores files that were outside the project root ([5d4b397](https://github.com/patchloom/patchloom/commit/5d4b397ae61b15008a38dbfa1a1d50892282e88c))
* update bench.yml upload-artifact to v7, add concurrency group ([4cbb7e9](https://github.com/patchloom/patchloom/commit/4cbb7e9cc62682af3fdb7dd88419013ef8dde52e))
* update stale test counts in README and agent test docs ([d57bc18](https://github.com/patchloom/patchloom/commit/d57bc182d70f7802442ea87886c93bbc22269afd))
* update-readme uses --all-features for accurate counts ([bf2f447](https://github.com/patchloom/patchloom/commit/bf2f447174a145471b6a52b77aeac66540fde64f))
* use .intoto.jsonl extension for attestation bundles ([#467](https://github.com/patchloom/patchloom/issues/467)) ([7c88e80](https://github.com/patchloom/patchloom/commit/7c88e80e3a8a452421a99e883e95f358af17f504))
* use correct lychee release tag and asset name ([05a6d72](https://github.com/patchloom/patchloom/commit/05a6d722626336d93ac65e50d540b538c942eddd))
* use ghcr.io for Trivy DB to avoid GCR credential errors ([78d7b74](https://github.com/patchloom/patchloom/commit/78d7b74ffae79900cb78698f327dd743a9b77bf6))
* use nanosecond timestamps in backup sessions ([5e1962a](https://github.com/patchloom/patchloom/commit/5e1962a5de0b814a73740039a4962689d91527e7)), closes [#363](https://github.com/patchloom/patchloom/issues/363)
* use streaming binary probe in tx dir search to avoid large allocations ([a05b639](https://github.com/patchloom/patchloom/commit/a05b639dd87ecbefd09efc22cebc79be3fcb14e0))
* warn on malformed .patchloom.toml, add backup pruning tests and troubleshooting docs ([2e300d5](https://github.com/patchloom/patchloom/commit/2e300d523c315a7b388d3828e16736b81defbfed)), closes [#369](https://github.com/patchloom/patchloom/issues/369) [#371](https://github.com/patchloom/patchloom/issues/371) [#372](https://github.com/patchloom/patchloom/issues/372)
* Windows backup test failures (directory open + external path prefix) ([be64e12](https://github.com/patchloom/patchloom/commit/be64e1288de5f10335568623add9fc1bb1fc441b))
* Windows CI test failures ([b32fa8d](https://github.com/patchloom/patchloom/commit/b32fa8dfcb92c3ef431689483d473e42c87f4471))
* Windows CI timeout in large-stderr validation test ([17efd15](https://github.com/patchloom/patchloom/commit/17efd157649c8d8faa3326151234b112cbf28999))
* Windows path colon in files_with_matches test ([8722360](https://github.com/patchloom/patchloom/commit/87223601d1381afbe98958d9d3a59774617678a5))
* wire prune_old_backups into backup session creation ([2709ce7](https://github.com/patchloom/patchloom/commit/2709ce7861c072850db1f08f759251f2853cad71))
* YAML CST safety net for array length changes, add parse validity checks ([24e4de0](https://github.com/patchloom/patchloom/commit/24e4de021e020b3da7fc1c2ab9bee8889a566819)), closes [#209](https://github.com/patchloom/patchloom/issues/209) [#210](https://github.com/patchloom/patchloom/issues/210) [#211](https://github.com/patchloom/patchloom/issues/211)


### Performance Improvements

* cache canonicalized cwd in MCP server ([c0ef850](https://github.com/patchloom/patchloom/commit/c0ef850c9acf50fbea233c8d0b89218680918674))
* cache parsed serde_json::Value in tx to avoid redundant parse-serialize cycles ([7812df2](https://github.com/patchloom/patchloom/commit/7812df247ef8468255f161090e92dfaa648c3e6b)), closes [#250](https://github.com/patchloom/patchloom/issues/250)
* four targeted optimizations across hot paths ([3d0ab90](https://github.com/patchloom/patchloom/commit/3d0ab901b70d3704036c79cc551d6dcbfe100bf8))
* reduce allocations across replace, search, doc ops, diff, and selector ([5543443](https://github.com/patchloom/patchloom/commit/5543443a25147a9df4d37c8715eb0639648045d3))
* trim agent_rules.md from 102 to 40 lines (71% smaller) ([cecbc18](https://github.com/patchloom/patchloom/commit/cecbc18d08373b8169d1e9be1a76a28e716b215e))
* use parallel file walking for directory traversal ([4a68a85](https://github.com/patchloom/patchloom/commit/4a68a85347151d5961078cc2548a1782f291ebe6)), closes [#249](https://github.com/patchloom/patchloom/issues/249)

## [Unreleased]

## [0.1.0] - 2026-06-04

### Security

- Fixed external path traversal bypass in `undo --apply` restore logic: crafted `__external__/../..` manifest entries could overwrite files outside the project root
- Added syntactic path traversal validation to undo restore paths
- Added `validate_path_resolved` symlink check to all 16 MCP write handlers

### Commands

19 commands (including `mcp-server`, enabled by default) covering search, structured editing, batching, and file operations:

- **search** / **replace** - Literal and regex search/replace across files, with context lines, `--nth`, `--case-insensitive`, `--insert-before`/`--insert-after`, `--assert-count`, and `--if-exists` for idempotent runs
- **doc** - Parser-backed JSON, YAML, and TOML editing (get, set, delete, merge, append, prepend, update, move, ensure, delete-where, select, flatten, diff). Preserves comments and formatting in YAML and TOML
- **md** - Heading-aware markdown editing (replace-section, insert-after/before-heading, upsert-bullet, table-append, dedupe-headings, lint-agents)
- **tx** - Atomic multi-file transactions with 23 operation types, format/validate lifecycle, strict rollback mode, and YAML/TOML plan format support
- **batch** - Line-oriented multi-operation syntax for quick multi-file edits without JSON
- **patch** - Apply or check unified diffs with fuzz matching
- **create** / **delete** / **rename** - File lifecycle operations with `--apply`/`--check`/`--force` modes. Rename handles binary files natively via `fs::rename`
- **read** / **status** - File inspection and git working-tree status
- **mcp-server** - MCP protocol server exposing all operations as structured tool calls
- **agent-rules** / **completions** - Generate AI agent instructions or shell completions

### Structured file safety

- YAML and TOML edits preserve inline comments, section comments, and formatting (CST-level editing)
- JSON/YAML/TOML mutations are parser-backed; output is always valid
- Sequence-rooted YAML files are handled correctly (falls back to non-preserving serialization when root is not a mapping)
- `doc` operations include depth guard (128 levels) on deep merge to prevent stack overflow
- All file writes go through atomic write (tempfile + rename) for crash safety

### Batching and transactions

- `tx` plans support `format` and `validate` lifecycle arrays with configurable timeouts
- `strict` mode reverts all writes on format/validate failure (exit code 7)
- `read` and `search` operations in tx plans for inspect-then-edit workflows in a single call
- `batch` provides simpler line-oriented syntax covering 20 operation types
- Operation ordering is well-defined: last write wins, delete-then-create works, each op sees prior results
- CLI `tx` validates plan `cwd` is a directory, returning PARSE_ERROR instead of confusing OS errors
- Relative plan `cwd` values resolve from invocation root, matching MCP behavior
- Lifecycle shell commands (format/validate) now capture first 512 bytes of stderr in error output
- Lifecycle failure messages include the working directory (`cwd: .` or `cwd: nested`)

### Correctness fixes

- `file.create` after `file.delete` in the same tx plan no longer silently loses the file
- Empty `--from` in replace/tx is rejected instead of inserting between every character
- tx replace with conflicting fields (`to` + `insert_before`) returns PARSE_ERROR
- tx replace missing all output fields returns PARSE_ERROR instead of silently deleting
- Replace-only tx plans with zero matches return NO_MATCHES (exit 3) instead of SUCCESS
- tx glob replace no longer buffers non-matching files into pending state
- `create --check` verifies parent directory exists (non-force mode)
- Race-free file creation via `File::create_new` for `create --apply` and tx `file.create`
- Fixed `read_file_content` double-join bug when transaction cwd is relative
- `create` command: backup finalize was called before the atomic write, producing a backup for a change that had not yet happened; finalize now runs after the write succeeds
- `create` command now creates backup sessions before writing, enabling `undo --apply` to remove or restore files created with `create --apply`

### Output and diagnostics

- `--json` structured output on all commands including tx error paths
- `--jsonl` streaming output for search and read
- Explicit `error_kind` values in tx JSON output (parse_error, rollback, validation_failed, format_failed)
- Stderr diagnostics for silently skipped files in search, replace, and tx glob
- File path context in doc operation error messages
- Improved doc command error messages to list supported file extensions
- `tidy fix` now emits structured JSON/JSONL output when `--json` or `--jsonl` is active, matching other write commands

### MCP server

- MCP `search_files` tool exposes `invert_match` and `assert_count` parameters, matching CLI and tx feature parity
- MCP `search_files`, `replace_text`, and `fix_whitespace` tool descriptions document text-file semantics (binary and invalid UTF-8 files are skipped)
- MCP `transaction` validates relative `cwd` resolves to a directory, not a file
- Cached canonicalized cwd at startup, eliminating redundant `realpath` syscall per tool invocation
- Consolidated `validate_path_contained` + `validate_path_resolved` into single `check_path` method, preventing partial validation
- Shared `resolve_plan_cwd` function deduplicates CLI and MCP cwd resolution

### Testing and benchmarks

- 1100+ tests verified on Grok 4.3, GPT-5.4, and Claude Opus 4.6
- Agent integration tests: 19 scenarios with invocation-capture shim
- 5 fuzz targets: selector parse, patch parse, patch apply, batch tokenize, selector eval
- CLI benchmarks vs native tools (grep, sed, jq) using hyperfine
- Agent A/B benchmarks measuring duration, tool calls, and success rate

### Internal improvements

- Extracted shared tx execution core (`execute_and_collect`, `run_lifecycle`) eliminating ~190 lines of duplication
- Extracted `backup_write_files` helper, refactored 5 call sites across replace, patch, and tidy commands
- Extracted `apply_replacements` helper in replace command, deduplicating backup+write block
- Extracted `with_doc_mutation` helper in doc command, eliminating 9x load/clone/serialize/write boilerplate
- Extracted `compile_replace_regex` shared helper

### Infrastructure

- MSRV: Rust 1.95+
- License: MIT OR Apache-2.0
- CI: fmt, clippy, tests, MSRV check, dependency audit, doc freshness checks, code coverage, Codecov upload
- CI: benchmark summary table with 90-day artifact retention and cross-run regression detection (20% threshold, 2ms minimum)
- `make check` runs the full gate locally, including generated doc freshness

### Documentation

- Documented column offset semantics in search JSON output
- Added `init` command to README Commands table
- Documented stderr capture and cwd context in lifecycle failure output (reference docs, quickstart)
- Added `cargo check --all-targets` to CONTRIBUTING.md for default-feature build verification

[Unreleased]: https://github.com/patchloom/patchloom/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/patchloom/patchloom/releases/tag/v0.1.0
