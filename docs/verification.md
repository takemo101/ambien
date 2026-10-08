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

## P02 雨の標準出力と視覚境界の限定検証

2026-10-07、Darwin 27.0.0／arm64、`feat/amb-004-rain-audio` の未 commit source、P01 normal merge `cd3cc1b762455881f2dc70fb9d73cfebfa228316` を基点として観測した。固定 FunDSP／CPAL と実 Cargo lock を使い、GPUI host を維持した。試聴候補はアプリ volume 0.30、雨量 0.34、風 0.12。確定済み音質・初期値とは呼ばず、OS 音量・TCC・画面ロックは変更していない。

### 自動試験とレビュー

初回の統合試験は fixture の未実装 `Revision::checked_next` と `An`／`AudioNode` の型違いで compile 失敗した。存在する `Revision::from_raw` と実 node の fixture に修正した。次の55 pass／1 failは、補償加算した世界秒と一度の除算を bit-exact に比較していた差（1 ulp）であり、生成 frame 数の不変条件に修正した。これらを対象規則の assertion RED と取り違えない。

レビューは復帰初回の古い粒、故障時の再読取りによる最終表示の変化、GPUI globals の Drop に依存した worker 終了を必須指摘した。凍結 cache の regression は修正前に `MachNanos(99)` 対期待 `MachNanos(40)` で assertion RED、修正後に受入済み header・粒・gains/breath・fade 時刻を保持した。新規の凍結シーンは checkpoint で一度だけ初期化する。復帰は timer の前に live 読取と notify を行い、quit は保持した `Context::on_app_quit` で停止・回収・join する。両 reviewer の follow-up は対象範囲に必須指摘なし（source review のみ）。

`cargo test --locked --bin ambien` は60 passed／0 failed、1.67秒。実 f32/i16 callback entrypoint の busy／closed／worker 所有の silence と世界不変、checked timestamp overflow、可変 frame と channel／端数、mute の世界継続、旧 epoch、atomic snapshot、amount／wind の zero・tail、64粒満杯と視覚 overflow、実 Mach clock、凍結 cache を含む。実 callback と満杯 pool の計測区間で Rust の確保・再確保・解放は0だった。これは `cfg(test)` の thread-local `GlobalAlloc` 観測であり、C／native heap、CPAL が user callback 前に行う timestamp conversion、条件付き owned error の整形・Drop、contended error discard、最悪 timing を測ったとはしない。

### 実 host と限定的な描画観測

使い捨て observer executable は現存 production modules を含み、実 GPUI／CPAL 標準出力を起動した。標準 stream は44,100 Hz・2 channel・F32、出力状態と予定時刻付き sample 世界の進行を観測した。GPUI public close API で実シーンを除去した後も zero-window host が生存し、世界 snapshot は0.767→1.800→2.833秒と進んだ。その後 epoch error cell へ `DeviceNotAvailable` を注入すると、実 worker が出力を止め、engine 世界3.007秒を保持し、実 app consumer が設定窓を再生成した。以降の世界 snapshot は3.000秒のままだった。これは実 Device の除去・標準 Device 不在の試験、物理スピーカーでの継続聴取、red button 操作の証拠ではない。

実 GPUI host の programmatic quit は修正前に exit0でも `worker_shutdown_complete=false`、修正後は停止・join の後に `true` と正常 exit0を観測した。両実行は4.12／4.18秒。Native keyboard・menu Quit の再確認と区別する。

同じ実 callback entrypoint の throwaway smoke は、確保／解放各1回の positive control で観測器を確認した後、frame 1／127／513、channel 1／2／6、両 format 各256回、最大 knob・容量1の視覚 overwrite を実行した。984,576 sample frames、世界22.325986秒、有限出力、error bits0、各区間の Rust heap操作0。callback pair の観測最大は17.875〜6,611.584 µsで、frame1／mono の初回区間には1,421 µsの外れ値もあった。debug build の直列 fixture／wall time であり、実 RT thread の deadline・native backend 全体の保証には用いない。

`gpui_platform/test-support` を使った `Window::render_to_image` は、自分の実 rendered scene から1920×1200 RGBAを2枚取得した。異なる位置の淡い青い点と弱い中心 glow を画像で確認した。画面録画 permission は使わず、実際の canvas／Metal の出力確認である。ただし offscreen readback に native glass、WindowServer の呈示、音との物理同期は含まれない。取得時の debug accessibility tree は `None` だった。observer／SDK の診断 source は最終検査前に除去し、production に source hook を残さない。

### Native 前提の不成立と残る受け入れ

新しい候補の既存 native E2E runner は `AXSize: -25205` で exit1した。SDK attribute probe では `AXWindows` の子が `AXApplication` を報告し、size／position は unsupported。一方 CGWindow metadata に自分の窓は存在し、stderr／startup panic はなかった。保存していた P01 の同一 SHA256 binary でも同じ AX 結果となり、P02 による regression と判定しない。AX 制約の原因そのものは未確定である。

別の SDK probe は主 display の `CGDisplayIsAsleep=1`／`CGDisplayIsActive=0`、session の on-console=1／screen-locked=1 を観測した。自分の app は activation policy Regular、activate の戻りはtrueでも foreground PIDは変わらず、自分の窓中心は別 PIDの窓に覆われていた。CGEvent driver は foreground 不成立で停止し、その owned child の SIGTERM cleanup を Quit 成功には数えていない。初回 error 注入時の cache時刻0→checkpointという観測は、まだ受入 frame のないシーンであり、表示済み cache の不具合証拠には用いない。

画面ロック解除・display復帰後に、既存の外部 native E2E、雨の実表示・同じ左右の光、scene close 後の音継続、故障表示の実画面、visibility復帰、実 key／menu Quitを確認する必要がある。試聴・物理同期・native RT error／最悪負荷の未確認を、自動試験・own-frame画像・programmatic lifecycle・source確認から合格にしない。受け入れ不足のまま normal merge／completed／P03への移行は行わず、次の操作は mikan と SPEC の gate に従う。

最終統合検査の初回は fmt・60 tests が通った後、strict Clippy が nested `if` 3件と fixture の `% 2` で exit101した。同じ short-circuit／入力境界の let-chain と `is_multiple_of` に直し、lint allow は加えていない。診断 source 除去後の `cargo fmt --all -- --check`、`cargo test --offline --locked`（60 passed、1.62秒）、`cargo clippy --all-targets --offline --locked -- -D warnings`、`cargo build --offline --locked` は exit0。`npm run check:docs` は All checks passed、既存の No init version recorded 警告を保持した。最終 `target/debug/ambien` の SHA256 は `4d6a9d1195a2f78c7d72592bb0492c757b7c7e741c45efdf03cf77ae3d455c76`。

受け入れ境界で SDK を読み直すと display は asleep=0／active=1に変化していたが、screen-locked=1は継続していた。AX／foreground scenario の同じ失敗を反復せず、未解除の前提を維持した。最終の診断なし binary 自体を SDK の owned child として起動し、native CG window 1個と startup stderrなしを観測した。`NSRunningApplication.terminate()` による cooperative quit は requested=true、PID62099が正常 `.exit`／status0で終了し、parent SDK も exit0。これは native termination request の実経路であり、foreground ⌘Q／menu操作・実表示・試聴の合格へ広げない。P02 の commit／PR／normal merge／completed は未実施である。

### 外部 native 再検証と雨のユーザー確認

続行依頼後の SDK 読取りでは、on-console=1、主 display の asleep=0／active=1、前回の screen-locked=1 は返らず lock metadata は nil だった。nil だけから未ロックを保証せず、既存の外部操作を実行した。ロック中の起動を抑止する案の使い捨て fixture は、この時点で前提が成立しなかったため regression と扱わず、runner への変更は行わなかった。

`swift scripts/native-e2e.swift /Users/takemo101/Desktop/workspace/ambien/target/debug/ambien` は4.54秒で8件の PASS と最終完了を返した。実メニューによる同一設定窓、両窓の最小化／復元・寸法不変・zoom無効、設定 close、foreground ⌘,、シーン close／zero-window host／再生成、menu Quit、シーン／設定からの foreground ⌘Q を確認した。Quit の PID は6842／7142／7145で実消失・正常 exit0。対象 binary の SHA256 は上記の最終統合候補と同一だった。

同じ external AX 操作を使う使い捨て SDK preview は PID22184 を起動し、シーンを15秒前面表示、実 close button で閉じて8秒、実メニューで再生成して8秒後に実 menu Quit した。32.44秒で native lifecycle／正常 exit0を確認した。OS音量、TCC、画面 capture、アプリ source hook は変更・使用していない。ユーザーは「確認できた」を選び、雨・近い粒の音と左右の光・シーン閉鎖中の音継続・再表示時に古い粒が一斉に出ないことを確認した。これはユーザーの試聴／実表示の証拠で、AI が音を聞いたとの主張ではない。

P02 の今回の smoke 条件の受け入れが揃った。三環境の心地よさと初期値確定、物理的な50ms以内の同期、native backend 全失敗経路・最悪負荷、別画面・下限OS・署名配布は未確認のまま各後続 gate に残す。PR の normal merge・workspace同期・Issue 完了は mikan Report で記録する。

## P03 雨ノブ・音量・独立消音の限定検証

### RED とレビュー指摘の修正

対象は P02 統合後の `feat/amb-005-rain-controls` の worktree、Apple Silicon／Darwin 27.0.0、固定 Cargo.lock の dependency である。変更前の shipping binary を外部 AX で起動すると、実メニューの「消音」がなく exit1した（1.54秒）。これは expected RED で、失敗後の owned child cleanup を Quit 成功にはしない。SDK stderr の IMK mach-port message も消去・成功扱いしない。

既存 `output_f32`／`EngineLease` を用いる seed 固定の音量・消音急変 regression は、変更前に2件とも失敗した。frame0の音量変更は期待 `-0.000119940196` 対 `-0.00031983876`、消音変更は期待 `-0.000119939796` 対 `-0`。独立した有限長 ramp に変更し、同じ callback consumer の検査で確認した。metadata wrapper の初回コンパイルは unkeyed `Div` の `aria_label` で失敗し、安定 ID を付けて解消した。key listener は具体的な `KeyDownEvent`／`KeyUpEvent` 型を指定した。

独立レビューは、受理した新制御を callback 途中失敗時に公開できないと、凍結 worker が旧受理を保持し続ける点を指摘した。新 regression は正常 callback→mute 編集→最大 u64 の再生時刻による実 callback timing fault→lease の worker への排他移譲を通し、修正前に ack `Revision(0)` 対期待 `Revision(1)` で失敗した。新 revision 採用時に受理未公開を記録し、凍結 worker が世界を進めず当該受理を公開する修正後は通った。

もう一人の reviewer は、固定 Ely が選択済み segment の change を抑止し、明示的な雨の再選択が消える点を指摘した。mouse bubble に加え、selector 内の clean Enter／Space pair と実 focus handle／blur subscription で再選択を配送し、新 segment の change は propagation を止める。両 follow-up は source review の対象範囲に重要指摘なし。実 UI の event／focus 成功を source review から主張しない。blur は draw 時の通知であり、任意の同一 frame focus ABA の完全な検証ではない。

### 実標準 stream と通知

production audio／controls／situations modules を直接含む使い捨て CLI を、標準 Device の実 worker／CPAL stream で実行した。44,100 Hz・2 channel・F32。mute revision1を受理した後、mute を維持して volume0.8 revision2を受理し、manual revision は初期値のままだった。消音中の実 `VisualRing` state 世界は0.033→0.533秒と進み、その500msでは出力状態不変かつ main 通知0件だった。volume0.2と mute解除の最新 snapshot は revision4で受理し、initial receipt の世界0.012→最新0.708秒。明示 shutdown の worker join が完了し、全 command は exit0（対象 regression と CLI の合計3.74秒）。

これは実 Device の control受理／論理世界と通知の証拠であり、物理スピーカーの静音・解除後の聴感、実画面の光、OS音量の比較、物理同期の合格にはしない。部分 modules の使い捨て CLI に出た unused-code warnings は shipping source の lint 結果ではない。CLI source は除去した。依存・feature・production testing hook は追加していない。

### Native 観測と現在の前提不成立

最初の実 P03 shipping AX 観測では、「設定」窓と3つの Slider（雨34%、風12%、音量30%）・「消音」CheckBox0を確認し、実 menu Quit による PID消失・exit0を観測した（3.10秒）。numeric値は候補端点の表示であり、音質の採用値ではない。generic wrapper の label はこの tree に現れなかったため、現存 metadata と画面に表示する端点／受理文を意味付き Group／Label にも渡すよう修正した。この修正後の AX 内容は未観測。

後の SDK 実行は Settings window missing（1.36秒）、さらに native window作成後の title を待つ診断では両窓の AXTitle が `ambien` のまま timeout（buildを含め7.93秒）だった。読取り専用 SDK で session の screen-locked=1／on-console=1、主 display の asleep=0／active=1を確認した。前回のロック中 AX 制約と整合するが、これだけで title の実装不具合や完全な原因特定を宣言しない。対象の cleanup は正常 Quit 成功に数えず、同じ操作失敗の反復を止めた。OS／TCC／認証や画面 capture は変更・使用していない。

