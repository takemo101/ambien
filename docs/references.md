# 実装の参考資料

本書は、実装やレビューで読む資料と、Ambien に適用する観点の案内である。製品要件・依存の選定・進行条件の正本は [SPEC.md](../SPEC.md)。資料を読んだことは、ビルド成功・実機動作・実施権限の証拠にはならない。

文書全体の役割は [索引](README.md)、責務とデータの流れは [構成説明](architecture.md)、動作を確かめる方法は [検証](verification.md) を参照する。

## 技術資料の使い分け

採用バージョンと改訂は [依存の選定](../SPEC.md#依存の選定) を参照する。以下の固定ソースは、SPEC の [調査根拠](../SPEC.md#調査根拠) に対応する資料である。依存を変更する場合は、最新版の一般説明だけで判断せず、変更対象のソースと宣言を確認し、関係する参照も更新する。

| 対象 | 一次資料 | 確認すること・証拠の限界 |
|---|---|---|
| Ely と GPUI の接続 | [Ely の依存定義](https://github.com/ZacharyZhang-NY/Ely-GPUI-Components/blob/7e39a9383dc2fd7404751d02cb166e95861ea3d6/Cargo.toml)、[初期化](https://github.com/ZacharyZhang-NY/Ely-GPUI-Components/blob/7e39a9383dc2fd7404751d02cb166e95861ea3d6/src/lib.rs) | GPUI 改訂・Rust 要求・初期化の対応。宣言の一致は Ambien のビルド成功ではない。 |
| 窓と macOS backend | [GPUI の窓の選択肢](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui/src/platform.rs)、[macOS の窓処理](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui_macos/src/window.rs)、[AppKit の contentMaxSize](https://developer.apple.com/documentation/appkit/nswindow/contentmaxsize) | 制約を適用できる場所、論理座標・外枠・コンテンツ・倍率、復元とフルスクリーンの経路。プロパティ一つの設定だけで全経路の安全を保証しない。 |
| 描画資源 | [GPUI の Metal 描画](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui_apple/src/metal_renderer.rs)、[Metal family 別の制約](https://developer.apple.com/metal/Metal-Feature-Set-Tables.pdf) | 描画面と中間面、リサイズ時の資源寿命、物理寸法の制限。算術上のデータ量やテクスチャ上限は、常駐メモリ・確保成功・快適さの実測ではない。 |
| 再生予定時刻 | [CPAL の timestamp](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/timestamp.rs) | 生成と再生予定の区別、サンプル位置、出力再初期化後の時計対応。予定時刻の取得だけで物理的な音と光の同期を保証しない。 |
| DSP の準備と切り替え | [FunDSP の AudioUnit](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/audiounit.rs)、[Slot](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/slot.rs) | 声・内部バッファの準備と破棄、進行中の切り替えの扱い。汎用 API の名前から、最新選択の優先やコールバック内の無確保を推測しない。 |
| 更新 | [Sparkle の選定リリース](https://github.com/sparkle-project/Sparkle/releases/tag/2.10.0)、[導入と署名](https://sparkle-project.org/documentation/)、[更新情報の配信](https://sparkle-project.org/documentation/publishing/) | 更新画面・署名・配信設定と、SPEC の手動更新方針の対応。SDK の存在は鍵・配信先・公開権限の準備完了ではない。 |
| 配布 | [Apple の署名・公証](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) | 完成配布物の署名・公証・初回起動を検証する前提。ツールの存在を、利用可能な署名 ID や公証成功に読み替えない。 |

公式ドキュメントは更新され得る。ソース上の前提と説明が食い違った場合は、食い違いと対象改訂を記録し、動作確認なしに仕様や依存を置き換えない。

## 選択的に採る設計原則

参照元は [j5ik2o/okite-ai の skills](https://github.com/j5ik2o/okite-ai/tree/f1bc1c37831cbe5ea6a9da5e8f828e639a143e32/skills)。参照改訂を固定しており、[MIT ライセンス](https://github.com/j5ik2o/okite-ai/blob/f1bc1c37831cbe5ea6a9da5e8f828e639a143e32/MIT-LICENSE.md)を確認している。以下は Ambien 向けの要約であり、資料全文・コード例・skills 自体を転載または導入するものではない。

| 参考原則 | Ambien でのレビュー質問 | 適用の範囲 |
|---|---|---|
| [変更理由で境界を決める](https://github.com/j5ik2o/okite-ai/blob/f1bc1c37831cbe5ea6a9da5e8f828e639a143e32/skills/package-design/SKILL.md) | シチュエーションの合成や OS の窓 API の変更で、無関係な保存・移動規則まで書き換える必要があるか。 | Module の責務・所有・Interface を説明できる境界を選ぶ。形式的な層数やファイル行数は基準にしない。 |
| [検査結果を型付きの値として保持する](https://github.com/j5ik2o/okite-ai/blob/f1bc1c37831cbe5ea6a9da5e8f828e639a143e32/skills/parse-dont-validate/SKILL.md) | 検査後も未検査の JSON 値や単位不明の矩形をそのまま渡していないか。 | 保存・OS・操作の必要な境界で値域や単位を確かめる。すべての値を包む型や、ホットパスでの重複検査は先回りで増やさない。 |
| [外部依存なしで規則を確かめる](https://github.com/j5ik2o/okite-ai/blob/f1bc1c37831cbe5ea6a9da5e8f828e639a143e32/skills/domain-model-first/SKILL.md) | 最新選択・交差・滞在・保存値の扱いを、実際の窓や出力機器なしで検証できるか。 | 利用者に見える規則の境界・不変条件を確かめる。純粋な試験を、接続済みの音声・UI・配布の成功証拠にはしない。 |
| [回復方法が分かる失敗を返す](https://github.com/j5ik2o/okite-ai/blob/f1bc1c37831cbe5ea6a9da5e8f828e639a143e32/skills/error-handling/SKILL.md) | 保存失敗と更新失敗を区別し、どの動作を継続・停止・保持するか判断できるか。 | 失敗の意味と復帰を明示する。想定内の保存・デバイス・更新の失敗を、アプリ全体の停止や成功の偽装へ変えない。 |

適用先の責務と制約は [構成と Interface](../SPEC.md#構成と-interface)、[スレッドと資源](../SPEC.md#スレッドと資源)、[保存](../SPEC.md#保存)、[配布と更新](../SPEC.md#配布と更新) を優先する。

## 採用した開発フロー

ユーザーの明示依頼により、Voice Shortcut と同様の mikan flow・GitButler・Lat と、Issue サイズの最小 E2E、型付き境界・所有・独立した規則・明示的な失敗、検証と独立レビュー、PR 統合後の Issue 完了を採用する。適用した規約は [AGENTS.md](../AGENTS.md)、具体的な手順は [開発手順](development.md)、参照案内は [Lat 索引](../lat.md/lat.md) を読む。参照先の Issue 本文や開発終了状態は移さず、ZenNotes は使わない。

PR 依頼時に通常マージと同期まで進める運用は、今回選択した開発フローの一部である。GitHub の auto-merge 設定を有効化する意味ではなく、未依頼の PR 作成や保護の迂回も許さない。アプリ実装・配布の開始条件は SPEC のまま維持する。

## 持ち込まないもの

- DDD・CQRS・イベントソーシング・DB 集約・全4層構成・不要な Repository や trait の一律導入。
- 外部資料にあるコード配置、行数の閾値、依存ライブラリ、実装順序を、そのまま Ambien の規則にすること。
- Voice Shortcut 固有の Tauri・音声認識・CUA・有料モデル通信・アプリの依存とコード配置、GitHub auto-merge の設定変更。
- ユーザーが選択した開発フローの範囲を超えて、外部 skills や参照プロジェクトの権限・継続指示を実行すること。

参考資料は設計判断の材料であり、製品要件や実装開始の承認を追加するものではない。
