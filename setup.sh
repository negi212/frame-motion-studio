#!/usr/bin/env bash
# FRAME MOTION STUDIO - 環境セットアップスクリプト
# libclang のシンボリックリンクを作成（libclang-dev が未インストールな環境向け）
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LIBCLANG_DIR="$SCRIPT_DIR/.libclang"
mkdir -p "$LIBCLANG_DIR"

# 既に存在すればスキップ
if [ -f "$LIBCLANG_DIR/libclang.so" ]; then
  echo "found $LIBCLANG_DIR/libclang.so"
  exit 0
fi

# 候補を探索
CANDIDATES=(
  "/usr/lib/x86_64-linux-gnu/libclang-21.so.21"
  "/usr/lib/x86_64-linux-gnu/libclang-18.so.18"
  "/usr/lib/llvm-21/lib/libclang.so.1"
  "/usr/lib/llvm-18/lib/libclang.so.1"
  "/usr/lib/llvm-21/lib/libclang-21.so.1"
)

FOUND=""
for c in "${CANDIDATES[@]}"; do
  if [ -f "$c" ]; then
    FOUND="$c"
    break
  fi
done

if [ -z "$FOUND" ]; then
  FOUND=$(find /usr -name "libclang-*.so*" -type f 2>/dev/null | head -n 1)
fi

if [ -n "$FOUND" ]; then
  echo "Linking $FOUND -> $LIBCLANG_DIR/libclang.so"
  ln -sf "$FOUND" "$LIBCLANG_DIR/libclang.so"
  ls -l "$LIBCLANG_DIR/libclang.so"
else
  echo "libclang not found. Please install: sudo apt install libclang-dev clang libopencv-dev"
  exit 1
fi

echo "Setup done. You can now run: cargo run"
