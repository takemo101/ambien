# Ambien の開発用コマンド。`just` で一覧を表示する。

# 検証用の HOME。本物の設定（~/Library/Application Support 配下）を触らない。
private_home := "/tmp/ambien-home"

default:
    @just --list

# release ビルド（macOS 15 を配備下限にする）
build:
    MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --release --locked

# release ビルドして、普段の設定で起動する
run: build
    ./target/release/ambien

# release ビルドして、一時 HOME で起動する（設定・保存を本物と分ける）
try: build
    mkdir -p '{{private_home}}'
    HOME='{{private_home}}' ./target/release/ambien

# 一時 HOME を消して、初回起動の状態に戻す
reset-try:
    rm -rf '{{private_home}}'

# ad-hoc 署名の .app と DMG を dist/ に作る（Release と同じ手順）
package:
    scripts/package-app.sh

# fmt・clippy・テスト・文書検査
check:
    cargo fmt --all -- --check
    cargo clippy --all-targets --locked -- -D warnings
    cargo test --locked
    npm run check:docs
