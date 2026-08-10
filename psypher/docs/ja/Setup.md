# セットアップと起動方法

ソースコードのクローンからビルド・起動までの手順。macOS向けのより詳細な手順（実際に引っかかったポイント付き）は [SetUpForMac.md](../../../SetUpForMac.md) を、GitHub上の `sec-veloren` を使う場合の構成は [SecVeloren.md](SecVeloren.md) を参照。

## 1. 前提ツール

| ツール | 用途 | 備考 |
|--------|------|------|
| git | リポジトリのクローン | |
| git-lfs | 大容量アセットの取得 | **必須。クローン前に `git lfs install` まで済ませておくこと** |
| Rust（rustup） | Velorenのコンパイル | `rust-toolchain` で指定された **nightly** が必要。stableでは不可 |
| cmake | ネイティブライブラリのビルド（macOS等） | |
| mold（Linux）/ mimalloc関連ライブラリ（Windows） | リンカ | `.cargo/config.toml` で自動設定される。未導入だとリンクエラーになりうる |

```bash
# macOS
brew install git-lfs cmake
git lfs install

# rustup（未導入なら）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
export PATH="$HOME/.cargo/bin:$PATH"
```

## 2. クローン

### 2.1 本家(GitLab)から通常クローンする場合

```bash
git clone https://gitlab.com/veloren/veloren.git
cd veloren
```

`git lfs install` を済ませてからクローンすること。済ませていない状態でクローンすると、LFSファイルが取得できずチェックアウトが失敗し、不完全なディレクトリが残る。その場合は一度 `rm -rf veloren` で削除し、`git lfs install` 実行後に再クローンする。

`Filtering content: 100% (6375/6375)` のような表示が出ればLFS込みで正常に取得できている（約400MB）。

### 2.2 sec-veloren(GitHub)からクローンする場合

こちらはコード＋LFSポインタのみで、アセットの実体は含まれていない。手順・アセットの指定方法は [SecVeloren.md](SecVeloren.md) を参照。

```bash
./psypher/scripts/clone-sec-veloren.sh
```

## 3. nightly toolchainの導入

```bash
cat rust-toolchain   # 例: nightly-2026-06-13
rustup toolchain install $(cat rust-toolchain)
```

rustupを使わずRustを入れている場合、`rust-toolchain`ファイルによる自動切り替えができないため、rustup経由で入れ直すこと。

## 4. ビルド・起動

### 4.1 クライアント（ゲーム本体）を起動

```bash
cd veloren   # または sec-veloren
cargo run
```

初回ビルドは依存クレートが非常に多く、相当な時間がかかる。以下のようなログが出て、シングルプレイヤー用の内部サーバーが立ち上がりゲームウィンドウが表示されれば成功。

```
INFO veloren_server: Server version: <hash> [<date>]
INFO veloren_voxygen::singleplayer: Starting server-cli...
INFO veloren_voxygen::singleplayer: Client connected!
```

sec-velorenを使っている場合は、起動前に必ず `VELOREN_ASSETS` を設定する（[SecVeloren.md](SecVeloren.md) 参照）。設定していないとアセット読み込みチェック(カナリアチェック)でパニックする。

### 4.2 サーバーのみ起動する場合

```bash
cargo run --bin veloren-server-cli
```

`.cargo/config.toml` にはよく使う組み合わせのエイリアスも用意されている（例: `cargo server`＝`cargo run --bin veloren-server-cli`、`cargo test-voxygen`＝hot-reloading等を有効にした開発用起動）。詳細はリポジトリルートの `.cargo/config.toml` の `[alias]` セクションを参照。

### 4.3 2回目以降の起動

`target/` にビルド済みバイナリが残るため、差分コンパイルのみで済む。

```bash
cd veloren
cargo run
```

## 5. 関連ドキュメント

- [SetUpForMac.md](../../../SetUpForMac.md) — macOS向けの詳細セットアップ手順とトラブルシューティング表
- [SecVeloren.md](SecVeloren.md) — GitHub(`sec-veloren`)へのコード配布構成とアセットの扱い
- [Terminal.md](specs/Terminal.md) — デバッグ用の半透明ターミナル機能
