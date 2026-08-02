# Veloren セットアップガイド（macOS）

ソースコードからビルドして起動するまでの手順を、実際に引っかかったポイントを含めて時系列で記載する。

- 対象OS: macOS（Apple Silicon / aarch64）
- 参考: https://wiki.veloren.net/wiki/Main_Page / https://gitlab.com/veloren/veloren
- 作業ディレクトリ: 任意（以下では `/path/to/workspace` と表記）

---

## 事前確認：必要なツール

| ツール | 用途 | 備考 |
|--------|------|------|
| Homebrew | パッケージ管理 | 未インストールなら https://brew.sh |
| git | リポジトリのクローン | Homebrew で管理推奨 |
| git-lfs | 大容量アセットの取得 | **必須・忘れやすい** |
| cmake | ネイティブライブラリのビルド | Homebrew でインストール |
| Rust（rustup） | Velorenのコンパイル | stable ではなく nightly が必要 |

---

## 手順

### 1. Homebrew の確認

```bash
brew --version
```

未インストールの場合は https://brew.sh の指示に従ってインストールする。

---

### 2. cmake のインストール

macOS では cmake が必須。

```bash
brew install cmake
```

---

### 3. git-lfs のインストールと初期設定

> **引っかかりポイント①**
> Veloren のリポジトリは画像・音声などのアセットを Git LFS で管理している。
> **git-lfs なしでクローンすると LFS ファイルがダウンロードされず、
> チェックアウトが失敗して不完全なディレクトリが残る。**
> git-lfs のインストールと `git lfs install` は**クローン前に必ず行う**。

```bash
brew install git-lfs
git lfs install   # グローバル設定への登録（1回だけ実行すればよい）
```

成功すると `Git LFS initialized.` と表示される。

---

### 4. Rust のインストール（rustup）

> **引っかかりポイント②**
> Veloren はリポジトリに `rust-toolchain` ファイルを持ち、
> **特定の nightly バージョン**を指定している。
> rustup を使わず Rust をインストールした場合、toolchain の切り替えができない。
> 必ず rustup 経由でインストールすること。

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
```

インストール後、カレントシェルに PATH を通す。

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

恒久的に有効にしたい場合は `~/.zshrc`（または `~/.bash_profile`）に追記する。

```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
```

インストールの確認。

```bash
rustc --version
cargo --version
```

---

### 5. nightly ツールチェーンのインストール

> **引っかかりポイント③**
> `rust-toolchain` ファイルに書かれたバージョンが存在しないと、
> ビルド時に自動インストールが走るが、明示的に入れておくと確実。
> ファイルの内容はリポジトリのバージョンによって変わる。

```bash
# リポジトリルートの rust-toolchain ファイルで確認
cat /path/to/workspace/veloren/rust-toolchain
# 例: nightly-2026-06-13

rustup toolchain install nightly-2026-06-13 --target aarch64-apple-darwin
```

---

### 6. リポジトリのクローン

> **引っかかりポイント④（git-lfs 未設定の場合の失敗例）**
> git-lfs が設定されていない状態でクローンすると以下のエラーが出る。
>
> ```
> git-lfs filter-process: git-lfs: command not found
> fatal: the remote end hung up unexpectedly
> warning: Clone succeeded, but checkout failed.
> ```
>
> この場合、中途半端なディレクトリが残る。
> git-lfs をインストールしてから `rm -rf veloren` で削除し、**再クローン**する。

git-lfs の設定が完了した状態でクローンする（約400MB、LFS込み）。

```bash
cd /path/to/workspace
git clone https://gitlab.com/veloren/veloren.git
```

LFS ファイルのフィルタリングが走り `Filtering content: 100% (6375/6375)` のように表示されれば成功。

---

### 7. ビルドと起動

> **引っかかりポイント⑤（PATH が通っていない場合）**
> インストール直後のシェルセッションでは `~/.cargo/bin` が PATH に含まれていないことがある。
> `cargo: command not found` になった場合は以下を実行してから再試行する。
>
> ```bash
> export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:$PATH"
> ```

```bash
cd /path/to/workspace/veloren
cargo run
```

初回ビルドは依存クレートが非常に多いため**相当な時間**がかかる（数百クレートをコンパイルする）。
`Compiling veloren-voxygen ...` が表示された後、ゲームウィンドウが起動する。

#### 起動成功の目安となるログ

```
INFO veloren_server: Server version: <hash> [<date>]
INFO veloren_voxygen::singleplayer: Starting server-cli...
INFO veloren_voxygen::singleplayer: Client connected!
```

---

### 8. サーバーのみ起動する場合

ゲームクライアントを起動せず、サーバーCLIだけを立ち上げたい場合。

```bash
cargo run --bin veloren-server-cli
```

---

## 2回目以降の起動

ビルド済みバイナリが `target/debug/` に残っているため、差分コンパイルのみで起動できる。

```bash
cd /path/to/workspace/veloren
cargo run
```

---

## 引っかかりポイント まとめ

| # | 問題 | 原因 | 対処 |
|---|------|------|------|
| ① | クローンが失敗し不完全なディレクトリが残る | git-lfs が未インストール | クローン前に `brew install git-lfs && git lfs install` |
| ② | nightly の切り替えができない | rustup を使わずに Rust をインストールした | rustup 経由でインストールし直す |
| ③ | 必要な nightly toolchain がない | `rust-toolchain` に指定されたバージョンが未インストール | `rustup toolchain install <version>` で明示的にインストール |
| ④ | `cargo: command not found` | インストール後に PATH が反映されていない | `export PATH="$HOME/.cargo/bin:$PATH"` を実行（または .zshrc に追記） |
| ⑤ | 中途半端なクローンが残った場合の復旧 | git-lfs 未設定でクローンした | `rm -rf veloren` して git-lfs 設定後に再クローン |

---

## 動作確認環境

- macOS Darwin 25.5.0（Apple Silicon / aarch64）
- Homebrew 経由: git, git-lfs 3.7.1, cmake 4.4.2
- Rust: rustup 経由 nightly-2026-06-13（rustc 1.98.0-nightly）
- Veloren: commit c424a2a8（2026-08-01）
