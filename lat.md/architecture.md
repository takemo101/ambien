# Architecture

選択済み設計を説明する現存文書と、固定二窓・雨／岩／深海の音声／視覚・共通手動交差・設定操作のソースへの案内。製品要件は [SPEC](../SPEC.md)、補助説明は [docs](../docs/README.md) を読む。予定する製品全体の Module と現存ソースを区別し、実機受け入れや作業状態の一覧にはしない。

## Specification

製品要件・受け入れ条件・次に進む条件の唯一の正本は [SPEC](../SPEC.md)。派生説明やツール導入を、仕様全体の最終承認・実装開始・公開の許可に置き換えない。

- [文書の状態](../SPEC.md#文書の状態)と[仕様の管理と次に進む条件](../SPEC.md#仕様の管理と次に進む条件)で作業の範囲を確認する。
- [構成と Interface](../SPEC.md#構成と-interface)、[依存の選定](../SPEC.md#依存の選定)、[構成説明](../docs/architecture.md)から責務の関係を読む。
- [検証手順と証拠](../docs/verification.md)で未実施・限定成功・実機受け入れを区別する。

具体的な実装契約と作業分割は [[architecture#Implementation design]]。分野別の入口は [[architecture#Audio and world]]、[[architecture#Scene and settings]]、[[architecture#Persistence]]、[[architecture#macOS and updates]]。進め方は [[architecture#Development]] を読む。

## Implementation design

承認済み SPEC を実装へ落とす予定 Interface・所有・時計・失敗と、P01〜P18 の成果・依存・検証・Issue 化条件への入口。設計済みを実装済みや実施許可と読み替えず、作業状態は mikan で確認する。

- [詳細設計](../docs/detailed-design.md)で型付き契約・DSP・RT 境界・固定窓・保存・終了・更新の実現手段を読む。
- [実装計画](../docs/implementation-plan.md)で予定ファイル、Consumes/Produces、behavior tests、実経路 smoke、coverage を読む。
- [Issue 化の条件と本文](../docs/implementation-plan.md#issue-化の条件と実装-ai-向け本文)で AI 向け粒度・依存・対象別権限を確認する。

製品判断は [[architecture#Specification]]、実際の操作フローは [[architecture#Development]] に戻る。

## Audio and world

音声が進める世界、切り替え、粒イベントと再生予定時刻の案内。CPAL と FunDSP の役割は選択済み設計として読み、音質・容量・同期の検証済み事実とは扱わない。

- [体験](../SPEC.md#体験)、[切り替えと途中操作](../SPEC.md#切り替えと途中操作)、[シチュエーション](../SPEC.md#シチュエーション)。
- [音と絵](../SPEC.md#音と絵)、[消音・スリープ・出力停止](../SPEC.md#消音スリープ出力停止)、[スレッドと資源](../SPEC.md#スレッドと資源)。
- [構成説明](../docs/architecture.md)で操作から音と絵への進行、世界停止と無音の違い、時計の対応づけを読む。[検証手順](../docs/verification.md)で必要な観測を確認する。
- 現存する登録・雨／岩／深海合成は [`src/situations/`](../src/situations/)、標準出力・sample世界時計・手動／自動slerp・到着基準滞在／有限抽選・atomic制御／表示とresume配送・epoch leaseと同一engineの凍結／再準備／新stream復旧は [`src/audio/`](../src/audio/)、型付き部分操作・対時間補正・manual ack優先の表示規則は [`src/controls.rs`](../src/controls.rs)。実clock観測とvirtual-time境界、制御signalと実OS通知は別の証拠であり、sourceの存在を音質・物理同期の受け入れに置き換えない。

操作と表示の入口は [[architecture#Scene and settings]]。要件は [[architecture#Specification]] に戻る。

## Scene and settings

GPUI がホストするシーンと設定窓、Ely の設定部品、窓サイズ・描画負荷・アクセシビリティの案内。窓の寸法と音の世界の寸法を混同しない。

- [窓](../SPEC.md#窓)、[固定サイズと内部描画](../SPEC.md#固定サイズと内部描画)、[高DPIと描画負荷](../SPEC.md#高dpiと描画負荷)。
- [設定窓](../SPEC.md#設定窓)、[ボリューム設定](../SPEC.md#ボリューム設定)、[アクセシビリティ](../SPEC.md#アクセシビリティ)。
- [構成説明](../docs/architecture.md)で固定窓と viewport、操作を音へ渡す責任を読む。[検証手順](../docs/verification.md)で固定サイズ・画面収まり・ガラス表現・高 DPI の証拠を確認する。
- 現存する窓とchecked menuは [`src/main.rs`](../src/main.rs) と [`src/app.rs`](../src/app.rs)、シーンのfixed cache・g²/息の予定時刻補間・visibility Task・最新viewportの円形clipは [`src/ui/scene.rs`](../src/ui/scene.rs)、atomic event/state bracketは [`src/audio/transport.rs`](../src/audio/transport.rs)。設定は [`src/ui/settings.rs`](../src/ui/settings.rs) のcontrolled Elyフォームから最新main stateへ部分操作を渡す。既存checked Mach/dry delayは [`src/audio/timing.rs`](../src/audio/timing.rs)、workarea／borrowed NSWindow制約は [`src/platform/macos.rs`](../src/platform/macos.rs)。source・実UI画像・task/fps診断・物理同期の確認範囲は [検証記録](../docs/verification.md) で区別する。

世界と粒の関係は [[architecture#Audio and world]]、復元と保存は [[architecture#Persistence]]、OS との境界は [[architecture#macOS and updates]]。

## Persistence

設定の読取り・正規化・未知値の保持・退避・原子的保存と、保存失敗が止めるものへの案内。現存 [`src/persistence.rs`](../src/persistence.rs) の直列 worker と [`src/app.rs`](../src/app.rs) の settled snapshot／通知／最新 flush を、未実装の終了・更新調停と区別する。保存例は初期 preset や実測結果として読まない。

- [保存](../SPEC.md#保存)、[復元](../SPEC.md#復元)。
- [構成説明](../docs/architecture.md)で設定の端点と瞬間的な世界の状態、通常保存と更新前保存の違いを読む。
- [検証手順](../docs/verification.md)で破損・退避失敗・未対応バージョン・保存失敗の確認範囲を読む。

設定の操作は [[architecture#Scene and settings]]、更新前保存は [[architecture#macOS and updates]]。

## macOS and updates

macOS の窓制約・アクセシビリティと、Sparkle による手動更新・署名・公証・配布前提への案内。platform Adapter は GPUI のホストを置き換えず、文書整備は公開権限を与えない。

- [基盤のサイズ制約と既知のコスト](../SPEC.md#基盤のサイズ制約と既知のコスト)、[アクセシビリティ](../SPEC.md#アクセシビリティ)。
- [配布と更新](../SPEC.md#配布と更新)、[アプリ内更新](../SPEC.md#アプリ内更新)、[配布前提](../SPEC.md#配布前提)。
- [参考資料](../docs/references.md)で選定に対応する一次資料を読み、[検証手順](../docs/verification.md)で下限 OS・配布物・実更新の証拠を確認する。
- [`src/platform/macos.rs`](../src/platform/macos.rs) の所有 NSWorkspace sleep/wake observer と、stream から独立した CoreAudio default/alive/rate/stream listener を [詳細設計](../docs/detailed-design.md#出力停止と復旧) から追う。native callback は固定 signal だけを渡し、app owner と音声 worker がそれぞれ登録解除する。実機スリープ・機器変更と依存内部の全資源回収は [検証記録](../docs/verification.md) で別 gate として読む。

保存の責任は [[architecture#Persistence]]、継続条件は [[architecture#Specification]]。

## Development

作業規約は [AGENTS](../AGENTS.md)、Issue 状態は mikan、Git 操作は GitButler に従う。Lat は参照案内であり、実装計画・Cargo・アプリコードの開始条件を変更しない。

- [開発規約](../AGENTS.md)、[開発手順](../docs/development.md)、[文書案内](../docs/README.md)、[参考資料](../docs/references.md)。
- [package.json](../package.json)と[package-lock.json](../package-lock.json)が開発用 Lat CLI と Node 条件を定義する。アプリの依存定義ではない。
- [`Cargo.toml`](../Cargo.toml)、[`Cargo.lock`](../Cargo.lock)、[`rust-toolchain.toml`](../rust-toolchain.toml) が実際のアプリ依存と toolchain を固定する。Rust の検査・GUI 起動の現在のコマンドと条件は [開発手順](../docs/development.md#文書検査と-rust-アプリ検証) を読む。
- [macOS 共通検証スキル](../.agents/skills/verifying-macos-apps/SKILL.md) と [Ambien 固有レシピ](../.agents/skills/verifying-ambien/SKILL.md) で実操作・画像・証拠判定を現存ケースへ対応付ける。三環境の連打・最新対象・独立mute／volumeは `mixing`、実gain／RT／再入場と耳の受け入れは別証拠を読む。専用scriptを他アプリへ流用せず、条件と実施権限は SPEC を参照する。
- [[workflow]] で実在する npm コマンドと副作用を読む。`check` は現状 `check:docs` のみであり、製品コードの試験・ビルド・実機証明ではない。

作業前に [[architecture#Specification]] と対象の正本文書を読み、変更後は関係する案内だけを更新する。Issue の Status・試験件数・仕様全文をここへ複製しない。
