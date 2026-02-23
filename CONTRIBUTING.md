# Contributing to Abigail

Thank you for your interest in contributing to Abigail. This guide explains how to set up a development environment, submit changes, and follow our project conventions.

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

# Build Rust workspace
cargo build

# Install frontend dependencies (one-time)
cd tauri-app/src-ui && npm install && cd ../..

# Launch with hot-reload
cargo tauri dev
```

For Docker-based development, see [documents/HOW_TO_RUN_LOCALLY.md](documents/HOW_TO_RUN_LOCALLY.md).

## Development Workflow

### Branching Strategy

- **`main`** is the protected default branch.
- Create **short-lived feature branches** from `main` for all work.
- Branch naming: `feat/description`, `fix/description`, `chore/description`.

### Making Changes

1. Create a feature branch from `main`.
2. Make your changes with clear, focused commits.
3. Ensure all checks pass locally before pushing.
4. Open a pull request against `main`.

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
- Follow existing patterns in `tauri-app/src-ui/src/`

### Running Checks Locally

```bash
# Rust formatting
cargo fmt --all -- --check

# Rust linting (excludes abigail-app which needs Tauri)
cargo clippy --workspace --exclude abigail-app -- -D warnings

# Check abigail-app compiles
cargo check -p abigail-app

# Rust tests (excludes abigail-app)
cargo test --workspace --exclude abigail-app

# Frontend build
cd tauri-app/src-ui && npm run build

# Frontend tests with coverage
cd tauri-app/src-ui && npm run test:coverage
```

## Pull Request Process

1. Fill out the PR template completely.
2. Link related issues using `Closes #123` or `Fixes #123`.
3. Ensure CI passes (formatting, clippy, tests, frontend build, security audit).
4. Request review from a code owner.
5. Address review feedback with additional commits (do not force-push during review).
6. A maintainer will merge once approved and CI is green.

### PR Checklist

- [ ] Code compiles without warnings (`cargo clippy --workspace --exclude abigail-app -- -D warnings`)
- [ ] Tests pass (`cargo test --workspace --exclude abigail-app`)
- [ ] Formatting is correct (`cargo fmt --all -- --check`)
- [ ] Tauri app compiles (`cargo check -p abigail-app`)
- [ ] Frontend builds (`cd tauri-app/src-ui && npm run build`)
- [ ] Documentation updated if needed
- [ ] No secrets or sensitive data in the diff

## Architecture Overview

See [CLAUDE.md](CLAUDE.md) for a detailed architecture reference, including crate responsibilities, the security boundary between capabilities and skills, and the Id/Ego routing model.

## Reporting Issues

- **Bugs**: Use the [bug report template](https://github.com/jbcupps/abigail/issues/new?template=bug_report.yml)
- **Features**: Use the [feature request template](https://github.com/jbcupps/abigail/issues/new?template=feature_request.yml)
- **Security**: See [SECURITY.md](.github/SECURITY.md) -- do NOT open public issues for vulnerabilities

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
