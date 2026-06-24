#!/usr/bin/env python3
"""Filter known false positives from FOSSA test JSON output.

Exit 0 if all issues are documented false positives.
Exit 1 if any genuine issues remain after filtering.

Documented false positives
--------------------------
1. cc, compiler_builtins, wasi (and other Rust compiler infrastructure)
   apache-2.0 WITH llvm-exception
   The LLVM exception explicitly permits compiling code with these
   crates without the Apache-2.0 license applying to the compiled
   output. Standard Rust compiler infrastructure; affects every Rust
   project. FOSSA flags the "WITH llvm-exception" clause as an
   unrecognized license modifier.

2. r-efi  LGPL-2.1-or-later
   Pure UEFI type definitions (data structures, no executable code).
   Pulled in transitively via std on some targets; not linked into the
   user binary in any meaningful way.

3. ring  openssl-ssleay
   ring is ISC-licensed. The OpenSSL-SSLeay marker comes from historical
   OpenSSL-derived assembly code that ring inherited. The SSLeay license
   is permissive (BSD-style) and compatible with MIT/Apache-2.0 projects.
   Pulled in transitively via rustls for TLS support.

4. aws-lc-sys  GPL-1.0-only / GPL-1.0-or-later
   aws-lc-sys wraps AWS-LC which is Apache-2.0 + ISC licensed. FOSSA
   flags GPL because the source tarball includes GPL-licensed test vectors
   from OpenSSL. These test files are not compiled into the binary.
   Pulled in transitively via rustls/aws-lc-rs for TLS support.

5. security-framework  APSL-2.0
   security-framework is MIT/Apache-2.0 licensed Rust bindings to Apple's
   Security.framework. FOSSA flags APSL-2.0 because the underlying macOS
   system library is Apple-licensed, but the crate itself does not bundle
   Apple code; it links to the system library at runtime. This is standard
   for any Rust crate using native macOS APIs. Pulled in transitively via
   rustls-platform-verifier for TLS certificate verification.
"""

import json
import sys


# Crates with "apache-2.0 WITH llvm-exception" that FOSSA cannot parse.
# The LLVM exception permits compilation without license propagation.
LLVM_EXCEPTION_CRATES = {
    "cc",
    "compiler_builtins",
    "wasi",
    "wit-bindgen-core",
    "wit-bindgen-rust",
    "wit-bindgen-rust-macro",
}

# License IDs that FOSSA uses for the LLVM exception variant.
LLVM_EXCEPTION_LICENSES = {
    "apache-2.0 WITH llvm-exception",
    "Apache-2.0 WITH LLVM-exception",
}


def is_false_positive(pkg: str, license_id: str, issue_type: str = "") -> bool:
    """Return True if the (package, license, type) tuple is a documented false positive."""
    # Rust compiler infrastructure: apache-2.0 WITH llvm-exception
    if pkg in LLVM_EXCEPTION_CRATES and license_id in LLVM_EXCEPTION_LICENSES:
        return True

    # r-efi: pure type definitions, transitively pulled by std
    if pkg == "r-efi" and license_id in ("LGPL-2.1-or-later", "LGPL-2.1+"):
        return True

    # ring: ISC-licensed; SSLeay marker from historical OpenSSL assembly
    if pkg == "ring" and "ssleay" in license_id.lower():
        return True

    # aws-lc-sys: Apache-2.0/ISC; GPL flags from test vectors in source tarball
    if pkg == "aws-lc-sys" and license_id.startswith("GPL-"):
        return True

    # security-framework: MIT/Apache-2.0 bindings; APSL-2.0 is the system library
    if pkg == "security-framework" and license_id == "APSL-2.0":
        return True

    return False


def extract_package(issue: dict) -> str:
    """Extract the package name from a FOSSA issue.

    FOSSA uses 'revisionId' with format 'cargo+cc$1.0.106'.
    Strip the ecosystem prefix and version suffix to get the crate name.
    Falls back to 'package' or 'name' fields if revisionId is missing.
    """
    rev = issue.get("revisionId", "")
    if rev:
        # Strip ecosystem prefix (e.g., "cargo+", "go+")
        if "+" in rev:
            rev = rev.split("+", 1)[1]
        # Strip version suffix (e.g., "$1.0.106")
        if "$" in rev:
            rev = rev.rsplit("$", 1)[0]
        return rev
    return issue.get("package", "") or issue.get("name", "") or ""


def main() -> int:
    if len(sys.argv) < 2:
        print("Usage: fossa-filter.py <fossa-results.json>", file=sys.stderr)
        return 2

    with open(sys.argv[1]) as f:
        data = json.load(f)

    # fossa test --format json may return a list or an object with an issues key
    if isinstance(data, list):
        issues = data
    else:
        issues = data.get("issues", data.get("issue", []))
    if not isinstance(issues, list):
        issues = []

    real_issues = []
    filtered_count = 0

    for issue in issues:
        pkg = extract_package(issue)
        lic = issue.get("license", "") or issue.get("licenseId", "") or ""
        itype = issue.get("type", "") or issue.get("issueType", "") or ""

        if is_false_positive(pkg, lic, itype):
            filtered_count += 1
            continue

        real_issues.append(issue)

    if real_issues:
        print(f"FAIL: {len(real_issues)} genuine issue(s) after filtering "
              f"{filtered_count} known false positives:")
        for r in real_issues:
            pkg = extract_package(r)
            lic = r.get("license", "") or r.get("licenseId", "?")
            itype = r.get("type", "") or r.get("issueType", "?")
            print(f"  - {pkg}  {lic}  ({itype})")
        return 1

    print(f"OK: All {filtered_count} issue(s) are documented false positives.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
