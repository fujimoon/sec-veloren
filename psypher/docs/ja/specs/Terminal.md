# Terminal (半透明の実ターミナル) 仕様書

Veloren (voxygen) のデバッグ用egui UI上に追加した、**本物のシェルが動く半透明ターミナル**の仕様。

![Terminalウィンドウの動作例](../../images/terminal.png)

上図: シングルプレイヤーのワールド内で「Debug Control」→「Terminal」を有効化した状態。ゲーム画面の上に半透明のウィンドウが重なり、その中で実際の `zsh` が起動し、プロンプトが表示されている。フォーカス中のためカーソルは白い塗りつぶしブロックで表示されている。

## 1. 目的

開発者/デバッグ用途で、ゲームプロセスを終了せずに手元のシェルへ即座にアクセスできるようにする。ログ確認・ファイル操作・簡単なスクリプト実行などを、ゲームウィンドウを行き来せずに行えることを目指す。

- 対象: 開発者・デバッグ目的のみ。一般プレイヤー向け機能ではない。
- 「シェルコマンドのふりをする独自パーサ」ではなく、**実プロセスとしてシェルを起動し、実ptyに接続する**。ANSIエスケープ・カラー・vim等のTUIアプリもそのまま動く想定。

## 2. 使い方

1. シングルプレイヤー/マルチプレイヤーでワールドに入る(メインメニューでは無効)
2. `ToggleEguiDebug` キー(デフォルト: **F7**)を押して「Debug Control」ウィンドウを開く
   - **Mac注意**: ノートPCキーボードは既定でF1〜F12がメディアキーとして動くため、素のF7では反応しないことがある。**fn + F7** を押すか、システム設定 → キーボード →「F1、F2 などのキーを標準のファンクションキーとして使用」をONにする
3. 「Terminal」チェックボックスをON → 半透明のTerminalウィンドウが開き、実シェルが起動する
4. ウィンドウ内をクリックしてフォーカスすると、以降のキー入力はターミナルへ送られる(ゲーム側の操作キーには渡らない)
5. チェックを外す、またはウィンドウの✕を押すとシェルプロセスは終了する

## 3. アーキテクチャ

既存の `egui-ui` フィーチャ(デフォルト有効)上に実装。追加の描画パイプラインは不要で、既存のegui→wgpu合成にそのまま乗っている。

```
psypher-terminal (独立クレート, psypher/voxygen/egui/)
  └─ pub struct TerminalState { .. }   … Veloren を一切知らない。egui + alacritty_terminal のみに依存

veloren-voxygen-egui (voxygen/egui/)
  └─ Debug Control (既存の egui ウィンドウ)
       └─ "Terminal" checkbox
            └─ TerminalState::new()  … pty + シェルを spawn
                 └─ TerminalState::show()  … 毎フレーム、グリッドを egui::Painter で描画
```

### 疎結合な構成

実装本体は `veloren-voxygen-egui` クレートの中には置かず、[`psypher/voxygen/egui/`](../../../voxygen/egui/) に独立したクレート `psypher-terminal` として切り出している。

- `psypher-terminal` は `egui` と `alacritty_terminal` にしか依存しない。`client`/`common` はもちろん、Veloren固有の型を一切importしていない。単体でも `cargo check -p psypher-terminal` が通る、完全に自己完結したクレート
- `voxygen/egui/Cargo.toml` から `path` 依存として参照するだけで、`veloren-voxygen-egui` 側が見えるのは公開API `TerminalState::{new, show}` のみ
- veloren本体のワークスペース(`members`)には**含めていない**。パス依存としては同じ最終バイナリ(`veloren-voxygen`)のビルドグラフに乗るが、独立したパッケージとして自己完結しているため、`veloren-voxygen-egui` 側の内部実装(ECS・クライアント状態など)を変更してもこのクレートには一切影響しない

### 使用ライブラリ

