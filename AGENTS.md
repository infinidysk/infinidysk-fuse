# AGENTS.md

Rules for AI agents working on **infinidysk-fuse**. Write the name lowercase, `infinidysk`, even at the start of a sentence. Never "InfiniDysk".

## What this project is

An async FUSE library for Rust, published on crates.io as `infinidysk-fuse` (imported as `infinidysk_fuse`). It is a fork of [`fuse3`](https://github.com/Sherlock-Holo/fuse3) (MIT), maintained for the FUSE mount in [infinidysk](https://github.com/infinidysk/infinidysk) (`../infinidysk-rs`), a Usenet-backed virtual filesystem that streams media to players.

- Two layers: `raw` (inode-based, close to the kernel protocol) and `path` (path-based, on top of `raw` through the inode bridge).
- Two runtimes behind features: `tokio-runtime` and `async-io-runtime`. Exactly one must be enabled; the crate does not build with neither.
- Targets: Linux, macOS (macFUSE) and FreeBSD. CI builds all three; keep them building.

## Load on demand

These files are binding. Read each one only when the task needs it.

| When | Read |
|------|------|
| Writing a commit message, creating a PR or editing a PR title | [`.agents/skills/infinidysk-commits/SKILL.md`](.agents/skills/infinidysk-commits/SKILL.md) |
| Addressing CodeRabbit, Code Quality or code scanning findings | [`.agents/skills/infinidysk-review-triage/SKILL.md`](.agents/skills/infinidysk-review-triage/SKILL.md) |
| Writing or reviewing Rust, when the `rust-skills` skill is installed | Its `SKILL.md` once per session (it is one file behind several symlinked paths), then only the relevant `rules/` files |

## Upstream

Upstream is `Sherlock-Holo/fuse3`. Read it with `gh` (`gh api repos/Sherlock-Holo/fuse3/...`) or add it as a remote named `upstream`.

- Keep the module layout and public API close to upstream, so upstream fixes still merge cleanly. Rename or restructure only with a reason.
- Keep upstream attribution (`authors`, `LICENSE`).
- Changes to the kernel ABI (`src/raw/abi.rs`) follow the kernel headers (`fuse_kernel.h` in libfuse, osxfuse for macOS). Cite the header version you used.

## Scratch space and command output

`scrap/` is gitignored scratch space for notes, logs and files that must not be committed.

**Keep long command output out of the context.** Send cargo and test output to a log under `scrap/<task>/` and read back only the tail or the failing lines (`tail -40`, `grep -E '^(error|warning)|FAILED|panicked'`). This applies to background commands too: their full output returns when they finish.

## Explaining code and systems

When explaining code or systems to the maintainer, use Simple Technical English. Short sentences. One idea per sentence. Use the domain words that already exist in the code (and in the FUSE protocol) instead of inventing abstractions for them.

## Rust guidelines

The maintainer's Rust rules are in the private [`hoivikaj/rust-skills`](https://github.com/hoivikaj/rust-skills) (see Load on demand). Contributors without access can ignore it. The project rules below are binding either way and override rust-skills where they differ.

- Edition 2024, MSRV in `Cargo.toml` (`rust-version`). Formatting follows `.rustfmt.toml`.
- Before committing, run what CI runs, for both runtimes:
  - `cargo fmt --all --check`
  - `cargo clippy --all-targets --locked --features=tokio-runtime,file-lock,unprivileged`
  - `cargo clippy --all-targets --locked --features=async-io-runtime,file-lock,unprivileged`
  - `cargo test --locked --features=tokio-runtime,file-lock,unprivileged`
- Prefer fixing a clippy lint over `#[allow]`; any `#[allow]` needs a one-line reason.
- The crate version in `Cargo.toml` is owned by release-please. Do not bump it by hand.
- Mark deliberate corner-cutting with a `ponytail:` comment naming the ceiling and the upgrade path.
- `unsafe` only where FFI or the kernel interface needs it; every new block needs a `// SAFETY:` comment.
- No new `unwrap` / `expect` outside tests and provably infallible cases (state the proof in the `expect` message). Existing upstream code is not rewritten just for this.
- Errors go to the kernel as `Errno`. Do not panic on bad kernel input or a failed reply.
- `tracing` with structured fields. Never log file contents or credentials.
- Tests must not need root or a real mount in CI unless the test is gated and skipped when FUSE is missing.

### Performance

Playback performance has priority over memory usage. The read path (request decode, reply encode, vectored writes to `/dev/fuse`) is hot during streaming. Never trade throughput for lower memory or fewer allocations without explicit approval, and measure before claiming a speedup.

## Commits and PRs (required)

- [Conventional Commits](https://www.conventionalcommits.org/) for commits and PR titles: `<type>(<scope>): <short imperative description>`. release-please reads them.
- PR titles describe the change for **crate users**; commit messages carry the technical detail.
- Non-behavior work (CI, tests, docs, `AGENTS.md`, skills, tooling) is `chore(<scope>)`; never bare `docs` / `ci` / `test`. Breaking API changes use `!`. Never edit `CHANGELOG.md` by hand.
- Types, scopes and examples: `infinidysk-commits` (see Load on demand).

## Never merge pull requests

Agents never merge PRs, in this repo or any infinidysk repo: no `gh pr merge`, no UI/API merge, no auto-merge. Opening the PR and reporting its URL ends the handoff. Never force-push shared branches or amend pushed commits.

## Addressing reviews

When you address CodeRabbit, Code Quality or code scanning findings, follow `infinidysk-review-triage` (see Load on demand). In short: reply to and resolve every thread, triage every open alert, and never dismiss a review just to bypass protection.