解除後に必要なのは、診断なし binary の実雨ノブ・音量・menu／設定 mute の交互操作、受理と最新値の一致、明示再選択の mouse／keyboard、viewport／受理更新中の focus 保持、消音中の実光と解除後の最新音量、既存 native lifecycle E2E の再確認である。現時点では P03 の PRマージ・completed・P04開始は行わない。三環境の品質・初期値確定、下限OS／他画面・native backend 全RT経路・物理同期・配布はそれぞれ後続 gate に残す。

### 診断 source 除去後の統合検査

使い捨て CLI／SDK source を除去し、`cargo fmt --all -- --check`、`cargo test --offline --locked`（82 passed／0 failed、1.64秒）、`cargo clippy --all-targets --offline --locked -- -D warnings`、`cargo build --offline --locked` は exit0。全 Rust 検査と hash の command は6.06秒。新しい急変・有限端点・retarget・世界／イベント継続、coherent受理・通知・凍結中の受理と callback fault の regression を含む。診断なし `target/debug/ambien` の SHA256 は `31582a07b6d7559cd18e6e844b6ff617fd7598bb118f463f6be5cc0e14d997ac`。この合格を未実施の native操作に広げない。

ユーザーの「解除したので検証を続ける」回答を受け、実窓検証を再開した。SDK preflight の初回は `NSNumber` の import 不足でコンパイル失敗し、Foundation を明示して当該段階を直した。再開時の実行先 SDK は screen-locked=1／on-console=1／display-asleep=1／display-active=0を返し、guard が exit77で実アプリの起動前に停止した。外部 UI の失敗を反復したのではない。別の読取りで console UID は実行プロセスと一致し、foreground は `so.stencil.tern` だった。これだけで画面復帰や lock metadata の stale を判定せず、ユーザーの解除操作とその後の SDK 状態を別の事実として保持する。OS設定・ロック解除・認証は AI が変更していない。

実ノブ／menu・設定消音／世代受理／selector mouse・keyboard／scroll時 focus・identity／設定再生成を通る外部 SDK driver は `swiftc -typecheck` が exit0（2.78秒）だったが、前提不成立のため未実行であり PASS ではない。使い捨て source は除去し、次回は実行先の display が復帰し、解除状態を維持できるときに同じ未完 native 条件を確認する。文書検査は `npm run check:docs` が All checks passed（0.53秒、既存の No init version recorded 警告を保持）。

### Native 自動判定と検証スキル

ユーザーの追加依頼に従い、操作だけでなく期待値のチェックを runner に組み込み、実窓 PNG のローカル比較とプロジェクト内の [検証スキル](../.agents/skills/verifying-ambien/SKILL.md) を追加した。画面収録権限はユーザーが準備する経路を選び、AI による許可 UI・TCC／OS／認証設定変更や画像外部送信は許可範囲に含めない。SPEC の進行条件・実機／試聴／配布 gate は変えていない。

2026-10-07、Darwin 27.0.0／arm64、同じ診断なし binary SHA256 `31582a07b6d7559cd18e6e844b6ff617fd7598bb118f463f6be5cc0e14d997ac` を使用した。SDK は console=1、display asleep=0／active=1、AX=true、event-post=true、capture=false、lock metadata=nilを返した。nil 単独では未ロックと判定せず、runner は完了 login・一致 UID・前面 PID／実 focus 窓も確認した。

旧 runner に `--case controls --artifacts ...` を渡した変更前の確認は Usage／exit1（0.43秒）。保管していた外部 driver は選択済み雨の Space／Enter／mouse の各1回受理、雨／風編集、消音中 volume0／10／20%、menu一致、実scrollでのfocus／identity保持まで通ったが、消音解除後の未チェック menu の `AXMenuItemMarkChar` が `.noValue`（-25212）を返して exit1（2.94秒）。これは観測器の未チェック値解釈の失敗として残し、production の成功や Quit に読み替えない。統合 runner は `.noValue` と取得成功時の空文字だけを未チェックとして扱い、unsupported／他エラーは失敗にした。

両 Swift source の `swiftc -typecheck` は成功した。screenshot helper の初回は SDK の compound macro `PROC_PIDPATHINFO_MAXSIZE` が Swift に import されず失敗し、SDK の定義 `4 * MAXPATHLEN` と `MAXPATHLEN = PATH_MAX` に従って buffer サイズを修正した。helper の PNG-backed `--self-test` は exit0：不変・単色・透明・寸法不一致・窓端だけの変化を拒否し、内部64 pixelの変化は計測して通した。これは比較器の境界検査で、実 Ambien の画像を取得した証拠ではない。capture CLI は実際の preflight=false で JSON `blocked`／exit77を返し、取得 API／権限要求を行っていない。

```sh
/usr/bin/swift scripts/native-e2e.swift "$PWD/target/debug/ambien" \
  --case all --artifacts "$PWD/target/native-e2e"
```

この実行は8.04秒・exit77、総合 `blocked`。今回のローカル証拠は `target/native-e2e/run-E64D3B07-7B21-49F9-8C05-681A50A64ED3/report.json`（実行14:23:02〜14:23:09 UTC）。directory0700／report0600も `stat` で確認した。

| check | 実観測 |
|---|---|
| binary／session | SHA256一致、実行前と入力前の安全前提を確認、PASS |
| lifecycle | 実menu、設定singleton、両窓minimize／AX復元／zoom無効、close／⌘,再生成、zero-window生存、menu Quitと両窓⌘Q、PASS。PID39837／39986／39992が実消失・正常exit0 |
| controls | metadata、選択済みSpace／Enter／mouseで各1回のsettings／manual世代更新と受理3/3、雨35／風13、menu消音・設定一致、消音のままvolume0／10／20%でmanual5維持、実scrollで同じfocused AX sliderと受理9/5を保持、設定で解除後20%／menu未チェック、close／再生成でも最新端点と受理10/5保持、PASS。実menu QuitでPID40130消失・正常exit0 |
| visual | Screen Recording不足でBLOCKED。画像取得未実施、PNGなし。独立したlifecycle／controlsを停止・偽PASSにしていない |

新しい runner・helper・skill の独立 Spec／Standards reviewer は source の対象範囲に重要指摘なし。実画像取得成功を review から主張しない。実窓 frame の変化、消音／解除の聴感、物理同期・最大サイズ／高DPI・下限OS・配布は、この自動チェックで合格にしていない。P03 の normal merge・completed・P04開始は実窓画像の前提と残る受け入れが揃うまで行わない。

選択ケースと失敗優先も実行確認した。`--case controls` は実アプリの同じチェックとmenu Quit（PID53813消失・正常exit0）を通し、総合pass／exit0、証拠は `target/native-e2e/run-52EC9D50-053A-492D-BA75-02441C9BF566/report.json`。visualを選ばなかった結果であり、画像取得の合格ではない。存在しない `/tmp/ambien-missing-candidate/ambien` を指定した実行はbinary.identity fail、全nativeケース未実行blocked、総合fail／exit1、証拠は `target/native-e2e/run-766B7680-AA03-4680-8DDE-B583E692165F/report.json`。失敗が前提不足より優先し、候補を起動していない。両commandをまとめた確認は5.30秒、期待する0／1を確認して終了した。

検証スキルは成功履歴や追加規律の正本ではなく、現存ケース／CLI／report の参照手順として作成した。未提供のCLI仕様を尋ねた変更前の5つの新規AI回答は、全て未実施の画像を合格・mergeにはしなかったが、ケース別の正確なコマンド／report契約は得られなかった。これは判定規律の失敗ではなく、参照手順の不足として扱う。スキル掲載のCLI／case／reportは上記の実行と独立source reviewで照合し、AGENTS・README・文書索引・Latから入口を設けた。スキルを渡した追加の新規AI回答確認は応答待ちでtimeout／interruptとなり、完了した結果を取得できていない。新規AIへの適用成功率・pressure耐性は未測定であり、推測で成功件数を記録しない。この未測定は実アプリのPASSや画像BLOCKEDとは別の検証制約である。

関連文書・スキル入口を揃えた後の `npm run check:docs` は All checks passed／exit0（0.48秒）。既存の No init version recorded 警告は保持し、init／hook／全体設定の追加はしていない。これは Lat の構造検査であり、新規AI回答・実画像・製品全体の受け入れの成功を意味しない。

## macOS 共通検証スキルへの分離

ユーザーが他のデスクトップアプリへの再利用を依頼し、macOS専用としたため、[共通手順](../.agents/skills/verifying-macos-apps/SKILL.md) と [Ambien固有レシピ](../.agents/skills/verifying-ambien/SKILL.md) に分離した。共通directoryはAmbienや隣接文書に依存せず、別プロジェクトへ持ち出せる。アプリの要件文書・言語・初期窓・CLI・report format・exit code・画像の期待関係は固有レシピから確認し、共通の決め打ちにしない。汎用runner、グローバル登録、他OS対応、新たな画像取得権限は追加していない。

変更前の2つの独立したレシピ適用確認は、READMEを正本とする文書エディタと、初期窓を持たず静止画像を要求するstatus-barアプリを対象とした。旧スキルのSPEC／Cargo前提、Ambien固定case／report契約、動く画面に偏った画像判定が再利用の不足として報告された。これは想定プロジェクトのcatalogを使う手順適用確認であり、別製品の実行結果ではない。

共通手順を渡した両agentの再適用では、各projectの既存commandを選び、保存内容とeditor focus、menu状態と時刻付きack、OSのcapture権限と撮影範囲の許可を区別できた。静的な画面の一致、対象ROIと事前mask、正当な単色canvasを扱い、exit0でも保存内容の不一致／期待するbadgeの停止はfailとした。残る要件値・時刻・selector・実format・実行証拠を捏造していない。別途Ambienレシピからcontrolsの選択・現存all command・77の意味を取得できた。これは記録済みの2ケースと反例への手順適用であり、任意のアプリへの成功率や実UIの受け入れを保証しない。

実経路を確かめるため、Ambienとは別名の小さなAppKitアプリと外部AX observerを隔離して作成した。Swift sourceの初回typecheckはthrowing式を `&&` で結合した箇所で失敗し、前面PID guardとAX focus照合を分けて修正した。fixture compileとobserver typecheckは成功したが、実行はpreflightで起動前にblocked／exit77（0.83秒）。ローカル証拠は `/tmp/macos-skill-smoke.psicXs/report.json`、checksは空で候補PID未作成。別の読取り専用SDK診断（0.44秒）はlock=1、console=1、login=1、consoleUID=processUID=501、display active=0／asleep=1、foreground=loginwindow、AX／event=true、capture=falseを返した。同じ起動失敗は反復せず、OS／TCC／認証変更・入力・画像取得は行っていない。したがって、この別fixtureのcounter／checkbox／Quitは未実施でありPASSではない。使い捨てsourceと生成binaryは残さず、productionのRust／Swift scriptは変更していない。

今回の手順適用資料はsession内の `macos-skill-baseline-editor.json`／`macos-skill-baseline-panel.json`、`macos-skill-guided-editor.json`／`macos-skill-guided-panel.json`、`macos-skill-ambien-retrieval.json` に保持した。前節のAI回答timeoutという過去の未測定を、今回の2ケース確認から全面的な成功へ書き換えない。P03の画像・聴感・通常mergeの残gateも維持する。

独立Spec／Standardsレビューは、共通手順が全体mouse入力にもkeyboard focusを必須としていた点を指摘した。これでは初期窓のないstatus itemを操作できず、frontmost／focused windowが合っていても別の最前面窓への誤clickを防げない。keyboardは従来のforeground／focus確認、mouseは所有AX action優先と実hit target／bounds／倍率／被覆の確認へ分け、確認不能ならblockedとした。両follow-up reviewは重要指摘なし（sourceのみ）。

修正した手順への3反例適用では、所有status itemのAXPressはCGEvent権限や存在しないfocused windowを要求せず選べる一方、他PIDの被覆を検出した座標入力とhit観測unsupportedの座標入力は、どちらも送信せずblockedとした。資料はsession内の `macos-skill-pointer-regressions.json`。これは手順の判定確認であり、ロック中の実mouse操作を行った証拠ではない。

分離・修正・案内を揃えた後の `npm run check:docs` はAll checks passed／exit0（0.41秒）。既存のNo init version recorded警告を保持した。Rust／shipping Swift sourceを変更していないため、この作業でCargo検査の新たな成功は主張しない。構造検査と想定ケースへの手順適用を、別アプリの実窓受け入れやP03の画像成功へ広げない。

## 順次実装の再開前確認

ユーザーの実装続行依頼を受け、P03の残受け入れへ再開する前に、読取り専用SDKで実行前提を確認した（0.43秒）。lock=1、console=1、login=1、consoleUID=processUID=501、display active=0／asleep=1、foreground=loginwindow、AX／event=true、capture=false。実アプリは起動せず、入力・capture API・許可要求・OS／TCC／認証変更は行っていない。同じAX失敗を反復したのではなく、再開可否の前提読取りである。既存controls／lifecycle PASSを維持し、実窓画像と消音解除後の聴感の未確認は残す。ユーザー側で実行先の画面復帰・解除と実行元の画面収録権限を準備してから未完の経路を再開する。P03の受け入れ・通常merge・同期を飛ばしてP04へ進めない。

