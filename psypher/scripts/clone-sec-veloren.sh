#!/usr/bin/env bash
# sec-veloren (GitHub) を、LFS実体を取得せずにクローンする。
# 実アセットはこのスクリプトでは取得しない。別途 GitLab 側の veloren を
# clone(+ git lfs pull)しておき、起動時に VELOREN_ASSETS でそちらを指すこと。
#
# 詳細: psypher/docs/ja/SecVeloren.md
set -euo pipefail

REPO_URL="https://github.com/fujimoon/sec-veloren.git"
DEST="${1:-sec-veloren}"

echo "==> $REPO_URL を $DEST にクローンします (LFS実体は取得しません)"
GIT_LFS_SKIP_SMUDGE=1 git clone "$REPO_URL" "$DEST"

cat <<EOF

==> クローン完了: $DEST
    アセットの実体は含まれていません(LFSポインタのみ)。

実行するには、GitLab側 veloren の assets ディレクトリを VELOREN_ASSETS で
指定してください(VELOREN_ASSETS_OVERRIDE ではありません。詳細はドキュメント参照):

  export VELOREN_ASSETS=/path/to/gitlab-veloren-clone/assets
  cd $DEST
  cargo run --bin veloren-voxygen

EOF
