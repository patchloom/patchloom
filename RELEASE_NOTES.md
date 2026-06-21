## 0.4.0

This is a major release focused on making Patchloom more usable as a library and improving long-term maintainability.

### Highlights

* **CLI is now optional**: The `cli` feature (clap + all commands) is enabled by default, but you can use `default-features = false` (or select only `mcp` and/or `ast`) for a much smaller dependency footprint when using Patchloom as a pure Rust library. This replaces the previous blunt `core` feature.

* **More of the public API is reusable**: Types like `LintIssue` and others have been moved to more appropriate modules (e.g. `ops::md`) and re-exported so they can be used directly without pulling in command implementations.

* **Semver safety**: All public structs and enums are now marked `#[non_exhaustive]`. This prevents future breaking changes when new fields are added.

### Other changes

See the full changelog in this PR for the complete list of features, fixes, and improvements that have landed since the last release.
