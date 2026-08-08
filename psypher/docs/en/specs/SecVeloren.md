# sec-veloren Distribution Layout

A setup for distributing code via a GitHub repository, `fujimoon/sec-veloren`, separate from upstream Veloren (GitLab: `veloren/veloren`). Assets (images, audio, `.vox` models, etc. — managed with Git LFS, roughly 423MB) are kept **on GitLab's LFS only**, so they don't eat into GitHub's free LFS quota (1GB storage / 1GB bandwidth per month, per repository).

## 1. Layout

- `origin` = `https://gitlab.com/veloren/veloren.git` — upstream. The actual LFS objects (asset binaries) live only here.
- `sec` = `https://github.com/fujimoon/sec-veloren.git` — code and LFS **pointers** only (small text references). Asset binaries are never pushed here.
- At runtime, the `VELOREN_ASSETS` environment variable is set explicitly to the `assets/` directory of a GitLab-side checkout.

```
[GitLab: veloren/veloren]           ← the real asset binaries (LFS objects) live only here
        │ git pull && git lfs pull
        ▼
local veloren/ (this repo)          --- assets/ is real ---
        │ psypher/scripts/push-to-sec.sh
        │  (= GIT_LFS_SKIP_PUSH=1 git push sec <branch>)
        ▼
[GitHub: fujimoon/sec-veloren]      ← code + LFS pointers only (no binaries)
        │ psypher/scripts/clone-sec-veloren.sh
        │  (= GIT_LFS_SKIP_SMUDGE=1 git clone ...)
        ▼
local sec-veloren/                  --- assets/ is pointer-only ---
        │ launch with VELOREN_ASSETS=<GitLab-side checkout's assets>
        ▼
      the game starts (reading the real assets)
```

## 2. Usage

### 2.1 Pushing to sec-veloren (run from this repo)

```bash
./psypher/scripts/push-to-sec.sh [branch name; defaults to the current branch]
```

Internally runs `GIT_LFS_SKIP_PUSH=1 git push sec <branch>`. The env var disables the LFS upload step of the `pre-push` hook, so only code and LFS pointers (plain text) are sent. **Do not run a bare `git push sec <branch>` directly** — that would upload the full LFS payload (423MB) to GitHub.

### 2.2 Cloning sec-veloren fresh

```bash
./psypher/scripts/clone-sec-veloren.sh [destination dir; defaults to sec-veloren]
```

Internally runs `GIT_LFS_SKIP_SMUDGE=1 git clone ...`. Since GitHub has no LFS objects for this repo, attempting to smudge (resolve pointers into real content) would error out, so smudging is disabled up front and pointers are left as-is.

### 2.3 Pointing at real assets at runtime

```bash
export VELOREN_ASSETS=/path/to/gitlab-veloren-clone/assets
cd sec-veloren
cargo run --bin veloren-voxygen
```

If you use direnv, copy the repo root's `.envrc.example` to `.envrc`, edit the path for your machine, then run `direnv allow` (`.envrc` itself is already gitignored, so it stays local to each developer).

## 3. Why `VELOREN_ASSETS`, not `VELOREN_ASSETS_OVERRIDE`

The canary check at startup (`common/assets/src/fs.rs`) always reads from `VELOREN_ASSETS` — the default base path (`ASSETS_PATH` in `common/assets/src/lib.rs`). `VELOREN_ASSETS_OVERRIDE` is an *additional* source consulted on top of the default, not a replacement for it. If the default path still points at sec-veloren's pointer-only `assets/`, the canary check panics at startup regardless of what `VELOREN_ASSETS_OVERRIDE` is set to. **Always use `VELOREN_ASSETS`.**

## 4. Caveats

- Cloning `sec-veloren` alone is not enough to run the game — there are no real asset binaries in it. You always need a full GitLab-side checkout (with `git lfs pull` already run) and must point `VELOREN_ASSETS` at it.
- `authc` (auth client, in `server/Cargo.toml` / `client/Cargo.toml`) and `conrod_core` (`voxygen/Cargo.toml`) are pulled as git dependencies directly from upstream GitLab (`gitlab.com/veloren/auth.git`, etc.) at `cargo build` time. sec-veloren does not make the build fully independent of upstream infrastructure.
- To avoid eating into GitHub's LFS free quota (1GB storage / 1GB bandwidth per month), always push to `sec` through `psypher/scripts/push-to-sec.sh` (i.e. with `GIT_LFS_SKIP_PUSH=1`).
