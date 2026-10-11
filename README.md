# Ambien

合成した環境音と、音に連動する静かなすりガラスのシーンを楽しむ、macOS 向けアンビエントアプリのプロジェクトです。作業の横で長く鳴らせる、柔らかく邪魔にならない体験を目指します。

- 雨・岩・深海の3つの環境音を、録音素材ではなく Rust で合成する。
- 環境を手動で切り替えたり、自動でゆっくり移り変わらせたりする。
- 音と絵が同じ世界を共有し、大きな粒だけがときどき光る。場所を写実的に描くアプリではない。
- アプリ内の音量・消音・音の調整と、設定の保存・復元を備える。

UI は GPUI＋Ely、DSP は FunDSP、音声 I/O は CPAL、アプリ内更新は Sparkle を使う設計です。対象は Apple Silicon の macOS 15以降ですが、macOS 15の実機は無く、確認できるまで下限の対応は宣言しません。配布は、Developer ID 署名・公証をしない DMG を `takemo101/ambien` の GitHub Releases に置く方針です（リポジトリの公開は別途の明示依頼を待つ未実施の操作です）。

承認済み仕様に沿って、固定したGPUIシーン・Ely設定窓、雨／岩／深海の合成、CPAL標準出力、共通の手動多声交差、同じ粒のcanvas描画、音量・独立消音・受理通知を実装しています。ノブ初期値と三環境の音色は、ユーザーの試聴で確定済みです（P13、範囲と未確認は検証記録）。声のソース・自動試験・offline波形と、実native／CPALの受け入れは別証拠です。自動移動・保存・更新は後続の成果で、物理同期、高DPI負荷、配布の受け入れは別に必要です。[詳細設計](docs/detailed-design.md)と[実装計画](docs/implementation-plan.md)は現存部分と製品全体の予定を区別し、[検証記録](docs/verification.md)は実際の確認・過去の失敗・残る制約を示します。Issueの状態はmikanで管理します。

詳細な要件と進行条件は [SPEC.md](SPEC.md)、開発時の作業規約は [AGENTS.md](AGENTS.md)、文書の役割と読む順序は [docs/README.md](docs/README.md) を参照してください。

build・native E2E の実行方法は [開発手順](docs/development.md#文書検査と-rust-アプリ検証)、ケース選択・結果判定・証拠確認は [macOS 共通検証スキル](.agents/skills/verifying-macos-apps/SKILL.md) と [Ambien 固有レシピ](.agents/skills/verifying-ambien/SKILL.md) を参照してください。E2E runner は Ambien 専用で、追加依存なしで実窓を外部操作して期待値をチェックし、実窓画像・前提不足・主観的なガラス／音質の確認を分けて記録します。
