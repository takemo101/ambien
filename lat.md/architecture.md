# Architecture

選択済み設計を説明する現存文書への案内。製品要件は [SPEC](../SPEC.md)、補助説明は [docs](../docs/README.md) を読む。以下は実装済み Module・API・ソース配置の一覧ではない。

## Specification

製品要件・受け入れ条件・次に進む条件の唯一の正本は [SPEC](../SPEC.md)。派生説明やツール導入を、仕様全体の最終承認・実装開始・公開の許可に置き換えない。

- [文書の状態](../SPEC.md#文書の状態)と[仕様の管理と次に進む条件](../SPEC.md#仕様の管理と次に進む条件)で作業の範囲を確認する。
- [構成と Interface](../SPEC.md#構成と-interface)、[依存の選定](../SPEC.md#依存の選定)、[構成説明](../docs/architecture.md)から責務の関係を読む。
- [検証手順と証拠](../docs/verification.md)で未実施・限定成功・実機受け入れを区別する。

分野別の入口は [[architecture#Audio and world]]、[[architecture#Scene and settings]]、[[architecture#Persistence]]、[[architecture#macOS and updates]]。進め方は [[architecture#Development]] を読む。

## Audio and world

音声が進める世界、切り替え、粒イベントと再生予定時刻の案内。CPAL と FunDSP の役割は選択済み設計として読み、音質・容量・同期の検証済み事実とは扱わない。

- [体験](../SPEC.md#体験)、[切り替えと途中操作](../SPEC.md#切り替えと途中操作)、[シチュエーション](../SPEC.md#シチュエーション)。
- [音と絵](../SPEC.md#音と絵)、[消音・スリープ・出力停止](../SPEC.md#消音スリープ出力停止)、[スレッドと資源](../SPEC.md#スレッドと資源)。
- [構成説明](../docs/architecture.md)で操作から音と絵への進行、世界停止と無音の違い、時計の対応づけを読む。[検証手順](../docs/verification.md)で必要な観測を確認する。

操作と表示の入口は [[architecture#Scene and settings]]。要件は [[architecture#Specification]] に戻る。

## Scene and settings

GPUI がホストするシーンと設定窓、Ely の設定部品、窓サイズ・描画負荷・アクセシビリティの案内。窓の寸法と音の世界の寸法を混同しない。

- [窓](../SPEC.md#窓)、[サイズ変更への耐性](../SPEC.md#サイズ変更への耐性)、[最大サイズと描画負荷](../SPEC.md#最大サイズと描画負荷)。
- [設定窓](../SPEC.md#設定窓)、[ボリューム設定](../SPEC.md#ボリューム設定)、[アクセシビリティ](../SPEC.md#アクセシビリティ)。
- [構成説明](../docs/architecture.md)で viewport と窓の寸法、操作を音へ渡す責任を読む。[検証手順](../docs/verification.md)でガラス表現・リサイズ・高 DPI の証拠を確認する。

世界と粒の関係は [[architecture#Audio and world]]、復元と保存は [[architecture#Persistence]]、OS との境界は [[architecture#macOS and updates]]。

## Persistence

設定の読取り・正規化・未知値の保持・退避・原子的保存と、保存失敗が止めるものへの案内。保存例は実装 API や実測結果ではなく、正本の保存契約として読む。

- [保存](../SPEC.md#保存)、[復元](../SPEC.md#復元)。
- [構成説明](../docs/architecture.md)で設定の端点と瞬間的な世界の状態、通常保存と更新前保存の違いを読む。
- [検証手順](../docs/verification.md)で破損・退避失敗・未対応バージョン・保存失敗の確認範囲を読む。

設定の操作は [[architecture#Scene and settings]]、更新前保存は [[architecture#macOS and updates]]。

## macOS and updates

macOS の窓制約・アクセシビリティと、Sparkle による手動更新・署名・公証・配布前提への案内。platform Adapter は GPUI のホストを置き換えず、文書整備は公開権限を与えない。

- [基盤のサイズ制約と既知のコスト](../SPEC.md#基盤のサイズ制約と既知のコスト)、[アクセシビリティ](../SPEC.md#アクセシビリティ)。
- [配布と更新](../SPEC.md#配布と更新)、[アプリ内更新](../SPEC.md#アプリ内更新)、[配布前提](../SPEC.md#配布前提)。
- [参考資料](../docs/references.md)で選定に対応する一次資料を読み、[検証手順](../docs/verification.md)で下限 OS・配布物・実更新の証拠を確認する。

保存の責任は [[architecture#Persistence]]、継続条件は [[architecture#Specification]]。

## Development

作業規約は [AGENTS](../AGENTS.md)、Issue 状態は mikan、Git 操作は GitButler に従う。Lat は参照案内であり、実装計画・Cargo・アプリコードの開始条件を変更しない。

- [開発規約](../AGENTS.md)、[開発手順](../docs/development.md)、[文書案内](../docs/README.md)、[参考資料](../docs/references.md)。
- [package.json](../package.json)と[package-lock.json](../package-lock.json)が開発用 Lat CLI と Node 条件を定義する。アプリの依存定義ではない。
- [[workflow]] で実在する npm コマンドと副作用を読む。`check` は現状 `check:docs` のみであり、製品コードの試験・ビルド・実機証明ではない。

作業前に [[architecture#Specification]] と対象の正本文書を読み、変更後は関係する案内だけを更新する。Issue の Status・試験件数・仕様全文をここへ複製しない。