ユーザーが「準備したので検証を再開」を選択した後、同じ診断なしbinaryの `--case all` を実行した。SHA256一致のみPASS、session preflightはlock=true／console=true／login=true／UID501／display active=0・asleep=1でblocked、全nativeケース未実行、0.86秒・exit77。証拠は `target/native-e2e/run-751FAB50-AF5C-4946-AB1D-D475CA61D3CE/report.json`。候補起動・入力なし。今回は画面のguardで止まったため、準備後のcapture権限を読んだとはしない。ユーザーの準備操作と実行先SDKの観測を別の事実として保持し、同じ操作を再試行しない。

食い違いの切分けとしてhost／実行user／実親processを読んだ。実行先ComputerNameは `takeoのMac mini`、userは `takemo101`、実行経路は `/Applications/Tern.app/Contents/MacOS/tern` → `/bin/zsh` → `omp`、`TERM_PROGRAM=tern`。ユーザーは準備先も同じMac mini・Ternと回答した。`/dev/console` 所有者UID501、実行processのlaunch manager=Aqua／UID501を観測し、別ユーザーの非GUI実行ではなかった。Hitchで参照できる端末は0件。環境設定や認証は変更していない。

`loginwindow` の直近ログには、2026-10-08 07:15:15.135+0900（UTCでは2026-10-07 22:15:15.135）のunlock successと、その後のscreen lock UI teardown completeがある。先のrunner記録の22:14:43Zより後の環境遷移として扱い、ユーザーの準備操作を否定したり、staleなSDK値が原因だったと決めつけたりしない。22:25:02Zの追加SDK snapshot（0.55秒）ではlockフラグなし、console／login=1、UID501、online display2件、両方active=1／asleep=0、foreground=Chrome。AX／event=true、capture=false。この読取りは環境遷移後の前提を識別するためのもので、同じ失敗したnativeケースの再実行ではない。

追加snapshotに対応するTCCログのrequest `27168.4` は、service=kTCCServiceScreenCapture／preflight=yes、subject=`so.stencil.tern`、responsible_path=`/Applications/Tern.app/Contents/MacOS/tern`、requesting binary=swift-frontend、authValue=1／authReason=5／error=null。値の意味や設定UIの状態を未確認のまま断定せず、公開preflight=falseと、この実行経路の許可対象がTernであることを証拠とする。画像取得・許可要求・TCCデータベース変更・アプリ再起動は行わず、既存native成功を保持して画面収録経路の利用可能化を待つ。IOPowerManagementの独立読取りは指定keyを取得できずexit1であり、画面状態の成功証拠には含めない。時刻範囲指定のログ照会は空だったため、直近ログの成功イベントと実行先の+0900を用いて時刻を対応させた。実窓画像と解除後の聴感はなお未確認。

ユーザーから「画面収録の許可入ってなかったです」と報告があり、未許可だったことをユーザー確認の事実として記録した。許可を有効化済み・アプリ再起動済みとは推定しない。未完経路を再開できるかの公開preflight読取りは2026-10-07 22:33:04Zにcapture=false（0.28秒・exit0）。失敗したnativeケースの再実行、画像取得、許可要求、OS設定変更は行っていない。ユーザー側でTernの画面収録を有効化し、OSが再起動を求めた場合はTernを再起動してから残る実窓検証へ戻る。過去のblockedや未確認範囲を合格へ書き換えない。

## 画面収録許可後の実窓撮影

ユーザーの「許可した」という報告後、公開preflightは2026-10-07 22:37:10Zにcapture=true（0.34秒）。既存の `native-e2e.swift` を同じ診断なしbinaryで `--case visual` として実行した（5.34秒・exit1）。binary SHA256 `31582a07b6d7559cd18e6e844b6ff617fd7598bb118f463f6be5cc0e14d997ac` とsession preflightはPASSで、AX／event／capture=true。証拠は `target/native-e2e/run-AB41F89B-C8B1-4564-B742-490B42302BD7/report.json`。

所有候補PID49422／CGWindowID9033のScreenCaptureKit `desktopIndependentWindow` 画像を2枚保存した。両helper結果は、同じ実行ファイル・PID・window ID・window_only=true・1920×1200・screen_recording_authorized=true・status=passを報告した。実statもrun directory0700、reportと両PNG0600、自分所有を確認。画像はローカルのみで、外部送信・他アプリ／desktop全体の撮影を行っていない。

3枚目前に `Scene no longer foreground before capture` で停止した。visualはFAILであり、2枚取得できたことを3枚／2比較／menu Quitを含む全ケースの成功にしない。failure cleanupで候補を終了し、正常Quitの証拠とはしない。保存済み2枚への既存helperのローカルcompareはexit0／status=pass、内部crop1536×840（x192／y180）・比較1,290,240画素中6,109画素変化（0.0047347780）、maximum channel delta72。これは消音中の実窓内部の変化だけであり、粒だけの運動・可聴無音・物理同期・ガラスの質感を証明しない。

読取り専用WindowServerログでは対象の前面化を観測したが、失敗前の追加の前面process切替は得られず、AXFocusedWindowの不一致原因は未特定。比較・既存capture証拠を保持し、focus guardを外したりcapture権限を再要求したりしない。通知観測用の一時Swiftプロセスはready後15秒でexit0。ユーザーのスキル改善依頼を優先したため、観測中の追加native試行は開始しなかった。これは対象のfocus遷移を特定した証拠ではない。

## 権限準備と検証再開の汎用手順

ユーザーが、権限の許可を促す案内とスキル改善を依頼し、汎用化する対象を「権限準備と検証再開」と選択した。既存の `verifying-macos-apps` へ、今回の実行経路の許可主体、設定項目、ユーザー本人の手動準備、条件付き再起動、同経路の公開preflightによる反映確認、未完ケースだけの新規runでの復帰を追加した。アプリ名・言語・CLI・report schema・画像数・消音は共通手順に固定せず、Ambienの公開probeとvisualコマンド・3枚／2比較／正常Quitは固有レシピに置いた。開発手順の参照元も更新し、新しいrunner・OS設定変更・グローバル登録・外部公開は行っていない。

変更前の独立参照適用では、共通5ケース・固有5ケースから、具体的な設定場所、responsible appとrequesting processの区別、再起動条件、反映確認と保留ケースへの復帰の未記載を確認した。変更後は同じ5ケースずつへ独立に適用し、DeskShell／BrightTerm等の非Ambien条件でも、手動案内とtrue／false後の行動を記載から導けた。これは参照手順の欠落を判定する検証であり、別アプリの実機権限や操作の成功ではない。文言の採用率・自然な遵守率の比較実験は行っていない。

追加の5境界反例では、許可拒否後の停止分岐不足が見つかったため、再要求・取得を止め必須ケースをblockedとして保持する規則を修正した。旧候補cleanup後に診断情報を得る反例では、通常再開の前に新情報を要求すると循環することを確認し、仮説・実在する追加読取り観測・有限待ちと停止条件・所有とpreflight・既存guardを持つ1回の診断専用runを通常再開から分けた。両修正の再適用と、2スキル／現存native sourceの独立統合レビューに残る重要な指摘はなかった。診断runの終了自体や部分画像をケース成功としない。

シナリオ資料はsession内の `macos-permission-guidance-cases.json`、`ambien-permission-focus-cases.json`、`macos-permission-boundary-cases.json`、追加診断反例はレビュー対話に保持した。純粋な参照・適用評価であり、permanentの転記試験や汎用driverを追加していない。固有レシピに記載した `/usr/bin/swift -e 'import CoreGraphics; print(CGPreflightScreenCaptureAccess())'` の実行はtrue／exit0（0.48秒）。これはその公開probeが動き、取得前提が利用可能だった証拠だけである。先の実窓2枚後のvisual FAILと解除後聴感の未確認は、そのまま保持する。Rust／shipping Swiftを変更しておらず、新しいビルドや全Rust試験の成功は主張しない。

## 汎用手順に従った限定診断と新規visual成功

スキル整備後、前面process切替・所有候補のAX focused window変更を追加観測し、1回の診断専用runで区別する仮説と停止条件を定めた。23:18:36Zの前提読取り（0.42秒）はconsole／login=1、UID=processUID=501、main display active=1／asleep=0、foreground=Chrome、AX／event／capture=true、lockフラグなし。他の成立条件と合わせて実操作可能と判断し、許可要求・OS設定変更は行っていない。

通知observerのready後に、既存ガード付きの `--case visual` を同じbinary SHA256で1回実行した（8.51秒・exit0）。今回の `target/native-e2e/run-62C30ABE-951F-4160-BC1C-2F33B0BC3AA7/report.json` はbinary／session／visualすべてPASS。所有PID98952／CGWindowID9522・1920×1200の実窓PNG3枚、cropped interior1536×840（x192／y180）の隣接2組に7,728／13,715画素の変化（0.0059895833／0.0106298053）、maximum channel delta78／212を報告した。実menu消音・所有／bounds／focus／mark・実menu Quit正常exit0まで現存runnerの全必須assertionが成立した。実statでrun0700、reportとPNG3枚0600、自分所有も確認。画像はローカルのみ。

observerは15.8秒・exit0、対象exeに一致したPID98952へAXFocusedWindowChanged／AXWindowCreatedをstatus0で登録し、設定窓への変更と元のシーンidentityへの復帰を観測した。撮影区間のfocus喪失は再現せず、正常終了後の前面変更を記録した。NSRunningApplicationのlaunchDateはnilであり、開始日時が取得できたとはしない。以前のvisual FAILを上書きせず、最初の喪失原因を特定・修正したとも主張しない。今回のPASSは診断プロセスの終了からではなく、実runnerの3枚／2比較／正常Quitまでの観測から判定した。

既存controls／lifecycle成功と今回visual成功は同一binary identityの独立証拠として保持し、別の `--case all` を成功実行したとはしない。描画frame変化を、粒だけの運動、可聴無音、解除後の最新音量、物理同期、主観的な音質・ガラス、最大負荷、製品全体の受け入れへ拡張しない。P03のユーザー聴感確認、通常mergeと同期は残る。

## P03 消音中の音量変更と解除後のユーザー聴感

ユーザーへ `/Users/takemo101/Desktop/workspace/ambien/target/debug/ambien` を起動し、設定窓で「音量20% → 消音 → 消音中80% → 消音中20% → 消音解除」を行うよう案内した。期待する結果は、消音中の音量変更で音が戻らず、解除後は最後に設定した20%程度へ戻ること。OSや他アプリの音量変更は手順に含めていない。

ユーザーは「期待どおり確認できた」を選択し、消音中に音量を変えても無音のまま、解除後は最後の20%程度で鳴ったことを受け入れた。これはユーザーの聴感観測であり、AIによる物理音圧測定や画像比較からの推定ではない。同じ成功を確認するための再試行はしない。候補のbinary SHA256は、上記native証拠と同じ `31582a07b6d7559cd18e6e844b6ff617fd7598bb118f463f6be5cc0e14d997ac`。

P03のこの聴感条件は受け入れ済み。初回visualのfocus喪失原因は未特定のまま保持する。音質の10分試聴・初期ノブ値確定、粒だけの運動、物理同期、最大負荷、下限OSと署名・配布の受け入れへ拡張しない。通常PR mergeとGitButler同期は、この記録時点では未完了。

## P03 最終レビュー：座標入力の実受け手と所有

独立したSpec／Standardsレビューは、native runnerのclick／scrollが前面・focus・矩形だけから受け手を推測する点を重要指摘とした。別processの非activating panelに被覆されても候補のfocusは維持できるため、送信先の所有確認が必要である。ほかの重要なP03指摘はなかった。

実runnerの宣言を使う一時probeで、自分の検証processが作成した空の非activating panelだけを候補へ重ねた。ユーザーの既存app／文書を操作・撮影していない。最初はprobe側のAppKit起動完了が欠けてhit lookupが-25208となり、2.73秒で停止・候補cleanupした。`finishLaunching`後のRED（3.60秒・exit1）では、前面・focusが候補PID84200のまま、hit PID84058の隔離panelへclick1回・scroll1回が実際に届いた。候補は実menu Quit正常0。模擬の許可判定やソース文言比較ではなく、実イベント配送で欠落を再現した。

`requirePointerTarget`は送信前に公開AX hit lookup（messaging timeout1秒）とPIDを確認し、実受け手と指定controlの両方が現在の所有AX windowに属すること、最新bounds・前面・focus・sessionを再確認する。確認不能・被覆・別windowなら入力せずblocked。最初に「論理controlの子だけ」を要求した版は、実GPUIのhitが所有NSWindowであるためcontrolsがblockedになった（1.98秒・exit77、`run-93FA439D-CA1B-408B-9A63-ED3783E23BB6/report.json`）。別の読取り診断は、被覆なしではhit role=AXWindowかつ期待windowとのCFEqual=true、論理selectorとのCFEqual=falseを観測した。個別のGPUI leafを取得できたとはせず、実際のnative受け手window identityを契約にした。PIDだけや前面だけで許可する回避ではない。

最終版の被覆GREENは2.90秒・exit0。候補PID97173のfocusを維持しつつ、隔離panelのhit PID97163を検出してclick／scrollともblocked、実配送は両方0回、実menu Quit正常0。private証拠先は `target/native-e2e/pointer-regression-2448AFCB-2F83-42F8-A0D3-1870CC1DC561`。probeはこの実SDK/UI境界を確認する一時コードで、既存の単一scriptを重複した恒久runnerとして残さない。

