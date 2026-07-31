//! Hand-written troubleshooting footer.

/// Verbose logging guidance (always shown).
pub(crate) fn append_troubleshooting(out: &mut String) {
    // Troubleshooting (always shown)
    out.push_str(
        "## Troubleshooting\n\n\
         If a command produces unexpected results, enable verbose logging to see \
         what patchloom is doing internally:\n\n\
         ```bash\n\
         patchloom --verbose <command> [args]\n\
         # or via environment variable:\n\
         PATCHLOOM_LOG=1 patchloom <command> [args]\n\
         ```\n\n\
         Diagnostic messages are printed to stderr prefixed with `[patchloom]`.\n",
    );
}
