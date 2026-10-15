# Contributing to Context Pilot

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md). Report vulnerabilities privately per [SECURITY.md](SECURITY.md), never in a public issue.

Read [README.md](README.md) first for the architecture, and [docs/README.md](docs/README.md) for which design docs are current.

## Setup

| Tool | Version | Notes |
| --- | --- | --- |
| Rust | **1.97.1**, pinned in `rust-toolchain.toml` | rustup picks it up automatically. CI reads the same version (`RUST_VERSION` in `.github/workflows/ci.yml` and `release.yml`); change all three together. |
| Node.js + pnpm | Node **22**, pnpm **11.6.0** (as in CI) | Frontend only. Always `pnpm`, never npm/yarn. |

```bash
cargo build --release                 # agent (tui) + all crates
cargo build --release -p cp-orchestrator
cd web && pnpm install && pnpm dev    # web cockpit (Vite)
```

Environment variables are typed specs in `crates/cp-env`; [docs/ENV.md](docs/ENV.md) is generated from them. Adding a `CP_*` variable means: a spec in `crates/cp-env/src/specs/`, a model field, regenerating ENV.md (`cargo test -p cp-env --test generate -- --ignored`) and a line in each `deploy/env/*.example`. An unknown variable fails boot.

## Rules the checks enforce

Every rule below is a script in `.github/checks/` and a CI job. Run the script locally before pushing; a red check is a hard stop.

| Rule | Script |
| --- | --- |
| `cargo fmt`, `cargo clippy -D warnings` against the workspace lint table (~1,000 lints, most at `forbid`) | `check-rust-lints.sh` |
| `cargo test --workspace` | `check-rust-tests.sh` |
| Files ≤ 500 lines, directories ≤ 8 entries (Rust and `web/src`), hash chain intact | `check-structure.sh` |
| `openapi.json` matches Rust routes and types; the generated TS client has no drift; no hand-written `fetch` | `check-api-contract.sh` |
| eslint, prettier, stylelint, `tsc -b`, type coverage, suppression and rule-census budgets, knip | `check-ts-lints.sh` |
| `web/src/mobile-components` mirrors `web/src/components` path for path, with no imports leaking across trees | `check-mobile-mirror.sh` |

**Lint suppressions.** `#[allow(...)]` is banned. When a lint truly cannot be satisfied, use `#[expect(lint, reason = "...")]` and add an entry (path, match, reason, strategy) to `.github/checks/allowed-lint-exceptions.yaml`; frontend suppressions go in `allowed-eslint-exceptions.yaml`. An unregistered suppression fails the build. Prefer fixing the code: split the function, or use `cp_macros::deref_match!` for reference patterns.

**Structure caps.** When a file nears 500 lines, factor code into a new file. Don't trim comments or docs to fit.

**Hash chain.** The lint config (`Cargo.toml` lint sections, `clippy.toml`, `rustfmt.toml`), the check scripts, the exception registries and the deploy scripts are listed in `.github/checks/protected-files.yaml` and sealed by a password-protected hash chain. Editing any of them breaks the chain until a maintainer runs `./chain.sh`. Contributors propose such changes in the PR; they cannot lower the bar alone.

**Blocking callbacks.** Inside Context Pilot, the same checks run as blocking edit callbacks: `*.rs` edits run the lint, structure and API-contract families, and `web/src/**` edits run the TS, mirror and structure families. An edit that breaks a rule is reported immediately.

## Code conventions

- **Backend owns logic.** All business logic, filesystem access, env handling and data shaping live in Rust. The frontend renders what the backend returns.
- **Docs.** Every public item has a doc comment that says something the signature doesn't. Changes to behaviour update the matching doc in `docs/` (and its status in `docs/README.md`).
- **Frontend.** Every new desktop component has a mobile twin at the same path (`pnpm mirror:scaffold` generates a stub). No entry animations on main content.
- **Commits.** Imperative subject, one logical change per commit. Never commit `.context-pilot/shared/entities/`.

## Pull requests

1. Branch from the current working branch, not `master`.
2. Keep the PR focused; describe what changed and why, and link the task or issue.
3. All CI jobs green: Rust, Structure, Contract, Frontend.
4. Changes to protected files need a maintainer to re-seal the chain.

## Testing

```bash
cargo test --workspace                # everything CI runs
cargo test -p <crate> <name>          # one crate / one test
cd web && pnpm build                  # type-checks the frontend
```

Frontend e2e (`check-ts-tests.sh`) is a stub until an ephemeral test stack exists.

## License

Contributions are licensed under the [MIT License](LICENSE).
