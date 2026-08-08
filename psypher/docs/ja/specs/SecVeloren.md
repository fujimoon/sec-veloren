# sec-veloren 配布構成仕様

本家Veloren(GitLab: `veloren/veloren`)とは別に、GitHub上の `fujimoon/sec-veloren` へコードを配布するための構成。アセット(画像・音声・`.vox`モデル等、Git LFS管理・約423MB)はGitHubのLFS無料枠(1リポジトリあたりストレージ1GB/帯域1GB・月)を消費しないよう、**GitLab側のLFSに残したまま**にする。

## 1. 構成

- `origin` = `https://gitlab.com/veloren/veloren.git` … 本家。アセットの実体(LFSオブジェクト)はここにのみ存在する。
- `sec` = `https://github.com/fujimoon/sec-veloren.git` … コードとLFS**ポインタ**(数百バイトの参照情報)のみ。アセット実体はpushしない。
- 実行時は `VELOREN_ASSETS` 環境変数で、GitLab側チェックアウトの `assets/` ディレクトリを明示的に指定する。

```
[GitLab: veloren/veloren]           ← 本物のアセット(LFS実体)がここにのみ存在
        │ git pull && git lfs pull
        ▼
ローカル veloren/ (このリポジトリ)   --- assets/ は本物 ---
        │ psypher/scripts/push-to-sec.sh
        │  (= GIT_LFS_SKIP_PUSH=1 git push sec <branch>)
        ▼
[GitHub: fujimoon/sec-veloren]      ← コード + LFSポインタのみ(実体なし)
        │ psypher/scripts/clone-sec-veloren.sh
        │  (= GIT_LFS_SKIP_SMUDGE=1 git clone ...)
        ▼
ローカル sec-veloren/                --- assets/ はポインタのみ ---
        │ VELOREN_ASSETS=<GitLab側チェックアウトのassets> を指定して起動
        ▼
      ゲーム起動(本物のアセットを読む)
```

## 2. 使い方

### 2.1 sec-veloren への push(このリポジトリ側で実行)

```bash
./psypher/scripts/push-to-sec.sh [branch名。省略時は現在のブランチ]
```

内部で `GIT_LFS_SKIP_PUSH=1 git push sec <branch>` を実行する。この環境変数により `pre-push` フックのLFSアップロードが無効化され、コードとLFSポインタ(テキスト)だけが送られる。**素の `git push sec <branch>` を直接打つと、LFS実体(423MB)がGitHubに送られてしまうので使わないこと。**

### 2.2 sec-veloren を新規クローンする側

```bash
./psypher/scripts/clone-sec-veloren.sh [クローン先ディレクトリ名。省略時は sec-veloren]
```

内部で `GIT_LFS_SKIP_SMUDGE=1 git clone ...` を実行する。GitHub側にLFS実体が無いため、smudge(ポインタ→実体変換)を試みるとエラーになりうるので、あらかじめ無効化してポインタのまま残す。

### 2.3 起動時のアセット指定

```bash
export VELOREN_ASSETS=/path/to/gitlab-veloren-clone/assets
cd sec-veloren
cargo run --bin veloren-voxygen
```

direnvを使う場合は、リポジトリルートの `.envrc.example` を `.envrc` としてコピーし、パスを自分の環境に合わせて編集後 `direnv allow` する(`.envrc` 自体は `.gitignore` 済みで各自ローカルに置く)。

## 3. なぜ `VELOREN_ASSETS_OVERRIDE` ではないのか

起動時のカナリアチェック(`common/assets/src/fs.rs`)は常に `VELOREN_ASSETS`(既定のベースパス、`common/assets/src/lib.rs` の `ASSETS_PATH`)を見る。`VELOREN_ASSETS_OVERRIDE` は既定パスに加えて**追加で**読みに行く上書き元であり、既定パスの代わりにはならない。既定パスがsec-veloren内の(ポインタのみの)`assets/`を指したままだと、`VELOREN_ASSETS_OVERRIDE` を設定していてもカナリアチェックの時点でパニックする。したがって**必ず `VELOREN_ASSETS` を使うこと**。

## 4. 注意点

- GitHub側(`sec-veloren`)単体をcloneしただけではゲームは起動しない。アセット実体が無いため、必ずGitLab側のフルチェックアウト(`git lfs pull`済み)を別途用意し、`VELOREN_ASSETS` で指すこと。
- `authc`(認証クライアント、`server/Cargo.toml` / `client/Cargo.toml`)や `conrod_core`(`voxygen/Cargo.toml`)は `cargo build` 時に本家GitLab(`gitlab.com/veloren/auth.git` 等)へ直接git依存としてアクセスする。sec-veloren単体で本家インフラから完全に独立するわけではない。
- GitHubのLFS無料枠(1GBストレージ/1GB帯域・月)を圧迫しないよう、`sec` へのpushは必ず `psypher/scripts/push-to-sec.sh`(= `GIT_LFS_SKIP_PUSH=1`)を経由すること。
