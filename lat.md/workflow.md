# Workflow

[lat.md](https://github.com/1st1/lat.md) 0.12.2 をプロジェクト固定の開発依存として使う。[[architecture]] で入口を探し、詳細はリンク先の正本を読む。グラフと検索は仕様・Issue 状態・製品実装を管理しない。

## Setup and commands

macOS の開発環境で Node.js `^24.15.0 || >=26.0.0` と npm を使い、リポジトリルートから実行する。バージョンは [package.json](../package.json) と [lockfile](../package-lock.json) に固定する。

導入済み依存を再現するときは次を使う。グローバル導入、`npx ...@latest`、直接の `lat` 呼出しではなく、npm script 経由でローカル固定 CLI を使う。

```bash
npm ci --ignore-scripts
npm run check
npm run check:docs
npm run lat -- locate "Specification"
npm run lat -- section "architecture#Specification"
npm run lat -- refs "architecture#Specification" --scope md
```

`locate` は見出しの探索、`section` は本文と参照の読取り、`refs` は graph 内の逆参照確認に使う。通常の Markdown リンクは `refs` の対象ではない。現在の文書案内には存在しない Rust ソースへの `refs` 例を置かない。

`npm run check` は現状 `npm run check:docs` だけを実行し、`check:docs` は安全 wrapper 経由の `lat check` を実行する。アプリの build、Cargo test、fmt、Clippy の script は存在しない。CLI は参照先の古い Node 22 説明ではなく、この manifest の Node 条件に合わせる。

## Local search only

探索は `locate`／`section`／`refs` を優先する。semantic search が必要な場合だけ同梱のローカル MiniLM モデルを使い、索引と設定を Git 対象外の `lat.md/.cache/` に保存する。

```bash
npm run lat -- reindex --local
npm run lat -- search "audio world timing"
```

`npm run lat -- ...` は `LAT_LLM_KEY`、`LAT_LLM_KEY_FILE`、`LAT_LLM_KEY_HELPER` を環境から除去し、`XDG_CONFIG_HOME` を `$PWD/lat.md/.cache/config` に隔離する。既存ユーザー設定や環境の credential helper を読まない。隔離設定へ API キーを追加せず、`--remote` と外部埋め込みを使わない。

ローカルモデルと WebAssembly の処理は依存パッケージに同梱され、検索のためのモデル取得や外部埋め込み API への文書・クエリ送信を必要としない。これは npm 導入の通信まで禁止・遮断する仕組みではなく、キーなし・隔離設定・ローカル索引で運用する契約である。

`reindex --local` は `lat.md/.cache/vectors.db` の索引を再構築し、隔離された `lat.md/.cache/config/lat/config.json` に local backend の選択を記録する。`search` も必要に応じて索引を自動作成・更新するため、純粋な読取りではない。これらは CPU・メモリとローカル書込みを伴う。キャッシュ削除後には再構築が必要になる。

`npm install`／`npm ci` は registry からの取得通信と `node_modules`・npm cache への書込みを伴う。`--ignore-scripts` は依存の lifecycle script を実行しないが、取得通信を無効にするものではない。初回導入は `npm install --ignore-scripts --no-audit --no-fund` を使う。キャッシュ・依存実体・キーをリポジトリへ登録しない。

## Manual agent integration

エージェントの入口は [AGENTS.md](../AGENTS.md) へ手動で結び付ける。`lat init`、自動 hooks、Lat MCP 登録、CI は導入せず、mikan と GitButler の運用や SPEC の gate を置き換えない。

`lat check` の `No init version recorded` 警告は、`lat init` の初期化バージョンを記録しない手動構成によるもので、検査エラーではない。警告を消すために `lat init` を実行したり、初期化 metadata を偽造したりしない。その他のエラーは個別に確認する。

## Maintenance and evidence

関係する節と正本を変更前に読み、変更で古くなった案内だけを同じ作業で更新する。Lat の検査成功は文書構造の証拠に限定し、製品の意味的整合・実機受け入れ・公開準備の成功と混同しない。

- 安定した英語の見出しを使い、各見出しの直後に250文字以内の概要段落を置く。
- wiki link は `[[architecture#Specification]]` のように graph 内の navigation に使う。repo の正本文書は `[SPEC](../SPEC.md)` のような通常の相対 Markdown リンクで参照する。
- graph ファイルを追加・移動・削除したら [[lat]] の一覧と関係する参照元を更新する。正本文書を移動・複製しない。
- 仕様全文・Issue の Status・試験件数をここへ転記せず、存在しない API や将来のソースを実装済み node として置かない。
- `lat check` は wiki link、code reference、索引、概要段落の構造を検査する。通常 Markdown リンクの存在や製品要件との意味的整合まで証明しない。参照した repo 文書は別途読む。
- 文書変更の統合後に `npm run check` を実行する。実機・試聴・署名・公証・更新の確認は [SPEC](../SPEC.md) と [検証手順](../docs/verification.md) の条件に従い、別の証拠として記録する。

この導入は開発ツールと運用文書の整備だけであり、仕様全体の最終承認、実装計画、Cargo、アプリコードの開始許可を追加しない。
