//! Hand-written **surface** recipes (MCP mode, params, batching, structured CLI).
//!
//! Policy-adjacent but oriented to which surface (MCP vs CLI batch/tx) to use.

/// MCP section, canonical parameter names, batching, structured edits, project config.
pub(crate) fn append_surfaces(
    out: &mut String,
    show_cli: bool,
    show_mcp: bool,
    show_linux: bool,
    show_windows: bool,
) {
    // MCP section
    if show_mcp {
        out.push_str(
            "## MCP mode\n\n\
             **ALWAYS use MCP tools for ALL file edits.**\n\n\
             - For any multi-op or multi-file change, **use `execute_plan`** with an inline plan (or plan_path). \
               It provides atomic execution + rollback, exactly like the CLI `tx` command.\n\
             - Use `batch_replace` or `batch_tidy` only when applying the *exact same* operation to multiple files.\n\
             - Do **not** issue parallel write calls against the same path(s) — per-call success does not guarantee a coherent combined result.\n\
             - Nested trees: set a **relative** `cwd` under the server workspace and keep op paths short. \
               Do not use absolute `cwd` on MCP. Do not combine `cwd` with `for_each`.\n\
             - `search_files`: canonical multi-root field is `paths` (array). Singular `path` is accepted as an alias for one root.\n\n\
             Example (edit under a fixture without prefixing every path):\n\n\
             ```json\n\
             {\n\
               \"plan\": {\n\
                 \"version\": 1,\n\
                 \"cwd\": \"fixtures/svc\",\n\
                 \"operations\": [\n\
                   {\"op\": \"doc.set\", \"path\": \"configs/app.yaml\", \"selector\": \"name\", \"value\": \"updated\"}\n\
                 ]\n\
               }\n\
             }\n\
             ```\n\n\
             That re-roots to `fixtures/svc/configs/app.yaml` (not a same-named file at the workspace root).\n\n",
        );
    }

    // Canonical parameter names (same on CLI flags, tx/MCP JSON, and batch positionals).
    out.push_str(
        "## Canonical parameter names\n\n\
         Use these names in plans, MCP args, and CLI flags (do not invent alternates):\n\n\
         | Concept | Canonical name | Notes |\n\
         |---------|----------------|-------|\n\
         | Text/identifier before | `old` | CLI **replace**: positional `OLD` (not `--old`). CLI **ast rename/replace**: `--old`. Plans/MCP: `\"old\"`. |\n\
         | Text/identifier after | `new` | CLI: `--new`. Plans/MCP: `\"new\"`. |\n\
         | Doc path into a document | `selector` | CLI positional. Plans/MCP: `\"selector\"`. |\n\
         | AST rename / replace / read | path first | `ast rename PATH --old X --new Y`; `ast replace PATH SYMBOL --old … --new …` (no `--symbol` flag). |\n\
         | AST refs / impact | symbol first | `ast refs SYMBOL PATH` / `ast impact SYMBOL PATH` (no `--name` flag; #1841). |\n\
         | Schema capability filter | `weak` / `medium` / `strong` | `schema --tier` only accepts these (not `small`/`large`). |\n\n\
         Replace example: `patchloom replace OLD --new NEW path` (positional OLD + `--new`; never `replace --old …`).\n\
         Insert example: `patchloom replace ANCHOR --insert-after 'new line' path` (or `--insert-before`; \
         insert text is never a second positional).\n\
         Some plan/MCP fields still **accept** legacy aliases (`from`/`to` for replace, `key` for doc selector, \
         `ops` for the plan `operations` array) so older agent prompts keep working, but examples and new plans \
         must use the canonical names above.\n\n",
    );

    // Batching section
    if show_cli {
        out.push_str("## Batching (the main speed win)\n\n\
             Six file edits via native tools = six round-trips. One `batch` call = one round-trip:\n\n");

        if show_linux {
            out.push_str(
                "```bash\n\
                 patchloom batch --apply <<'EOF'\n\
                 doc.set config.json version \"2.0.0\"\n\
                 doc.set config.yaml app.version \"2.0.0\"\n\
                 replace README.md \"1.0.0\" \"2.0.0\"\n\
                 replace src/main.rs \"proccess\" \"process\" --fuzzy --min-fuzzy-score 0.80\n\
                 file.create hello.txt \"Hello, World!\"\n\
                 file.rename old.txt new.txt\n\
                 md.upsert_bullet CHANGELOG.md \"## Changes\" \"- Bumped to 2.0.0\"\n\
                 EOF\n\
                 ```\n\n\
                 One line per operation. Double-quote values with spaces. Unquoted JSON objects/arrays \
                 (`file.create f.json {\"x\":1}`) keep inner quotes. In `file.create`/`append`/`prepend` content, \
                 `\\n` `\\t` `\\r` `\\\\` `\\\"` expand (multi-line content on one batch line). Prefer positional \
                 content (`file.create f.txt \"hi\"`); a leading `content=` / `body=` key is also accepted so \
                 plan-shaped lines do not write literal `content=…` bytes.\n\
                 Batch `replace` is `replace PATH OLD NEW` (not CLI `replace OLD --new NEW path`). Optional \
                 `old=`/`new=` (or `from=`/`to=`) prefixes on the pattern tokens are peeled. Optional flags after \
                 path/old/new: `--fuzzy`, `--min-fuzzy-score`, `--word-boundary`/`-w`, `--command-position`, \
                 `--require-change`, `-i`/`--case-insensitive`, `--if-exists`. Advanced options (regex, context, \
                 nth) need a `tx` plan.\n\n",
            );
        }

        if show_windows {
            if show_linux {
                out.push_str(
                    "On Windows (where heredocs are not available), write operations to a file and pass it:\n\n",
                );
            }
            out.push_str(
                "```bash\n\
                 patchloom batch ops.txt --apply\n\
                 ```\n\n",
            );
            if !show_linux {
                out.push_str(
                    "One line per operation in the file. Double-quote values with spaces. Unquoted JSON objects \
                     keep inner quotes. In `file.create`/`append`/`prepend` content, `\\n` `\\t` `\\r` expand.\n\n",
                );
            }
        }

        out.push_str(
            "**Note:** Values are parsed as JSON first. An unquoted `1.0` is parsed as a number. \
             Batch `doc.set f.json v \"2.0\"` is still a number (quote removal leaves `2.0`). \
             Force a string with nested JSON quotes: batch `doc.set f.json v \"\\\"2.0\\\"\"`, \
             or CLI Unix `doc.set config.json version '\"1.0\"'` / Windows \
             `doc.set config.json version \"\\\"1.0\\\"\"`. Prefer `tx`/MCP JSON \
             `\"value\": \"2.0\"` when quoting is painful.\n\n",
        );

        out.push_str(
            "For complex plans needing format/validate lifecycle, regex replace, or `--nth`, use `tx` with JSON:\n\n\
             ```bash\n\
             patchloom tx plan.json --apply\n\
             ```\n\n\
             Relative paths for batch ops files, `tx`/`explain` plan files, `patch` files, and \
             `--files-from` lists resolve under `--cwd` (same base as operation targets). \
             Absolute meta-input paths are unchanged. With `--contain`, those meta-input paths \
             (and operation targets) must resolve inside the workspace: absolute paths under \
             `--cwd` are allowed; `../` escapes and absolute paths outside the workspace are \
             rejected.\n\n\
             In plan JSON, doc ops use the field name `selector` (not `key`). \
             `key` is accepted as an alias if a model emits it, but prefer `selector`.\n\n",
        );

        // Structured edits section
        out.push_str("## Structured edits\n\n");

        if show_linux {
            out.push_str(
                "```bash\n\
                 # Edit a value in JSON/YAML/TOML by selector path (parser-backed, preserves comments)\n\
                 patchloom doc set config.json version '\"2.0.0\"' --apply\n\
                 patchloom doc merge config.yaml --value '{\"db\":{\"pool\":10}}' --apply\n\
                 \n\
                 # Append a row to a markdown table\n\
                 patchloom md table-append README.md --heading \"## API\" --row \"| new | row |\" --apply\n\
                 ```\n\n",
            );
        }

        if show_windows {
            if show_linux {
                out.push_str("On Windows, use double-quote escaping:\n\n");
            }
            out.push_str(
                "```bash\n\
                 patchloom doc set config.json version \"\\\"2.0.0\\\"\" --apply\n\
                 patchloom doc merge config.yaml --value \"{\\\"db\\\":{\\\"pool\\\":10}}\" --apply\n\
                 patchloom md table-append README.md --heading \"## API\" --row \"| new | row |\" --apply\n\
                 ```\n\n",
            );
        }

        out.push_str(
            "Add `--apply` to all write commands. Without it, patchloom previews changes without writing.\n\n",
        );

        out.push_str(
            "## Project configuration\n\n\
             Create `.patchloom.toml` in the project root to set defaults for all commands:\n\n\
             ```toml\n\
             [write_policy]\n\
             ensure_final_newline = true\n\
             normalize_eol = \"lf\"\n\
             trim_trailing_whitespace = true\n\
             collapse_blanks = true\n\
             \n\
             [tx]\n\
             strict = false\n\
             \n\
             [exclude]\n\
             globs = [\"target/**\", \"node_modules/**\"]\n\
             ```\n\n\
             CLI flags override config values. The file is searched upward from the working directory.\n\n",
        );
    }
}
