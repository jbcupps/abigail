# GitHub Repository Settings Checklist

Manual configuration steps for the GitHub repository. These settings cannot be managed in code and must be configured via the GitHub web UI.

## Branch Protection Rules (Settings > Branches)

### `main` branch

- [ ] **Require a pull request before merging**
  - [x] Require approvals: 1
  - [x] Dismiss stale pull request approvals when new commits are pushed
- [ ] **Require status checks to pass before merging**
  - [x] Require branches to be up to date before merging
  - Required checks:
    - `gate` (from ci.yml -- aggregates lint, test, frontend, audit, codeql)
- [ ] **Require conversation resolution before merging**
- [ ] **Do not allow bypassing the above settings**
- [ ] **Restrict force pushes**
- [ ] **Restrict deletions**

## Security Settings (Settings > Code security and analysis)

- [ ] **Dependency graph**: Enabled
- [ ] **Dependabot alerts**: Enabled
- [ ] **Dependabot security updates**: Enabled
- [ ] **Secret scanning**: Enabled
- [ ] **Push protection**: Enabled (blocks commits containing detected secrets)
- [ ] **CodeQL analysis**: Enabled (configured via `codeql` job in `.github/workflows/ci.yml`)

## Repository Settings (Settings > General)

- [ ] **Description**: "Abigail - Sovereign Entity platform with Hive/Entity daemon architecture, constitutional integrity, Ed25519 verification, and multi-provider LLM routing"
- [ ] **Topics**: `tauri`, `rust`, `react`, `desktop-app`, `ai-agent`, `llm`, `ed25519`, `local-first`, `sovereign-entity`
- [ ] **Website**: Set to GitHub Pages URL or releases page
- [ ] **Social preview**: Upload a social preview image (1280x640 recommended)
- [ ] **Features**:
  - [x] Issues enabled
  - [x] Projects enabled (optional)
  - [x] Discussions enabled (optional, for community Q&A)

## GitHub Pages (Settings > Pages)

- [ ] **Source**: Deploy from a branch
- [ ] **Branch**: `main`, folder `/docs`
- [ ] Verify the download page loads at the configured URL

## Actions Settings (Settings > Actions > General)

- [ ] **Actions permissions**: Allow all actions and reusable workflows (or restrict to verified creators)
- [ ] **Fork pull request workflows**: Require approval for first-time contributors
- [ ] **Workflow permissions**: Read repository contents (default). The `release.yml` workflow explicitly sets `contents: write`.

## Secrets (Settings > Secrets and variables > Actions)

Required repository secrets:

- [ ] `NPM_TOKEN` -- npm access token for publishing `abigail-desktop` package

## Collaborators and Teams (Settings > Collaborators)

- [ ] Verify CODEOWNERS file is recognized (`.github/CODEOWNERS`)
- [ ] Add any additional collaborators or teams as needed

## Environments (Settings > Environments)

Optional. If you want deployment protection:

- [ ] Create a `production` environment with required reviewers for release workflows

---

Last reviewed: 2026-02-21