通常controlsも最終scriptで再実行し、4.40秒・exit0。`target/native-e2e/run-4771E5FF-6062-4EE1-B72E-EBF174F81875/report.json` はbinary／session／controlsすべてPASS、同じbinary SHA256を報告した。実selected Space／Enter／mouseの各1世代、雨と風の各1stepと受理、menu／設定消音一致、消音中volume0／10／20、実scrollでfocus／identity／revision維持、解除後20%、close／recreate保持、PID96869の実menu Quit正常0を観測した。未確定の雨34%／風12%を試験で固定する検査は削除し、実開始値から1stepの変化・受理・再生成後の保持を確認する。

Rust実装・binary・lifecycle／visual経路はこの修正で変更していない。既存の独立証拠とユーザー聴感を保持し、初回visual失敗原因や未実施の品質・物理同期・配布を合格へ変更しない。

## P03 最終版のレビュー・検査と限定focus診断

pointer確認はAX treeの準備後に最新session／前面／focus／boundsを照合し、hit lookupを送信直前の最後のAX操作にした。hit後はPIDとCF identityの比較だけを行い、確認不能の例外もblockedとして入力を止める。Spec／Standardsの独立follow-upは、重要指摘の解消と残る重要指摘なしをソース上で報告した。レビュー担当は実行検証を行っていない。

parentの最終 `cargo fmt --all -- --check`、`cargo test --locked --offline`、`cargo clippy --all-targets --locked --offline -- -D warnings`、`cargo build --locked --offline` は計4.10秒・exit0、82試験成功（1.66秒）。当該コマンドだけ既存 `~/.cargo/bin` をPATHへ渡した。build後の実SHA256も `31582a07b6d7559cd18e6e844b6ff617fd7598bb118f463f6be5cc0e14d997ac` のまま。

最終版の被覆probeではhit PID10856／候補PID11007、前面・focusを保持しつつclick／scrollともblocked、実配送0回、実menu Quit正常0（`target/native-e2e/pointer-regression-A7C66EDB-49EC-426B-B7C8-5649B133B179`）。続けた通常controlsは途中で「Foreground changed; no key posted」によりFAIL、候補PID11057をcleanupした。連続コマンド計7.75秒・exit1、失敗reportは `target/native-e2e/run-47AB72D8-69C2-41D5-AEC8-0FB7C6CC4903/report.json`。この停止を正常QuitやPASSへ変更しない。

以前の検証用panel終了に伴う前面遷移と別のfocus変更を区別する仮説を立て、1回だけ追加観測を持つ限定診断を行った。実runnerの同じ最終宣言・全controls assertionとCLIを一時copyへ使い、実際に生成・所有する `Candidate` へAXFocusedWindowChanged／AXWindowCreatedとWorkspace activation observer、各key直前の読取りsnapshotだけを追加した。既存guard・値・待ち・必須条件を変更していない。4.68秒・exit0、`target/native-e2e/run-9E7AC785-D446-4AB7-9F2E-A5C7E9D3F96D/report.json` はbinary／session／controlsすべてPASS、PID21148の実menu Quit正常0まで成立した。8回のkey前で前面PID21148・期待AX focused windowとの一致を観測し、設定窓生成／再生成のidentity変更、正常Quit後のloginwindowへのactivationを記録した。

この診断では喪失が再現せず、先のFAILの原因を特定・修正したとはしない。診断copyの成功を、未計測の通常run成功や全case同時実行へ書き換えない。最新guardの実成功と実runnerの既存controls／lifecycle／visual成功、同binaryのユーザー聴感は、それぞれの条件と限界を保持してP03の独立証拠とする。一時probeと診断copyは削除し、永久の重複runnerや新しいdiagnostic flagを追加しない。

## P04 岩と最終手動多声交差の限定検証

対象は `feat/amb-006-rock-manual-mix` の未commit worktree、共通base `fe945dbc7a7df01eefe4182e0ebe3395500c3971`。SPEC document-state17でユーザーが選択したP04拡張（実岩voiceと、現在gainから割込み可能な2秒equal-power共通N-voice手動交差）を対象にする。最終commit／PRはこの記録時点では未確定。P06の三実環境・再入場・最大三声RT受け入れやP13の音質／preset採用を完了扱いにしない。

現存sourceでは `src/situations/mod.rs` が岩moduleを登録し、`rock.rs` のdefinitionは量・大きさ・硬さを持つ実RockVoice factoryへ接続される。衝突excitation・共鳴・左右panとvisual eventを持ち、単なる登録stubではない。`src/audio/mix.rs` のGainRampは準備済みN要素のcurrent／origin／tangentでunit-vector arcを扱い、`render.rs` のmanual世代受理だけが `Duration::from_secs(2)` でretargetする。receiptの手動表示対象と実mixer gainは別状態である。これは実装経路のsource確認であり、callback全経路のallocation計測・知覚品質・native操作の成功を推定しない。

### Rust・実factory波形・実CPAL

初回feature REDは岩未登録で2試験失敗・exit101（session artifact1018）。統合途中のcast括弧不足によるcompile失敗を修正し、次の104試験中101成功／3失敗は登録数fixtureを更新して解消した。parentの `cargo test --locked --offline` は104成功・14.48秒（artifact1031）。production `cargo build --locked --offline` はexit0、build表示1.16秒／wall1.25秒、warningsなし。Swift `--help` はthrowing boolean-expressionの訂正後、実実行exit0・1.29秒。これらはRust試験・build・CLI成立の証拠で、実操作や試聴の成功ではない。

後の独立レビューが示したP2（光のowner gain未反映）に対して、実Scene／VisualRing consumerのregression `particle_brightness_uses_presented_crossover_gains_and_hides_departed_voice` は変更前に0.45000002対期待0でRED（artifact1076）。光のhelperをpresented owner gain×strength×fadeとし、zero gainでは描画を早期skipした。app volume／muteは光に掛けない。修正後のparent aggregateは105試験／fmt／strict Clippy／buildすべてPASS（artifact1078）。独立Spec／Standardsのbounded再レビューも修正を正しいとし、残る指摘なし。ただしsource／regression／build／レビューの証拠であり、新候補のnative最終承認や物理同期を示さない。

production factoryを使う使い捨て波形／CPAL smokeは1試験成功・12.56秒／wall16.35秒（artifact1033）。48kHz・5秒生成、RMSは最後の3秒、dry delay0で測定した：

| voice／候補ノブ | peak | RMS |
|---|---:|---:|
| 雨候補 | 0.166777626 | 0.010071152 |
| 雨all1 | 0.237679243 | 0.020071973 |
| 岩候補(0.27,0.61,0.18) | 0.018213162 | 0.003903708 |
| 岩(1,0,0) | 0.024622219 | 0.003738747 |
| 岩all1 | 0.028469594 | 0.007910246 |
| 岩(1,1,0) | 0.034252387 | 0.009738408 |

観測したvoice別peak和0.271931618は当該波形の値で、任意条件の上限や知覚上の音量一致ではない。候補初期値はpublic採用未承認で、P13の校正・主観判断を残す。

実標準Device／CPALは44,100Hz・2ch・F32・epoch1。消音中world0.533333秒でphase1、雨／岩gain `[0.9524935,0.30455896]`、world2.166667秒でphase0 `[0,1]` を観測した。途中volume編集は2秒交差を再始動しなかった。消音中の実RockImpactはserial22、world3.804263、x0.7334362／y0.43643302、strength0.8900345、radius4.933726、life733.101283ms。雨への出発world4.1秒のgain `[0.11053086,0.9938727]` から岩へ割込み、world6.266667秒に `[0,1]`、latest manual4へ到達。app volume0.2で解除後world6.533333秒も `[0,1]`／Running、再消音後owned workerを正常joinした。これは実consumerのgain・世界・event・制御受理であり、物理可聴無音、試聴、OS allocation、物理同期、全native RT境界の証明ではない。使い捨て `target/p04-smoke.rs` とmainのcfg-test接続は除去され、恒久testing hookは残さない。

### 新候補のnative初回：失敗も保持

実 `--case all` は16.17秒・exit1、総合FAIL。`target/native-e2e/run-237D3EB8-0B87-4D21-A877-51372A70D25D/report.json` は2026-10-08T01:43:58Z〜01:44:13Z、binary SHA256 `a4a2dee150356992b4e5bb61a30b64bae39b4ffa689463f6ed572df580de64b6`。binary／sessionはPASS（AX／event／capture=true、OS／許可変更なし）。lifecycleは所有PID61322／61643／61649の実menu／両窓⌘Q・正常exit0までPASS。controlsはPID61662、situationsはPID61819で `Owned selector label missing: 雨` によりFAIL。failure cleanupは正常Quitの証拠ではなく、岩3ノブ・選択・岩の消音画像を成功にしない。

独立visualはPID61965／window10147、1920×1200の所有シーンPNG3枚・隣接2組の内部変化・実menu Quit正常exit0までPASS。これは初期シーンの画像契約成立であり、失敗したsituationsの岩画像、粒だけの運動、音と光の物理同期、ガラスの主観品質ではない。画像はローカルのみ、失敗runと保存済み証拠を保持する。selectorの追加読取り診断／修正／限定再開は新しい観測が得られた場合に別記し、このFAILを上書きしない。

native担当の追加確認では、初回run directory0700／report0600／20 artifactsすべて0600。visualのcropはx192／y180／1536×840、1,290,240pixel中の変化は9,151（0.0070924789）／18,404（0.0142640129）。実PNGの目視ではnative信号ボタン3つ、位置／半径が変わる疎なcyan／blueの同心円光、中央〜下側に残る大きなpurple円形と薄い水平形、viewer上の黒／透明領域を観測した。形のすべてを粒に帰属させたり、ガラス／音質の合格としない。

selector failureの追加読取りは `target/native-e2e/diagnosis-AE3CCD0E-63C5-4DBE-B005-B6BA3344FC0B` のPID65125ではreceipt準備前のnative chromeだけを得たため、別の有限診断でreceipt0/0を明示的に待った。`target/native-e2e/diagnosis-BC5A2D36-DE70-4FB2-A4D9-DBADA703946E/owned-settings-ax.json` は所有設定窓にlabel付きAXGroup「シチュエーション」はあるがselector childrenが0であることを示した。対応するEly segment source141–186にrole／aria_labelがなく、per-segmentの所有boundsを得るrunner-only回避では安全契約を満たさないと判断した。PID66657は実menu Quit正常0。これは診断の成立であってcontrols／situations受け入れではない。permission／focus／ownership guardやthresholdは変更していない。この診断時点では、UIの所有された選択surfaceの修正と新証拠に基づく未完ケースを統合担当へ引き継いだ。

### 所有RadioButton修正後の限定再開

parentのUI修正・rebuild後のbinary SHA256は `5a11c39acb161999e00d4427bb9113731ec670dcd2ce5d7cc23d6c74c16e9f43`。receipt準備後の `target/native-e2e/diagnostic-5C0D2216-4EA9-44D2-A473-0FF726C4B58B/owned-settings-ax.json` で雨／岩の実AXRadioButton labelとboundsを検証し、PID16348は実menu Quit正常0。runnerは実AXRadioButtonを取得するhelperへ変更し、既存の所有／前面／focus guardを保持した。situationsは再消音後のfocused-other選択（雨Space、receipt13/7 → 岩Return、14/8）も観測対象に加えた。

通常 `--case controls` は4.48秒・exit0、`target/native-e2e/run-0CE9E41F-F396-4268-9063-B0206B87F251/report.json` にPASS、PID18929の実menu Quit正常0。通常 `--case situations` は2.78秒・exit1、`target/native-e2e/run-55E10A18-830D-42EC-A72B-D5B2E89CEAAD/report.json` にFAIL、PID19288の `AX children: -25202`。candidate logは空、PNGなし、正確な停止段階とroot causeは未特定。cleanupを正常Quitへ変更せず、このFAILを保持する。

新しい段階観測を持つ1回の有限診断は `target/native-e2e/diagnostic-935B6F07-2A09-4B03-965E-F9F49857C672/stage-observations.log`。13.268秒で既存situations全assertion、14/8までのexact receipts、pointer／keyboard／menu／mute、実PNG3枚と隣接2比較、PID23695／CGWindow10260の実menu Quit正常0まで成立した。これは段階traceを追加した診断であり、通常の選択case PASSや `all` 成功実行に読み替えない。cropはx192／y180／1536×840、変化508（0.0003937252）／308（0.0002387153）、maximum channel delta1。native担当が実3枚を目視すると、native信号ボタンと下側中央の大きなgray円、残りはviewer上で黒／透明、個々の岩粒の変化は明瞭に見えなかった。ごく微細な数値pixel変化は現存比較契約の証拠であるが、知覚的な岩粒の運動、音の質、ガラス、物理同期の証明にはしない。follow-upのdirectory0700／全files0600を確認し、画像はローカルのみ。既存失敗とsource／CPAL／native／主観の証拠境界を保持する。

### 岩captureノブを実変更した通常situations成功

