#!/usr/bin/env bash
# sec-veloren (GitHub) へ、コード + Git LFS ポインタのみを push する。
# アセットの実体(LFSバイナリ、約423MB)は GitLab (origin) 側に残したままにし、
# GitHub 側の LFS 無料枠(1GBストレージ/1GB帯域・月)を消費しないようにする。
#
# 詳細: psypher/docs/ja/specs/SecVeloren.md
set -euo pipefail

REMOTE="sec"
BRANCH="${1:-$(git branch --show-current)}"

if ! git remote get-url "$REMOTE" >/dev/null 2>&1; then
  echo "エラー: remote '$REMOTE' が登録されていません。先に以下を実行してください:" >&2
  echo "  git remote add $REMOTE https://github.com/fujimoon/sec-veloren.git" >&2
  exit 1
fi

echo "==> $REMOTE (`git remote get-url "$REMOTE"`) へ push します"
echo "==> branch: $BRANCH"
echo "==> LFS実体はアップロードしません (GIT_LFS_SKIP_PUSH=1)"

GIT_LFS_SKIP_PUSH=1 git push "$REMOTE" "$BRANCH"

echo "==> 完了。GitHub側にはコードとLFSポインタのみが乗っています。"
echo "    実アセットは引き続き GitLab (origin) の LFS にのみ存在します。"
