# Setup & Launch

Steps from cloning the source to building and launching the game. For a more detailed macOS walkthrough (with real gotchas encountered along the way, in Japanese), see [SetUpForMac.md](../../../SetUpForMac.md). For the layout used when distributing code via GitHub's `sec-veloren`, see [SecVeloren.md](specs/SecVeloren.md).

## 1. Prerequisites

| Tool | Purpose | Notes |
|------|---------|-------|
| git | cloning the repo | |
| git-lfs | fetching large assets | **Required. Run `git lfs install` before cloning** |
| Rust (rustup) | compiling Veloren | Requires the **nightly** version pinned by `rust-toolchain`; stable won't work |
| cmake | building native libraries (macOS, etc.) | |
| mold (Linux) / mimalloc-related libs (Windows) | linker | Auto-configured via `.cargo/config.toml`; missing them can cause link errors |

```bash
# macOS
brew install git-lfs cmake
git lfs install

# rustup (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
export PATH="$HOME/.cargo/bin:$PATH"
```

## 2. Cloning

### 2.1 A normal clone from upstream (GitLab)

```bash
git clone https://gitlab.com/veloren/veloren.git
cd veloren
```

Make sure `git lfs install` has been run before cloning. If it hasn't, LFS files won't be fetched and the checkout fails, leaving a partial directory behind. In that case, delete it (`rm -rf veloren`), run `git lfs install`, and clone again.

A message like `Filtering content: 100% (6375/6375)` indicates the LFS content (roughly 400MB) was fetched successfully.

### 2.2 Cloning sec-veloren (GitHub) instead

This contains code and LFS pointers only — no real asset binaries. See [SecVeloren.md](specs/SecVeloren.md) for the full procedure and how assets are supplied.

```bash
./psypher/scripts/clone-sec-veloren.sh
```

## 3. Installing the nightly toolchain

```bash
cat rust-toolchain   # e.g. nightly-2026-06-13
rustup toolchain install $(cat rust-toolchain)
```

If Rust was installed without rustup, the automatic toolchain switch driven by `rust-toolchain` won't work — reinstall via rustup.

## 4. Building & running

### 4.1 Launching the client (the game itself)

```bash
cd veloren   # or sec-veloren
cargo run
```

The first build compiles a very large number of dependency crates and takes a while. Success looks like the following log lines, followed by the game window appearing (an internal server starts for singleplayer):

```
INFO veloren_server: Server version: <hash> [<date>]
INFO veloren_voxygen::singleplayer: Starting server-cli...
INFO veloren_voxygen::singleplayer: Client connected!
```

If you're working from sec-veloren, set `VELOREN_ASSETS` before launching (see [SecVeloren.md](specs/SecVeloren.md)) — without it, the asset-loading canary check panics at startup.

### 4.2 Running only the server

```bash
cargo run --bin veloren-server-cli
```

`.cargo/config.toml` also defines aliases for common combinations (e.g. `cargo server` = `cargo run --bin veloren-server-cli`, `cargo test-voxygen` = a dev launch with hot-reloading and other dev features enabled). See the `[alias]` section of the repo root's `.cargo/config.toml` for the full list.

### 4.3 Subsequent launches

Build artifacts remain in `target/`, so later runs only need to recompile the diff.

```bash
cd veloren
cargo run
```

## 5. Related docs

- [SetUpForMac.md](../../../SetUpForMac.md) — detailed macOS setup steps and a troubleshooting table (Japanese)
- [SecVeloren.md](specs/SecVeloren.md) — the GitHub (`sec-veloren`) code distribution layout and how assets are handled
- [Terminal.md](specs/Terminal.md) — the debug semi-transparent terminal feature
