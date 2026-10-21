# Contributing to Abigail

Thank you for your interest in contributing to Abigail. This guide explains how to set up a development environment, submit changes, and follow our project conventions.

Abigail is a framework for creating and hosting persistent AI Entities, not a single-agent app. The active product is two desktop apps and two daemons: `hive-app` + `hive-daemon` create, birth, sign, and supervise Entities, and `entity-runtime-app` + `entity-daemon` run each Entity against its own `entity_<UUID>` database in the Hive-owned `memory.db`. See the [README](README.md) and [`docs/architecture.md`](docs/architecture.md). The retired single-agent app is isolated under [`legacy/`](legacy/README.md) and is not part of the workspace or CI.

## Getting Started

### Prerequisites

- **Rust** stable toolchain (managed via `rust-toolchain.toml`)
- **Node.js** 20+
- **OS dependencies** required by Tauri 2.0 (see platform-specific notes below)

### Platform-Specific Dependencies

**Windows**: Install NSIS for building installers (`choco install nsis`).

**Ubuntu/Debian**:

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libappindicator3-dev \
  libayatana-appindicator3-dev librsvg2-dev \
  patchelf libssl-dev libgtk-3-dev
```

**macOS**: Xcode Command Line Tools (`xcode-select --install`).

### Development Setup

```bash
# Clone the repository
git clone https://github.com/jbcupps/abigail.git
cd abigail

# Install frontend dependencies (one-time)
npm ci --prefix hive-app/src-ui
npm ci --prefix entity-runtime-app/src-ui

# Build the daemons and the Runtime app that Hive launches
cargo build -p hive-daemon -p entity-daemon -p abigail-entity-runtime-app

# Serve the Runtime UI for debug Entity windows, then launch Hive with hot-reload
npm run dev --prefix entity-runtime-app/src-ui &
cd hive-app && cargo tauri dev
```

For Docker-based development, see [documents/HOW_TO_RUN_LOCALLY.md](documents/HOW_TO_RUN_LOCALLY.md).

## Development Workflow

### Branching Strategy

- **`beta`** is the permanent iteration branch. Every push to `beta` produces a tagged `vX.Y.Z-beta.N` installer prerelease for UAT.
- **`main`** is the protected stable branch. It receives validated changes promoted from `beta`, plus explicitly requested stable hotfixes.
- Create **short-lived feature branches** for all work. Branch naming: `feat/description`, `fix/description`, `chore/description`.

### Making Changes

1. Create a feature branch from `beta` (or from `main` for an approved stable hotfix).
2. Make your changes with clear, focused commits.
3. Ensure all checks pass locally before pushing.
4. Open a pull request against the branch you started from.

### Commit Conventions

We use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` -- new feature
- `fix:` -- bug fix
- `chore:` -- maintenance, dependency updates
- `docs:` -- documentation only
- `refactor:` -- code change that neither fixes a bug nor adds a feature
- `test:` -- adding or updating tests
- `ci:` -- CI/CD changes

Example: `feat: add skill event filtering by category`

### Code Style

**Rust:**

- Format with `cargo fmt`
- Lint with `cargo clippy -- -D warnings`
- All public items should have doc comments

**TypeScript/React (frontend):**

- Use TypeScript for all new frontend code
- Follow existing patterns in `hive-app/src-ui/src/` and `entity-runtime-app/src-ui/src/`
- Follow the design system in [`docs/design/README.md`](docs/design/README.md)

### Running Checks Locally

```bash
# Rust formatting
cargo fmt --all -- --check

# Rust linting
cargo clippy --workspace -- -D warnings

# Hive and Entity Runtime desktop apps compile
cargo check -p abigail-hive-app -p abigail-entity-runtime-app

# Rust tests
cargo test --workspace

# Frontend builds and tests
(cd hive-app/src-ui && npm run build && npm test)
(cd entity-runtime-app/src-ui && npm run build && npm test)

# Release-lane and security-claim guards
node scripts/check_unsigned_stabilization.mjs
node scripts/check_crypto_claims.mjs
```

`node scripts/check_stability.mjs` runs the Windows stability subset in one step.

## Pull Request Process

1. Fill out the PR template completely.
2. Link related issues using `Closes #123` or `Fixes #123`.
3. Ensure CI passes (formatting, clippy, tests, frontend build, security audit).
4. Request review from a code owner.
5. Address review feedback with additional commits (do not force-push during review).
6. A maintainer will merge once approved and CI is green.

### PR Checklist

- [ ] Code compiles without warnings (`cargo clippy --workspace -- -D warnings`)
- [ ] Tests pass (`cargo test --workspace`)
- [ ] Formatting is correct (`cargo fmt --all -- --check`)
- [ ] Desktop apps compile (`cargo check -p abigail-hive-app -p abigail-entity-runtime-app`)
- [ ] Both frontends build and test (`hive-app/src-ui`, `entity-runtime-app/src-ui`)
- [ ] Documentation updated if needed
- [ ] No secrets or sensitive data in the diff

## Architecture Overview

- [README.md](README.md): how Hive, the Entity Runtime, and the shared store fit together, the Entity lifecycle, and the framework crates.
- [docs/architecture.md](docs/architecture.md): process boundaries, persistence scopes, and the skill topology.
- [CLAUDE.md](CLAUDE.md) and [AGENTS.md](AGENTS.md): project rules and the active implementation plan.

## Reporting Issues

- **Bugs**: Use the [bug report template](https://github.com/jbcupps/abigail/issues/new?template=bug_report.yml)
- **Features**: Use the [feature request template](https://github.com/jbcupps/abigail/issues/new?template=feature_request.yml)
- **Security**: See [SECURITY.md](.github/SECURITY.md) -- do NOT open public issues for vulnerabilities

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
