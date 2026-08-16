# Tracer(構造化デバッグログ) 仕様書

サーバー側で生成した内容(ボクセルの座標、実際に読み戻した地形など)を、人間がゲーム画面を見てスクリーンショットで報告する代わりに、**ファイルに直接書き出して検証できるようにする**ための汎用デバッグ基盤。[OS Dungeon](OsDungeon.md) の開発中、通路の出入り口が塞がっているかどうかの確認に何度もスクリーンショットのやり取りが必要になったことがきっかけで追加した。

## 1. 目的

- ゲームクライアントの画面を見なくても、サーバー側の処理が何を生成した(しようとした)かを直接ファイルから確認できるようにする
- 特に、`State::set_block()` で書き込んだ「つもり」の内容と、実際に地形へ反映された内容が食い違うケース(通路のバグなど)を、スクリーンショットの目視ではなく座標の突き合わせで機械的に確認できるようにする
- Veloren本体・特定機能に依存しない、汎用的なJSON Linesロガーとして実装している。現状は[OS Dungeon](OsDungeon.md)機能のみが利用しているが、将来別の機能のデバッグにも転用できる

## 2. 使い方

Rustコードから次の2関数を呼び出すだけ:

```rust
// 1回のデバッグセッションの開始時に、古いログを消す
psypher_trace::clear();

// 任意の箇所でイベントを1行追記する。第2引数は Serialize を実装していれば
// 何でもよい(serde_json::json! マクロで作ったオブジェクトなど)
psypher_trace::log("room", serde_json::json!({
    "label": "current",
    "center": [17200, 10096, 819],
    "half": [10, 10, 6],
}));
```

出力先は `psypher/trace/dungeon_trace.jsonl`(このクレート自身のソースディレクトリを指す絶対パス。詳細は「3. アーキテクチャ」参照)で、1行1オブジェクトのJSON Lines形式。各行には呼び出し順に増える連番 `seq` と、呼び出し時に指定した `kind`(イベント種別、例: `"room"` `"corridor"` `"doorway"` `"probe"`)に加え、渡した任意のフィールドがそのまま展開されて入る。

ゲームクライアントを一切必要とせず、テキストエディタや `jq`、あるいは他のエージェント/スクリプトから直接読める。[OS Dungeon](OsDungeon.md) では、生成した部屋・通路・扉の座標や `/osdungeon_probe` コマンドで読み戻した実際の地形ブロック種別をこの仕組みで記録し、スクリーンショットのやり取り無しでバグ(通路のバグ、テレポート順序のバグ)を特定した。

## 3. アーキテクチャ

```
psypher-trace (独立クレート, psypher/trace/)
  ├─ src/lib.rs     … 薄いエントリポイント。pub use tracer::{clear, log};
  └─ src/tracer.rs  … 本体実装
       ├─ clear()  … トレースファイルを空にし、連番をリセット
       └─ log()    … JSON1行を追記
```

- 依存クレートは `serde` / `serde_json` のみ。Veloren固有の型は一切知らない
- ファイルI/Oは `std::sync::Mutex` で直列化しており、複数スレッド・複数呼び出し元から同時に呼んでも壊れない
- 各行の `seq` はプロセス内のグローバルな連番(`AtomicU64`)。壁時計時刻ではなく単調増加カウンタにしているのは、時刻取得ができない/避けたい実行環境でも呼び出し順序だけは常に正しく再現できるようにするため
- ファイル書き込みに失敗しても(例: ディレクトリが無い、権限が無い等)エラーを握りつぶし、呼び出し元の処理には一切影響を与えない(デバッグ用の仕組みが本来の機能を壊してはいけないという設計方針)
- `clear()` / `log()` のどちらも呼ばれるたびに `OpenOptions::create(true)` でディレクトリ・ファイルの存在を都度保証している
- 出力先パス(`TRACE_FILE`)は `concat!(env!("CARGO_MANIFEST_DIR"), "/dungeon_trace.jsonl")` — このクレート自身の `Cargo.toml` があるディレクトリ(ビルド時に一度だけ解決される絶対パス)を指す。呼び出し元プロセスの実行時カレントディレクトリには一切依存しない

### 修正履歴: カレントディレクトリ依存だった頃の不具合

当初は `TRACE_FILE` を `"psypher/trace/dungeon_trace.jsonl"` という相対パスで持っていた。これは `cargo run` でリポジトリルートから起動する分には問題なかったが、`cargo test -p veloren-server`(テスト実行時のカレントディレクトリは `server/`)から呼び出すと、意図とは別の場所に `server/psypher/trace/dungeon_trace.jsonl` という重複ディレクトリが作られてしまっていた。呼び出し元(OS Dungeon機能の自動テスト)を追加した際に発覚し、`CARGO_MANIFEST_DIR` を使った絶対パスに変更して解消した。

## 4. 変更ファイル

- [`psypher/trace/Cargo.toml`](../../../trace/Cargo.toml) — crate定義。`serde` / `serde_json` のみに依存
- [`psypher/trace/src/lib.rs`](../../../trace/src/lib.rs) — `pub use tracer::{clear, log};` のみの薄いエントリポイント
- [`psypher/trace/src/tracer.rs`](../../../trace/src/tracer.rs) — 本体実装

利用側:

- `server/Cargo.toml` — `psypher-trace` へのpath依存
- [`psypher/server/src/os_dungeon/os_dungeon.rs`](../../../server/src/os_dungeon/os_dungeon.rs) — 各種イベント(`enter`/`exit`/`navigate`/`render_layout`/`room`/`corridor`/`doorway`/`punch_doorway`/`probe`)の記録元。詳細は [OsDungeon.md](OsDungeon.md) を参照

## 5. 既知の制限

- 常時有効なデバッグ機構で、配布ビルドから除外するフィーチャーフラグ等は未整備(利用側である[OS Dungeon](OsDungeon.md)と同様の課題)
- ファイルサイズの上限や自動ローテーションが無い。呼び出し側が `clear()` を呼ばない限り追記され続ける
- 単体テストが無い(`clear`/`log` それぞれの正常系・ファイルI/O失敗時の挙動を検証するテストは未整備)
- 出力先(`psypher/trace/dungeon_trace.jsonl`)はビルド時に絶対パスとして固定される(上記の修正履歴参照)ため、実行のたびに変わるデバッグ出力であってもソースツリー配下に生成される。git管理からは除外済み(`.gitignore`)だが、リポジトリの外(例: 一時ディレクトリ)に出力先を切り替えることは検討の余地がある

## 6. セキュリティ上の位置づけ

- クライアントからは一切到達できない、サーバープロセス内部でのみ完結するデバッグ機構
- 呼び出し元(現状は[OS Dungeon](OsDungeon.md))が記録する内容次第では、サーバーのファイルシステム構造など機微な情報がログファイルに残る。ログファイル自体はOSのファイルパーミッションにのみ依存し、ゲーム内の権限システムとは無関係に読めるため、配布運用を考える際はログの出力自体を無効化できるようにする必要がある(OsDungeon.mdの「セキュリティ上の位置づけ」も参照)
