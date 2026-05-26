# Contributing to agent-start

Thanks for your interest! agent-start is a Rust host + Vite+ SPA. This document
covers the setup you need before opening a PR.

## Toolchain

- **Rust** (stable, ≥ `rust-version` declared in `server-rs/Cargo.toml`)
- **Vite+ CLI (`vp`)** — bundles `vite` / `vitest` / `oxlint` / `oxfmt` / `tsgo`
  / a Node runtime. Install once:

  ```bash
  curl -fsSL https://vite.plus | bash    # macOS / Linux
  # irm https://vite.plus/ps1 | iex      # Windows
  vp env                                 # verify
  ```

You do **not** need a separate Node / npm install for the front-end side; `vp`
provides everything.

## Building

```bash
git clone https://github.com/imanect-labs/agent-start
cd agent-start

# Front (SPA bundle)
cd front && vp install && vp build && cd ..

# Host (single binary, SPA gets embedded via rust-embed)
cd server-rs && cargo build --release && cd ..
```

The release host is at `server-rs/target/release/agent-start-host`. Run with
`--port 3030` and open `http://localhost:3030`.

## Running locally (dev mode, two processes)

```bash
# Terminal A
npm run dev:host                # Rust host on :3030

# Terminal B
(cd front && vp install)        # first time only
npm run dev:front               # Vite+ SPA on :5173 with hot reload
```

Open <http://localhost:5173>.

## Tests, lint, format

| Area     | Command                                                      |
| -------- | ------------------------------------------------------------ |
| Rust fmt | `cd server-rs && cargo fmt --all`                            |
| Rust lint| `cd server-rs && cargo clippy --all-targets -- -D warnings`  |
| Rust test| `cd server-rs && cargo test --all`                           |
| Rust audit| `cd server-rs && cargo audit`                               |
| Front type| `cd front && vp check`                                      |
| Front lint| `cd front && vp run oxlint`                                 |
| Front fmt | `cd front && vp run oxfmt`                                  |
| Front build| `cd front && vp build`                                     |

CI (`.github/workflows/rust.yml`, `.github/workflows/web.yml`) runs the same
checks on every PR — please make sure they pass locally first.

## Pull requests

1. Fork the repo and create a feature branch from `main`.
2. Keep commits focused. [Conventional Commits](https://www.conventionalcommits.org/)
   are encouraged but not enforced.
3. Update [CHANGELOG.md](./CHANGELOG.md) under the `## [Unreleased]` section
   for user-visible changes.
4. Open a PR using the template; describe the motivation, the change, and how
   you tested it.
5. Make sure CI is green. A maintainer will review.

## Filing issues

Use the issue templates in `.github/ISSUE_TEMPLATE/`. For security
vulnerabilities **do not** open a public issue — follow [SECURITY.md](./SECURITY.md)
instead.

## Code of conduct

By participating you agree to the [Contributor Covenant Code of Conduct](./CODE_OF_CONDUCT.md).
