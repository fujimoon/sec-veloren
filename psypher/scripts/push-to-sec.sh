#!/usr/bin/env bash
# sec-veloren (GitHub) へ、コード本体のみを push する。
#
# GitHubはサーバー側で「LFSポインタだけで実体オブジェクトが無いpush」を
# 拒否する(GH008)。GIT_LFS_SKIP_PUSH=1 でアップロードだけスキップする方式は
# GitHubでは使えないため、代わりに .gitattributes でLFS管理されている
# 拡張子(*.png, *.vox, *.ogg 等)を、独立した一時クローン上で
# git-filter-repo により全コミット履歴から除外し、その一時クローンだけを
# push する。元のこのリポジトリ(origin=GitLab)には一切変更を加えない。
#
# 前提(初回のみ): brew install git-filter-repo
# 詳細: psypher/docs/ja/specs/SecVeloren.md
set -euo pipefail

REMOTE="sec"
REMOTE_URL="https://github.com/fujimoon/sec-veloren.git"
REPO_ROOT="$(git rev-parse --show-toplevel)"
BRANCH="${1:-$(git -C "$REPO_ROOT" branch --show-current)}"

if ! command -v git-filter-repo >/dev/null 2>&1; then
  echo "エラー: git-filter-repo が見つかりません。先に以下を実行してください:" >&2
  echo "  brew install git-filter-repo" >&2
  exit 1
fi

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

echo "==> LFS実体を取得せず、branch=$BRANCH だけの独立したフルクローンを作成: $WORKDIR"
GIT_LFS_SKIP_SMUDGE=1 git clone --no-local -q --branch "$BRANCH" --single-branch "$REPO_ROOT" "$WORKDIR"
cd "$WORKDIR"
git remote remove origin

echo "==> .gitattributes のLFS対象パターンを読み取り、全履歴から除外"
# mapfile/readarrayはbash 4+限定(macOS標準の/bin/bashは3.2のため使えない)。
# while read + プロセス置換で代用する。
LFS_GLOBS=()
while IFS= read -r g; do
  LFS_GLOBS+=("$g")
done < <(grep -E 'filter=lfs' "$REPO_ROOT/.gitattributes" | awk '{print $1}')
if [ "${#LFS_GLOBS[@]}" -eq 0 ]; then
  echo "エラー: .gitattributes からLFS対象パターンを読み取れませんでした。" >&2
  exit 1
fi

FILTER_ARGS=()
for g in "${LFS_GLOBS[@]}"; do
  FILTER_ARGS+=(--path-glob "$g")
done

git filter-repo --force "${FILTER_ARGS[@]}" --invert-paths

REMAINING="$(git lfs ls-files | wc -l | tr -d ' ')"
if [ "$REMAINING" != "0" ]; then
  echo "エラー: 除外後もLFS対象ファイルが ${REMAINING} 件残っています。push を中断します。" >&2
  git lfs ls-files >&2
  exit 1
fi

echo "==> $REMOTE ($REMOTE_URL) へ push します (branch: $BRANCH)"
# git-filter-repoによる再フィルタリングは、クローン範囲(ブランチ単体かフルか等)
# によって過去分のコミットハッシュまで変わりうる(実行毎に再現される保証がない)。
# sec-veloren の master はこのスクリプトだけが更新する派生物という位置づけなので
# force push で確定させる。sec-veloren に対して他から直接pushする運用は想定しない。
git remote add "$REMOTE" "$REMOTE_URL"
git push --force "$REMOTE" "$BRANCH:$BRANCH"

echo "==> 完了。コミットハッシュはこのリポジトリ(origin)とは一致しません(履歴書き換えのため)。"
echo "    実アセットは引き続き GitLab (origin) の LFS にのみ存在します。"
