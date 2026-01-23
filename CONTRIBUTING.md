# Contributing to QBZ

This project is actively evolving. Contributions are welcome, but we have a few rules to keep releases stable and avoid regressions (especially around audio output).

## Quick rules

- Write clear, concise English (no emojis in code, comments, or commit messages).
- Keep PRs focused and small when possible.
- Do not change app branding or legal disclaimers without discussing it first.
- Do not modify protected audio-backend behavior unless explicitly requested by the maintainer.

## Branch naming

We use a consistent branch naming scheme:

`<type>/<scope>/<branch_name>`

- `type`: `feature` | `bugfix` | `hotfix` | `chore` | `docs`
- `scope`: `internal` (maintainer work) | `external` (PR author branch name when checked out locally)

Examples:

- `feature/internal/offline-cache-encryption`
- `bugfix/internal/login-footer-alignment`
- `docs/internal/contributing-process`
- `feature/external/add-album-to-playlist`

## PR acceptance workflow (no direct merges to main)

We do not merge external PR branches directly into `main`. Instead, we create an internal integration branch to:

- re-run checks on top of the latest `main`
- detect conflicts early
- keep a clean review trail for the maintainer

### Procedure (maintainer)

1. **Triage**
   - Confirm scope and that it does not touch protected areas (audio routing/backends, credential storage, etc.) unless requested.
2. **Check out the PR**
   - `gh pr checkout <PR_NUMBER>`
3. **Rename the checked-out branch (local)**
   - `git branch -m feature/external/<topic>`
4. **Create an integration branch from upstream main**
   - `git fetch origin main`
   - `git checkout -b feature/internal/pr-<PR_NUMBER>-<topic> origin/main`
5. **Merge the external branch into the integration branch**
   - `git merge --no-ff feature/external/<topic>`
6. **Run checks**
   - Frontend: `npm run build`
   - Backend (when Rust changes): `cargo check` (run from `src-tauri/`)
7. **Push the integration branch (do not merge to main yet)**
   - `git push -u origin feature/internal/pr-<PR_NUMBER>-<topic>`

After this, you can either:

- open a PR from the integration branch to `main`, or
- merge the integration branch to `main` locally when you are ready.

## What to include in PRs

- A short description of the problem and solution.
- Screenshots for UI changes when possible.
- Notes about any breaking changes or migrations.

## What not to include

- Large refactors mixed with feature work.
- Changes that reintroduce removed UI/UX patterns (for example, exporting offline cache files).

