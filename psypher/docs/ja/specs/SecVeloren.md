# sec-veloren 配布構成仕様

本家Veloren(GitLab: `veloren/veloren`)とは別に、GitHub上の `fujimoon/sec-veloren` へコードを配布するための構成。アセット(画像・音声・`.vox`モデル等、Git LFS管理・約423MB)はGitHubのLFS無料枠(1リポジトリあたりストレージ1GB/帯域1GB・月)を消費しないよう、**GitLab側のLFSに残したまま**にする。

## 1. 構成

- `origin` = `https://gitlab.com/veloren/veloren.git` … 本家。アセットの実体(LFSオブジェクト)はここにのみ存在する。
- `sec` = `https://github.com/fujimoon/sec-veloren.git` … コードのみ。`.gitattributes`でLFS管理されている拡張子(`*.png` `*.vox` `*.ogg` 等)は**全コミット履歴から除外**されており、LFSポインタも一切含まれない。ただし`psypher/docs/images/`配下は例外で、実体ごと`sec`側のLFSストレージにもpushされ、通常のLFS管理ファイルとしてGitHub上でも閲覧できる。
- 実行時は `VELOREN_ASSETS` 環境変数で、GitLab側チェックアウトの `assets/` ディレクトリを明示的に指定する。

```
[GitLab: veloren/veloren]           ← 本物のアセット(LFS実体)がここにのみ存在
        │ git pull && git lfs pull
        ▼
ローカル veloren/ (このリポジトリ)   --- assets/ は本物 ---
        │ psypher/scripts/push-to-sec.sh
        │  (= 一時クローンを作り、LFS対象拡張子を全履歴からgit-filter-repoで除外してpush)
        ▼
[GitHub: fujimoon/sec-veloren]      ← コードのみ(LFSポインタも含まない)
        │ psypher/scripts/clone-sec-veloren.sh
        │  (= GIT_LFS_SKIP_SMUDGE=1 git clone ...)
        ▼
ローカル sec-veloren/                --- assets/ ディレクトリ自体が存在しない ---
        │ VELOREN_ASSETS=<GitLab側チェックアウトのassets> を指定して起動
        ▼
      ゲーム起動(本物のアセットを読む)
```

### なぜLFSポインタも許されないのか(GH008)

最初は「コード＋LFSポインタ(実体なし)」だけをpushする案(`GIT_LFS_SKIP_PUSH=1`)を試みたが、**GitHubはサーバー側で「参照先の実体を持たないLFSポインタ」を含むpushを拒否する**(`GH008: unknown Git LFS objects`)。GitLabと違い、GitHubは「ポインタだけ置いて実体は別サーバーに置く」運用を許可していない。そのため、ポインタごと全コミット履歴から除外する方式に切り替えている。

## 2. 使い方

### 2.1 sec-veloren への push(このリポジトリ側で実行)

```bash
./psypher/scripts/push-to-sec.sh [branch名。省略時は現在のブランチ]
```

これ1つで完結する。内部の処理:

1. 対象ブランチだけの独立した一時クローンを作成(`GIT_LFS_SKIP_SMUDGE=1`、`--no-local` — 元のこのリポジトリには一切影響しない)
2. `.gitattributes` からLFS管理パターンを読み取り、`git-filter-repo` の`--filename-callback`で、例外パス(`EXCEPT_PATH_PREFIXES`、既定は`psypher/docs/images/`)配下を除く全コミット履歴からLFS対象ファイルを除外(ポインタも実体も無くなる)
3. 除外後、例外パス以外にLFS対象ファイルが残っていないことを確認
4. 例外パス配下のLFS実体を、このリポジトリのローカルLFSキャッシュ(`.git/lfs/objects/`)から取得し、`git lfs push --object-id`で`sec`側のLFSストレージへ個別にアップロード
5. `sec`(GitHub)へpush
6. 一時クローンを削除

例外パスを増減させたい場合は、`push-to-sec.sh`内の`EXCEPT_PATH_PREFIXES`配列を編集する。