| ライブラリ | 役割 |
|---|---|
| [`alacritty_terminal`](https://docs.rs/alacritty_terminal) 0.25 | Alacritty本体が使っているのと同じ pty 起動 (`tty::new`) ＋ ANSI/VTEパーサ ＋ ターミナルグリッド状態管理。バックグラウンドスレッドで pty 出力を読み、`Term` を更新する仕組みまで含めて提供される |
| `egui` (既存依存) | 半透明ウィンドウの表示、キー/テキスト入力イベントの取得、グリッドの描画(矩形・テキスト) |

`psypher-terminal` と `veloren-voxygen-egui` はどちらも `egui = "0.33"` を要求するため、最終的なビルドでは同一バージョンに解決され、型の不整合は起きない。

### シェル起動

- `alacritty_terminal::tty::new` が `$SHELL` を実行(macOSではデフォルトで `/bin/zsh` にフォールバック)
- `TERM=xterm-256color` を注入
- 初期サイズ 80x24、以後はウィンドウサイズから実測フォント寸法で列/行数を再計算し、変化があった時だけ `Term::resize` + pty へ `Msg::Resize` を送信

### 入出力

- **出力**: `EventLoop`(alacritty_terminal提供)がバックグラウンドスレッドでpty読み取り→ANSI解析→`Arc<FairMutex<Term<_>>>` を直接更新。描画側は毎フレーム `term.lock()` してグリッドを読むだけ
- **入力**: egui の `Event::Text` / `Event::Paste` / `Event::Key` を、ターミナルウィジェットがキーボードフォーカスを持っている時だけ収集し、バイト列(通常文字、`\r`、`\x7f`、矢印キーのエスケープシーケンス、Ctrl+文字の制御コード等)に変換して `Msg::Input` で pty へ送信

### 半透明表示

```rust
Frame::window(&ctx.style())
    .fill(Color32::from_rgba_unmultiplied(12, 12, 16, 200))
```

既存のegui-uiがwgpu上にアルファ合成描画されている仕組みにそのまま乗るため、追加のシェーダ変更は不要。

### 描画

`term.renderable_content()` の `display_iter` を1セルずつ走査し、`egui::Painter` で背景矩形とグリフを描画。色は `vte::ansi::Color`(Named/Indexed/Spec)から標準xterm 16色 + 256色パレット + 24階調グレースケールへ解決。`Flags::INVERSE`(反転)、`Flags::BOLD`(太字→明色化)、`Flags::*UNDERLINE`(下線)、`Flags::HIDDEN` に対応。

### カーソル表示(フォーカス連動)

実ターミナルと同じ挙動:

- **フォーカスあり**: ブロック全体を白で塗りつぶし、下の文字を反転色で再描画
- **フォーカスなし**: 白い枠線のみ(中は透明)

これはウィジェットが `ui.allocate_exact_size(..., Sense::click_and_drag())` で得た `Response` の `has_focus()` を毎フレーム見て切り替えている。クリックで `request_focus()` される。

### ゲーム操作との競合回避

追加コードなしで解決している。[`voxygen/src/run.rs`](../../../../voxygen/src/run.rs) に既存の仕組みとして「egui が消費(consume)したウィンドウイベントはゲーム入力へ渡さない」というガードがあり、ターミナルウィジェットがegui上でキーボードフォーカスを取っている間は自動的にこのガードが効く。

## 4. 変更ファイル

`psypher-terminal`(新規クレート、Veloren非依存):

- [`psypher/voxygen/egui/Cargo.toml`](../../../voxygen/egui/Cargo.toml) — クレート定義。`egui` / `alacritty_terminal` のみに依存
- [`psypher/voxygen/egui/src/lib.rs`](../../../voxygen/egui/src/lib.rs) — `pub use terminal::TerminalState;` のみの薄いエントリポイント
- [`psypher/voxygen/egui/src/terminal.rs`](../../../voxygen/egui/src/terminal.rs) — 本体実装

`veloren-voxygen-egui`(既存クレート、配線のみ):

- [`voxygen/egui/Cargo.toml`](../../../../voxygen/egui/Cargo.toml) — `psypher-terminal` へのpath依存を追加
- [`voxygen/egui/src/lib.rs`](../../../../voxygen/egui/src/lib.rs) — 「Debug Control」ウィンドウへ「Terminal」チェックボックス追加、`EguiInnerState` に `terminal: Option<TerminalState>` を追加(チェックON時に遅延生成、OFF時にDrop=シェル終了)。`psypher_terminal::TerminalState` をimportするだけで、内部実装には触れない

## 5. 既知の制限 (v1)

- スクロールバック(マウスホイールでの巻き戻し)は未実装。表示は常に最新のビューポートのみ
- 端末機能を問い合わせるDA/OSCクエリ等、一部の `PtyWrite` イベントに応答を返していない(`VoidListener` で無視)。TUIアプリの一部機能が完全に動かない可能性がある
- シェルプロセスが終了した場合、画面はそのまま固まった表示になる(終了検知・再起動導線は未実装)
- マウス操作(選択・スクロール・URLクリック等)は未対応。キーボード入力のみ

## 6. セキュリティ上の位置づけ

- `egui-ui` フィーチャ配下のクライアントローカルなデバッグ機能であり、サーバーやネットワーク越しの操作からは到達できない
- 実シェルを起動する機能であるため、配布ビルドで一般プレイヤーに晒す運用は想定していない(開発者のローカル起動を前提)