見えにくかった消音画像について、比較thresholdを弱めず、既存のsize0／hardness100／amount100端点受理は保持したまま、capture前に実size End100（receipt15/9）、hardness Home0（16/10）へ変更した。これは主観品質の採用値ではなく実入力による今回の検証条件。parentの意味的に同等なiterator修正・rebuild後のbinary SHA256は `161b406bdbc4f03033801a4e64a242883f2372fbcac164cb81ec2a515a0807bd`。runnerにはaction／receipt／capture段階の恒久failure contextを追加したが、blocked／fail区別を維持し、retry／追加sleep／threshold／focus／ownership guardは変更していない。先のAX -25202の正確なroot causeはなお未確認で、この成功を原因修正の証拠にしない。

実 `--case situations` は14.94秒・exit0、`target/native-e2e/run-82805739-177B-40EA-B710-CFC6C15AA183/report.json` はschema1／selected_case situations、2026-10-08T02:07:25Z〜02:07:38Z。binary／session／situationsすべてPASS、AX／event／capture=true、PID56824／CGWindow10280の実menu Quit正常0。既存3ノブ端点、pointer雨→岩7/6、消音volume0／10／20・解除20%後再消音12/6、focused-other雨Space13/7→岩Return14/8、追加のcaptureノブ16/10、最新岩と消音状態、所有PNG3枚・隣接2比較まで実観測した。directory0700、reportと全15files0600を確認した。

実1920×1200 PNG3枚をnative担当が目視し、persistentな中央gray円／native信号ボタン以外にも、異なる位置・半径の疎なgray／white同心円状impact-like形が明瞭に見えることを確認した。一部edge clippingと薄い水平形もあった。cropはx192／y180／1536×840、隣接変化53,998（0.04185112847）／71,910（0.05573381696）、maximum channel delta197。画像だけの限定観測であり、個々の粒の厳密identity、音、ガラスの質、物理同期やP13採用値の証拠ではない。画像はローカルのみ。

初回all FAIL、通常situationsのAX failure、微細変化だけだったbounded diagnosticを別証拠としてすべて保持する。controls PASSは前のSHA256 `5a11c39…`、lifecycle／visual PASSは初回 `a4a2dee…` であり、最終hashで `all` を成功実行したとはしない。最終commit／PR、ユーザー聴感、P06／P13と他のRT・負荷・配布gateはそれぞれ必要な証拠へ分ける。

### gain修正後の新候補：receipt段階でFAIL

parentのgain修正後のbinary SHA256 `e87797b28d18bc8159b05da3c27c26ae08558e65cd9512ed01db72182a89cfba` で、更新した通常 `--case situations` を1回実行した。5.52秒・exit1、`target/native-e2e/run-FAE4CC9D-E143-449F-BA84-C07907761C6E/report.json` はschema1、2026-10-08T02:29:44Z〜02:29:49Z、binary／session PASS（AX／event／capture=true）、situations FAIL。恒久stage contextにより、所有PID44295のfocused非選択雨Space後、receipt13/7確認中の `AX children: -25202`（invalidUIElement）まで停止箇所を限定した。root causeは未特定。candidate logのIMK CFRunLoopWakeUpReliable mach-port messageも原因と確定しない。

今回のrunnerは端点PNG3枚に加え、実sceneの雨を2.3秒後に岩へ選択し直した直後の `rock-crossing-muted` PNG3枚／2比較を準備したが、このbinaryの実行は端点／交差どちらのcaptureにも到達せず、PNGなし・正常Quitなし。owned failure cleanupのみ。directory0700／reportとcandidate log0600を確認した。追加capture手順を書いたことを実交差frame成功にせず、過去の別hashのPASS／FAIL／診断はversion固有証拠のまま保持する。retry／guard弱化／エラー抑制による再実行は行っていない。

同じSHA256の厳密なAX寿命診断は `target/native-e2e/diagnosis-17E4F4DA-08F2-426E-A1BD-DDAEA0426F9B/ax-lifetime-observations.json`、2.32秒・FAIL、PID64051。予定したfocused岩→雨Spaceより前、雨→岩setupのreceipt2/1で最初のinvalidに停止した。invalid path0/0/1、CFHash1668309234は、それ以前の実読取りでrole AXGroup／title「雨の量」、親はunnamed AXGroup CFHash1668309138。同じidentityは初回読取り成功後、role／title／value／childrenが-25202となった。一方、所有設定root AXWindow／「設定」CFHash1668309122／PID64051は最初と最後のwalkで読取り可能だった。これは非atomicなAX tree観測中に除去された雨ノブdescendantが無効になった証拠であり、権限不足・focusやroot喪失の証拠ではない。UI全体の故障、正常Quit、選択case成功を示さず、全3files0600／directory0700、cleanupのみ・PNGなし。parentのAccessKit tree／remove semantics確認と根本対応判断は別に扱い、過去reportを変更しない。

別の1回のretained-descendant discriminatorは `target/native-e2e/diagnosis-533B9B1D-61EC-41AE-9CD4-13B52F128C75/retained-descendant-observations.json`、2.41秒・exit0、同じe87797…binary、PID71014の実menu Quit正常0。focused雨Space前に実所有の岩「量／大きさ／硬さ」group／slider計6nodeを保持し、切替後は除去された6nodeすべてのAXChildrenが1回の読取りで-25202となった。同時に同じ所有設定window CFHash1668316085・PID・bounds・前面focusは有効、安定したmain／audio labelはexact雨3/2と消音、fresh雨ノブgroup／sliderは有効、menuは雨checked／岩uncheckedだった。これは実virtual descendantの正当なstalenessと全UI／focus喪失を区別した診断証拠。エラーretry／抑制はなく、full smoke／capture／選択case PASSではない。directory0700／全3files0600。receiptだけの限定再サンプルの設計判断はparentへ渡し、以前のFAILは保持する。

この実staleness証拠を受け、parentはtyped `AXReadFailure` と `ReceiptObservation`（waiting／exact／invalidated）を導入し、receipt待ちだけにfresh owned-window snapshotの再観測を限定した。同じwindowとinvalid node PIDを検証し、既存5秒deadline内で待つ。最後の150msで安定した値不一致はFAIL、input／menu／capture guardの例外はcatchしない。これは確認済みのAX descendant寿命を扱う観測修正であり、入力再送／focus自動回復／権限回避ではない。修正直後はnative成功未観測として保持し、以下の新規実行を旧hash成功から推定せず別記する。

### typed AX観測修正後の同一候補all成功

修正後の通常 `--case all` は1回、37.08秒・exit0。`target/native-e2e/run-0D90E497-F697-4377-A653-BFB5489866F2/report.json` はschema1／selected_case all、2026-10-08T02:40:59Z〜02:41:35Z、binary SHA256 `e87797b28d18bc8159b05da3c27c26ae08558e65cd9512ed01db72182a89cfba`、binary／session／lifecycle／controls／situations／visualすべてPASS。実menu／⌘Q正常exit0のPIDはlifecycle75485／75643／75657、controls75662、situations75894、visual76810。過去のFAIL／診断／異なるhash成功を上書きせず、これを新しい同一候補のall証拠とする。

今回の実stdoutには1回 `OBSERVE invalidated owned AX descendant (AXRole); discard receipt snapshot 6/5` があり、receipt-only invalidation処理を実行した後、exact receiptsを受理した。抜粋はrun内 `receipt-invalidated-observation.txt`0600、full transcript保存とはしない。situationsは0/0から16/10までの端点・focused-other・captureノブを成立させ、実scene雨→2.3秒後の岩再選択直後のcaptureも実行した。situationsは同じPID75894／window10458でPNG6枚／4比較、visualはPID76810／window10483でPNG3枚／2比較、計9枚すべて1920×1200。

| 画像組 | crop x／y／width／height | 隣接変化pixel（割合） |
|---|---|---|
| 岩端点 | 192／180／1536／840 | 26,954（0.020890687）／59,374（0.046017795） |
| 雨→岩選択直後 | 同上 | 45,109（0.034961713）／19,290（0.014950707） |
| 雨visual | 同上 | 2,133（0.001653181）／1,441（0.001116846） |

native担当は実PNG9枚すべてを目視し、端点では位置／半径が異なるgray／white同心円状の形とpersistent中央gray円、交差組の最初ではcyan／blueとgray／white双方の形、その後はgray形、雨visualでは異なる位置のcyan／blue形とpurple中央円を観測した。edge clippingと薄い水平形もあり、背景か粒かをすべて帰属させない。これは実窓frameの限定観測であり、exact gain／phase／brightness、可聴音、ガラス品質、物理同期を画像から証明しない。run0700、reportと初期46 artifactsすべて0600、追加抜粋を含む47filesもprivateであることを確認。画像はローカルのみ。source regression／実CPAL／native操作／画像／ユーザー聴感／P13採用と、P06の三実環境RTなど未完gateの境界を維持する。

最終のbounded Native Spec／Standards再レビューはそれぞれconfidence0.97、correct／findings0。承認対象はreceipt-onlyのAX寿命区別とsourceのguard境界で、実native all成功は上記の別観測に基づく。最終Swift `--help` は実実行0.92秒・PASS。parentも今回reportの6checksがPASSでcurrent hash一致を直接読取り確認した。これらを試聴・P06／P13・物理同期・配布gateや未知のcommit／PRへ拡張しない。最終native追記前のdocs検査は0.42秒／本体96msでPASSだったが、今回の文書内容を含む最終docs検査の証拠ではない。

### 固有レシピの参照欠落：fresh-context前後評価

既存recipeだけをsystem参照として、tools／履歴なしのstateless completionへ「Rock3ノブ・menu選択・消音frame検証を探し、記載範囲とproof limitsを回答。不足を推定しない」という同一課題を変更前後に各1回実行した。baselineは `controls` の雨／風／音量と `visual` しか見つけられず、「Rock-specific runner、knob identities、menu items、acceptance contractがない」「only lifecycle/controls/visual/all」と回答し、岩を別appと誤解する欠落も露呈した。

最小更新として既存case表に `situations` を追加し、capture不足はvisualだけでなくsituations画像段階もblocked、独立lifecycle／controlsは継続、receiptは実gain／可聴出力ではないと対応付けた。変更後のfresh-context回答は `--case situations`、量／大きさ／硬さ、雨→岩の最新対象とmenu mark、消音volume0／10／20→解除20%→再消音、PNG3枚／2比較／正常Quitを取得し、実gain・音・粒だけ・同期・P13候補値の限界も区別した。画像間隔／閾値などrecipe単独にない実装細部は未記載として報告した。これはpure-reference retrieval gapの実前後評価であり、5-rep wording microtestはN/A、agent-pressure／自然遵守率や実native成功の試験ではない。新docs／runner／discipline redesignは追加していない。

最終caseのfocused-other keyboard選択とcaptureノブを既存rowへ反映後、同じ課題をrecipeだけのfresh-context stateless completionへもう1回適用した。`situations`、3ノブ、menu／最新対象、消音制御、雨Space→岩Return、実size100／hardness0、PNG3枚／2比較／正常Quitを取得し、capture値を品質採用値やreceiptを実gain／可聴出力にしなかった。未記載の閾値／実装細部／実行結果は推定せず不足として報告した。前後各1回に加えた最終更新後1回のpure-reference評価であり、pressure試験や5-rep wording実験ではない。

交差capture追加の参照評価も変更前後に各1回、recipeだけのfresh-context completionで実行した。同じRock3ノブ／menu／消音frame課題に「実雨→岩交差の独立capture段階と枚数」を加えたbaselineは「separate crossing-frame stageは未記載、3PNG／2比較は対象／markとは別で交差証拠にできない」と回答した。既存case rowへ独立`rock-crossing-muted`段階を追記後は、雨選択から2.3秒後の岩再選択直後、追加3PNG／2比較、計6PNG／4比較を取得し、実gain／同期／可聴出力／現run成功とは分離した。pure-reference gapの前後適用であり、pressureや実native成功の試験ではない。

## P05 深海登録のoffline検証とnative前提待ち

対象はP04統合commit `156a11b352c5a4792398f12253855fbd1981ee5d` を基点とする `feat/amb-007-deep-voice` の未commit候補。環境はDarwin27.0.0／Apple Silicon arm64、実際のcompilerはcargo1.95.0・rustc1.95.0、Apple Swift6.4／arm64-apple-macosx27.0.0。下限macOS15の動作証拠ではない。

### RED／GREENと実factory波形

parentのreal-factory回帰 `situations::deep::tests::shallow_deep_air_zero_retains_the_low_sound_and_stops_new_visual_bubbles` は、未登録Deepで `deep voice must be available`、1test失敗・exit101・3.54秒（artifact1123）。最初の `cargo` 起動はPATH不足exit127でありREDではない。既存 `~/.cargo/bin/cargo` を確認して絶対pathで再実行し、導入・全体設定変更は行わなかった。

実装後のparent aggregate（artifact1128）は `cargo fmt --all`、locked／offlineの114tests（15.46秒）、all-targets Clippy `-D warnings`（1.63秒）、build（1.57秒）、実Swift `--help` がPASS、全体23.96秒。private behavior試験はair0／低帯域、depth曲線、入場eventの実音との一致、full pool／極端rate・大幅ノブ変更／有限出力／heap0、下降pitchと終端、非寄与のaging・再入／旧尾失効・rate準備失敗を確認する。初期値やsource文言の転記を固定する試験は追加していない。

