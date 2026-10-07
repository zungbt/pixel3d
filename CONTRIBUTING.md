# Contributing

## 1. Branches

- `main` always runs. **Never commit directly to `main`.**
- One branch per feature/fix, cut from the latest `main`:

  | Prefix | Use for |
  |---|---|
  | `feat/<name>` | New features (`feat/zoom-out`, `feat/dirt-path`) |
  | `fix/<name>` | Bug fixes (`fix/grass-under-rocks`) |
  | `refactor/<name>` | Code restructuring with no behaviour change |
  | `docs/<name>` | Documentation only |

  Branch names: lowercase, words joined with `-`.

```bash
git switch main && git pull
git switch -c feat/<name>
```

## 2. Commits

- One idea per commit, message in English; the first line briefly says *what changed*:
  - `Zoom out: VIEW_HEIGHT 13 -> 18`
  - `fix: vColor is vec4 in three r186`
- Before committing: `npm run build` must pass, and check the scene by eye with `npm run dev` (visual changes have no automated tests).

## 3. Merging into `main`

Merge with `--no-ff` so history keeps each feature branch:

```bash
git switch main && git pull
git merge --no-ff feat/<name> -m "merge feat/<name>: <summary>"
git push origin main
git branch -d feat/<name>
```

## 4. Versioning (SemVer)

Versions take the form `MAJOR.MINOR.PATCH` and live in **two places that must always match**: `version` in `package.json` and the git tag `vX.Y.Z`.

| Bump | When | Example |
|---|---|---|
| `PATCH` | Bug fixes, small tweaks (colours, parameters) | `0.1.0` → `0.1.1` |
| `MINOR` | New features | `0.1.1` → `0.2.0` |
| `MAJOR` | Large or breaking changes; `1.0.0` = first official release | `0.9.0` → `1.0.0` |

Not every merge needs a version — batching several features into one release is fine.

## 5. Releasing

On `main`, once everything is merged:

```bash
git switch main && git pull
npm version minor -m "release v%s"   # or patch / major
git push origin main --tags
```

`npm version` updates `package.json` + `package-lock.json`, creates a `release vX.Y.Z` commit and the `vX.Y.Z` tag.

## 6. Looking up versions

```bash
git tag -n                                  # list versions
git switch --detach v0.1.0                  # inspect an old version (back: git switch main)
git diff v0.1.0 v0.2.0                      # compare two versions
git log --merges --oneline v0.1.0..v0.2.0   # features/fixes included in a release
```
