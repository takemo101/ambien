# 検証手順と証拠の扱い

## 役割と現在の状態

製品要件・受け入れ条件・作業を続ける条件の正本は [SPEC.md](../SPEC.md) である。本書は、実装後にその条件をどう観測し、何を証拠として残すかを説明する補助文書であり、新しい仕様、実装計画、実行許可を定めない。文書の役割と参照順は [文書案内](README.md) を参照する。

[SPEC の文書の状態](../SPEC.md#文書の状態) にあるとおり、ユーザーの追加依頼を受け、Cargo と固定二窓の実装を開始した。以下のシナリオは観測方法であり、実施・未実施を表の存在から推定しない。末尾の記録で、過去の開発ツール・文書の確認、P01 のコード・実 GPUI/NSWindow の限定観測、ユーザー確認を区別する。画面操作は 2x の主画面で成功回答を得た。診断を除去した shipping source の候補で、外部 AX と foreground CGEvent による設定・閉鎖・再生成・両窓からの ⌘Q を観測し、実 PID の消失と exit0 を確認した。過去の physical 入力と host 生存の不一致は履歴として保持する。製品全体の受け入れではない。

実装・検証の開始には [仕様の管理と次に進む条件](../SPEC.md#仕様の管理と次に進む条件)、署名・公証・外部配信には [配布前提](../SPEC.md#配布前提) が適用される。本書は、鍵生成、認証変更、公開、ツール導入の追加許可ではない。現在の Rust 検査・GUI 起動の実在コマンドは [開発手順](development.md#文書検査と-rust-アプリ検証) を参照し、未作成の試験 CLI や配布経路が利用可能とは扱わない。

数値基準、範囲、初期値、依存改訂はリンク先の SPEC で確認する。シナリオ表の期待結果は要件の観測方法であり、仕様値の別管理には使わない。

## 確認の種類を混ぜない

| 確認の種類 | 何を確認するか | 単独では確認できないこと |
|---|---|---|
| ソース確認 | 実際に使う Module の責務、Interface の受け渡し、Implementation の所有・容量・失敗経路、OS とつなぐ Adapter の範囲を追う。GPUI がホストのままであることと、音声・描画・保存の境界を確認する。 | 実機の音質、物理的な同期、描画負荷、配布成功。選定したライブラリの能力を読むだけでは実装経路の成功にもならない。 |
| 自動試験 | 状態遷移、最新操作、時間境界、ゲイン連続性、有限値、容量、保存の往復・失敗を、入力と出力で確認する。ソースの文言や配線を固定するだけの試験にしない。 | 心地よさ、すりガラス、対応画面・倍率での負荷、物理的な音と光の差、初回配布の成功。 |
| 実機観測 | 実際の音声出力、画面、OS 操作、資源使用、状態の復旧を、操作前後と操作中に観測する。 | 観測していない OS・GPU・ディスプレイ・倍率・出力機器への一般化。短い観測だけで長い試聴を代替すること。 |
| ユーザー試聴 | 各環境の質感の違い、作業を妨げないこと、ループ感・ピーク・クリック・切り替え時の聞こえる大きさについて、ユーザーの判断を得る。 | 数値的な有限値、リアルタイム制約、署名・公証などの機械的な条件。実装者の試聴だけでユーザーの心地よさを確定すること。 |
| 配布検証 | 実際の署名・公証済み配布物を取得し、初回起動と手動更新を対象環境で確かめる。 | 開発環境での起動やビルド成功だけによる一般配布の受け入れ。署名確認だけによるガラス・音・設定の動作保証。 |

確認の分離は [検証条件](../SPEC.md#検証条件)、責務と資源の確認は [構成と Interface](../SPEC.md#構成と-interface) と [スレッドと資源](../SPEC.md#スレッドと資源) に従う。同じシナリオでも、必要な種類の証拠を別々に取得する。

## 安全な試行条件と中止条件

### 試行前に揃えること

- 実施権限と対象を確認する。実装後の対象バージョン、実在する操作経路、参照した SPEC の版を特定し、今回確認する条件をシナリオ表から選ぶ。配布・更新を伴う試行は、配布前提とその操作の許可が揃ってから行う。
- 音声試行は、耳に負担のない再生環境から始める。アプリ内音量の上端や出力機器の抜き差しを試す前に、スピーカーから突然鳴っても安全な出力レベルと周囲の条件にする。大きな音を出して有限値やピークを確認しようとせず、先に自動試験と出力の記録で確認する。
- 保存・終了・更新を扱う試行は、ユーザーの現用設定から隔離したコピー、使い捨ての設定と実行環境を使う。隔離できていることを確かめられなければ故障注入をしない。SPEC の保存先を現用のまま加工しない。
- JSON の破損、権限による書き込み・退避失敗、未対応バージョン、更新物の不正や置き換え失敗は、隔離した試験対象だけに作る。現用データ、公開中の配信物、鍵、キーチェーン、認証、署名設定を故障注入のために変更しない。実際の秘密鍵や認証情報を証拠へ含めない。
- スリープ、出力停止、ディスプレイ移動は、ほかの作業を妨げない状態で行う。固定窓の [高DPIと描画負荷](../SPEC.md#高dpiと描画負荷) と [基盤のサイズ制約と既知のコスト](../SPEC.md#基盤のサイズ制約と既知のコスト) を確認し、固定サイズを解除して過大な描画面を確保させない。
- 観測手段が何を測るかを区別する。音声サンプルの生成時刻、予定再生時刻、物理的な出音、描画指示、実際の発光は別である。負荷計測や記録処理の影響も残し、音声コールバック内の新しい I/O や待ちを観測手段として持ち込まない。

### その試行を中止する条件

- 不意の強い音、耳の負担、繰り返すクリックなどがあれば、安全に音を止め、操作と発生位置を記録する。同じ異常を再現するためだけに試聴を繰り返さない。
- UI が操作不能、資源が増え続ける、描画失敗が空の窓として隠れるなど、安全な継続・終了が難しくなったら試行を止める。正常復帰を確認していないまま別の画面・倍率へ試行を広げない。
- 隔離が崩れた、現用ファイルに触れる恐れがある、更新が許可していない配信先・鍵・認証・公開操作を必要とする場合は、その試行を止める。前提不足を無断の環境変更で補わない。
- 物理同期の対応イベントが識別できない、記録の時刻基準が不明、試聴時間が確認できないなど証拠が不足する場合は、合否を推測せず「未確認」とする。既に報告された失敗は事実として残し、再試行は原因を分ける仮説、変更した条件、停止条件がある場合に限る。

## 実施の流れ

1. 対象シナリオの SPEC 節を読み、操作開始時の状態と期待する変化を決める。成功時だけでなく、継続・停止・保持されるものを確認対象に含める。
2. 自動試験で規則を確認するものと、実機・ユーザー・配布物で確認するものを分ける。実際のアプリが通る音声・描画・保存・更新経路に証拠を結び付ける。
3. 安全な環境で操作し、途中の変化と終点を記録する。画面だけで世界時計の進行を判断せず、音声・論理状態の記録と照合する。聞こえないことだけで世界停止と判定しない。
4. 期待結果と観測結果を分け、成功・失敗・未確認・中止をその対象範囲で判定する。機器依存の制約や計測できなかった項目を残す。
5. 必要な証拠の種類が揃ってから、そのシナリオの条件を判断する。表の未実施状態は、本書作成時点の事実であり、実施後の記録なしに成功へ読み替えない。

### 音・切り替え・設定

以下の状態欄は本書作成時点を示す。

| シナリオと SPEC 参照 | 操作・観測手順 | 期待結果 | 必要な証拠 | 現在の状態 |
|---|---|---|---|---|
| 各環境の音と初期状態：[成功条件](../SPEC.md#成功条件)、[体験](../SPEC.md#体験)、[初期のシチュエーションとノブ](../SPEC.md#初期のシチュエーションとノブ)、[音と操作](../SPEC.md#音と操作) | 隔離した初回状態から起動し、初期表示・設定を照合する。雨から音の確認を始め、雨・岩・深海の各環境を SPEC の試聴時間以上聴く。各環境内の揺らぎと切り替え時の聞こえる大きさを観測し、ユーザーにも同じ対象と操作を示す。 | 初回状態が SPEC に従う。環境の違いが耳で分かり、柔らかさ、ループ感、ピーク、クリック、作業への影響が条件を満たす。初期ノブ値は試聴による決定方法に従い、保存例をプリセットと誤認しない。雨だけの成功を全環境の完成としない。 | 環境別の開始・終了と実試聴時間、出力機器と設定、音の記録、異常があった時刻、ユーザーの判断と対象。 | 未実施・証拠なし |
| ノブの端と独立性：[音と絵](../SPEC.md#音と絵)、[初期のシチュエーションとノブ](../SPEC.md#初期のシチュエーションとノブ) | 各ノブを下端・上端へ動かし、大きく往復する。発生成分を止める下端では、既存の余韻が終わる過程と新しい発生を分けて観測する。独立する他成分を残し、天候の揺らぎ中も確認する。 | ノブごとの意味と値域が守られ、消音を意味しない下端を消音にしない。停止成分を天候が再発生させず、余韻は自然に終わる。追従は滑らかで、硬さなどの上端でも有限値とピークの条件を満たす。 | 全ノブの入力と出力・イベントの自動試験、非有限値の検査、実機の音と光、成分別の観測、滑らかさの試聴。 | 未実施・証拠なし |
| 通常交差・手動の第三声：[切り替えと途中操作](../SPEC.md#切り替えと途中操作)、[体験](../SPEC.md#体験) | 通常の自動交差を観測する。その途中で、メニューと設定窓からそれぞれ第三の環境を選ぶ。既に寄与する側も選び、単独で鳴る同じ環境を再選択する。 | 自動交差は設定した時間で進む。手動選択は現在の混合をいったん片側へまとめず、新しい音を入れ、SPEC の固定時間で選択先だけへ到着する。不要な交差、無音、ゲイン段差、意図しない音量増加がなく、完了後の非寄与声は生成を止める。 | 操作受信と区間境界、声ごとのゲイン・生成状況の時系列、自動試験、両操作経路の画面・音声と試聴。出力遅延と交差開始を分けた記録。 | 未実施・証拠なし |
| 短時間の連続選択・途中編集：[切り替えと途中操作](../SPEC.md#切り替えと途中操作)、[設定窓](../SPEC.md#設定窓)、[スレッドと資源](../SPEC.md#スレッドと資源) | 交差が完了する前に別の環境を続けて選び、最後の選択後は操作を止める。途中で音量・消音・移動設定も変更する。別の試行では表示中の環境のノブを編集する。 | 最新選択をその時点のゲインから扱い、古い選択を順番待ちにしない。意味の異なる共通設定を巻き戻さない。選択・ノブ編集は対象をすぐ表示して自動移動を止め、ノブ編集の対象へ寄せて留まる。 | 操作列と受信・適用の時刻、最新選択から終点までのゲイン、共通設定の状態、自動試験と実機の画面・音。 | 未実施・証拠なし |
| 自動選択・滞在・オンオフ：[体験](../SPEC.md#体験)、[音と操作](../SPEC.md#音と操作) | 自動試験で抽選対象、滞在時間の境界・同値、登録件数の境界を確認する。実機では到着から次の交差までを記録し、滞在中と交差中で「ひとりで移る」をオフ・オンにする。 | 現在を選ばず、他の登録を等確率で選び、固定巡回にならない。単独登録で交差しない。到着を基準に滞在し、オフ時の進行中交差とオン時の再抽選は SPEC に従う。 | 抽選入力・結果と境界試験、実機の到着・抽選・交差の時系列。短い選択履歴だけで等確率を証明せず、抽選規則のソース確認も併記する。 | 未実施・証拠なし |
| 時間設定の次回適用・閲覧：[時間設定](../SPEC.md#時間設定)、[設定窓](../SPEC.md#設定窓) | 待ち時間中と自動交差中に時間設定を変更し、現在と次回の期限を比較する。最小・最大を逆転させる向きへ操作する。設定窓を開いて閲覧し、共通設定だけを変更する。自動交差の主に聞こえる側と同率時の表示も確認する。 | 進行中の時間を変更せず、次回の抽選・自動交差から適用する。範囲・刻み・最小最大の整合と適用説明が守られる。閲覧と共通設定変更だけで自動移動を止めず、選択表示はゲイン由来の規則に従う。 | 変更前後の期限と設定値、次回適用の自動試験、説明と選択表示の画面、ゲインとの照合。 | 未実施・証拠なし |
| 音量・消音：[ボリューム設定](../SPEC.md#ボリューム設定)、[消音・スリープ・出力停止](../SPEC.md#消音スリープ出力停止) | SPEC の検証点の音量で比較し、消音中に音量を変える。消音と音量下端で滞在・交差・光を観測し、消音のまま終了して再起動する。Mac 本体と他アプリの音量も前後で比較する。 | 混合後のアプリ出力だけを滑らかに変え、消音を音量変更で解除しない。無音でも世界と自動移動は進み、光を止めない。表示と復元が一致し、本体・他アプリの音量を変えない。 | 音量・消音・世界時刻の自動試験、出力と光の記録、メニュー・設定の表示、再起動前後と本体・他アプリの比較。 | 未実施・証拠なし |
| リアルタイム経路・視覚容量：[スレッドと資源](../SPEC.md#スレッドと資源)、[視覚イベント](../SPEC.md#視覚イベント) | 実際の音声コールバックから準備・生成・混合・制御・イベント受け渡しを追う。自動試験と隔離した実行で制御・視覚イベントの容量境界を通し、表示が追い付かない場合の音とイベントを比較する。 | 確保・破棄・ロック待ち・I/O・UI 呼び出しを音声経路に持ち込まず、必要資源は外で準備する。最新の整合した制御を読み、視覚容量不足で音を待たせたり音の粒を省いたりしない。 | 実使用経路のソース確認、容量試験と実行時の観測範囲、音声の連続性、視覚だけが省略された記録。ライブラリ一般の説明を経路の証拠に代用しない。 | 未実施・証拠なし |

### 窓・同期・OS 状態

| シナリオと SPEC 参照 | 操作・観測手順 | 期待結果 | 必要な証拠 | 現在の状態 |
|---|---|---|---|---|
| 窓と設定の基本操作：[窓](../SPEC.md#窓)、[メニュー](../SPEC.md#メニュー)、[設定窓](../SPEC.md#設定窓) | 初回の窓を観測し、空いた場所で移動する。「設定…」を繰り返し操作して前面化を確認し、設定窓を閉じる。シーンと設定それぞれの更新状況を観測する。終了は指定メニューまたはショートカットから行う。 | すりガラス、タイトルバー、OS の信号ボタン、シーンに操作部品を置かない外観が守られる。設定は同じ GPUI 実行ループの別窓であり、閉じても音やアプリを終了せず、常時更新をシーンに連動させない。 | 画面と操作記録、窓の前面化、更新の実機観測、ホストと窓寿命のソース確認。 | 未実施・証拠なし |
| シーン閉鎖・最小化・隠れる：[窓](../SPEC.md#窓)、[視覚イベント](../SPEC.md#視覚イベント)、[消音・スリープ・出力停止](../SPEC.md#消音スリープ出力停止) | 音を出しながらシーンの赤い閉じるボタン、最小化、完全に隠す操作をそれぞれ行う。「シーンを表示」または OS の操作で復帰させる。自動交差中も同様に確認する。 | シーンのアニメーションだけが止まり、音と世界・滞在・交差は進む。復帰時は生きているイベントだけを扱い、古い粒をまとめて光らせない。 | 操作中の音声と論理時刻、描画更新の有無、復帰直後の画面とイベント寿命の照合。 | 未実施・証拠なし |
| スリープ・出力使用不能・復旧：[消音・スリープ・出力停止](../SPEC.md#消音スリープ出力停止)、[再生予定時刻](../SPEC.md#再生予定時刻) | 滞在中と交差中に Mac をスリープさせ、直前と復帰直後の残り時間を比べる。隔離した安全な環境で標準出力を使用不能にし、操作を続けて復旧させる。出力機器・サンプルレートの変更、消音中の復旧も確認する。 | 停止中は世界も止まり、経過時間を一括反映しない。出力不能でも窓と操作を維持し、復旧で残りから滑らかに再開する。消音を解除せず、時刻対応を張り直して古いイベントを再生しない。DSP 内部位相や残響の完全一致は要求しない。 | 直前・停止中・復旧後の世界時刻と残り時間、出力状態・形式、操作可能性、音・画面、再対応後のイベント。 | 未実施・証拠なし |
| 固定窓と内部描画：[固定サイズと内部描画](../SPEC.md#固定サイズと内部描画)、[窓高DPI同期](../SPEC.md#窓高dpi同期) | 両窓を開き、端のドラッグ、緑ボタン、タイトルバー操作、OS の拡大・タイル操作でコンテンツ寸法が変わらないことを確認する。狭い作業領域への起動・移動と設定スクロール、内部表示領域の更新、過渡的な空領域を確認する。 | 窓の自由な拡縮・最大化・フルスクリーンを提供しない。固定寸法が画面に収まらなければ補正する。内部の粒は最新領域へ置き、円を歪ませず、世界や入力状態を再初期化しない。空領域は描画だけ休む。 | 両窓の論理寸法と操作記録、画面動画、設定の可達性・フォーカス、音と世界時刻、内部配置・空領域の試験。通常タイルを fullscreen の禁止 flag だけで保証しない。 | 未実施・証拠なし |
| ディスプレイ移動と高 DPI：[高DPIと描画負荷](../SPEC.md#高dpiと描画負荷)、[基盤のサイズ制約と既知のコスト](../SPEC.md#基盤のサイズ制約と既知のコスト)、[復元](../SPEC.md#復元) | 固定窓を倍率の異なる画面へ移し、画面撤去や Dock・メニューバーによる作業領域変更を確認する。論理コンテンツ、外枠、実物理描画寸法と、粒・操作の見え方を記録する。 | 現在の領域と倍率を使い、各寸法を区別する。画面が一時的に得られない場合は最後の有限制約を維持する。通知後の補正を確保前の防止としない。 | 画面・GPU・倍率・実物理寸法、領域取得と補正の経路、画面・音・操作の継続。未検証の画面・倍率を明記する。 | 未実施・証拠なし |
| 固定窓の負荷・資源寿命：[高DPIと描画負荷](../SPEC.md#高dpiと描画負荷)、[基盤のサイズ制約と既知のコスト](../SPEC.md#基盤のサイズ制約と既知のコスト) | 同じ世界条件で対応する画面・倍率を比較し、窓の閉鎖・再表示・画面移動を繰り返す。変更中と落ち着いた後の CPU・GPU・メモリ、イベント発生数・表示数、操作応答と音を観測する。 | 倍率や表示領域に比例して音・粒数を増やさず、アプリ独自の全面画像を毎フレーム作らない。不要な資源が残り続けない。基盤や OS のコストを実測し、固定粒数や固定論理サイズから一定負荷を推測しない。 | 比較条件と計測方法、時間に沿った資源使用、繰り返し回数と復帰後の傾向、粒・音の発生数、実機の音・画面。 | 未実施・証拠なし |
| 物理的な音と光の同期：[音と絵](../SPEC.md#音と絵)、[視覚イベント](../SPEC.md#視覚イベント)、[再生予定時刻](../SPEC.md#再生予定時刻)、[窓高DPI同期](../SPEC.md#窓高dpi同期) | 対応する粒と音を識別できる場面を選ぶ。内蔵・有線で実際の出音と発光を共通の時間基準で記録し、対応イベントの差を求める。通常時、画面移動・内部表示領域更新、出力再初期化後で比較する。Bluetooth は機器別に残る差を記録する。 | 同じ世界イベントと左右位置を使い、声の寄与を光に反映する。予定再生時刻・DSP 遅延を含む対応が維持され、内蔵・有線は SPEC の目標を満たす。Bluetooth の残る差を一律保証しない。 | イベントの対応根拠、物理的な音声・映像記録、取得系の遅延・時間分解能・不確かさ、差の求め方と結果、出力別・状態別の記録。ログや画面収録だけを物理同期の証明にしない。 | 未実施・証拠なし |
| 保存位置と固定寸法での復元：[復元](../SPEC.md#復元)、[固定サイズと内部描画](../SPEC.md#固定サイズと内部描画) | 複数画面で位置を保存して終了する。隔離コピーで復元先なし、画面外、過大な保存寸法を用い、起動時の補正を確認する。 | 保存の座標と寸法の意味を守り、現在の固定寸法を基準に作業領域へ補正する。保存寸法を任意の窓サイズへ戻さず、表示の補正で世界を再初期化しない。 | 保存矩形と画面識別子、復元先の領域・倍率、補正前後の画面、固定寸法、音・世界時刻。 | 未実施・証拠なし |
| システム外観・アクセシビリティ：[アクセシビリティ](../SPEC.md#アクセシビリティ)、[設定窓](../SPEC.md#設定窓)、[成功条件](../SPEC.md#成功条件) | ライト・ダーク、システムの「視差効果を減らす」「透明度を下げる」を起動前と起動中に切り替える。シーンの光・背景、設定窓の外観・動きを比較する。 | 音を変えずに粒と設定窓の動きを減らし、透明度低下時だけ不透明背景を許す。起動時と変更時の両方で反映し、GPUI と Ely の設定を片方だけに適用しない。 | 設定状態と前後の画面動画、同条件の音、起動時・実行中の記録、Adapter と両側への反映経路のソース確認。 | 未実施・証拠なし |

### 保存・配布・手動更新

| シナリオと SPEC 参照 | 操作・観測手順 | 期待結果 | 必要な証拠 | 現在の状態 |
|---|---|---|---|---|
| JSON の往復・既定値・未知値：[保存](../SPEC.md#保存) | 隔離した設定で各保存項目を変え、保存・終了・起動の往復を確認する。欠落設定、欠落ノブ、未知キー・シチュエーション・外したノブ、範囲外値、逆転した時間、登録にない `last`、未対応の新しいバージョンを個別に用いる。 | 既定値・範囲・単位・時間整合が SPEC に従い、識別子で値を引き継ぐ。未知値を保持し、不明な `last` を定めた先へ置く。未対応の新しい保存バージョンを上書きしない。保存対象としない瞬間状態を復元しない。 | 入力・保存後・再起動後の設定比較、バージョン別と境界の自動試験、実際の保存経路の結果。ファイル内に機密情報を含めない。 | 未実施・証拠なし |
| 破損・読取不能・退避失敗：[保存](../SPEC.md#保存)、[保存と配布](../SPEC.md#保存と配布) | 使い捨て設定を破損・読取不能にし、元ファイルと退避先を記録して起動する。別の隔離試行で退避が失敗する条件を作り、起動後に設定を変更する。 | 読めない設定を退避して初期設定で起動し、失敗を知らせる。退避失敗時は元ファイルを上書きせず、その回の保存失敗を示す。 | 元・退避ファイルの比較、初期設定と失敗表示、変更後にも元ファイルが保持された証拠、自動試験と実機観測。 | 未実施・証拠なし |
| 書き込み失敗・保存確定：[保存](../SPEC.md#保存)、[スレッドと資源](../SPEC.md#スレッドと資源) | 設定変更を続けた後に落ち着かせ、保存の時点を観測する。隔離環境で一時書き込み・置き換えの失敗を起こし、音と操作、メニュー・設定の失敗表示を見る。終了前の未保存変更も確認する。 | 一時ファイルから置き換え、変更をまとめて音声外で保存する。通常の保存失敗で音を止めず、成功に見せず、既存データを破壊しない。終了前に未保存変更の確定を行う。 | 保存開始・完了・失敗の記録、前後のファイル比較、音の継続、失敗表示、正常終了前の保存結果。失敗した保存を復元成功としない。 | 未実施・証拠なし |
| 交差途中の終了と再起動：[保存](../SPEC.md#保存)、[保存と配布](../SPEC.md#保存と配布) | 手動交差中、自動交差の出発側優位・同率・到着側優位でそれぞれ終了し、次回起動の環境と滞在開始を確認する。 | 手動は選択先、自動は SPEC が定める側を保存し、混合の途中割合を復元しない。新しい滞在時間を抽選し、設定の端点だけを復元する。 | 終了前の選択・ゲイン・保存結果、再起動の環境と抽選、自動試験と各状態の実機記録。 | 未実施・証拠なし |
| 署名・公証 DMG の初回取得：[配布と更新](../SPEC.md#配布と更新)、[配布前提](../SPEC.md#配布前提)、[保存と配布](../SPEC.md#保存と配布) | 前提と許可が揃った実際の配布物を、配布先から対象環境へ初回取得する。署名・内包物・Hardened Runtime・タイムスタンプ・公証・チケット添付を確認し、取得したアプリを起動してガラス、音、設定を操作する。下限 OS と対応する最大画面・倍率を区別して実施する。 | 対象環境で SPEC の配布要件と初回体験を満たす。開発環境だけの起動、既に信頼済みのコピー、現在の OS だけの確認を初回取得や下限 OS の証拠にしない。 | 実配布 URL、配布物の識別情報、取得条件、署名・公証確認の実結果、環境別の初回起動とガラス・音・設定の記録。秘密鍵・認証は記録しない。 | 未実施・証拠なし |
| 手動確認・取得・更新：[アプリ内更新](../SPEC.md#アプリ内更新)、[保存と配布](../SPEC.md#保存と配布) | 初回起動と通常利用中の更新通信を観測した後、「アップデートを確認…」を操作する。実際の更新情報と署名された配布物を確認・取得し、利用者が再起動を明示的に選んだ場合だけ更新する。更新前後の設定を比較する。 | 手動確認していない間は更新通信を行わず、自動許可・自動インストール・無断再起動をしない。確認・取得中は音を続け、互換性と更新署名を検証する。保存成功後にフェードアウトして終了し、環境・音量・消音・移動設定を復元する。GPUI のホストを変更しない。 | 起動から手動操作前後の通信観測期間・宛先、更新情報と版・最低 OS、配布物と署名検証の結果、利用者の選択、保存完了と再起動・復元の時系列。任意プロファイル送信が無効である確認も残す。 | 未実施・証拠なし |
| 更新の保存失敗・取得／検証／置き換え失敗：[アプリ内更新](../SPEC.md#アプリ内更新)、[保存と配布](../SPEC.md#保存と配布) | 隔離した試験対象で、更新前保存の失敗、確認・取得の失敗、不正な更新署名や不適合版、置き換え失敗を分けて起こす。各試行で音、設定、状態ファイル、更新画面を比較する。公開配信・現用アプリ・鍵を改変して試さない。 | 保存失敗ではその回の更新・再起動を中止して音を続ける。確認・取得・検証の失敗を更新画面に示し、通常の音と設定を維持する。不適合版へ更新せず、置き換え失敗でも状態ファイルを破壊しない。完全な自動ロールバックを無条件に成功要件へ加えない。 | 故障条件と隔離範囲、段階別の失敗結果・表示、更新中止と音の継続、前後の状態ファイル比較、置き換え後に実際に残ったアプリの状態。 | 未実施・証拠なし |

## 実施後に残す記録

空欄を埋めるための記録ではなく、実施した観測を追跡できる記録を残す。実施していないものには結果や架空の証拠を置かない。

- **対象と根拠**：シナリオ名、対応 SPEC 節、参照した SPEC の版、アプリ・試験・配布物のバージョンと識別情報を残す。ビルド条件や依存は実際の定義に結び付け、仕様の選定値を検証済みの値として転記しない。
- **環境と開始状態**：実施日時、確認者、OS、機種・GPU、ディスプレイ識別・作業領域・論理寸法・倍率・物理描画寸法、出力機器・接続・サンプルレート、関連するシステム設定とアプリ設定を、結果に関係する範囲で残す。故障注入は隔離方法と現用データに触れなかった範囲も残す。
- **操作と観測**：実行した操作の順序、開始・終了・到着・停止・再開などの時点、期待結果、実際の結果を分ける。実在する試験コマンドを実行した場合に限り、そのコマンドと終了結果を記録する。試聴は実時間とユーザーの判断、同期は時刻基準と計測誤差、負荷は計測期間と操作中・復帰後の推移を残す。
- **証拠の所在と意味**：実際に取得した試験出力、画面・音声・動画、計測、ファイル比較、署名・公証・更新結果を識別できる所在へ結び付ける。それぞれがソース確認、自動試験、実機、ユーザー試聴、配布検証のどれかを示し、何を直接観測したかを記す。資格情報や秘密鍵を含む出力は共有しない。
- **判定と限界**：その条件の成功・失敗・未確認・中止、確認できた範囲、未検証の機器・OS・画面・操作、観測への影響を残す。失敗の記録と後の修正・再試行は別にし、元の失敗を消したり、限定成功を総合成功へ書き換えたりしない。

アプリの証拠は末尾の [P01 固定二窓の限定観測](#p01-固定二窓の限定観測) に記録する。実 GPUI/NSWindow の状態観測と幾何試験は実施したが、各表の可視ガラス・実操作・音・同期・高DPI負荷・配布の総合シナリオは未受け入れである。限定観測を表全体の合格へ読み替えない。

## 完了の判断

**限定成功**は、特定のバージョン・環境・操作・条件について、必要な種類の証拠が揃った場合だけ記録する。たとえばゲインの自動試験成功はゲイン規則の証拠であり、音の心地よさや物理同期の成功ではない。一つの画面・倍率の成功は未検証の画面・倍率の成功ではなく、現在の OS の成功は下限 OS の成功ではない。対象範囲を添えない「動作確認済み」は使わない。

**総合受け入れ**は [成功条件](../SPEC.md#成功条件)、[検証条件](../SPEC.md#検証条件)、[配布前提](../SPEC.md#配布前提) の条件について、各シナリオの証拠とユーザー試聴を突き合わせて判断する。未実施・未確認・中止の条件を成功で埋めず、音、固定窓と高DPI、同期、保存、配布と手動更新の一部だけで全体の完成を宣言しない。試聴が必要な初期ノブ値も、未実装のまま聞こえ方まで確定したとは扱わない。

本書を作成したこと、文書のリンクが通ること、ソース確認・ビルド・自動試験が通ることは、それぞれの範囲に限る。アプリの総合受け入れ、一般配布の準備完了、次の作業の実行許可を意味しない。

## 開発ツールの導入確認

2026-10-06、Ambien の開発ツール設定と運用文書を対象に実施した。製品の受け入れではない。環境は `sw_vers` が報告した macOS 27.0.1（26A434）・arm64、Node v24.15.0、npm 11.12.1、Bun CLI 1.3.14、mikan CLI 0.0.21、GitButler CLI 0.22.3、プロジェクト内 Lat 0.12.2。手順は [開発手順](development.md)、Lat の副作用は [運用説明](../lat.md/workflow.md) を参照する。

| 対象 | 実行・観測 | 確認できた範囲と限界 |
|---|---|---|
| mikan | `mikan init --key AMB --name ambien` で初期化し、参照先と同じボード設定と Issue テンプレートを読んだ。mounted MCP で実 Issue の作成、各遷移前の再読取り、Status 遷移、Reports 追加を実行した。 | 開発作業を実際の MCP 経路で記録できる。Issue の統合・現在の状態は mikan の `AMB-001` を読む。GitHub mirror は作成していない。 |
| MCP の分類 | `get_tool_details` が分類 metadata を返さず、`get_board` を `call_read_tool` から呼ぶと拒否された。未分類ツール用の `call_write_tool` に切り替えてボードを取得した。 | 現在の gateway で使える経路を確認。取得という名前から読取り専用の分類を推測しない。 |
| GitButler | 旧 target `gb-local/main` から `origin/main` への変更は applied branch により拒否された。必要な配置確認後、CLI の指示どおり branch を unapply、target を設定し、`but pull --status-after` を実行。upstream `4b7b372` の1 commit と `Rebase successful` を観測し、既存文書と新規設定は保持した。push remote は `origin`。 | Ambien の統合先設定と workspace 同期。commit・push・PR 作成・PR merge は今回未実施で、将来の PR 統合成功を証明しない。 |
| Lat の参照案内 | npm wrapper 経由の `--version` は 0.12.2。`locate Specification`、`section architecture#Specification`、`refs architecture#Specification --scope md` で、定義した入口・本文・逆参照を取得した。 | ローカル固定 CLI の実際の出力。graph は現存文書への案内であり、アプリの実装済み宣言ではない。 |
| Lat の構造検査 | `npm run check` を実行し、`check:docs` → wrapper → `lat check` が `All checks passed` を返した。 | wiki link・索引・概要・section の構造。`No init version recorded` は手動連携による想定警告として残した。通常 Markdown リンク・内容の意味・実機動作を検査したことにはならない。 |
| ローカル検索 | `reindex --local` は13 sectionを `local:minilm-l6-v2` で再索引。`search "audio world timing"` の結果に `Audio and world` を取得した。隔離 config の `embedding` は `local`。 | 同梱ローカルモデルを使う索引と検索の実行。検索精度の総合評価や通信の packet capture は行っていない。 |
| 生成物の除外 | 初回導入時は `git check-ignore` で `node_modules`、実際の Lat index/隔離 config、mikan `.state` の除外ルールを確認した。その後、ユーザーの追加指示により mikan 全体を除外する設定へ変更した。 | manifest・lockfile と開発規約・Lat graph を管理し、mikan の Issue・設定・テンプレートは Git に含めない。追加指示後は mikan 配下を直接参照せず、操作は MCP に限定する。 |
| 独立レビュー | 運用・権限と Lat の固定依存・隔離の2範囲を、実装担当と別の reviewer が読取り専用で確認。両方とも修正を要する指摘なし。 | 文書・設定と installed CLI ソースのレビュー。reviewer は検査やアプリ実行を行っていない。 |
| 通常 Markdown の参照 | 導入時の13文書について、Bun と GitHub heading slugger を使う一時検査で、相対リンクの参照先と見出しを確認した。欠落なし。その後の追加指示で mikan 配下を検査対象から除外した。 | Lat 検査とは別の参照確認。外部 URL の疎通・本文の意味・製品動作を検証したことにはならない。 |

追加指示の反映後、`git check-ignore -v .mikan/` がルートの `/.mikan/` 除外ルールを返し、MCP のボード取得は archived を含む6列・警告なしで成功した。mikan 配下を読まない11文書の参照検査は、相対リンク204件・見出し参照129件で欠落なし。`npm run check:docs` も再実行して `All checks passed`。除外指示前の限定証拠は残し、現在の直接参照・検査対象は mikan 配下を含めない。

この開発ツール導入時の確認は、アプリの実動作と製品受け入れが未実施であることを変更しない。現在の作業範囲・仕様承認・実装と Git の許可は、最新の SPEC を参照し、この過去の確認から作らない。

## 詳細設計と実装計画の確認

2026-10-07、固定窓の選択を反映した承認済み SPEC と `docs/detailed-design.md`、`docs/implementation-plan.md`、関連文書・Lat 案内を対象に実施した。アプリや Cargo は作成していない。既存の macOS arm64 開発環境、固定 Lat 0.12.2、Bun Eval 1.4.2 と既存 Node／Markdown parser を使用した。新しい依存・hook・索引作成・外部公開は行っていない。

| 対象 | 実行・観測 | 確認できた範囲と限界 |
|---|---|---|
| 固定一次ソース | CPAL の実 config／Mach 時刻／遅延 error／monitor 寿命、FunDSP の tick／未初期化 process／Q／delay と scalar reset、GPUI/Ely の窓・描画・通知、Sparkle の Install／Dismiss／Skip・cancel／agent の経路を読んだ。採用と不採用の理由は詳細設計の primary URL に対応づけた。 | ソース確認のみ。実 lockfile、build、ABI、RT、物理同期、固定窓・ガラス・署名更新の成功は未確認。 |
| 独立レビューと修正 | 音声・所有・時計と、UI・保存・終了・更新・計画の2範囲を別 reviewer が確認。非寄与声の古い tail、旧 epoch の error、抽選の無制限 rejection、近い target の acos、Installing stage の Dismiss の指摘を修正。再レビューは両方とも追加の修正必須事項なし。 | 文書 readiness の判定。private lazy-valid 残響・実 DSP・RT 最悪時間、実 agent の取消 barrier は将来の受け入れ gate のまま。 |
| ゲインの数値モデル | 三声の最初の slerp モデルは18009点・連続 retarget1000回、最大 norm 誤差6.661338147750939e-16。安定な atan2／接平面のモデルは30015点、同最大誤差。44.1/48kHz・区間補正257 frame の recurrence は2578828点、最大 norm 誤差4.04121180963557e-14。1e-20／1e-200の微小な正の残差も扱い、終端 one-hot を確認した。 | 一時的な JS 数学モデル。実 Rust、実波形の相関・peak・心地よさ・RT の証拠ではない。frame 補正間隔はモデルの入力であり製品要件ではない。 |
| 境界の状態モデル | sequence と2 payload fields がすべて SeqCst の単一 writer／reader の70 interleavings は整合2件採用・68件拒否、混合採用なし。lazy-valid delay は142離脱位置で旧値不復活・新 impulse の規定遅延を確認。残響モデルは無効化後200 sample の旧 tail なし、新 impulse200 sample は初期状態と一致。拒否8試行で pending、次 callback1試行で採用、8bit縮小モデルの3候補は各85件。 | 一時的な数値・状態モデル。Rust memory 実装、FunDSP／アプリ DSP、実 callback の時間、PRNG 品質、OS 復旧を証明しない。 |
| Lat の検査と実ナビゲーション | 初回 `check:docs` はディレクトリ索引へ section を列挙したため失敗。installed CLI の索引検査が直下の file stem のみ許すと確認し、その1行を削除。修正後 `npm run check:docs` は All checks passed、`section "architecture#Implementation design"` と `refs` は本文・正本への参照・逆参照を取得した。 | 実 CLI 経路の文書 smoke。No init version recorded は手動設定の既知警告として残し、無断 init で消さない。検索索引の再作成は未実施。 |
| 通常 Markdown と coverage | 13文書を既存 unified／remark-parse と github-slugger 2.0.0 で解析。相対リンク400件・見出し参照291件に欠落なし。coverage matrix の SPEC34節に欠落なし、P01〜P18の18成果を確認した。`.mikan`・依存・cache は文書検査の対象から除外し、mikan 配下は直接読んでいない。 | 参照の存在と見出し対応の確認。件数や coverage だけを意味的整合・アプリ受け入れとしない。外部 URL の疎通は検査対象外。 |
| parser の実行経路 | Bun の直接 import は既存 devlop の解決で失敗した。同じ既存 global parser を Node subprocess で実行すると参照検査が成功した。再インストールや repo 依存変更はしていない。 | 検査時の実行器の制約。Bun の失敗を削除せず、Node の成功だけを文書参照検査の証拠とする。 |

設計・計画の作成と Issue 化準備の確認であり、アプリの受け入れや実装開始の許可ではない。三環境のユーザー試聴・初期ノブ値、下限 OS／対応画面と倍率、RT と資源回収、物理同期、native ABI／実更新 agent、署名・公証・鍵・実 URL／公開権限は未確認のまま残る。今回アプリ実装、Git 書込み・PR 統合、鍵生成・署名・公開、将来の実装 Issue 作成は行っていない。

## mikan Issue 化の登録確認

2026-10-07、ユーザーの「AI が実装しやすい粒度の mikan Issue をすべて作成する」という明示依頼を受け、レビュー済み計画 P01〜P18 を mounted mikan MCP から登録した。作業環境は既存の macOS arm64、Bun Eval 1.4.2、Node 24.15.0、Lat 0.12.2。Issue ID と依存は作成結果から取得し、予定番号を実在 ID として捏造していない。現在の Status は mikan を読む。

| 対象 | 実行・観測 | 確認できた範囲と限界 |
|---|---|---|
| 登録と保存本文 | `create_issue` で18成果を登録し、`get_issue` で全件の title・全文・metadata・初期 Status・依存を照合した。登録 ID は AMB-003〜AMB-020。各本文に exact 予定 files、Consumes/Produces、型と署名、作業 checklist、入力と期待値、実経路 smoke、停止条件、対象別権限を含めた。 | Issue 作成の実 MCP 経路。アプリ実装・実物の受け入れを完了したことにはならない。 |
| 依存と既存 Issue | 計画の20依存辺を実在 ID に対応付け、欠落・循環なし。MCP の全 inventory を保存した応答の `jq` 集計は全20件、新規 backlog18件、active0件。既存2件は新規登録時に本文・title・Status を保持した。 | 未完了の前提に対する `dependency_incomplete` 警告20件は正常な前提待ちとして残した。それ以外の scanner 警告はなかった。依存解消は実装開始の許可ではない。 |
| 本文の読みやすさと契約 | 各作業が消費する例だけを残し、enum の区切りを表で escape、Normalized の無次元0..1と dry 遅延 frames を明確化した。登録済み75 Interface 行の列数と、表で値が欠落しないことを確認し、修正文も MCP で再取得した。 | Markdown の契約欠落防止。Rust の compile・外部 API の実 ABI・RT 成功の証拠ではない。 |
| 正本への参照と coverage | MCP で取得した18本文を既存 unified／remark-parse／github-slugger で解析。相対327件・見出し237件の参照先7文書に欠落なし。SPEC34節を計画へ対応付け、P18本文にも全 coverage を含めた。 | `.mikan` のファイルを直接読まず、MCP が返した本文と通常文書だけを対象にした。外部 URL の疎通・製品受け入れは対象外。 |
| 文書と実ナビゲーション | SPEC・詳細設計・計画の権限記述を、Issue 作成の依頼と実装開始の区別へ更新。`npm run check:docs` は All checks passed、Lat の `section` と `refs` は Implementation design の本文・正本参照・逆参照を取得した。 | 手動設定の No init version recorded 警告は保持し、無断 init・hook・索引作成を行っていない。 |
| 検査側の失敗と代替 | 初回の警告0件 assertion は未完了依存を想定せず失敗したため、実警告を分類し前提待ちと区別した。巨大な board JSON の Eval／raw 読取りは途中で切れたため、個別 MCP 再取得と全 inventory の MCP 応答 artifact に対する `jq` 集計を使った。一時 parser の文字列 escape は初回失敗し、raw 文字列へ修正後の実行が成功した。 | 不完全な JSON・失敗した検査を成功と扱っていない。新しい依存・永続 test／script は作成せず、mikan 配下の直接参照・編集も行っていない。 |

今回は全 Issue の作成と登録検証までであり、アプリ／Cargo 作成、Git 書込み・PR 統合、認証変更・鍵生成、署名・公証・一般公開・GitHub Issue mirror は行っていない。試聴、配布環境の準備、実署名 helper、最終実機受け入れは、人の判断・環境・操作許可が必要な別の gate として各 Issue に残している。

## P01 固定二窓の限定観測

2026-10-07（ローカル日付）、`feat/amb-003-fixed-windows` 上の未コミットの `ambien 0.1.0` 候補を対象に実施した。基点は `20c02bb64c8d32df6c55f5d97ec7c10c60ac051b`。環境は macOS 27.0.1（26A434）・arm64、rustc 1.95.0（59807616e、2026-04-14）、cargo 1.95.0（f2d3ce0b）、Xcode Command Line Tools。Rust コマンドは既存の `~/.cargo/bin` を当該 process の PATH に渡し、`MACOSX_DEPLOYMENT_TARGET=15.0` で実行した。フル Xcode の導入・選択変更、OS の権限変更はしていない。

| 対象 | 実行・観測 | 確認できた範囲と限界 |
|---|---|---|
| 固定依存と初回失敗 | 最初の `cargo fetch` は、実装側が指定した存在しない Foundation `NSNumber` feature により失敗。実 manifest の `NSValue` と、NSScreen の `NSGraphics` を確認して必要 features だけを修正し、`cargo fetch --target aarch64-apple-darwin` は exit 0、723 packages の Cargo.lock を生成した。 | SPEC の Zed/Ely rev と直接 native versions は維持。依存の差替え・shim・追加インストールはしていない。fetch は実窓や製品全体の成功ではない。 |
| RED と幾何試験 | 六つの geometry tests を先に作成。初回 `cargo test platform::macos::tests --locked` は未実装 API の23 compiler errors により exit 101。native 実装後は6 passed、0 failed。 | 初回は **compile-time RED** であり assertion の失敗ではない。狭い負座標 workarea、基準寸法維持、厳密／小数の往復、無効・取得不能・overflow 入力を検査した。実 NSWindow の保持は別の live smoke で観測した。 |
| 統合検査 | `cargo fmt --all`、上記 geometry test、`cargo clippy --all-targets --locked -- -D warnings`、`cargo build --locked` の一連は exit 0。 | 現行環境での build・型・lint・幾何規則の証拠。下限 OS、画面の外観、RT、音声、物理同期を証明しない。 |
| 実窓の初回 action 失敗 | 同じ `app::start`／view／platform を使う一時 bin を実 GPUI host で起動。設定を二度開くと `Ambien: 設定を表示できません: window not found` を観測。GPUI の active-window action dispatch が窓 slot を一時借用する一次ソースを確認し、ShowScene/ShowSettings を `App::defer` 後の更新へ修正した。 | 最初の smoke が PASS を出していても、この error を失敗として保持する。窓を作り直す fallback や error の抑制では修正していない。 |
| 修正後の live lifecycle | 一時 bin の再実行は exit 0、application error なし、`P01_SMOKE_PASS`。実 NSWindow から scene content 960×600、settings content 480×360／outer 480×392、workarea 内の外枠、表示状態、min=max=current content、zoom 無効、fullscreen/tiling exclusion flags、role 別 titlebar transparency と opacity を観測した。active scene 再表示と設定再表示は同一 handle、設定は active、scene close 後の再表示は新 handle・既存設定維持。全窓 close 後も host が生存し、設定を再生成して明示 Quit action で終了した。 | 実際の GPUI/NSWindow の状態と action/lifetime 経路。close は GPUI の remove、操作は dispatch_action によるため、赤ボタン・物理的な ⌘,／⌘Q・空白 drag・ガラスの pixel・OS タイリングを観測したことにはしない。一時 bin は観測後に削除した。 |
| 実窓の失敗保護 | live 設定窓で取得不能 display の prepare と NaN origin の constrain を実行。typed error を返し、既存 NSWindow の frame・min/max・collectionBehavior は変更されなかった。 | 既存有限窓の失敗保護を実際の native getter で比較した。renderer のメモリ不足や画面撤去の通知経路は別の未実装 gate。 |
| 画面と観測権限 | 2026-10-06T23:59:10Z の AppKit 読取りでは一次画面 logical frame 2560×1440、visible 2560×1410、倍率2。別画面は logical frame `(2560,-81,1066,1600)`、倍率1。System Events の UI elements enabled は false。JXA の capture preflight 呼出しは bridge 関数なしで失敗し、Swift/SDK の `CGPreflightScreenCaptureAccess()` は false を返した。 | live smoke は一次画面のみ。別画面への移動・倍率切替・ガラスの visual capture は未観測。preflight は権限要求ではなく、設定を変更していない。 |

P01 のガラス、シーン／設定操作、最小化と復帰、system appearance、両窓の全 OS サイズ変更／タイリングについて、ユーザーが 2x の主画面での成功を回答した。終了の成功回答後に host が生存した2候補の不一致は履歴として保持する。続く一時診断では actual native ⌘Q → ambien::Quit → native shutdown → host exit0 を観測した。observer／marker を除去した shipping source の最終候補では、[native E2E runner](#native-e2e-runner) から実 AX メニューと foreground ⌘,／両窓からの ⌘Q を操作し、対象 PID の消失と exit0 を確認した。これは現在候補の受け入れ証拠であり、過去の不一致の根本原因を特定したとの主張ではない。別1x画面・倍率切替・負荷へ一般化しない。

独立レビューでは品質・所有・native 安全性と、SPEC/P01・権限・証拠の2軸を別 reviewer が確認し、実装の必須修正はなかった。任意指摘に従い、GPUI f32 境界の content 縮小・underflow と、座標の両側丸め・表現可能値のない区間の境界試験を追加した。検証記録の「アプリ結果がない」という古い現時点の説明も限定観測への案内に更新した。reviewer は build・test・GUI 操作を実施していない。

active-window action 修正と一時 bin 削除後、`cargo fmt --all -- --check`、`cargo test --locked`（この時点で6 passed）、`cargo clippy --all-targets --locked -- -D warnings`、`cargo build --locked` は exit 0。`npm run check:docs` は All checks passed、手動設定の No init version recorded 警告は保持した。別の既存 AST parser による13文書の相対426件・見出し294件の参照確認は欠落なし。追加した f32 試験と文書修正は、この結果の後の変更であり、未実行の検査を成功として含めない。

追加の f32 境界試験後、`cargo fmt --all -- --check`、`cargo test --locked`（8 passed、0 failed）、`cargo clippy --all-targets --locked -- -D warnings` は exit 0。AST の参照確認も相対427件・見出し295件、欠落なし。品質 reviewer は追加した試験を再読して追加の必須修正なしとした。production Rust の動作は、この追加の前に成功した live smoke から変更していない。

通常 entry の `cargo run --locked --bin ambien` を session service `ambien-p01-acceptance` として起動した。起動ログは build 終了と `Running target/debug/ambien`、process 生存を示したが、service の running/ready 表示をガラスの visual proof として扱わない。ユーザーの実画面確認に接続する候補であり、実機 gate の結果を得るまでは PR 未作成・未マージである。

実画面確認の前提として、Swift/AppKit の `NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency` は false を返した。`defaults read com.apple.WindowManager` に表示された保存値からは desktop-edge／Option drag の有効状態を確定できなかったため、OS の機能が無効な状態の no-op をアプリ制約の成功に読み替えない。どちらも読取りだけで設定は変更していない。手動 keyboard tiling の操作例は、現行 macOS 27 が選択された [Apple の公式操作表](https://support.apple.com/en-gb/guide/mac-help/mchl9674d0b0/mac) の Fn-Control-F と Fn-Control-arrow keys を使う。未実施の経路は未確認として残す。

### ガラス表示のユーザー観測

2026-10-07T03:11:55Z に、起動した P01 候補についてユーザーの訂正と選択回答を記録した。先の「今は観測できない」を「見えました」と訂正したため、窓自体の表示とガラスの品質を区別して質問した。ユーザーは、背面に文字や色のある窓を置いた観測について **「背面がぼけて透けた」** を選択した。これはシーンのぼけ・透過のユーザー確認であり、ツール側で screenshot を取得した証拠ではない。

使用した画面・倍率はこの回答だけでは特定できず、別画面・下限 OS・全外観モードへの一般化はしない。native ボタン・drag・menu/key・設定外観・全 OS タイリングはこの回答の対象ではない。ガラス確認をやり直すためだけの再実行は行わず、残る操作を個別に確認する。

### シーン操作のユーザー観測

2026-10-07T03:15:53Z に、シーンの空白を左ボタンでドラッグ→左上の赤ボタンで閉じる→Ambien 実メニュー「シーンを表示」の操作を質問し、ユーザーは **「移動・閉じる・再表示できた」** を選択した。空白 drag、native close、閉窓後の実メニュー再表示を限定確認した。位置保存は後続 P11 の責務であり、P01 では再表示で中央へ戻ることを不具合に含めないと事前に説明した。

この回答で、設定窓の操作、黄色ボタン、physical keyboard、system appearance、OS タイリングまで合格にはしない。使用画面・倍率を一般化せず、ガラス表示の先の成功を保持して次の経路へ進める。

### 設定操作のユーザー観測

2026-10-07T03:31:03Z に、設定窓の実操作に対するユーザー回答を記録した。Ambien を前面にして ⌘, で開く→不透明な背景で説明文が読める→実メニュー「設定…」を再選択しても窓が増えず同一窓が前面→赤ボタン close 後もシーンと app が継続→再度 ⌘, で開き直す、の質問に **「すべて期待どおり動いた」** を選択した。音声などの未接続 controls は後続 Issue の範囲であり P01 の不具合とはしないと事前に説明した。

この回答は設定の外観・実キー／実メニュー・close/reopen の限定確認であり、live system appearance、最小化と復帰、全 OS タイリング、physical ⌘Q の確認ではない。既に確認済みのガラスとシーン操作は維持する。

### 残る4項目の回答と終了状態の切り分け

2026-10-07T03:40:15Z に、両窓の黄色ボタンでの最小化と Dock 復帰、開いた設定窓の live light/dark 追従、両窓の枠・zoom/fullscreen・desktop edge・Option drag・OS menu・keyboard tiling で固定サイズ維持、最後の physical ⌘Q の質問に **「4項目すべて確認できた」** という回答を記録した。使用画面・倍率と各無効／非提供経路の補足はなかった。画面上の成功報告は維持するが、未特定の画面・倍率へ一般化しない。

その後、named service は running と報告し、`ps -p 59837 -o pid,ppid,state,comm` は元の候補が PID 59837、PPID 36768、`Ss+`、`ambien` として生存していることを示した。Swift の NSRunningApplication 読取りでも executable は当該 `target/debug/ambien`、active=false、hidden=false、terminated=false。1秒の `sample` は main thread が `NSApplication run` のイベント待ちにいることを示した。終了の見え方を否定せず、候補の同一性・操作経路・プロセス寿命の不一致として扱う。

原因層の識別用に、現存 app/platform/ui をそのまま使う一時 bin で実 GPUI の `Window::dispatch_keystroke("cmd-q")` を動かした。probe 初版は `cx.update` の戻り値 `()` に `expect` を付けたため compile exit101となり、probe の呼出しだけを訂正した。シーン前面の場合と、二窓が存在して設定が前面であることを確認した場合の双方が `handled=true`、process exit0。production source は変更していない。この結果は実キー入力の再確認ではなく、現存 keybinding から native shutdown までの限定 smoke である。

一時 bin と sample ファイルは削除した。元の候補 PID 59837 に NSRunningApplication.activate を実際に要求し、candidateFound=true、activateRequested=true を観測した。最初の activation probe は Swift 補間の escaping ミスで式を文字として出力しただけだったため、実際の要求とは数えず訂正後の結果を採る。残る確認は、この特定候補の実メニュー終了と、既に確認した画面・倍率の特定である。

### 候補の再表示

ユーザーが元候補の実メニュー終了の質問に「もう一度表示お願いできますか？」と回答したため、候補を重複させず再表示した。PID 59837 の NSRunningApplication に通常の terminate を要求し、candidateFound=true、normalTerminateRequested=true、supervised process exit0 を観測した。強制終了・source 修正・OS 設定変更はしていない。

同じ `cargo run --locked --bin ambien` を `ambien-p01-acceptance-reopened` として起動し、build 0.30秒、`Running target/debug/ambien`、PID 85344 を観測した。これは再起動の証拠であり、ユーザー実画面での再表示や physical ⌘Q の代わりとはしない。新候補に対する終了と、既に成功した観測の画面・倍率を次の確認対象とする。

### 観測画面と actual native quit

再表示した候補に対しユーザーは「見えた」と回答し、以前の成功したガラス・窓操作・固定サイズ／タイリングの画面は、別の選択質問で **「2x の主画面」** と特定した。1x の別画面・倍率切替の成功には一般化しない。新候補 PID 85344 の foreground click→⌘Q の質問にも「アプリが終了した」と回答したが、host は `Ss+` で生存し、Swift の当該 NSRunningApplication は active=false/hidden=false/terminated=false、CGWindowList の onscreen-only 行は空だった。窓の消失と host exit を混同しない。

PID 85344 を通常 terminate 要求で exit0 とし、同じ ambien binary の入口に一時的な owned-app observer を追加して候補 PID 32084 を起動した。観測対象は当該 app の Q 解決・Quit action・window close・native shutdown だけで、他 app のキーを監視していない。こちらがキー入力や terminate を要求する前に、service は45.3秒で exit0となり、`Quit action received`、`KeystrokeEvent` の platform=true/control=false/alt=false/shift=false/function=false、key="q"、key_char=None、action="ambien::Quit"、context_stack=[]、`native shutdown received` を記録した。実 native 入力から現存 action・shutdown・host exit までを識別できた。過去2候補の不一致の原因を特定したとは主張しない。

一時 observer を全て除去し、入口は元の `.run(app::start)` に復元した。復元 source で fmt --check、8試験、clippy --all-targets -D warnings、build --locked が全て exit0。診断の追加を恒久 fix とせず、review 済み source に戻した。instrumentation なしの `ambien-p01-shipping-quit` を PID 50490 として起動し、build0.30秒／Running を観測した。最後に Dock 前面化から physical ⌘Q を行ったときの host exit を照合する。

### 診断除去後の不一致

最終候補 PID 50490 に対し、Dock 前面化→physical ⌘Q の質問にユーザーは「終了した」と回答した。ただし service は ready、`ps` は同じ PID の `Ss+ ambien` を示した。診断入り候補の native key/action/shutdown/exit0だけを、復元した source の終了成功へ転記しない。原因は未特定であり、PR を統合しない。

識別する差分を native input／simulated IME／一時 observer に絞り、pinned GPUI の read-only 追跡と、owned NSApplication へ AppKit NSEvent を postする一時 probe の準備を別の担当に分けた。OS-wide 入力注入・TCC／依存変更・delegate 置換はせず、実行結果を待って恒久変更の要否を決める。ユーザーの画面上の成功報告と、tool が観測した host 寿命の両方を保持する。

### Controlled native event の対照

現存 app/platform/ui を使う一時 bin で、active GPUI role と native key window number を確認し、App／Window の借用を終了してから own NSApplication queueへ command-q の NSEvent KeyDownを postした。OS-wide 入力注入はしていない。シーン／設定と observerなし／ありの4組全てが host exit0、deadline超過なし。observerありでは key="q"/platform=true/key_char=None→ambien::Quit→native shutdownもログした。probeは削除した。pinned source の独立追跡でも、cmd-q は simulated IMEで key_charを変えず、一時observerがshutdownを有効化するという根拠はなかった。

これでnative queue routeがobserverを必要とするという仮説は支持されなくなったが、shipping候補のphysical入力とhost寿命の不一致は未解決である。PID50490は通常terminate要求でexit0。global macOS NSUserKeyEquivalentsのreadはkeyなしだったが、全入力設定／app別設定の不存在へ一般化しない。

次の識別では、既存Quit callbackの `cx.quit()` の直後に一行markerを置き、listener数／順序／subscriptionを変えずenqueueから戻ったことだけを記録する。候補 `ambien-p01-menu-quit-diagnosis` PID87074を同じbinaryで起動(build1.04秒)。実メニュー「終了」でproduction callbackへ到達するかを見る。これは原因識別の一時markerであり、恒久fix／PRへ残さない。

## macOS native E2E 自動化の調査

2026-10-07、P01 の繰り返し操作をこの coding AI に任せる方法を一次文書で調査した。**現在の環境では、小さなローカル AXUIElement helper と決定的な scenario harness を最初の候補とし、必要な foreground key/drag を許可済み caller の CGEvent で補う**。親担当の Swift caller は AX trust と event-post preflight が true、native menu/window の限定 smoke も後述の範囲で成功した。capture は未許可。既製工具の便利さが必要なら Peekaboo の通常 CLI をこの AI が操作する方式を選べる。MCP 接続も可能だが、Peekaboo 自身の `agent` や別モデル／provider／API key は不要。Peekaboo/Appium の能力は一次文書に基づく候補であり、ここで実行成功した工具ではない。[Peekaboo README](https://github.com/openclaw/Peekaboo/blob/main/README.md)／[MCP](https://github.com/openclaw/Peekaboo/blob/main/docs/MCP.md)

### 選択肢と操作経路

| 方法 | 適する範囲と前提 | P01 での限界 |
|---|---|---|
| Peekaboo CLI／MCP | macOS 15+ の capture、AX inspection、menu、close/minimize/restore、synthetic key/drag を既製工具として使える。この AI が通常 tools を呼ぶ方式と、provider 設定が必要な Peekaboo `agent` は別物。[README](https://github.com/openclaw/Peekaboo/blob/main/README.md)／[automation](https://github.com/openclaw/Peekaboo/blob/main/docs/automation.md) | exact PID/window と fresh observation を使う。実候補の native menu/window と traffic lights は AX で観測できたが、それを GPUI の描画内 control 全体の semantic coverage としない。coordinate 操作も可能だが、focus・倍率・bounds を拘束し、操作後の状態を別に観測する。 |
| 小さなローカル AXUIElement helper | Apple の [PID 指定の application element](https://developer.apple.com/documentation/applicationservices/1459374-axuielementcreateapplication)、[attribute read](https://developer.apple.com/documentation/applicationservices/1462085-axuielementcopyattributevalue)、[action request](https://developer.apple.com/documentation/applicationservices/1462091-axuielementperformaction) で native menu/window の観測・操作に絞れる。別 AI・サーバーは不要。 | caller の Accessibility trust が必要。[trust check](https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions) は permission grant とは別。今回の direct Swift caller は trusted=true。AX に公開されない情報は読めず、独自 helper の timeout・identity・error/readback を保守する必要はあるが、P01 の native 操作には最小の導入候補。 |
| XCTest／Appium Mac2 | Apple の [XCUIAutomation](https://developer.apple.com/documentation/xcuiautomation) は UI interaction と state assertion の正式な仕組み。Mac2 は XCTest/WebDriverAgentMac を WebDriver 経由で動かし、click、drag、key 操作を提供する。[overview](https://appium.github.io/appium-mac2-driver/v4/overview/)／[execute methods](https://appium.github.io/appium-mac2-driver/v4/reference/execute-methods/) | 長期の regression suite には候補だが P01 だけには導入が重い。Mac2 v4 は Appium 3、full Xcode 13+、その bundled CLT の選択、Xcode Helper の Accessibility、runner/session 構成が必要。[getting started](https://appium.github.io/appium-mac2-driver/v4/getting-started/) appPath/bundleId と session の自動 relaunch/kill を管理し、driver cleanup を Quit 成功に数えない。[capabilities](https://appium.github.io/appium-mac2-driver/v4/reference/capabilities/) |

**background targeting と foreground keyboard の試験は別にする。** Peekaboo の default は対象が分かると background AX／targeted input。menu click、window close と minimized restore は AX 経路であり、実 menu/window に作用するが物理 mouse/key の証拠ではない。CLI の exact-window background chord と、MCP の fresh exact non-dialog snapshot を必要とする chord も foreground shortcut の代用にはしない。foreground mode は対象を focus して通常/global input を送るので、⌘,／⌘Q の試験では明示的に選び、対象 app と key window を直前に検証する。それでも入力は **synthetic** であり physical keyboard の再現済みとは書かない。command の成功や `dispatched_unverified` は効果の合格ではない。[automation](https://github.com/openclaw/Peekaboo/blob/main/docs/automation.md)／[press](https://github.com/openclaw/Peekaboo/blob/main/docs/commands/press.md)

Apple の [CGEvent.postToPid](https://developer.apple.com/documentation/coregraphics/cgevent/posttopid(_:)) は PID 宛て、[CGEvent.post(tap:)](https://developer.apple.com/documentation/coregraphics/cgevent/post(tap:)) は event stream への投入である。own NSApplication queue への NSEvent post や GPUI dispatch での過去 smoke と、OS foreground input の E2E を区別する。Peekaboo の bounded background drag は exact-window 内への未検証・retry-unsafe dispatch であり、P01 の空白を掴んで窓を移動する操作の成功証拠へ自動的に置き換えない。human-like/window-moving drag を調べる場合は明示 foreground と frame readback を使う。[automation](https://github.com/openclaw/Peekaboo/blob/main/docs/automation.md)

### 権限と現環境の前提

- **capture**：Peekaboo の screenshot／CGWindow capture は Screen Recording（設定 UI の Screen & System Audio Recording）が必要。静止画許可を音声収録の実施と混同しない。
- **AX inspection/action**：Accessibility が必要。background semantic click、menu、window action はこの経路。
- **synthetic keyboard／drag**：Peekaboo の Event Synthesizing grant は background keyboard、bounded background drag、foreground synthetic pointer に必要。foreground key も入力送信者の permission 状態を確認する。Peekaboo 文書は Apple Events Automation permission を不要としているので、System Events 経由 AppleScript の追加権限を当然の前提にしない。helper が CGEvent を送る場合も AX trust だけから許可を推定せず、Apple の [post-event preflight](https://developer.apple.com/documentation/coregraphics/cgpreflightposteventaccess()) を分けて扱う。以上の Peekaboo 権限は [permissions](https://github.com/openclaw/Peekaboo/blob/main/docs/permissions.md) に対応する。
- **誰に grant するか**：実際の caller／selected Bridge host に対する grant を確認する。CLI を起動した Terminal だけへの許可では十分とは限らない。background runner では Aqua session と matching Bridge host/version も確認する。[permissions](https://github.com/openclaw/Peekaboo/blob/main/docs/permissions.md)
- **現在**：親担当が `/usr/bin/swift` の ApplicationServices/CoreGraphics で prompt なしの preflight を実行し、`AXIsProcessTrusted()=true`、`CGPreflightScreenCaptureAccess()=false`、exit0 を観測した。追加の CoreGraphics preflight は `CGPreflightPostEventAccess()=true`、exit0。direct Swift caller の AX trust と event-post 許可は確認済みであり、過去の System Events の UI enable=false から direct AX が使えないとは判断しない。これは実操作の効果や Peekaboo の caller/Bridge permission へは一般化しない。親担当の PATH 確認では Peekaboo／Appium は未検出、今回の利用経路は未導入・未構成。xcode-select は CLT であり、`xcodebuild` の存在だけで full Xcode/Mac2 の前提を満たしたとはしない。本調査担当は install、MCP/config、認証、TCC／OS 設定変更、入力注入、アプリ操作を実施していない。

開始には、対象アプリの inspection/action への明示許可、対象 binary と実 PID の識別、実対象の native menu/window AX tree、ローカル証拠の保存先が必要。現在の trusted Swift caller と同じ実行経路を使う AX-only helper は新工具導入や screenshot grant を必須としない。別の compiled helper/caller を使う場合はその identity で permission を再確認する。Peekaboo を選ぶ場合は追加導入とその caller/host の grant 確認、synthetic key/drag を選ぶ場合は送信者の permission 確認とユーザー入力が競合しない foreground 試験時間が必要になる。capture は別途許可が必要で、ローカル保存できることと AI client/provider への画像共有が安全・許可済みであることは別。MCP `see` は inline image を client に返すため、共有許可なしに画像をこの AI／外部 provider へ送らない。provider-backed `analyze` も使わない。[MCP](https://github.com/openclaw/Peekaboo/blob/main/docs/MCP.md) Mac2 の authentication prompt を無効化する方式は採用しない。

### 決定的な P01 scenario harness

AI は初回の操作選択・失敗調査を助け、繰り返しは固定 scenario と assertion にする。毎回同じ shipping candidate を一つだけ起動し、cargo wrapper ではなく実 binary の PID・process start identity・executable と各 native window ID/role を記録する。window ID はその process/session 内の identity として使い、再起動や close 後の永続 ID とはしない。fresh native state を期限付きで待ち、曖昧な target、permission 不足、未公開 attribute は「未確認／中止」にする。indeterminate input を blind retry せず、先に readback する。[window identity/readback](https://github.com/openclaw/Peekaboo/blob/main/docs/commands/window.md)

process exit は supervisor の終了通知／status と OS の PID 存在を突き合わせる。`NSRunningApplication.isTerminated` だけを main runloop を進めずに sleep-poll しない。Apple は time-varying properties が main runloop の次 turn まで保持され、instance 自体は app exit 後も有効と説明している。helper 側の観測 timeout と対象 app の生存を混同しない。[NSRunningApplication](https://developer.apple.com/documentation/appkit/nsrunningapplication)

| scenario | 外部操作と機械的 assertion |
|---|---|
| menu／同一窓 | native「シーンを表示」「設定…」を操作。role 別 window 数、owner PID、表示／focused state を読む。既に開いた role の再表示は同じ live window ID、重複なし。 |
| close／再表示 | native close control を操作。対象 ID の消失、他窓と PID の生存を確認。全窓 close 後も PID が生存し、menu から role を再生成できる。AX close と foreground red-button click の証拠を区別する。 |
| minimize／restore | 両窓の native minimized state を確認し、restore 後は同じ live ID・寸法。background `AXMinimized` restore と Dock click の試験は別に記録する。 |
| ⌘, | scene/settings foreground を分け、直前の frontmost PID／key window を確認して synthetic foreground chord。設定窓の生成・同一窓再利用・前面化を fresh native state で確認する。 |
| 固定 bounds／drag | 操作前後の outer frame と content size を別々に読む。content の 960×600／480×360、画面 workarea、倍率を記録し、許可した resize/zoom/tiling／blank drag の経路ごとに寸法保持・移動を判定する。AX resize が拒否されたことだけで全 OS tiling 成功とはしない。content geometry が外部から読めない場合は不足を明記し、outer frame を content size と偽らない。 |
| menu Quit／⌘Q | 別々の起動で native menu Quit と synthetic foreground ⌘Q を試す。scene/settings を key window にした条件を分ける。**起動時に捕捉した実 PID の終了と supervisor の exit status を deadline 内に確認**し、窓消失や tool dispatch 成功だけを PASS にしない。timeout 時は状態・ログを保存して失敗とし、cleanup terminate や driver session deletion を成功へ転記しない。 |

各 step は時刻、scenario、candidate identity、target PID/window、delivery mode、操作 request/result、前後の属性／bounds、assertion と timeout、stdout/stderr、許可したローカル capture の所在を残す。AI の「見た感じ成功」ではなく assertion を合否の根拠とする。ガラスの心地よさ・可読性・appearance 品質、音の心地よさと実出力同期はこの操作 harness だけでは受け入れられず、ユーザー観測／試聴・対応する計測を残す。調査は既存 P01 の成功報告と host 寿命の不一致を消さず、SPEC、製品要件、受け入れ gate、未解決 Quit の判定を変更しない。

### ローカル AX helper の限定実行

親担当は unchanged shipping binary を別候補として起動し、direct Swift helper から外部 AX inspection/action を実行した。native tree は `AXWindow/AXStandardWindow`、`AXGroup/AXUnknown`、`AXCloseButton/AXZoomButton/AXMinimizeButton`、own menu の `ambien → シーンを表示／設定…／終了` を公開していた。アプリ内部 dispatch や source hook で代用していない。

初回候補 PID80499 では helper が main runloop を進めず cached `NSRunningApplication.isTerminated` を sleep-poll したため、最後の終了待ちで fatal timeout／exit133。stdout の PASS 行は buffer に残り取得できなかったので成功証拠には数えない。一方 target supervisor は app exit0 を報告した。これは helper の観測失敗として保存し、対象 app の終了失敗へ読み替えない。

helper の output を unbuffered にし、OS の PID 存在を `kill(pid, 0)`／`ESRCH` で観測する方式へ修正。候補 PID93697 の実行は2.24秒、helper exit0、target supervisor exit0。stdout の4 PASS は **実メニューの設定表示・再選択で同一 AX window維持／native 設定 close 後の app 生存／native scene close と menu 再生成（旧・新 AX element は CFEqual で不一致）／実 AX menu Quit と対象 PID93697 の終了**。close 後の PID 生存と、Quit 後の `kill(pid, 0)=-1`／`ESRCH` を明示的に確認した。

この実行では screenshot、minimize/restore、blank drag、固定 bounds/tiling、foreground ⌘,／⌘Q、ガラス・音の品質は未検証。AX menu Quit の成功を以前の physical ⌘Q と host 生存の不一致の解消へ一般化しない。production source、工具導入／config、TCC／OS 設定を変更しておらず、一時 helper は除去した。これは小さなローカル helper による外部操作・PID assertion の限定証拠であり、P01 総合合格や恒久 harness の完成ではない。

固定 GPUI は macOS で [AccessKit→NSAccessibility の bridge](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui_macos/src/window.rs#L2356-L2407) を持つ。default features 無効だけを根拠に AX がないとは判断しない。一方、現存の settings Paragraph／Heading と blank scene は semantic content node を付けておらず、今回の AX 実観測も native window／信号ボタン／menu が対象である。[node inclusion の role／ID条件](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui/src/element.rs#L365-L383) と [Ely typography の anonymous rendering](https://github.com/ZacharyZhang-NY/Ely-GPUI-Components/blob/7e39a9383dc2fd7404751d02cb166e95861ea3d6/src/typography/text.rs#L173-L192) を区別し、今後の custom control 全般が AX 非対応とは一般化しない。

### Native E2E runner

ユーザーの「確認できる操作を AI に任せて実装を進める」依頼に基づき、[scripts/native-e2e.swift](../scripts/native-e2e.swift) を追加した。SDK 同梱の Swift／AX／CGEvent のみを使い、渡した `ambien` executable を自分で起動して、その PID の AX tree を操作する。状態待ちに5秒の deadline、各 AX message に runner プロセスだけの global 1秒 timeout を置き、foreground 入力は対象 PID と `AXFocusedWindow` が選択窓に一致することを確認してから送る。capture、permission prompt、OS 設定変更、in-app dispatch、source hook は使わない。失敗時の自分の子プロセスの SIGTERM cleanup は、Quit 成功には数えない。

初回 compile は Swift の throwing `&&` に対する `try` の不足で失敗した。修正後の初回起動は AX server 準備前の `AXWindows` messaging error `-25204` で停止した。startup のみ `cannotComplete` を5秒以内で待つ境界に修正し、通常操作での AX error は失敗を維持した。いずれも runner の不成立として記録し、アプリの成功には数えない。

その後の `/usr/bin/swift scripts/native-e2e.swift /Users/takemo101/Desktop/workspace/ambien/target/debug/ambien` は4.36秒・exit0。実メニューによる設定表示／再選択で同一窓、両窓の native minimize／AX restore と外枠寸法不変／ZoomButton 無効、設定 native close 後の host 生存、foreground ⌘, による設定再生成、scene native close → zero-window host → menu 新窓生成・外枠寸法不変、実 menu Quit、scene／settings を前面にした foreground ⌘Q が PASS。3候補 PID83723／83882／84024 は、それぞれ対象 PID 消失と正常 exit0 を確認し、captured application error はなかった。

この runner の restore は AXMinimized の変更であり、Dock 操作の再確認ではない。今回の寸法観測は AX の外枠であり、過去の native getter による content 観測と区別する。blank drag・ガラス・system appearance・edge／Option／menu／keyboard tiling は先行する2x主画面のユーザー確認を維持し、今回の runner で実行したとはしない。CGEvent は OS foreground 入力であり、実物のキー押下そのものでも全入力レイアウトの保証でもない。画面録画 permission は未許可のまま。別1x画面・倍率切替・GPU負荷・音・保存・配布はこの結果の対象外である。

独立 Spec reviewer は P01 の source・既存2x主画面のユーザー確認・新 native E2E の証拠構成に必須欠落なしと判定した。Standards reviewer の必須指摘3件は、`AXMain` だけでは keyboard focus の証拠にならないこと、app element の timeout が他 AX element に継承されないこと、失敗時の stderr が失われること。SDK／Apple 文書を照合し、`AXFocusedWindow` の型と `CFEqual` を待ち・送信直前に確認、`AXUIElementCreateSystemWide` へ正の global timeout を設定、stderr を O_EXCL／0600 の private temporary file へ記録して元の失敗とともに回収するよう修正した。owned child cleanup は bounded terminate／reap とし、log は回収後に削除する。これは runner 内の観測修正であり、production source や TCC の変更ではない。

startup failure 用の使い捨て executable は、修正前に runner exit1 だが診断欠落、修正後に `EXPECTED_NATIVE_E2E_STARTUP_DIAGNOSTIC` と元の startup error を出して exit1。fixture と空 directory は除去した。修正後の実 Ambien native E2E は3.97秒・exit0、全 scenario が PASS。menu Quit PID13013、scene／settings の foreground ⌘Q PID13167／13168 は、送信直前の exact keyboard focus と実 PID 消失・正常 exit0 を確認した。

修正後の Standards follow-up review は3件の必須指摘の解消と、残る evidence-backed 必須指摘なしを確認した（source review のみ）。統合担当の `cargo fmt --all -- --check`、`cargo test --locked`（8 passed）、`cargo clippy --all-targets --locked -- -D warnings`、`cargo build --locked` はすべて exit0。`npm run check:docs` は All checks passed、既存の No init version recorded 警告は保持した。再buildした候補の native E2E は4.91秒・exit0／全 scenario PASS、menu／scene keyboard／settings keyboard Quit の PID19406／19418／19419 が消失・正常 exit0。実行した binary の SHA256 は `da593032c8f9a05dcf789353b5e3c3842d16c3dbb8320daea49b07d8d637eab3`。PR の head・normal merge・workspace 同期は mikan Report に記録し、これらの観測から未実施の製品全体の合格を作らない。