parentは実Registryから準備した声を48kHz・seed137・各5秒、全sample有限かつ絶対値1未満を観測し、最後3秒のstereo RMSを測定した（artifact1133、throwaway1test PASS・5.61秒、全体9.76秒）。Device／stream／UIは起動していない。実登録順とknob所有rangeはrain0..2、rock2..5、deep5..7、準備経路が返すdry delayは各0framesだった。

| 実factory／knob | raw peak | 最後3秒RMS | 観測した強いevent |
|---|---:|---:|---:|
| rain候補 .34／.12 | 0.166777626 | 0.010071152 | 41 |
| rock候補 .27／.61／.18 | 0.018213162 | 0.003903708 | 1 |
| deep候補 .68／.16 | 0.019970099 | 0.005058582 | 0 |
| deep depth0／air0 | 0.024619939 | 0.005925881 | 0 |
| deep depth1／air0 | 0.018730294 | 0.004676820 | 0 |
| deep depth0／air1 | 0.024746109 | 0.005885361 | 12 |
| deep depth1／air1 | 0.018293355 | 0.004515441 | 12 |

air0でも低い実出力が残り、観測した新しい強いSlowGlowはなかった。弱い粒を含むadmissionのzero境界は上記のbehavior試験で別に確認する。同じseed／air1のdepth0→1では最初の実event world0.328792・x0.009237・y0.024788・radius3.777719が一致し、strength0.700335→0.315151、寿命406.761371→1016.903412ms。二つ目もworld／位置／radius一致と暗いstrength／長い寿命を観測した。これは同じ気泡のsource属性と有限波形の証拠であり、実画面の暗さ／速度、CPAL出力・可聴音・物理同期・全入力のpeak保証・聴感音量の校正ではない。temporary test sourceと `main.rs` のcfg attachmentは測定後に除去した。係数・initialはP13未確定のcandidateのまま。

### native allは起動前BLOCKED

現binary SHA256 `2f1cbba7196114758b666148db36e1850d59ef8468b092327d1824ecfd57e426` へofficial `all` を1回実行した。今回reportは `target/native-e2e/run-B9B47665-988C-4B15-9BA7-7F2C0A6D1E2C/report.json`、2026-10-08T03:21:30Z→03:21:31Z、全体1.04秒／exit77。binary.identityはPASSだがsession.preflightは `lock=true`、console=true／login=true／UID501／active1／asleep0でBLOCKED。lifecycle／controls／situations／deep／visualは未実行で、候補PID・PNG・画像測定・正常Quitはない。directory0700／report0600を実確認した。

これは権限不足やAX failureではなく、安全なconsoleの外部前提不足である。ユーザーへMacのunlock後「解除した」と返信するよう案内した。自動unlock・入力・OS／TCC変更・同条件再試行は行わず、positive lock中の実CPAL起動も行わない。actual session変化後の新規native受け入れと実CPAL観測が必要で、以前のP04成功やoffline波形をP05 native成功へ書き換えない。

### 独立sourceレビューと固有recipe参照評価

read-only Spec／Standards reviewはそれぞれcorrect・findings0・confidence0.91／0.92。source・SPEC・pinned DSP／resource／inactive／event／native guardを対象とし、reviewerはchecks・launch・editsを実行していない。これはsource-only verdictで、stable docsの最終claim確認、native／CPAL、P06最大三声RT、P08物理同期、P13試聴を承認していない。

既存recipeだけをsystem参照に、tools／履歴なしのfresh-context completionへ同じ「深海case・depth／air端点・三環境menu／settingsと保持・消音frame枚数／比較／正常Quit・capture／lockの分類とproof limits」課題を変更前後各1回適用した。baselineはdeep case／ノブ端点／深海選択・保持／画像契約を未記載と回答し、岩の契約を転用しなかった。既存case表とpreflight対応を最小更新した後は `--case deep`、深さ0／空気0→空気100／深さ100、0/0→11/10、三環境choice／独立mute／深海端点保持、設定close後2.3秒のsetup分離、deep-muted PNG3枚／2比較／正常exit0、capture不足とlockのBLOCKEDを取得した。receipt／待ち時間／画像を実gain・低い可聴音・気泡頻度・物理的な暗さ／速度／同期へ拡張せず、未記載の入力細部・雨岩の値保持・実行結果は不足と回答した。pure-reference retrieval／application gapの前後観測であり、5-rep wording microtestはN/A、pressure／自然遵守率／実native成功の試験ではない。

stable docsを供給したbounded claim follow-upもSpec／Standardsがcorrect・findings0、confidence0.93／0.94。code再監査・checks・edits・launchは行わず、上記の数値とsource／native前提待ちの区別を確認した。parentの `npm run check:docs` は全体0.41秒・本体114msでPASS、21Rust／2Swift等の現存sourceをscanした。`No init version recorded` warningは残し、Lat init／hooks／設定変更を行っていない。このdocs検査はこの結果追記前のP05文書・recipeを対象とし、製品受け入れやblockedの解除を意味しない。

### ユーザーunlock後の同一候補native allと実CPAL

ユーザーの「解除した」報告後、同一binary SHA256 `2f1cbba7196114758b666148db36e1850d59ef8468b092327d1824ecfd57e426` の未完全caseを新規private runで実行した。今回reportは `target/native-e2e/run-2A83FEC5-11E6-4EFA-B8BD-D5C0EDA2A0C0/report.json`、2026-10-08T04:32:17Z→04:33:07Z、全体50.44秒／PASS。own console・login完了・UID一致・awake／active display・non-loginwindow foregroundが揃い、positive lockなしを確認した（nilだけを解除証拠にしていない）。AX／event／captureもtrue、OS／TCC／認証設定変更はない。binary／session／lifecycle／controls／situations／deep／visualの7checksすべてPASS。過去のlock BLOCKEDは前節のまま保持する。

実native `deep` はmenu／settingsの全三環境choice、exclusive mark／radio／mainと音声receipt、独立muteを確認し、深さ0・空気0→空気100・深さ100、世代0/0→11/10、戻った深海で両100の保持がPASS。候補初期値は観測だけで採用値として固定していない。設定close／2.3秒setup分離後、所有PID14286／CGWindowID10710の1920×1200実PNG3枚と隣接2比較がPASS、変化6528pixels／fraction0.0050595238095238098、5864／0.0045448908730158733（crop1536×840、1290240pixels）。parentも実 `deep-muted-2.png` を読み、青い円形の光を観測したが、暗さ・速度の比較・Deepのみの帰属・主観品質の合格にはしていない。全allは合計12PNG／8比較。実menu／Cmd-Q正常0の所有PIDsは12181・12506・12522・12532・12714・14286・15188。parentがdirectory0700／report0600／deep PNG3枚0600を実確認した。画像はlocalのみ、透ける背景の影響や音・物理同期の限界は今回reportに残る。

native終了後に実 `AudioController::start` とworker／CPAL callback／control・visual transportを使うtemporary probeを実行した（artifact1154、1test PASS7.31秒／全体10.91秒）。実標準出力44100Hz／2channel／F32。mute・deep選択・volume18%・depth0／air0のreceipt5/3を受理し、実header phase0／gains `[0,0,1]`、deep breath0.35936114を観測。mute中world2.000000→4.000000が進み、新しいdeep eventはなかった。mute解除後のdepth0／air0を2秒、18%の実出力経路で動かし、Running継続とreceipt6/3を確認。air1へ変えた実receipt7/4と、world6.9327210884353745・x0.13902998・y0.068993315・strength0.8021462・radius4.569639・SlowGlow・寿命467.619832msの実入場eventを観測し、controllerの正常worker shutdown／joinが戻った。sample waveformは前節のoffline測定、物理的な可聴音・録音・聴感・同期は今回probeで測定していない。temporary probeとmain cfg attachmentは除去し、shipping sourceは変更していない。

これにより今回P05のnative／CPAL前提待ちは実観測で解消した。P06最大三声RT／再入統合、P08物理同期、P13試聴／係数・initial確定、下限OS・高DPI負荷・配布の別gateへ一般化しない。現時点では通常PR merge／同期／Issue completionをまだ実施していない。

今回native／CPAL追記の最終bounded Spec／Standards evidence reviewもcorrect・findings0、confidence0.95／0.97。unchanged sourceと実report／artifact1154の整合、過去BLOCKED保持、P06／P08／P13と物理的な未測定範囲の区別を確認し、reviewerによるchecks／launch／edit／source再監査はない。追記後のparent docs検査は0.45秒／本体109msでPASS（この結果行の追加前）、Latのinit警告は保持した。

## P06 三実声の手動交差・再入場・実callback診断

対象baseはP05通常merge `1bb610829c873ceee10faeb5d9b88d5c1206886c`、専用branch `feat/amb-008-three-voice-acceptance`。Darwin27／arm64、既存Rust1.95.0／Swift6.4を使用した。P04の最終共通交差を置き換えず、productionの声・reducer・mix・renderer・UIは変更していない。実consumerと実声privateのbehavior試験、実native `mixing`、既存文書／recipeを受け入れ対象として追加した。

### 実consumer試験とfixtureの訂正

新しい実三声試験は8k／44.1k／48k／96kで、二声途中の第三声、coalesced latest intent、retarget前後vector、manual revisionの一回消費、独立volume／muteと復帰後の同一DSP／event履歴、指定frameでの正確one-hotと非targetの生成停止を確認する。短いretargetを実consumerで千回行い、各回のfinite／nonnegative／Σg²を1e-12の数値検査値で確認し、最後の雨だけへ到着する。これは聴感一定や相関波形のpeak保証ではない。千回へ拡張後の対象試験は34.47秒／全体36.33秒でPASSした。

最初のaggregateは115PASS／1FAIL（25.31秒、artifact1169）。非寄与中にzeroノブへ変更するfixtureが、実reducerの「ノブownerへ手動選択」を見落とし雨へ戻っていた。zero編集と最新deep選択を同一公開へまとめ、比較側もdeep選択を受理するよう訂正した。次の対象実行はworld6.4に対し実値6.3999999999999995のexact比較でFAIL（artifact1172）。十進時刻のbinary表現を製品不具合とせず、時計の数値比較だけ1e-12へ訂正した。その後116tests（25.19秒）、fmt／all-targets Clippy／buildがPASS、全体29.80秒（artifact1175）。二つの失敗をproduction修正や交差不具合のREDとして扱わない。

独立Spec reviewは微小正寄与、rate残り秒、新しい励起／dry delayのconsumer証拠に三件のP2不足を指摘した（confidence0.96、production defect未確立）。微小vectorは既存rampの長いduration操作で作り、targetが1へ丸められても別声の約1.57e-20正寄与を実consumerが維持し、固定2秒後に正確one-hot／生成停止、exact one-hotへの新manual選択が不要交差にならないことを確認する。rate境界は48k→32k／44.1k→96kで別rateの実engineを非callback準備し、進行中の実ramp／Movementだけを移譲して残り1.5秒を消費する。これは合成したownership境界のrenderer証拠で、世界／DSP checkpoint保持・実device再構成・P10復旧の証拠ではない。

岩は実衝突で旧roomを励起し、非寄与中のage／PRNG保持と旧尾失効を確認後、同期したclean-history実声と同じ未使用slotへ新しい衝突を決定的に入場させる。12,000frameのstereo波形がexact一致、新onsetは意図したage-zero attackから1world tick＋prepared dry delay、別測定したwet到来より前である。深海は48k／worker準備8kで同じ時計のdirty／clean声とlow-bed baselineを使い、200msの各full波形をexact比較し、新気泡の独立raw／dry励起と混合後の1e-7分解能のonsetをwet到来前に照合する。raw励起frame1と、low-bedのf32分解能より小さいconsumer最初のnonzeroを同一証拠にしない。量や低音のeventual energyだけを新しい気泡の証拠にしない。

この追加後のaggregateは118PASS／新深海試験1FAIL（37.58秒、artifact1190）。sample235でbubble-minus-bed −0.0011440609と独立dry −0.0011440598が旧1e-9境界を超えた。実DSPはf32でbed＋bubbleを丸めてからDC処理するため、出力の差と単独bubbleのDC処理は理想的な線形重ね合わせではない。任意のtolerance拡大やDSP変更はせず、誤った振幅oracleを削除し、独立onsetと同じ条件のexact clean-history波形を保持した。訂正後119testsすべてPASS（37.69秒、artifact1195）。続くClippyは新fixtureのrange loopで停止しbuild未実行だった。二channelの実比較をzipへ訂正後、該当深海試験PASS0.37秒、fmt／all-targets Clippy／buildもPASS、全体6.05秒。これらもfixture／lintの訂正で、productionの遅延不具合を観測したとはしない。

### native mixingのsafe BLOCKEDと実viewport訂正後PASS

追加前の実CLIは `--case mixing` を `Unknown case mixing`、exit1で拒否した。これは検証caseの欠落であり、アプリ音のREDではない。追加後の実Swift `--help` はmixingの入口・capture制約・receiptと実gainの違いを表示した。

