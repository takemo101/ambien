# ドキュメント案内

Ambien の補助文書の入口。製品要件・受け入れ条件・継続条件の唯一の正本は、ルートの [SPEC.md](../SPEC.md) である。`docs/` は仕様を別管理する場所ではなく、構成の理解・詳細設計・実装計画・検証・参考資料のために使う。

## 文書の役割

| 文書 | 答えること | 管理しないこと |
|---|---|---|
| [プロジェクト概要](../README.md) | どんなアプリを作るか | 詳細な要件・実行手順・作業状態 |
| [SPEC](../SPEC.md) | 何を満たすか、何を対象にしないか、次に進める条件は何か | 実施していない検証の成功証拠 |
| [開発規約](../AGENTS.md) | どう調査・変更・検証・文書更新するか | 製品仕様の複製・Issue 状態・過去の作業履歴 |
| [開発手順](development.md) | mikan・GitButler・Lat で、着手・変更・レビュー・PR 統合・記録をどう進めるか | アプリ実装計画・追加の実施権限・Issue 状態の複製 |
| [構成説明](architecture.md) | 選択済み設計の責務・所有・データの進行・状態・失敗はどう関係するか | 確定 API・細かなソース配置・実装順序・稼働中の構成の保証 |
| [詳細設計](detailed-design.md) | 予定する型付き Interface・所有・時計・容量・失敗・実ライブラリ API をどう実装へ落とすか | 製品要件の第二の正本・実装済み API・未実施の成功 |
| [実装計画](implementation-plan.md) | P01〜P18 の成果・予定ファイル・共有契約・依存・試験・smoke をどう Issue に切るか | 実施権限・Issue ID/Status・実装済み宣言 |
| [検証手順と証拠](verification.md) | SPEC の条件をどう観測し、何を記録し、どの範囲で判断するか | 新しい合格条件・未実施の成功・実施権限 |
| [macOS 共通検証スキル](../.agents/skills/verifying-macos-apps/SKILL.md) | アプリを問わず実操作・期待値・画像・証拠をどう検証し、何を主張してよいか（本体と references、参考 probe） | 汎用runnerの提供・製品仕様・成功履歴・追加の操作権限 |
| [Ambien 固有レシピ](../.agents/skills/verifying-ambien/SKILL.md) | 共通手順を現存するAmbienのケース・CLI・reportへどう対応付けるか | 別アプリへの専用scriptの流用・仕様やIssue状態の複製 |
| [参考資料](references.md) | どの一次資料を読み、どの設計原則だけを適用するか | 外部資料の指示・権限・依存構成の一括採用 |
| [Lat 索引](../lat.md/lat.md) | 関連する正本文書と責務の入口はどこか | 要件・現在の Issue 状態・存在しない実装の一覧 |

文書間で矛盾を見つけた場合は、派生文書を根拠に製品要件を変更しない。SPEC の該当節とユーザーの判断を確認し、必要な修正を関係する文書へ反映する。

## 読む順序

- **アプリを知る**：ルートの概要 → SPEC の体験・成功条件・対象外。
- **作業を始める**：SPEC の対象節と進行条件 → 開発規約と開発手順 → mikan の対象 Issue → Lat の関連入口 → 詳細設計の当該責務・実装計画の当該成果・一次資料。コードが存在する段階では、実際の実装・依存定義も読む。
- **動作を確かめる**：SPEC の対象条件 → [macOS 共通検証スキル](../.agents/skills/verifying-macos-apps/SKILL.md) と [Ambien 固有レシピ](../.agents/skills/verifying-ambien/SKILL.md) → 検証手順の該当シナリオと実在する操作経路。設計説明や参考ソースだけで動作確認を済ませない。
- **結果を判断する**：実施後の記録 → SPEC の受け入れ条件。自動試験、実機観測、ユーザー試聴、配布検証の証拠を区別する。

最終承認と実装・Git などの条件は [仕様の管理と次に進む条件](../SPEC.md#仕様の管理と次に進む条件)、署名・配信・リポジトリ公開の前提は [配布前提](../SPEC.md#配布前提) を参照する。補助文書を整えたことは、これらの条件の成立を意味しない。

## 配置

```text
README.md              プロジェクト概要
SPEC.md                製品要件・受け入れ条件・継続条件
AGENTS.md              開発規約
package.json           開発用 Lat CLI・Node 条件・実在する検査コマンド
package-lock.json      固定された開発依存の再現
Cargo.toml             GPUI/Ely と実使用 native 依存の定義
Cargo.lock             解決したアプリ依存の固定
rust-toolchain.toml    実使用 Rust toolchain
src/                   現存する固定二窓の entry・所有・view・AppKit Adapter
scripts/
  native-e2e.swift      実窓操作・期待値・受理・focus・画像チェックと判定 report
  native-screenshot.swift 所有窓 PNG 取得・内部 pixel 比較・比較器検査
.agents/skills/
  verifying-macos-apps/SKILL.md 他のmacOSアプリへ持ち出せる共通検証手順（本体）
  verifying-macos-apps/references/ 権限・入力安全・画像・probe・配布物・報告の詳細
  verifying-macos-apps/scripts/native-probe.swift 窓の縁・ホイール・AX・ぼかしの参考 probe
  verifying-macos-apps/README.md, EVALS.md 立ち位置・限界と、効果の評価設計（未実施）
  verifying-ambien/SKILL.md Ambien固有のケース・CLI・証拠契約レシピ

docs/
  README.md            この索引
  development.md       mikan・GitButler・Lat の開発フロー
  architecture.md      構成・状態・所有・失敗の説明
  detailed-design.md   型付き契約・所有・時計・容量・実現手段
  implementation-plan.md 成果単位・依存・試験・smoke・Issue 化条件
  verification.md      検証手順・証拠・未確認範囲
  references.md        一次資料・選択的な設計原則

lat.md/
  lat.md               参照グラフの索引
  architecture.md      現存する仕様・構成説明への分野別入口
  workflow.md          ローカル CLI・索引作成・検査・副作用
```

現在の文書量では `docs/` 直下に役割別の文書を置き、空のサブディレクトリや重複する索引を作らない。SPEC を移動・分割せず、別の引き継ぎ書も維持しない。詳細設計・実装計画は承認済み SPEC を基準にするが、作成済みであることを実装開始・Issue 化・外部操作の許可にしない。

mikan は Git 管理外のローカル作業状態として使い、配下のファイルをこの配置や文書・Lat の参照先に含めない。Issue の読取り・更新・Report は MCP だけで行い、状態を文書へ複製しない。

## 更新時の扱い

- 製品の意味・数値・範囲・初期値を変える場合は、SPEC を正本として扱う。補助文書に同じ値を別管理せず、必要な節へ参照する。
- 構成説明は選択した境界とその理由、検証記録は実際の操作と観測、参考資料は対象ソースと採用範囲を記す。説明・予定・結果を混ぜない。
- 文書を追加・移動・削除したら、この索引と参照元を同じ変更で更新する。見出し変更では、その見出しを参照するリンクも確認する。
- 未実施・失敗・限定的な成功を残す。後の修正や再試行を別の記録として結び付け、過去の証拠を全面的な成功へ書き換えない。
- 利用していないツールの操作手順や架空のコマンドを案内しない。リンク検査の成功は文書参照の確認であり、仕様整合・実機受け入れ・公開準備の成功ではない。
- リポジトリ変更の完了前に `npm run check:docs` を通す。現在の `npm run check` も Lat の構造検査だけであり、アプリの検証コマンドではない。通常 Markdown リンクと、要件・説明・証拠の整合は別に確認する。
