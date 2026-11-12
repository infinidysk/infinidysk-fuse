---
name: infinidysk-commits
description: infinidysk-fuse Conventional Commit types, scopes, breaking-change rules and user-facing PR titles, with examples. Use before writing a commit message, creating a PR, or editing a PR title.
---

# Commits and PR titles

Format: `<type>(<scope>): <short imperative description>`. release-please reads it.

| Type | Use for |
|------|---------|
| `feat` | New public API or user-visible behavior |
| `fix` | Bug fixes, regressions, compatibility fixes |
| `perf` | Performance-only improvements |
| `refactor` | Internal restructuring worth a release note (rare; prefer `chore`) |
| `build` | Packaging or feature-flag changes crate users care about (otherwise `chore(build)`) |
| `revert` | Reverts |
| `chore` | **All** non-behavior work: CI, tests only, docs, examples, `AGENTS.md`, skills, tooling, repo hygiene |

- No bare `docs` / `ci` / `test` / `tests` types; use `chore(<scope>)`. A change that does not alter library behavior is `chore` on both commit and PR title, so release notes leave it out.
- **Breaking changes** (public API removals or signature changes, MSRV bumps, removed features) use `!` and/or a `BREAKING CHANGE:` footer. Before 1.0 a breaking change bumps the minor version.
- `chore(main): release …` is reserved for release-please. Never edit `CHANGELOG.md` by hand.
- Small, focused commits; one logical change each. No vague messages (`wip`, `fix bug`), no unrelated changes in one commit.

Scopes (smallest accurate one; omit only for small cross-cutting changes): `raw`, `path`, `abi`, `session`, `connection`, `notify`, `mount`, `deps`, `ci`, `docs`, `test`, `examples`, `agents`, `skills`, `release`.

```
feat(raw): add a reply type for FUSE_LSEEK
fix(notify): send filenames and data in kernel notifications
perf(connection): write replies with one vectored write
feat(path)!: pass nlookup to batch_forget
chore(ci): run clippy on FreeBSD 15
```

## PR titles

Same `<type>(<scope>): <description>` format; never omit the type. Describe the change for **crate users**: what is fixed or introduced from their point of view, not how. Commit messages carry the technical detail.

| Commit message (technical) | PR title (user-facing) |
|----------------------------|------------------------|
| `fix(notify): copy the name bytes into the notify payload` | `fix(notify): inval_entry and delete notifications reach the kernel` |
| `perf(connection): use IoSlice for reply writes` | `perf(connection): faster reads by avoiding a reply copy` |

For changes with no user-visible effect (`chore`, internal refactors) a concise technical title is fine.