前提として `git-filter-repo` の導入が必要(初回のみ):

```bash
brew install git-filter-repo
```

**素の `git push sec <branch>` を直接打たないこと。** LFS実体がそのままアップロードされる(GitHubの無料枠を消費する)か、履歴全体に必要なLFS実体がローカルに揃っておらず`missing or corrupt local objects`エラーで失敗する。

### 2.2 sec-veloren を新規クローンする側

```bash
./psypher/scripts/clone-sec-veloren.sh [クローン先ディレクトリ名。省略時は sec-veloren]
```

`sec-veloren`側には`assets/`ディレクトリ自体が存在しない(LFS対象拡張子のファイルが全履行歴から除外されているため)。`GIT_LFS_SKIP_SMUDGE=1`はそのための安全策(将来誰かがsec-veloren側に直接画像等を追加した場合の誤動作防止)。

### 2.3 起動時のアセット指定

```bash
export VELOREN_ASSETS=/path/to/gitlab-veloren-clone/assets
cd sec-veloren
cargo run --bin veloren-voxygen
```

direnvを使う場合は、リポジトリルートの `.envrc.example` を `.envrc` としてコピーし、パスを自分の環境に合わせて編集後 `direnv allow` する(`.envrc` 自体は `.gitignore` 済みで各自ローカルに置く)。

## 3. なぜ `VELOREN_ASSETS_OVERRIDE` ではないのか

起動時のカナリアチェック(`common/assets/src/fs.rs`)は常に `VELOREN_ASSETS`(既定のベースパス、`common/assets/src/lib.rs` の `ASSETS_PATH`)を見る。`VELOREN_ASSETS_OVERRIDE` は既定パスに加えて**追加で**読みに行く上書き元であり、既定パスの代わりにはならない。既定パスがsec-veloren内の(存在しない)`assets/`を指したままだと、`VELOREN_ASSETS_OVERRIDE` を設定していてもカナリアチェックの時点でパニックする。したがって**必ず `VELOREN_ASSETS` を使うこと**。

## 4. 注意点

- GitHub側(`sec-veloren`)単体をcloneしただけではゲームは起動しない。`assets/`ディレクトリ自体が存在しないため、必ずGitLab側のフルチェックアウト(`git lfs pull`済み)を別途用意し、`VELOREN_ASSETS` で指すこと。
- `authc`(認証クライアント、`server/Cargo.toml` / `client/Cargo.toml`)や `conrod_core`(`voxygen/Cargo.toml`)は `cargo build` 時に本家GitLab(`gitlab.com/veloren/auth.git` 等)へ直接git依存としてアクセスする。sec-veloren単体で本家インフラから完全に独立するわけではない。
- 拡張子がLFS対象パターンに一致するファイルは、原則として用途を問わず`sec-veloren`側の履歴から除外される。README等の画像リンクはGitHub上では表示されない(GitLab側でのみ閲覧可能)。**例外は`psypher/docs/images/`配下**(`push-to-sec.sh`の`EXCEPT_PATH_PREFIXES`)で、ここは実体ごと`sec`側のLFSストレージにもpushされるため、GitHub上でも通常通り表示・閲覧できる。この実体pushにはローカルの`.git/lfs/objects/`キャッシュを使うため、対象ファイルを事前に`git lfs pull`済みの状態(通常の開発環境であれば自動的に満たされている)で実行すること。
- `sec-veloren`側のコミットハッシュは`origin`(GitLab)と一致しない(除外処理で全コミットが書き換わるため)。
- 履歴を書き換える処理のため、`push-to-sec.sh`実行には多少時間がかかる(veloren全履歴で数十秒程度)。
- **再実行時のコミットハッシュは、以前のpush結果と一致する保証がない**(クローン範囲や実行時の状況次第で、過去分を含めて全コミットのハッシュが変わりうる)。そのため `push-to-sec.sh` は毎回 `git push --force` で`sec`側の`master`を上書きする。**`sec-veloren`のmasterはこのスクリプトだけが更新するものとし、他から直接pushしないこと**(force pushで消えるため)。
