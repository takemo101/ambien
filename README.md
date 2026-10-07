# Ambien

合成した環境音と、音に連動する静かなすりガラスのシーンを楽しむ、macOS 向けアンビエントアプリのプロジェクトです。作業の横で長く鳴らせる、柔らかく邪魔にならない体験を目指します。

- 雨・岩・深海の3つの環境音を、録音素材ではなく Rust で合成する。
- 環境を手動で切り替えたり、自動でゆっくり移り変わらせたりする。
- 音と絵が同じ世界を共有し、大きな粒だけがときどき光る。場所を写実的に描くアプリではない。
- アプリ内の音量・消音・音の調整と、設定の保存・復元を備える。

UI は GPUI＋Ely、DSP は FunDSP、音声 I/O は CPAL、アプリ内更新は Sparkle を使う設計です。対象は Apple Silicon の macOS 15以降です。

承認済み仕様に沿って実装を開始し、Cargo と固定した GPUI シーン・Ely 設定窓のコードを作成しています。音声・粒の描画・保存・更新はまだ実装されていません。[詳細設計](docs/detailed-design.md)と[実装計画](docs/implementation-plan.md)は予定する製品全体、[検証記録](docs/verification.md)は実際の確認と残る制約を示します。Issue の状態は mikan で管理します。

詳細な要件と進行条件は [SPEC.md](SPEC.md)、開発時の作業規約は [AGENTS.md](AGENTS.md)、文書の役割と読む順序は [docs/README.md](docs/README.md) を参照してください。

build・native E2E の実行方法は [開発手順](docs/development.md#文書検査と-rust-アプリ検証) を参照してください。E2E runner は追加依存なしで実窓を外部操作し、主観的なガラス・音質の確認とは分けて記録します。