最初の実mixing runは `target/native-e2e/run-FD457F84-1C69-48B5-B7BD-60E2FD05992B/report.json`、2026-10-08T05:07:22Z→05:07:28Z、全体7.14秒／exit77。binary／sessionはPASSだが、settings pointer解除前に消音switchが現在のviewport外でsafe BLOCKED、入力未送信、failure cleanupを正常Quitにしなかった。既存controls caseと同じ実scrollを追加し、focused volumeのAX identityとreceipt17/11不変を確認してからpointer解除する。guardの抑制・AXPress代用・permission変更・入力retryはしていない。

訂正後reportは `target/native-e2e/run-887ED652-50EF-49E2-86F7-2CB676B88A40/report.json`、2026-10-08T05:09:57Z→05:10:10Z、全体13.76秒／PASS。実binary SHA256 `d6b2133a3c1faf5a7c50bc4561d3346a2af332ec825c39ac87e7817bedc831dc`、own console・login完了・UID一致・awake／active display・non-loginwindow foregroundが揃い、positive lockなし（nilだけで解除認定せず）、AX／event／capture=true。OS／TCC／認証設定は変更していない。

実menu雨→岩→深海→岩→深海→選択済み深海の開始間隔は0.226601541／0.264252375／0.232373542／0.259199458／0.234323375秒。実settings pointer雨→岩→深海→選択済み深海、観測depth68.00000071525574→69.00000071525574、消音中volume0→10→20→19→18、実scroll後のidentity／receipt保持、settings解除18%→19→20、menu再消音と最新深海22/12、設定再生成後の最新値がPASS。各操作のexact main／音声receipt・排他radio／menu mark・独立muteを確認した。開始間隔は入力cadenceであって、callback受理時刻・三正gain・2秒到着の実測ではない。

DSP settle待ちなしの所有PID40183／CGWindowID10775、1920×1200の `mixing-muted` PNG3枚／隣接2比較がPASS。crop1536×840／1290240pixelsで変化7380pixels（fraction0.0057198660714285711）と64pixels（0.000049603174603174603）。parentも二枚目を読み青い円形の光を観測したが、第二比較の小さいpixel変化を粒の運動・三声重なり・品質の証明にしない。capture時点が交差中か到着後かは未保証、透過背景の影響も残る。実menu QuitでPID40183が正常exit0。directory0700／reportとPNG3枚0600を実確認、画像はlocalのみ。

### 実CPAL最大三声のscope付き診断

native終了後、実標準出力44100Hz／2channel／F32で、実Registryと同じ `output_f32`／EngineLease／control・visual transportのtemporary test-only probeを動かした（artifact1179、1test PASS8.14秒／全体8.51秒）。app volume18%、全ノブ上端（depthだけ0）、二声途中のdeep、短いrock／deep／rain／deep、mute中のcoalesced latest deep・volume変更／解除、deep→rain→deepの正確one-hotを実headerで観測した。Mac本体／他アプリの音量は変更していない。

684callback、うち244callbackで三正gainのcoherent transport headerを読み、application `output_f32` 区間のRust-global-allocator allocation／deallocation／reallocationはすべて0。実headerは30Hzなので、この244件の各sampleが三声とは断定しない。最大application時間6.8885ms、三正gain header時の最大も6.8885ms、実callbackのframe期間に対する最大比0.593325。instrumentationのTLS／timer overheadを含む区間であり、区間外のsample集計やCPAL／framework内部の全時間・確保を測った値ではない。

出力buffer700416sampleのpeak0.034132350／RMS0.002707961／最大隣接delta0.016143650、driver error／invalid sampleは0。これらは今回の18%・seed911・bounded実経路の値で、全波形のpeak保証・聴感一定・no-click判定・物理録音ではない。lease close後stream pause／drop、非RT test ownerがengineを回収／破棄した。probeとaudio moduleのcfg attachmentは実測後に除去し、通常testsがignored target sourceや物理streamに依存しない。

独立Standards source reviewはcorrect／findings0／confidence0.96。application callbackの正常経路は準備済snapshot swap、bounded mix／実声scalar tickとlazy validity、bounded atomic publication、ownershipだけを返すguardであり、normal-path heap release・待ち・file／network I/O・UI呼出を見つけなかった。これはsource auditと上記の実allocator／時間観測を組み合わせた診断で、syscall traceやCPAL依存内部まで測定した無wait／無I/O保証ではない。三件のSpec証拠補完後の最終レビューと短いユーザー耳／光smokeはまだ必要であり、P06の通常PR merge／but同期／completed、P08物理同期、P13音質／initial確定をこの節で宣言しない。

### 固有recipeの同一課題での参照評価

P06の「短い三環境menu／選択済み対象・settingsノブ・独立mute／volume・latest保持・所有PNG／Quit・capture／lockとproof limits」課題を、既存recipeだけをsystem参照とするtools／履歴なしのfresh-context completionへ変更前後で適用した。baselineは専用caseと統合操作列が未記載で、近接situations／deepやallでは不足を補えないと回答した。変更後はmixing、menu開始間隔2秒未満、実pointer／depth1段・消音volume0→18／実scroll17/11・解除18→20／再消音・latest22/12、PNG3枚／2比較／正常Quit0、captureとlockのBLOCKEDを取得した。未記載の個々のreceiptや実行結果を推測せず、入力間隔／receipt／画像をmax-three／RT／tail／waveform／no-click／物理同期へ拡張しなかった。

最初の変更後評価はEval待ち60秒でtimeout／VM resetとなり、回答は取得できなかった。保存された実transcriptから正確な元課題とbaseline回答を回収し、reset後の適正なasync呼出で上記の変更後回答を取得した。失敗した呼出を成功結果へ数えない。これはpure-reference retrieval／application gapの観測、5-rep wording microtestはN/A、pressure／自然遵守率・実native成功の試験ではない。

### 最終独立レビューとユーザー耳／光smoke

三件のP2補完とstable文書のbounded follow-upはSpec／Standardsともcorrect、findings0、confidence0.97。微小正寄与・実rate consumer残り秒・決定的fresh collision／bubbleの不足解消と、fixture／lint失敗・native／CPAL scope・P10／P13の限界が整合すると判定した。reviewerによるchecks／launch／editやunchanged productionの広い再監査はない。parentの `npm run check:docs` もPASS（全体0.37秒／本体71ms、この結果行追加前）、Latの `No init version recorded` 警告は保持し、init／hooks／設定変更をしていない。

parentはtemporaryな所有manual-smoke hostで現binary SHA256 `d84bd407f6747a0f63c003f2a697295d6aff36683c095901f9c82bf777edb0c8` を実起動し、同じsession／foreground／AX guardと実menu／focused sliderでapp volume20%・解除、receipt5/0を確認してシーンを前面へ置いた。所有PID2638、private evidence directory `target/native-e2e/human-90E6630B-878F-4B77-94D9-E047283352B4`。OS音量・TCC変更・画像取得はしていない。userへmenu雨→岩→深海の約0.5秒間隔と深海再選択、settings三択で同じ選択、消音中選択→解除の最新deep、操作時のclick／音切れ／不意な増音と消音中の光、最後の実menu Quitを具体的に依頼した。

ユーザーは「異常なし」（音の切替と最新対象、消音中の光を確認できた）を選択した。これは今回の短い耳／光の受け入れ報告で、個々の操作時刻／物理delay／全波形／P13の音色とinitialを計測・確定したとはしない。hostは4分52秒の所有process lifetime後、`P06_HUMAN_EXIT owned_pid=2638 normal_exit=0`／service exit0を返し、timeout cleanupではない正常Quitを確認した。temporary hostは終了後に除去し、shippingの観測hookや通常testsへの依存を残していない。これで今回の耳／光gateは解消した。通常PR merge／but同期／mikan completionの実結果はIssue Reportへ記録し、未実施の統合をこの行で済んだとはしない。

## P07 自動移動・時間設定・実時計の表示観測

