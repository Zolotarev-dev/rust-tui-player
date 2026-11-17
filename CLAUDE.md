
## Core Engineering & Clean Code

- **Scope Control:** Implement strictly what was requested. Do not refactor unrelated modules, change public APIs without reason, or introduce speculative abstractions.
- **Explicit Error Handling:** Never silently swallow errors or use panicking shortcuts (`unwrap`, bare `except:`, ignored promises) in production paths. Propagate or handle every failure state explicitly so UI/API state stays consistent.
- **Small, Focused Units:** Keep functions single-purpose with clear naming and flat control flow (prefer early returns over deep nesting). Avoid magic numbers and duplicated logic.
- **Dependency Hygiene:** Do not add third-party libraries if the standard library or existing project dependencies solve the task cleanly. Ask before introducing heavy frameworks.
- **Review Before Done:** Re-read all modified files before finishing. Never silently delete existing functionality or tests.

## Security Baseline (Non-Negotiable)

- **Untrusted Input:** Treat all external input (CLI args, filenames, file contents, network payloads, env vars) as hostile. Validate types, lengths, and bounds at the boundary; handle invalid UTF-8 according to data format requirements (reject malformed structured data/protocols, or safely sanitize/replace in unstructured display text).
- **Output & Terminal Safety:** Strip ANSI escape sequences and ASCII/C1 control characters (`[\x00-\x1f\x7f\u0080-\u009f]`) before printing untrusted strings to logs or TUI.
- **Filesystem & Path Safety:** Prevent path traversal (`..`) and symlink escapes outside the target root directory. Perform critical file writes via temp file in the same filesystem + atomic rename/replace (and `fsync` where durability matters), accounting for filesystem semantics and power-loss limitations.
- **Secrets & Leaks:** Never hardcode tokens, passwords, or keys. Keep `.env` and local credentials out of git and error messages.

## Testing & Verification Discipline

- **Environment First:** Verify that required compilers, linters, and test runners exist before running them. Distinguish clearly between missing tools and actual test failures.
- **Edge-Case Coverage:** Cover boundary conditions (empty input, malformed data, invalid UTF-8 handling per format, missing files/devices, interrupted operations) alongside happy paths.
- **Incremental & Full Verification:** Run relevant targeted checks after incremental changes, and execute the full project linter and test suite before completing a task. Never claim checks passed without actual execution output.
- **No Unverified Claims:** Do not claim functionality, compatibility, security, or performance without evidence. Clearly distinguish implemented, tested, and assumed behavior.
