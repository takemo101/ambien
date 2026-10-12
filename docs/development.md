# 開発手順

Ambien の調査・変更・Issue・PR・ローカル文書案内を、一つの作業単位として進めるための手順。製品仕様やアプリ実装計画ではなく、[AGENTS.md](../AGENTS.md) の規約を具体的な運用へ結び付ける。README は概要だけを維持し、作業手順は本書に置く。

## 正本と開始時の読取り順

| 対象 | 正本・役割 |
|---|---|
| 製品要件・受け入れ・次に進む条件 | [SPEC.md](../SPEC.md) が唯一の正本。数値、初期値、依存の選定はそこを参照する。 |
| 作業規約・操作権限 | [AGENTS.md](../AGENTS.md)。本書はその派生手順であり、追加の実施許可ではない。 |
| Issue の現在の状態・Report | mikan。文書や Lat に Status の写しを維持しない。 |
| 構成説明・観測方法・証拠 | [docs の索引](README.md) から読む補助文書。設計説明と実測結果を分ける。 |
| 関連文書の所在 | `lat.md/` の参照案内。SPEC、mikan、実際のソースを置き換えない。 |

1. 依頼の対象と、SPEC の [文書の状態](../SPEC.md#文書の状態)、[仕様の管理と次に進む条件](../SPEC.md#仕様の管理と次に進む条件) を読む。署名・配信・リポジトリ公開に関係するなら [配布前提](../SPEC.md#配布前提) も読む。
2. AGENTS と本書を読み、変更範囲、許可された操作、今回の確認方法を特定する。既存のユーザー変更を保持する。
3. mikan で対象 Issue の本文・現在の Status・関連 Issue・Report を読み、要件はリンク先の SPEC で確認する。`depends_on` は助言であり、前提達成や承認の代わりではない。
4. Lat の `locate`、`section`、`refs` で関係する入口を探し、リンク先の [構成説明](architecture.md)、[詳細設計](detailed-design.md)、[実装計画](implementation-plan.md)、[検証手順](verification.md)、[参考資料](references.md) の必要な範囲を読む。実装が存在する段階では、実際のソース・依存定義・利用可能な検証コマンドも読む。

ツールと運用文書の導入は、仕様全体の最終承認ではない。mikan の設定や Issue 作成、GitButler の target 設定、Lat の検査成功だけで、設計・実装・Cargo 作成の許可を作らない。現在の明示依頼と操作範囲は最新の SPEC で確認する。アプリコードと Cargo の作成を開始しており、実際の検証と未確認範囲は [検証記録](verification.md) に置く。コードの存在を製品全体の受け入れや配布準備の成立に読み替えない。

ZenNotes は採用しない。別の引き継ぎ書や、製品要件・Issue 状態の第二の正本も作らない。

## 作業単位と最小 E2E

一回の作業を、利用者に見える一つの成果と、それを観測する方法で定義する。文書量、試験件数、部品数、PR の数は完成基準にしない。対象範囲と対象外を明記し、許可された範囲の**最小で安全な端から端までの経路**を早く確かめる。周辺の抽象化や将来統合を先行させず、その経路に必要な安全性は省かない。

- 既存の定義やソースで分かる環境前提を早く読む。調べられる事実はユーザーへ聞かず、設定・権限・鍵・認証を変更して前提不足を勝手に補わない。
- 変更、対象挙動と失敗ケースの試験、必要な全体検査、独立レビュー、文書更新を、一貫した Issue サイズの変更にまとめる。中間の helper ごとに承認ラウンドを増やさない一方、必要な製品・外部操作の gate は省略しない。
- 実機試行の前に、仮説、結果を区別できる観測、停止条件、必要最小限の診断を定める。ユーザーへ依頼するときは、画面・場所・操作・期待する可視変化を具体的に示し、候補版の実際の経路へ接続済みであることを確認する。
- ユーザー報告は事実として扱う。同じ失敗を確認するだけの再試行や、根拠のない推測修正はしない。曖昧な観測は未確認として、こちらの説明と観測手段を改善する。

この進め方は実装の順序を新たに定めず、SPEC の受け入れ条件を減らさない。ソース確認、自動試験、実機観測、ユーザー試聴、配布検証を混同しない。

## 設計とレビューの4観点

変更前に SPEC の [構成と Interface](../SPEC.md#構成と-interface)、[スレッドと資源](../SPEC.md#スレッドと資源)、対象の要件を読み、次の質問を変更とレビューに使う。

1. **変更理由と所有権**：責務・依存方向・状態と資源の所有者は明確か。音声・UI・保存・OS 境界を曖昧に共有していないか。Rust の所有と資源寿命、スレッド間の受け渡しを説明できるか。
2. **型付きの境界**：外部設定・OS・操作からの入力を必要な境界で検査し、検査済みの値として渡しているか。単位・時計・値域を取り違えないか。ホットパスで不要な再検査・確保・コピー・計算を増やしていないか。
3. **独立して検証できる規則**：状態遷移・時間・選択・保存などの規則を、不要な UI・OS・I/O 依存なしに試験できるか。その試験を実機経路の成功へ読み替えていないか。
4. **明示的な失敗**：失敗・取消・古い操作・資源寿命を区別できるか。何を継続・停止・保持するかが明らかか。エラーを握り潰す、成功らしい代用品へ置き換える、未確認入力を受け入れる処理がないか。

YAGNI を優先し、依頼されていない機能・互換経路・将来用フレームワークを足さない。一律の4層構成、DDD、CQRS、不要な Repository、全操作の trait 化を持ち込まない。選定済みの GPUI・Ely・CPAL・FunDSP・Sparkle を別のホストや依存へ置き換えず、改訂と制約は SPEC と実際の依存定義で確認する。参考資料の具体的なアプリ配置・API・権限は継承しない。

## mikan の Issue 進行

プロジェクトは `ambien`、Issue prefix は `AMB` とする。`.mikan/` 全体は Git 管理外のローカル作業状態であり、別の clone へ Issue やボード設定は引き継がれない。配下を直接参照・編集せず、文書・Lat のリンクやコード探索・リンク検査の対象にも含めない。Issue の読取り、Status 更新、Report 追加は mikan MCP だけで行う。GitHub Mirrors は別途許可なしに公開しない。

新しい clone で初期化が必要な場合は、ツール導入の権限を確認し、Ambien のルートから `mikan init --key AMB --name ambien` を実行する。既存のローカルボードを上書き・削除しない。MCP は Ambien を作業ディレクトリとして起動し、対象プロジェクトの Issue を扱う。

### MCP の入口

利用可能な機能は `search_tools` で発見し、返されたツール名を `get_tool_details` で調べて、入力 schema と分類を確認する。読取り専用であることが明示されている場合だけ `call_read_tool` を使い、更新または未分類のツールは `call_write_tool` から呼ぶ。内容を読む機能でも分類が返らない場合があるため、取得という名前だけで読取り専用と断定しない。未分類の呼出しでも、schema と操作内容を確認して意図しない書込みを避ける。Issue 取得・Status 更新・Report 追加の具体的ツール名や引数を推測しない。MCP の入口があること自体を、未承認の実装や公開の権限にしない。

### 状態と完了条件

許可された実装 Issue は **`backlog → ready → active → completed`** の順で進める。各遷移の直前に MCP で現在の Status を再読取りし、期待する前状態に一致することを確認する。記憶や文書の写しだけで上書きしない。

| Status | 扱い |
|---|---|
| `backlog` | 候補。Issue があるだけでは着手許可にならない。 |
| `ready` | 対象範囲、前提・依存、SPEC の gate と今回の操作権限を確認済み。 |
| `active` | 実装直前に移す。同時に active の実装 Issue は最大1件。 |
| `blocked` | 未承認、前提不足、検査失敗、未解決レビュー、merge・同期・受け入れの阻害などを理由付きで記録する。解消後は現在状態と再開条件を再確認し、実装は active に戻してから進める。 |
| `completed` | 当該 Issue の必要な検証・レビュー・受け入れ、PR の normal merge、workspace 同期、Report が揃った後にだけ移す。 |

通常の一連の流れは、実装 → 対象と失敗ケースの試験 → 必要な全体検査・診断 → 独立レビュー → 指摘修正と影響範囲の再確認 → PR 作成 → `main` へ normal merge → GitButler 同期 → mikan Report → Status 再読取り → `completed` とする。PR を依頼されていない場合は、無断で commit・push・PR へ進めず、その gate を未達として残す。

検査が失敗、指摘が未解決、PR が未 merge、同期が未完了、受け入れが部分的なら、`active` または理由付き `blocked` を保持し、完成を宣言して次の実装 Issue を開始しない。意思決定 Issue と実機受け入れには、それぞれのユーザー判断・SPEC の gate が残る。Issue の完了は製品全体の受け入れではなく、オフラインの代用品は実機の証拠にならない。

### Report に残すこと

Report は実際の作業と証拠に結び付ける。対象と SPEC 節、実行した検査と結果、独立レビューと指摘対応、PR URL・review 済み head・merge commit、同期結果、当該 Issue の受け入れ範囲、未実施事項・制約・blocker を記録する。詳細な実機証拠は [検証手順](verification.md#実施後に残す記録) に従う文書へリンクし、製品要件や Status の写しを増やさない。実行していないコマンドや観測結果を Report に成功として置かない。

## GitButler と PR の一連の手順

Git の書込みは `but` を使う。統合先は `main`、GitButler target は `origin/main`。feature・fix は Issue 単位の専用ブランチ／stack で扱い、無関係な変更を混ぜない。`develop` を新設せず、target の変更を勝手に行わない。

操作前に対象が Ambien のルートであることを確認し、インストール済み `but` skill を読む。読取り専用の Git 調査は可能だが、raw Git の add・commit・checkout・merge・rebase・push などで書き込まない。GitButler の設定・認証が足りない場合は blocker とし、raw Git やグローバル設定変更へ逃げない。

### 必要最小限の inspect

- 未 commit の変更を選ぶなら `but diff`。branch／stack の配置や conflict が判断に必要なら `but status`。
- ファイル／hunk ID や commit 内の詳細が必要なときだけ `but status -fv`。既知の branch／commit の詳細は `but show <id>`。
- 目的なしに status を繰り返さない。ID は現在の出力からコピーし、推測しない。操作結果で次の判断に十分なら、確認用 diff を儀式的に追加しない。

### PR 依頼時の standing lifecycle

ユーザーが PR を依頼した場合は、**検証・独立レビュー → scoped commit → PR 作成 → `main` への normal merge → workspace 同期 → Report と Issue 完了**を一連で行う。その依頼の範囲で必要な branch・commit・push・PR・通常 merge・同期を、各段階で再承認を求めず進める。draft、review-only、no-merge、別 target の明示依頼は優先する。PR 依頼がなければ、現在の変更を無断で commit・push・PR 化しない。

1. 対象 diff を読み、関係する検証と独立レビューを済ませ、指摘を解消する。必要なファイル／hunk だけを `but commit -b <branch> -m <message> <ids...>` で選ぶ。Issue に結び付く branch 名を使い、既存の無関係な変更を取り込まない。
2. `but pr new <branch> -F <message-file>` で PR を作る。この操作は push を含む。作成された PR の実際の target、head SHA、変更ファイルを読む。
3. required checks、必要な reviews、mergeability を確認する。head が変わったなら、以前のレビュー・検証をそのまま新 head の承認としない。未達・失敗・conflict があれば merge せず blocker を残す。
4. 適用可能な GitButler の PR merge 操作があればそれを使う。利用できるのが auto-merge 操作だけで、GitHub auto-merge が無効／利用不可の場合に限り、`gh pr merge` を PR merge 段階の例外として使う。通常 merge の例は `gh pr merge <number> --repo takemo101/ambien --merge --match-head-commit <reviewed-head-sha>`。review 済み head に束縛し、target は `main` のままにする。
5. GitHub の実際の PR 状態で `MERGED` と merge commit を確認する。成功らしい CLI 表示、auto-merge の予約、ユーザー報告だけで merge 済みとはしない。
6. `but pull --check`、続けて `but pull --status-after` を実行して workspace を同期する。GitButler に統合済み branch の整理を任せ、merge commit が workspace `HEAD` に含まれることと残る変更を確認する。conflict を reset・discard・強制同期で消さず、無関係な作業を保つ。
7. PR URL、merge commit、同期結果、残る変更・制約を報告し、mikan Report に記録する。Issue の受け入れが揃ったことを確認し、Status を再読取りしてから `completed` へ進める。

「一連で行う」は assistant が手順を完了する意味であり、GitHub の auto-merge 設定を有効化する指示ではない。`--admin`、force push、checks・reviews・branch protection の迂回、`but land` による PR 回避、auto-merge 設定変更、リポジトリの公開範囲変更、mikan mirror 公開は禁止する。これらの権限、アプリ実装・署名・配布の権限を PR 依頼から推測しない。

## Lat のローカル案内と副作用

Lat はプロジェクト内の `lat.md` パッケージを **0.12.2** に固定し、npm と Node **`^24.15.0 || >=26.0.0`** を使う。グローバル導入や `npx ...@latest` を使わず、ルートの manifest と lockfile を参照する。導入は `npm ci --ignore-scripts` とし、パッケージ取得の通信を文書・クエリの外部埋め込み送信と区別する。

`npm run lat -- ...` がローカル CLI の入口である。wrapper は `LAT_LLM_KEY`、`LAT_LLM_KEY_FILE`、`LAT_LLM_KEY_HELPER` を子プロセスの環境から除去し、`XDG_CONFIG_HOME` をルートの `lat.md/.cache/config` に隔離する。既存ユーザーの埋め込み設定を継承せず、鍵を追加しない。wrapper を迂回する直接 CLI、remote embeddings、`--remote` は使用しない。

まず索引を作らずに使える案内を優先する。

```bash
npm run lat -- locate "Specification"
npm run lat -- section "architecture#Specification"
npm run lat -- refs "architecture#Specification" --scope md
```

`locate` で得た実在する `file#section` を `section` へ渡し、リンク先の正本を読む。`refs` は graph の節などへの参照を調べるもので、通常の Markdown リンクの逆引きではない。semantic search が必要な場合だけ、同梱ローカルモデルで `npm run lat -- reindex --local` を実行し、`npm run lat -- search "<topic>"` を使う。

reindex は読取り専用ではない。文書からローカル index の `lat.md/.cache/vectors.db` を再構築し、隔離された `lat.md/.cache/config/lat/config.json` に local 選択を書き込む。search も必要に応じて index を自動作成・更新する。同梱 MiniLM／WASM を使い、外部モデルを download しない。index と隔離設定は ignored の `lat.md/.cache/` に留め、Git に取り込まない。文書・クエリの外部埋め込み API への送信は導入範囲に含めない。

Lat は現存する文書への案内として保守する。意味のある文書・責務の変更では関連 entry とリンクを更新するが、SPEC 全文、Issue Status、過去の試験件数を複製しない。未作成の `src`・Cargo・将来 API を実装済み graph として記述しない。`lat init`、自動 hooks、MCP 登録、CI は追加しない。

## 文書検査と Rust アプリ検証

現在使える `npm run check:docs` と `npm run check` は **Lat の参照・section・graph 構造検査だけ**である。Rust の build、fmt、Clippy、テスト、リンク先内容の意味的な仕様整合、実機動作、製品受け入れ、配布準備を証明しない。検査手順を書いたことも、その実行成功の記録ではない。

文書や graph を変更した作業の完了前に Lat 検査を行い、実行したコマンドと実際の結果を残す。並行作業では変更が揃った後に統合担当がまとめて検査し、途中の未完成状態で各担当が重複実行しない。検査の範囲を超える成功を報告しない。

現在のアプリは単一 Cargo package である。macOS の Xcode Command Line Tools と SPEC の Rust toolchain を使い、リポジトリ root で `cargo fmt --all -- --check`、`cargo test --locked`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo build --locked` を実行する。実際の GUI の起動は `MACOSX_DEPLOYMENT_TARGET=15.0 cargo run --locked`。Ely の `runtime_shaders` を使用するため、この経路に full Xcode を必須にしない。Rust 実行ファイルが PATH にない環境では、既存の `~/.cargo/bin` を当該コマンドの PATH に渡し、無断でインストールや全体設定の変更をしない。独立レビューの指摘修正後は影響する検査と実経路を再確認する。npm の `check` は Rust の検査を実行しない。

操作・期待値を自動判定する native E2E は、build 後に `/usr/bin/swift scripts/native-e2e.swift "$PWD/target/debug/ambien" --case all --artifacts "$PWD/target/native-e2e"` で実行する。`--help` と [固有レシピ](../.agents/skills/verifying-ambien/SKILL.md) の参照表から `lifecycle`／`controls`／`situations`／`deep`／`mixing`／`wandering`／`presentation`／`persistence`／`visual`／`all` を選ぶ。captureノブや保存fixtureは検証条件で、音質の採用値ではない。runner は自分で起動した候補の AX 窓・メニューと、exact foreground 窓を確認した CGEvent を使う。表示可能な自分の console session・非ロック・非 display sleep・AX／event 権限が前提で、実行中は他アプリへの入力と競合させない。前提不足は `blocked`、assertion 失敗は `fail` とし、unsupported 必須属性の無視や source hook へ切り替えない。

座標click／scrollは、公開AX hit lookupで実受け手のPIDと現在の所有window identityを確かめ、指定controlの所属・最新bounds・前面／focus／sessionも送信直前に確認する。GPUIの実hitがNSWindowの場合も、論理leafを取得したとはせずnative受け手を照合する。被覆・別window・確認不能なら入力せずblocked。雨・岩・深海のpreset数値はpublic採用値として試験で固定せず、雨は実開始値からの1step・音声側受理・窓再生成後の保持、岩と深海は各ノブのmetadataと操作端点の受理を観測する。表示されたmanual targetと世代receiptは音声consumerの受理を示すが、実交差gain・2秒完了・可聴出力を証明しない。deepの2.3秒待ちもcapture前のsetup分離であり実gainのassertionではない。それらは音声callback試験・実CPAL観測・ユーザー聴感と分ける。初期値と音質の採用判断はP13のユーザー試聴で確定した（[検証記録](verification.md)）。

`mixing` の各menu開始間隔2秒未満は入力cadenceの観測であり、callback受理時刻・三正gainの重なり・2秒到着を画像から推定しない。消音中のvolume0→18%、実scroll後のsettings解除18%→20%、menu再消音と深海22/12は操作と受理の契約。最大三声の実RTは別の実CPAL診断、離脱／再入場の旧tail・新しい衝撃とdeclared dry delayは実声の決定的試験、短い耳／光のsmokeはユーザー観測として分ける。診断probeはtemporary test-onlyで、実測後にsource attachmentも除去する。allocator／時間を測ったapplication callbackをCPAL／framework全体の資源保証やsyscall traceに広げない。

`wandering` はcapture不要で、実menu／pointer／focused keyboardだけから時間を1/1分・自動1秒へ設定し、対補正と「次の滞在から適用」「次の自動切り替えから適用」の実AX説明を確認する。final雨19/4からon20/4、on開始後60〜95秒内に別対象、さらに最初の表示変更から60〜95秒内に別対象、off21/4後65秒の最新対象保持と正常Quitを観測する。menu三項目を別AX treeで読むため、その読取をmain／audioの対象・世代と前後で照合し、5秒内に排他的な一対象へ収束しなければFAIL。途中の混在markを一時点の複数選択とは断定せず、入力guardをcatch／再送しない。これはUI表示とwall cadenceの証拠であり、生成sample数、gainの到着、抽選一様性、可聴無音、物理同期は別証拠である。

`presentation` はcapture必須で、実scene最小化中のon6/0／manual0、滞在1/1分・自動1秒からwall60〜95秒内の別対象とreceipt world進行を確認する。runner所有のopaque非対話cover3秒→除去、public AXHidden setter／実NSとAX hidden状態の3秒hide/show、scene close2秒中のtime1→2と7/0・wander/mute on・現在対象/world、実menu再生成を続ける。minimize/unhide/reopenでそれぞれ所有PNG3枚／隣接2比較、計9枚／6比較と正常Quit。coverには入力・撮影を行わず、GPUI hostも変更しない。開窓animation中のAX/CG frame差は所有／foreground／同じAX boundsを検査した5秒bounded observationでexact一意matchになるまで待つだけで、capture後のID/bounds検査や入力guardを弱めない。時間、receipt、画像をold particle／scene Task停止／maxfps／生成sample数／RT／物理同期の証拠にせず、それらの直接観測と分ける。

`persistence` は各候補の HOME を private run の配下へ隔離し、production の `Library/Application Support/Ambien/state.json` をそのまま使う。保存ケースだけ同じ fixture HOME を正常 Quit 後の別 process へ渡す。正規化済み time、独立 mute／volume、knob、固定 scene 位置、手動交差中の最新 last、settled save と終了直前の最新 flush、nested unknown の保持を JSON bytes／実 UI／音声 receipt と照合する。future version／backup 不能では元 bytes 不変、corrupt／unreadable では一意 backup の元 bytes を確認する。owned support directory の POSIX mode で本当の temp-create／backup-rename 失敗を起こし、menu／設定通知と、別の実制御を受理した後の world 進行を観測する。通常 HOME／元 state／TCC／OS 音量へ故障を入れない。receipt は可聴無音・音質・物理同期を証明せず、P12 の終了 veto／fade や更新 gate と分ける。

出力復旧は同じ engine を凍結保持して新 stream epoch を作る経路であり、`lifecycle`／`controls` の成功や合成した signal だけで実機スリープ・標準出力変更を合格にしない。実機確認は通常 HOME と音量を変更せず、private HOME の保存端点を mute on／volume zero にして最初のサンプルから無音の候補を使う。fixture と現在値を同一視せず、各操作前後に実設定・受理世代・通知時点の世界時刻を AX で照合する。「標準出力」の rate／出力世代／失敗 text は現存 Paragraph の視覚表示であり、未exportの AX label を読んだことにしない。本人による表示の読取、または許可された所有窓の取得で別に確かめ、シーンの静止／継続、消音保持も観測する。OS のスリープ／復帰・利用可能な出力の選択／接続変更は本人が実施し、AI は OS 設定・既定出力・TCC を変更しない。sleep は正常状態、再準備は失敗と区別され、実際の失敗は設定から分かることを確認する。報告された実失敗は事実として保存し、同じ操作を確認のためだけに繰り返さず、原因と修正候補・判別する結果を確定してから未完条件を再開する。rate 変更・出力使用不能を実際に作れない環境ではその条件を未確認のまま記録し、制御signalや短い待機へ代用しない。実解決 CPAL／coreaudio-rs の内部資源診断はアプリ DSP の排他回収とは別で、throwaway probe を shipping や恒久的な検証 hook として残さない。

受理・自動対象の非atomic AX観測は `observeOwnedSettings` でowned virtual descendantの `.invalidUIElement` を区別する。invalid nodeのPIDと同じsettings windowを照合できた場合だけsnapshot全体を破棄し、case-localの既存deadline内でfresh observationを行う。`ReceiptObservation` はwaiting／exact／invalidatedを分け、最後の150msの安定した値不一致をFAILにする。入力再送・focus回復ではなく、menu／input／captureのguardやroot windowの失敗はcatchしない。permission不足・安定した不一致を通す一般retryとして使わない。

各実行は ignored の `target/native-e2e/` など指定した parent に一意の0700 directoryを作り、0600の `report.json` と候補診断、private candidate HOME を残す。全 case の候補を隔離し、保存ケースの正常再起動だけ同じ HOME を再利用する。report は schema version、実行時刻、実行ファイル・SHA256、選択ケース、check ごとの `id/status/expected/observed/artifacts` を含む。`fail` が `blocked` より優先し、終了コードはそれぞれ1／77、選択した全ケースが通った場合のみ0。stdout が示す**今回の report**を読み、部分成功を総合成功や製品受け入れへ広げない。

画像段階は `scripts/native-screenshot.swift` とScreenCaptureKitでAmbien自身のsceneだけを取得する。visual・deep・mixingはPNG3枚／隣接2比較、situationsは2組の計6枚／4比較、presentationはminimize/unhide/reopenの3組で計9枚／6比較をprivate local保存する。capture preflight falseは取得APIを呼ばず該当段階blocked、独立lifecycle／controls／wanderingは継続する。本人による権限準備は共通スキルに従い、AIによるTCC／OS設定変更・デスクトップ／他app撮影・外部送信をしない。helperは空・全透明・uniformなcropped RGBA・寸法不一致・不変を拒否する。単一色のglowでも透明backgroundとのcoverage/alphaが異なるならuniform interiorではないため、全cropped premultiplied RGBAを比較しvisible RGBだけでblankにしない。背景透過も差分に含むため、可視品質・粒だけの運動・gain・可聴無音・ガラス・物理同期を証明しない。[共通スキル](../.agents/skills/verifying-macos-apps/SKILL.md)、[固有レシピ](../.agents/skills/verifying-ambien/SKILL.md)、[実行証拠と限界](verification.md#native-自動判定と検証スキル) を参照する。両Swift scriptはAmbien専用で、共通スキルを他projectで使えても汎用runnerとして配布・登録していない。

`scripts/native-e2e.swift` が見ない見た目・操作（設定窓の縁の伸縮・スクロールバー・背景のぼかし・演出の描画）の確認には、`scripts/native-probe.swift`（窓一覧・縁のドラッグ・ホイール・AXPress・背面の市松窓・隣接画素差の測定）を使う。使い方と注意は [Ambien 固有レシピ](../.agents/skills/verifying-ambien/SKILL.md) と共通手順の5・6にある。

アプリの実動作は [検証手順と証拠の扱い](verification.md) に従い、SPEC の該当条件に必要な観測を別に実施する。ツール検査と製品受け入れ、操作許可と製品仕様の最終承認を最後まで分離する。

## DMG のビルドとリリース

- 手元で DMG を作る：リポジトリ root で `scripts/package-app.sh`（Apple Silicon の Mac が必要）。release build（配備下限 macOS 15）、`Info.plist` の組み立て、ad-hoc 署名、第三者ライセンス一覧の同梱、`dist/Ambien-<version>-arm64.dmg` と `.sha256` までを行う。Developer ID 署名・公証・Sparkle は含まない。`AMBIEN_BUILD` で `CFBundleVersion` を指定する（リリースごとに増やす）。`dist/` は Git 対象外。
- Release は GitHub Actions（`.github/workflows/release.yml`）が作る。`main` にあるコミットへ、`Cargo.toml` の version と一致する `vX.Y.Z` のタグを打って push する。workflow がタグ・version・コミットが `main` 上であることを確認し、DMG を作って署名状態・hash・arm64・最低 OS を検査し、GitHub Releases に DMG と SHA-256 を公開する（`0.x`・接尾辞付きは pre-release）。CFBundleVersion は run 番号。リリースノートは `.github/release-notes.md` のひな形から作る。
- 公開せずに workflow だけ確かめるときは、Actions の「Release」を手動実行する。DMG は成果物 `ambien-dmg`（14日）として残り、Release は作られない。
- 署名鍵・認証情報はこの workflow に無い。必要な権限は `contents: write`（Release の作成）だけで、`GITHUB_TOKEN` を使う。