対象はP06統合base `e25170249af6ab4727086ed82045cacd208803e5` からのP07候補。環境はDarwin27／arm64、Rust1.95、Swift6.4、現存GPUI／Ely／CPALと登録された雨・岩・深海。製品条件は [体験](../SPEC.md#体験)、[時間設定](../SPEC.md#時間設定)、[切り替えと途中操作](../SPEC.md#切り替えと途中操作)。保存・sleep／出力復旧・音質・物理同期はこの作業の証拠へ含めない。

### 独立規則と実render consumer

変更前のcontrols／world regressionは追加API未実装によるcompile RED（44 errors、exit101、2.40秒）であり、実行されたassertion失敗とは区別する。追加前のnative `--case wandering` はUnknown case／exit1、候補は起動しなかった。実装後の `cargo test --locked` は全129件PASS（39.65秒）、同じchainのfmt／fmt check／Clippy all-targets `-D warnings`／locked buildもPASS（全体48.02秒）。

- currentを除くindex写像、N=1の無交差、固定dwellの無draw、min-inclusive／max-exclusive、raw上端の丸めを独立規則で確認した。raw0拒否を8回に達した同一budgetで1000回pollしても追加drawせず、次callbackの新budgetから再開する。これは固定有限境界と写像の試験で、実PRNGの統計調査ではない。
- time変更は進行deadlineを保持、off／manualはpending取消、crossing中onは到着起点、staying中onは今を起点とする。自動g²同率は以前の表示を保持し、resumeはdeparture、manualは最新target。対時間補正、revision exhaustionと不正時間／slider値は変更前に拒否する。
- 登録3実声を準備した8kHzのactual `output_f32`／lease経路を仮想125秒分実行した。43秒の時間変更後も旧deadline60秒を保持、次自動4秒arcの途中で新しい1秒／off／onを編集しても旧arcは64秒到着まで継続、到着後の新dwellで124秒開始・125秒到着を確認した。manualと区別したphase・g²・表示／resume、mute／volume0中の世界進行、ack／errorも観測し、当該threadのheap trackingはalloc／realloc／dealloc=0/0/0。実deviceのwall cadence、CPAL／framework全体の資源・syscall・可聴品質はこのconsumer試験から主張しない。

### 実nativeの失敗と観測修正

最初の `all` は60.21秒／exit1、binary `cfdb48f694ce55f33a5cf9ef3539d07f5bcd84f4f5b14b96e4e7fe2d290d22ca`、report `target/native-e2e/run-5C4A5C90-D1E3-4705-B87B-5FAAB32D86D3/report.json`。binary／session・lifecycle・situations・deep・visualはPASS、controlsのmuteは追加時間fieldでviewport外、mixingは所有前面bounds外のpointerを送らずBLOCKED、wanderingは時間17/3まで受理したが説明のAX nodeがなくFAIL。failure cleanupを正常Quitとして数えない。

一回の限定診断では所有候補PID6223の設定窓だけをprivateに撮影し、実時間説明の視覚存在を確認して正常Quit0（2.98秒）。AX snapshotはrender準備前の7 nodesのみで、説明不存在の独立証拠ではない。installed Elyの `FormField.description` はCaptionを描画するが意味付きLabelがなく、アプリ側で同じ実CaptionをRole Labelへ載せた。入力helperは既存guardを保ち、最新owned viewportへ有限scrollで実controlを見せる共通経路へ移した。修正後fmt check／Clippy／locked buildはPASS（4.47秒）。

最初の長時間wanderingは単独menu markの変更を44.913秒に観測し、60〜95秒条件でFAIL（report `run-F5A74282-3CEF-4B1C-A529-A9FCD456F58B`）。その瞬間のmain／audio receiptがないため、生成世界時計・実到着・原因は確定していない。次のclock記録追加版も、別々のAX treeから読んだmark列「雨」「岩」を排他違反としてFAILし、clock記録まで達しなかった（`run-05FBD437-60CD-4053-AC61-67E241C701EF`）。これらを成功や製品時計の原因特定へ書き換えない。

sourceの `menuMarked` は各項目でfresh menu treeを取得する。三読取間のmenu更新を一時点のmulti-selectionとして扱わないよう、main／receiptの対象と世代、menuの排他一対象、前後の同じ文を5秒内で照合する観測へ修正した。安定した不一致はtimeout FAIL、inputの再送・guard例外のcatch・wall下限の緩和・production clock変更はない。旧44.913秒の単独観測の原因を、この修正後PASSだけから断定しない。

### 修正後の実native・所有画面

現binary SHA256 `ae16115e962808908daed06d4eed3cee4ed9fba6fb388c8211f2f0f62fe454bd`。controlsとmixingはそれぞれreport `run-9EDF020E-CF75-4B1C-B571-4CC9349C3165`／`run-8AFD4081-5956-40E4-94D2-F14E36B3C4DC` で全選択check PASS（順次chain22.12秒）。controlsはexact10/5・窓再生成後の実雨ノブ／20%解除保持・PID50781正常Quit0。mixingは既存22/12・実短間隔／独立mute・所有PNG契約を保ち、PID51253正常Quit0。

`wandering` は196.52秒／exit0、report `target/native-e2e/run-DB458DF0-0974-4F03-A9F3-FD130AF4A882/report.json`、所有PID44350。実menu／pointer on/off、menu岩・settings雨・ノブによるoff、volume/timeによるon保持、対補正1/1→2/2→1/1、最終自動1秒と実AX説明を確認した。final雨19/4からon20/4、最初のcoherent深海表示はon command開始後60.665秒、receipt文の世界時刻68.627秒（on受理文8.127秒）。深海を60.042秒保持、最初の表示変更から60.976秒後に岩へ二回目の自動表示、off21/4後65.129秒岩を保持した。全期間muteとmanual世代4は維持、実menu Quitで正常exit0。初回と二回目の60〜95秒boundは変更していない。capture・仮想時計hookは使わなかった。これはUI receipt／排他menu-radio表示／wall cadenceの実経路証拠で、生成sample counter・gain到着・抽選統計・可聴無音・物理同期ではない。

同じbinaryの所有PID97949／設定窓11165だけをhelperで撮影したprivate `target/native-e2e/p07-wandering-frame.xWegOC/settings-time.png` は960×784、directory0700／PNG0600。parentが実画像で時間slider3本、1/1分・1秒、実Caption2種とmute onを確認した。画像は上記二回のtimer成功の証拠ではない。診断Swift scaffoldは実測後に除去し、local private画像／失敗reportは保持した。権限・OS音量・認証変更、他アプリ／desktop capture、外部画像送信はない。旧 `all` の部分結果と修正後targeted三caseを区別し、現binaryの新たな `all` PASSは主張しない。

### 固有recipeの同一課題での参照評価

P07の「menu／settings on/off・対時間補正と次回適用説明・volume/timeだけでwanderが外れない・最短実時計で二回の自動表示・off後最新保持・正常Quit・receipt／時間／画像／proof limit」課題を、変更前後の実recipeだけをsystem参照にしたtools／履歴なしのcompletionへ同じ文で渡した。変更前は専用case・操作・時間／receipt契約が不足とし、既存controls／allから推測しなかった。変更後は `wandering`、Home／Right／Leftの1/1→2/2→1/1、自動1秒、説明2種、19/4→on20/4、60〜95秒の二回表示／60秒保持、off21/4後65秒保持、5秒coherent観測／正常Quit0／capture不要を取得した。共通skill／SPEC／実施権限／現存実装の未確認を不足とし、実行結果や合否を作らず、wall／AXをsample数・gain・PRNG・RT・音質・物理同期へ拡張しなかった。これはpure-reference retrievalの観測、5-rep wording microtestはN/A、自然遵守率・実native合格の試験ではない。

### 最終独立レビュー

最終UI semantic説明・owned viewport helper・nativeのcoherent表示観測・文書／recipeの現存Interface cutoverを、同じbaseからworktreeを対象にSpecとStandardsが別々にread-only reviewした。両方correct／findings0、confidence0.90／0.91。pending manual優先、変更のconsumer到達、元のtime bound、persistent incoherence FAIL、所有／hit／session guardと離散menu更新が整合すると判定した。旧44.913秒の原因はなお未確定であり、current PASSからの因果主張やproof limit拡張はない。reviewerによるchecks／launch／編集は行っていない。

parentの最終 `npm run check:docs` はPASS（全体0.42秒／本体67ms、本行追加前）。これはLat参照／graphの検査であり、製品受け入れではない。`No init version recorded` 警告は保持し、init／hooks／設定変更はしていない。

## P08 予定視覚のbracket・非表示復帰・直接Task観測

対象はP07 merge `f88d5cde9f39f408e86a6b82d5ff69b3097ccebc` からのworktree。環境はDarwin27 arm64、Rust1.95、Swift6.4。製品要件はSPECの視覚イベント／再生予定時刻／窓／高DPI、物理同期と最大負荷の全体受け入れはP18と区別する。

### 実consumerと整合境界

既存checked `presentation_time`／`MachClock`、atomic event/state slots、epoch、GPUI visibility observerを再利用した。I07は一回のpublished serialからeligible currentと最短futureを選び、同phaseだけの `WorldVisualBracket` をcaller4scratchへ整合読取する。mainはg²と息を補間し、future birthをlive cacheへ前倒ししない。phaseが変わるactual sampleにもstateを公開する。Noneは受け入れ済み色・粒・fade時刻を保持する。capacityは既存固定値を維持、bounded scan／事前確保でhistory全配列の毎frameコピーはない。最新finite viewport・論理円radius・clipだけを描画に使う。

変更前のactual `cargo test --locked p08_` は2件runtime RED（3.34秒／exit101）：t100/200のpower補間でt150の粒opacity0.28125とfuture birth非採用、regular tick直後のmanual phase変更sampleが即phase1になる条件。実装後のparent chainは141件PASS（test39.54秒）、fmt／fmt check／all-targets Clippy `-D warnings`／locked build PASS（全体47.62秒）。nearest/tie/out-of-order／phase・epoch／selected slot rewrite／future payload不正、g²と息・外挿禁止・frozen frame・live-only restore、zero/nonfinite/current viewport・円形radiusを実consumerと境界の試験で確認した。既存のstep-hold implementation testは削除し、変更後の実装へre-pinしていない。

### 実nativeの失敗を残した観測修正

以下4回はshipping SHA256 `9162386997fb208987d5d00ef7e9865fd9fdcaa98ce4f8049bf10b3f0402297c`。部分画像とworld/wanderの成功を全case成功・正常Quitへ書き換えない。

- `run-7638E83A-1F62-4BBE-8B92-FA63308AE0EA`（75.74秒／exit1）：最小化中onから60.676秒で雨→深海6/0、world進行とfirst PNG3／2比較まで成功、`NSRunningApplication.hide()` falseでFAIL。限定所有診断（3.41秒、`run-763BE3C0-FCA3-445C-9114-E146898521C1`）ではfalseでもNSHidden／AXHidden trueを観測し、AXHidden=false後正常Quit0。便利APIのreturnをhidden状態の証拠にせず、public AX setter成功と実NS/AX状態を確認する経路へ移した。
- `run-084C3412-FFAB-443B-A56E-BD549E35580C`（85.94秒／exit1）：hide復帰画像、scene close後7/0とworld進行まで成功、新sceneの即時CG/AX一致がなくFAIL。限定所有診断（8.16秒、`run-243536B5-162D-4007-AC7D-B12AED52EE3D`）ではAX960×600に対し開窓直後CG952×594、50ms後にexact同一boundsと正常Quit0。OS開窓animationを区別し、既存5秒内でowned foreground／同じAX bounds／一意CG matchを観測してからcapture開始する。位置・寸法の許容差、capture後のID/bounds guardを緩めない。
- `run-D4DD3B50-52D7-4F6D-A62F-23A47FC0F0AA`（64.31秒／exit1）：自動表示更新時のvirtual AXRoleが -25202でFAIL。既存P07のowned descendant invalidation規則を `observeOwnedSettings` へ共有し、PID／同じsettings windowを確認したsnapshot全体だけを破棄する。安定した不一致・root window失敗・menu/input/captureのguardはcatchせず、fake clockや入力再送はない。
- `run-1144F40F-A4C0-4483-B986-F12E8CEF2437`（91.93秒／exit1）：9PNG取得後reopen比較のuniform判定でFAIL。helperはvisible RGBだけを見て透明background上のmonochrome shapeをblank扱いしていた。実frameはvisible13248／crop1290240 pixels、隣接差分13080 pixelsで、interior全RGBAはuniformではない。PNG encode/decodeを通る新shape regressionはruntime RED（旧6項目PASS、新1項目blank FAIL、0.65秒）。全cropped premultiplied RGBAのuniformityへ修正後7項目PASSと同じ実frame pair PASS（chain2.49秒）。全透明／uniformRGBA／不変／寸法不一致／chrome-onlyの拒否は保持した。alpha差のPASSを可視品質・粒運動へ広げない。

限定診断の初回にはhelper prefixに含まれないprivate directory関数によるcompile失敗と、settings未作成のwindow-count失敗があった。Task probeの初回にもSwiftのthrowing expression／diagnostics呼出のcompile失敗があり、いずれも修正前の成功証拠ではない。上記失敗の再確認だけのunchanged再実行はせず、識別できる仮説と変更した観測経路で次の結果を得た。

### 修正後shipping nativeの実経路

`/usr/bin/swift scripts/native-e2e.swift ... --case presentation` は95.98秒／exit0、report `target/native-e2e/run-0CAED54E-35F0-416E-BAB5-90FF1031806F/report.json`、SHAは上記916238...、所有PID97892。消音1/0・wander off2/0、production focused Homeで滞在1/1分・自動1秒3/0→5/0、on6/0のworld2.299秒を受理した。sceneを最小化のまま、on command後60.64416秒で雨→岩、unchanged6/0／manual0、排他menu/radio/main/audioとworld62.81秒を観測。実minimize restore、runner自身の非対話opaque cover3秒→除去、実AX/NS hide3秒→show、scene close2秒中の実time1→2・receipt7/0／wander/mute on／最新岩・world86.494秒、実menu再生成と固定sizeを続けた。minimize/unhide/reopenの各stageは所有PNG3枚／隣接2比較、合計9枚／6比較すべてPASS。最後の実menu Quitは正常exit0。

parentは保存済みowned restore画像を開いて実surfaceを確認した。透過coverageとalphaも差分に含まれるため、画像は物理的な明るさ／音質／old particleの同定／Task停止／maxfps／可聴無音／RT／物理同期の証拠ではない。権限・OS設定・全体音量変更、他app／desktop capture、画像外部送信はない。

### temporary直接counterとsource attachment除去

実main-thread scene timerのtick count／最短intervalと、restore直後のaccepted live cacheをtemporaryに記録した。RT/audioにはinstrumentationを追加していない。diagnostic binary SHA256 `a2672d08840fd9f5e0db1d285ebdde0ef12a75a8a7960a156f8bab70ebcd4e07`、独立bounded run31.92秒、private directory `target/native-e2e/run-AB85C486-908D-482D-9121-67655161AA1A`、所有PID8527正常menu Quit0。

- 最小化stop→restore startはticks101不変、gap3.6731095秒。
- opaque full-cover stop→除去後startはticks190不変、gap3.061476792秒。
- 実hide stop→show startはticks279不変、gap3.109238209秒。
- closeでは旧entityのTaskをdropしてticks365、3秒のclosed保持後、新entityがticks0から開始した。
- 可視timer tickの最短intervalは33596792ns（30Hz interval以上）。再生成entityは33769417ns。startup／restoreの一回refreshとOSのdiscrete repaintはtimer tickと別であり、GPU実present／全frame cost／高DPI負荷をこのcounterへ含めない。
- 各restore直後のaccepted cacheは0／0／2／3件、future／expired／wrong_epochはすべて0。current accepted Mach時刻を更新してから描画し、隠れていた期間のexpired backlogを一括描画しなかった。この有限Rain runだけで全密度・音声pool・物理同期を保証しない。

counter field／logging／Drop実装とprivate Swift scaffold3本は実測後に除去、private failure report／PNG／候補logだけを保持した。除去後fmt／fmt check／all-targets Clippy／locked build PASS（3.31秒）、rebuilt shipping SHA256 `9c04d5670f727f59597c65df02c162bed69fd751d47fee12cfabfd8a1b11412f`。temporary診断を配布sourceやproduction test hookに残していない。

除去後のshipping SHA9c04...で同じproduction `presentation` を再実行し96.54秒／exit0、report `target/native-e2e/run-13DF0865-3E17-4418-A673-2EF2CFFF0F34/report.json`。on6/0世界2.032秒から最小化のまま60.697089209秒後に雨→深海／world62.543秒、cover／hide／close→time1→2・7/0のworld86.401秒、3組9PNG／6比較すべてPASS、所有PID20652は実menu Quit正常0。temporary counterを除いた実経路での成功であり、上記別binaryの直接Task観測とは区別する。

### 固有recipeの同一課題での参照評価

P08の「最小化中の実自動移動、hide/close中のworld/wander、再表示所有画像、正常Quit、receipt／時間／画像契約と不足前提」を、変更前後の実recipeだけにtools／履歴なしcompletionで同じ課題文を渡した。beforeは専用caseと複合契約が不足、既存lifecycle/wandering/visualを代用品として補わなかった。afterはpresentation、0/0→6/0、wall60〜95秒とmanual0／world進行、own cover3秒・AXHidden3秒・close2秒／time1→2／7/0、9PNG／6比較、exact CG readinessの5秒、正常Quit0を取得した。共通skill／SPEC／実施権限／実権限・session・buildの未確認を不足とし、実行結果を作らず、UI/wall/PNGをsample数・old particle・Task/fps・RT・音質・物理同期へ拡張しなかった。これはreference retrievalのみで、自然遵守率・実native合格の試験ではない。

### 最終独立レビュー

同じbaseからcomplete P08 worktreeを、Spec／Standardsが別々にread-only reviewした。両方correct、actionable findingsなし、confidence0.90／0.91。Specはactual consumerのbracket／g²・息／epochとphase・bounded cache・viewport・visibility、owned native契約とevidence限界が対象要件に整合すると判定。Standardsはatomic last-current再検査、None scratch非採用、資源／Task所有、pinned GPUI visibility、settings notify抑制、AX/CG/input/capture guard、RGBA regressionとscaffold除去を確認した。saved source／report／counter logを読むレビューで、reviewer自身のchecksやapp launchはない。耳／物理同期／maxDPI／GPU cost／普遍的RTを独立検証したとの主張はしない。

parentの `npm run check:docs` はPASS（全体0.41秒／本体102ms、本行追加前）。Latの参照／graph検査であり、製品受け入れではない。既存 `No init version recorded` 警告は保持し、init／hook／認証設定を変更していない。
