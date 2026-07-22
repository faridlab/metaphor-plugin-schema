# Metaphor Crate

> Type: **`crate`** — a focused Rust library. Single concern, single responsibility, independently usable.
> This file orients Claude. Skills carry depth; load them on demand.

## What this is

A Rust library crate (`lib.rs`, no `main.rs`) inside a Metaphor workspace. Exposes a small, well-named public API that other projects consume as a dependency. Expected to be releasable on its own.

## Golden path

```bash
metaphor dev build               # build this crate
metaphor dev test                # run tests
metaphor lint check              # clippy + rustfmt + cargo-audit
metaphor docs generate           # rustdoc + coverage check
```

Fallback when outside a Metaphor workspace: `cargo build`, `cargo test`, `cargo clippy`, `cargo doc`.

## Rules

- **MUST** be a library (`[lib]` in `Cargo.toml`; no `[[bin]]`). If you need a binary, this is the wrong project type — use `cli-tool`.
- **MUST** keep the crate focused on one concern (one domain noun or one responsibility).
- **MUST** keep `Cargo.toml` **self-describing** — do NOT use `workspace.dependencies` inheritance for crates intended to be independently publishable. Pin direct versions.
- **MUST** write doc comments (`///`) on every public item; `metaphor docs coverage` will enforce.
- **SHOULD** use `thiserror` for library error types, `anyhow` only in binaries.
- **SHOULD** expose feature flags for optional capabilities (e.g. `features = ["tokio", "sqlx"]`) rather than forcing all deps.
- **NEVER** add business / domain logic here — that belongs in `module` projects. Crates are plumbing.
- **NEVER** couple to a specific backend (db, cache) when a trait-based abstraction is viable. Keep the core transport-agnostic; put adapters behind features.

## Folder cheatsheet

```
src/
├── lib.rs                 # crate entry; re-exports public API
├── <submodule>/
│   └── mod.rs
├── error.rs               # thiserror-based error type
tests/                     # integration tests (one file = one scenario)
benches/                   # criterion benches (optional)
Cargo.toml
README.md                  # what this crate does, in 5 lines
```

## Common tasks

- "Add a public type" → define in the appropriate submodule, re-export from `lib.rs`, add doc comment with example.
- "Add an optional capability" → guard with a feature flag in `Cargo.toml`; default-off if it pulls heavy deps.
- "Release a new version" → bump `Cargo.toml` version, update `CHANGELOG.md`, `metaphor build && metaphor test`, then publish.

## Key files to read before editing

- `Cargo.toml` — dependencies, features, MSRV.
- `src/lib.rs` — the crate's public surface.
- `README.md` — stated purpose (don't drift from it).

## Deeper knowledge (load on demand)

- Skill: `api-and-interface-design` — when shaping the public API.
- Skill: `crate-maintainer` — versioning, feature design, MSRV policy.
- Skill: `code-simplification` — keeping surfaces small.
- Skill: `performance-optimization` — measure-first; don't preemptively optimize.

## Anti-patterns

- Adding a `main.rs` (wrong project type).
- Re-exporting an entire dependency as your own public API (creates tight coupling).
- Heavy default features that force users to pull deps they don't need.
- Domain logic leaking into a crate intended as plumbing.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
