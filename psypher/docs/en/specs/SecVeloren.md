# sec-veloren Distribution Layout

A setup for distributing code via a GitHub repository, `fujimoon/sec-veloren`, separate from upstream Veloren (GitLab: `veloren/veloren`). Assets (images, audio, `.vox` models, etc. — managed with Git LFS, roughly 423MB) are kept **on GitLab's LFS only**, so they don't eat into GitHub's free LFS quota (1GB storage / 1GB bandwidth per month, per repository).

## 1. Layout

- `origin` = `https://gitlab.com/veloren/veloren.git` — upstream. The actual LFS objects (asset binaries) live only here.
- `sec` = `https://github.com/fujimoon/sec-veloren.git` — code only. Every extension managed by Git LFS in `.gitattributes` (`*.png`, `*.vox`, `*.ogg`, etc.) is **stripped from the entire commit history**, so it contains no LFS pointers either.
- At runtime, the `VELOREN_ASSETS` environment variable is set explicitly to the `assets/` directory of a GitLab-side checkout.

```
[GitLab: veloren/veloren]           ← the real asset binaries (LFS objects) live only here
        │ git pull && git lfs pull
        ▼
local veloren/ (this repo)          --- assets/ is real ---
        │ psypher/scripts/push-to-sec.sh
        │  (= build a temp clone, strip LFS-tracked extensions from all
        │     history with git-filter-repo, then push)
        ▼
[GitHub: fujimoon/sec-veloren]      ← code only (not even LFS pointers)
        │ psypher/scripts/clone-sec-veloren.sh
        │  (= GIT_LFS_SKIP_SMUDGE=1 git clone ...)
        ▼
local sec-veloren/                  --- the assets/ directory doesn't exist at all ---
        │ launch with VELOREN_ASSETS=<GitLab-side checkout's assets>
        ▼
      the game starts (reading the real assets)
```

### Why even LFS pointers aren't allowed (GH008)

The original plan was to push code plus LFS **pointers** only, with no binary content (`GIT_LFS_SKIP_PUSH=1`). That doesn't work: **GitHub's server rejects any push containing LFS pointers whose referenced objects it doesn't have** (`GH008: unknown Git LFS objects`). Unlike GitLab, GitHub does not support "pointers here, real objects hosted elsewhere." So instead, the pointers themselves are stripped from history entirely.

## 2. Usage

### 2.1 Pushing to sec-veloren (run from this repo)

```bash
./psypher/scripts/push-to-sec.sh [branch name; defaults to the current branch]
```

This one command does everything:

1. Creates an isolated temporary clone of just the target branch (`GIT_LFS_SKIP_SMUDGE=1`, `--no-local` — this repo is never touched)
2. Reads the LFS-managed patterns from `.gitattributes` and strips them from every commit in history with `git-filter-repo` (removing both the binaries and the pointers)
3. Verifies zero LFS-tracked files remain
4. Pushes to `sec` (GitHub)
5. Deletes the temporary clone

Prerequisite, once: `git-filter-repo`.

```bash
brew install git-filter-repo
```

**Never run a bare `git push sec <branch>` directly.** It either uploads the full LFS payload to GitHub (eating into the free quota) or fails with `missing or corrupt local objects` because the full history's LFS objects aren't all cached locally.

### 2.2 Cloning sec-veloren fresh

```bash
./psypher/scripts/clone-sec-veloren.sh [destination dir; defaults to sec-veloren]
```

`sec-veloren` has no `assets/` directory at all (every LFS-tracked extension was stripped from all of history). `GIT_LFS_SKIP_SMUDGE=1` is a safety net here, in case someone later adds an image directly to sec-veloren.

### 2.3 Pointing at real assets at runtime

```bash
export VELOREN_ASSETS=/path/to/gitlab-veloren-clone/assets
cd sec-veloren
cargo run --bin veloren-voxygen
```

If you use direnv, copy the repo root's `.envrc.example` to `.envrc`, edit the path for your machine, then run `direnv allow` (`.envrc` itself is already gitignored, so it stays local to each developer).

## 3. Why `VELOREN_ASSETS`, not `VELOREN_ASSETS_OVERRIDE`

The canary check at startup (`common/assets/src/fs.rs`) always reads from `VELOREN_ASSETS` — the default base path (`ASSETS_PATH` in `common/assets/src/lib.rs`). `VELOREN_ASSETS_OVERRIDE` is an *additional* source consulted on top of the default, not a replacement for it. Since the default path in sec-veloren points at an `assets/` directory that doesn't exist, the canary check panics at startup regardless of what `VELOREN_ASSETS_OVERRIDE` is set to. **Always use `VELOREN_ASSETS`.**

## 4. Caveats

- Cloning `sec-veloren` alone is not enough to run the game — there's no `assets/` directory in it at all. You always need a full GitLab-side checkout (with `git lfs pull` already run) and must point `VELOREN_ASSETS` at it.
- `authc` (auth client, in `server/Cargo.toml` / `client/Cargo.toml`) and `conrod_core` (`voxygen/Cargo.toml`) are pulled as git dependencies directly from upstream GitLab (`gitlab.com/veloren/auth.git`, etc.) at `cargo build` time. sec-veloren does not make the build fully independent of upstream infrastructure.
- Any file matching an LFS-managed extension pattern — including things like `psypher/images/logo.png` or `psypher/docs/images/terminal.png` — gets stripped from `sec-veloren`'s history regardless of purpose. Image links in docs/README won't render on GitHub (they're only viewable on the GitLab side).
- Commit hashes on `sec-veloren` do not match `origin` (GitLab), since every commit is rewritten during the strip.
- Because history is rewritten, `push-to-sec.sh` takes some time to run (tens of seconds, given Veloren's full history).
- **Re-running the script is not guaranteed to reproduce the same commit hashes as a previous push** — depending on clone scope and other conditions at run time, even old, content-identical commits can hash differently. Because of this, `push-to-sec.sh` always `git push --force`es `sec`'s `master`. Treat `sec-veloren`'s `master` as owned exclusively by this script — never push to it directly from elsewhere, since a subsequent run will force-overwrite it.
