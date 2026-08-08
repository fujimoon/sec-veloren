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
# ただしEXCEPT_PATH_PREFIXESに列挙したパス配下は例外として除外せず、
# 実体(バイナリ)ごと sec-veloren 側のLFSストレージへ個別にpushする
# (`git lfs push --object-id`)。これにより該当パスの画像等はGitHub上でも
# 通常のLFS管理ファイルとして表示・閲覧できる。
#
# 前提(初回のみ): brew install git-filter-repo
# 詳細: psypher/docs/ja/specs/SecVeloren.md
set -euo pipefail

REMOTE="sec"
REMOTE_URL="https://github.com/fujimoon/sec-veloren.git"
REPO_ROOT="$(git rev-parse --show-toplevel)"
BRANCH="${1:-$(git -C "$REPO_ROOT" branch --show-current)}"

# LFS管理拡張子でも、これらのパス配下は除外せず実体ごとGitHub側へ運ぶ。
# (末尾は "/" で終えること。ディレクトリ単位の前方一致でマッチする)
EXCEPT_PATH_PREFIXES=(
  "psypher/docs/images/"
)

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

echo "==> .gitattributes のLFS対象パターンを読み取り、例外パス以外を全履歴から除外"
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

# --path-glob + --invert-pathsでは「このパターンに一致するが、この配下だけは
# 残す」という例外を表現できないため、--filename-callback で
# 「例外パス配下は常に残し、それ以外でLFS対象拡張子に一致するものだけ除外する」
# というロジックに切り替える。
CALLBACK_FILE="$WORKDIR.filename_callback.py"
{
  echo "EXCEPT_PREFIXES = ["
  for p in "${EXCEPT_PATH_PREFIXES[@]}"; do
    printf "  b'%s',\n" "$p"
  done
  echo "]"
  echo "LFS_GLOBS = ["
  for g in "${LFS_GLOBS[@]}"; do
    printf "  b'%s',\n" "$g"
  done
  echo "]"
  cat <<'PYEOF'
if any(filename.startswith(p) for p in EXCEPT_PREFIXES):
    return filename
for g in LFS_GLOBS:
    if fnmatch.fnmatch(filename, g):
        return None
return filename
PYEOF
} > "$CALLBACK_FILE"

git filter-repo --force --filename-callback "$CALLBACK_FILE"
rm -f "$CALLBACK_FILE"

echo "==> 除外漏れが無いか確認(例外パス配下以外にLFS対象ファイルが残っていないか)"
REMAINING_BAD=()
REMAINING_OIDS=()
while IFS= read -r line; do
  [ -z "$line" ] && continue
  oid="${line%% *}"
  file="$(printf '%s\n' "$line" | cut -d' ' -f3-)"
  keep=0
  for p in "${EXCEPT_PATH_PREFIXES[@]}"; do
    case "$file" in
      "$p"*) keep=1 ;;
    esac
  done
  if [ "$keep" -eq 1 ]; then
    REMAINING_OIDS+=("$oid")
  else
    REMAINING_BAD+=("$file")
  fi
done < <(git lfs ls-files -l)

if [ "${#REMAINING_BAD[@]}" -ne 0 ]; then
  echo "エラー: 例外パス以外にLFS対象ファイルが残っています。push を中断します。" >&2
  printf '  %s\n' "${REMAINING_BAD[@]}" >&2
  exit 1
fi

git remote add "$REMOTE" "$REMOTE_URL"

if [ "${#REMAINING_OIDS[@]}" -ne 0 ]; then
  echo "==> 例外パス配下のLFS実体を、このリポジトリのローカルLFSキャッシュから取得して $REMOTE へ個別push"
  for oid in "${REMAINING_OIDS[@]}"; do
    src="$REPO_ROOT/.git/lfs/objects/${oid:0:2}/${oid:2:2}/${oid}"
    dst=".git/lfs/objects/${oid:0:2}/${oid:2:2}/${oid}"
    if [ ! -f "$src" ]; then
      echo "エラー: LFS実体がローカルキャッシュに見つかりません: $src" >&2
      echo "  このリポジトリ(origin)側で該当ファイルを 'git lfs pull' してから再実行してください。" >&2
      exit 1
    fi
    mkdir -p "$(dirname "$dst")"
    cp "$src" "$dst"
  done
  git lfs push "$REMOTE" --object-id "${REMAINING_OIDS[@]}"
fi

echo "==> $REMOTE ($REMOTE_URL) へ push します (branch: $BRANCH)"
# git-filter-repoによる再フィルタリングは、クローン範囲(ブランチ単体かフルか等)
# によって過去分のコミットハッシュまで変わりうる(実行毎に再現される保証がない)。
# sec-veloren の master はこのスクリプトだけが更新する派生物という位置づけなので
# force push で確定させる。sec-veloren に対して他から直接pushする運用は想定しない。
git push --force "$REMOTE" "$BRANCH:$BRANCH"

echo "==> 完了。コミットハッシュはこのリポジトリ(origin)とは一致しません(履歴書き換えのため)。"
echo "    実アセットは引き続き GitLab (origin) の LFS にのみ存在します。"
echo "    例外パス(${EXCEPT_PATH_PREFIXES[*]})配下は実体ごと $REMOTE 側のLFSにも存在します。"
