# 仕様書

本リポジトリで本家Velorenに追加した機能の設計/仕様ドキュメントの一覧。ゲーム自体のビルド・起動手順は [Setup.md](Setup.md) を参照。

## 仕様一覧

- [Terminal](specs/Terminal.md) — voxygenのデバッグ用egui UIに追加した、本物のシェルが動く半透明ターミナルの仕様。
- [OS Dungeon](specs/OsDungeon.md) — サーバーのファイルシステムをVeloren本体の3Dゲーム世界内に歩けるダンジョンとして生成する機能の仕様。
- [Tracer](specs/Tracer.md) — サーバー側の生成結果をスクリーンショット無しで検証できるようにする、構造化デバッグログ(JSON Lines)の仕様。
- [sec-veloren 配布構成](SecVeloren.md) — 大きなアセットはGitLab側のGit LFSに残したまま、GitHubの `fujimoon/sec-veloren` リポジトリでコードを配布する構成。
