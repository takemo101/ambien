---
name: verifying-macos-apps
description: Use when macOS デスクトップアプリの実操作、画面取得、権限不足や許可後の反映、フォーカス喪失、終了、または検証の証拠を確認するとき。
---

# macOS アプリを検証する

**操作の成功ではなく、要件に対応した実観測を判定する。** このスキルは macOS の共通手順。アプリ名・言語・UI framework・固定ケース・CLI・仕様ファイル名を決めない。Windows／Linux は対象外。汎用 runner や権限を同梱・提供するものではない。

## 1. アプリ固有レシピを確定する

プロジェクトの規約と実際の要件文書を読み、既存の検証レシピ・実装・runner の help/source から次を対応付ける。`SPEC.md`、Cargo、通常の初期窓、特定 report schema の存在を前提にしない。

| 必要な項目 | 確認する内容 |
|---|---|
| 対象と権限 | project root、要件の参照、app bundle／executable、操作・取得・保存の許可範囲 |
| 起動・準備 | 実在する build／launch／case コマンドと実行環境。既存コマンドを使い、他アプリの flags をコピーしない |
| ケース | 事前状態 → 操作 → 期待する値／状態／時間 → 観測方法 → 証拠 → 停止条件 |
| 隔離と後始末 | 新規候補 PID、test-only 設定・文書・保存先、正常終了と失敗 cleanup の区別 |
| 観測契約 | AX selector、時刻基準・待ち上限、画像の比較領域／期待関係、実 report／exit の意味 |

欠けた情報は repo・help・source から調べる。観測手段がなければ未実装、必須の前提を確立できなければ **blocked**。CLI・属性・成功出力を捏造しない。ユーザーの既存 app instance／文書を無断で使わず、未保存文書・終了確認を自動破棄しない。

## 2. macOS の前提と対象を確認する

既存の検証ツールが行う preflight を確認する。SDK 診断が必要なら既存 Command Line Tools の Swift から以下を読む。これらは permission request ではなく、単独で安全入力を保証するものでもない。

- `CGSessionCopyCurrentDictionary()`：console、login 完了、実行 UID、一致 session、lock。lock=nil だけを解除の証拠にしない。
- `CGDisplayIsActive`／`CGDisplayIsAsleep`：対象を表示する display。ロック・sleep・loginwindow・対象不明のまま入力しない。
- `AXIsProcessTrusted()`：Accessibility。CGEvent を使うケースだけ `CGPreflightPostEventAccess()` も必要。
- `CGPreflightScreenCaptureAccess()`：実画面 capture の権限。AX やユーザーの撮影許可とは別の前提。

### 権限不足の手動準備と検証再開

**blocked だけを返さず、必要な権限・準備対象・ユーザーの操作・再開条件を案内する。** 撮影／操作への同意とOS権限は別。成功済みの権限、使わないAPI、画像が任意のケースのために追加の許可を求めない。

**ユーザーが該当許可を拒否した場合**は、追加の許可依頼・権限要求・取得を停止し、依存する必須ケースをblockedとして保持する。既に許可された独立ケースの成功を全体のPASSにしない。必須チェックを外すには明示的な範囲変更の承認が必要。ユーザーが自発的に準備再開を申し出るまで同じ許可を求めない。

1. 保留ケースに必要な前提を特定する。lock／display sleep／別consoleは権限不足に読み替えず、実行先の画面復帰・解除をユーザーへ案内する。
2. **今回の実行経路の許可対象**を、host・実行user・親appのPID／bundle／pathと、利用できる読取り専用のOS帰属情報から確定する。検証対象app、terminal／IDE等の実行元、SDK helperのrequesting processは別物。必要なら対象要求に絞ったTCCログのresponsible／subjectを照合し、TCCデータベース・鍵・認証情報は読まない。CLI名や別端末での成功から対象を推測しない。確定できなければ具体的な未確認境界を案内する。
3. ユーザー本人に、システム設定の次の項目で**確認した対象app／実行ファイル**を許可するよう依頼する。OSが終了・再起動を求めた場合は、その実行元の未保存作業を保存してから本人が再起動する。会話や他の作業も止まり得るため、エージェントは実行元を終了しない。項目に対象がない／追加できない場合も許可済みとは扱わない。

| 不足する前提 | システム設定 → プライバシーとセキュリティ |
|---|---|
| AX | アクセシビリティ |
| 画面取得 | 画面収録／画面とシステムオーディオの収録に相当する項目。オーディオのみの許可は代用にならない |
| CGEvent送信 | 使用APIの契約とOS帰属で必要な項目を確認。送信だけのために「入力監視」を一律要求しない |

OSのバージョン・言語による表示名に従う。依頼例：**「画面収録の公開確認がfalseで、画像ケースは未実行です。確認した〈実行app名・path〉を画面収録の項目で許可してください。OSが求めたら同appを再起動し、完了後はこちらで反映を確認します。」** 対象欄は実観測で埋める。許可dialogの操作、permission request API、TCC／OS／認証設定変更、helperの再署名や登録を代行しない。

4. ユーザーの変更報告や実環境の遷移を受け、**同じ実行経路の既存の非prompt型公開probe**で現在の利用可否を一度読む。申告を成功証拠にせず、準備操作とSDK観測をそれぞれの時刻で保持する。falseが続く場合は、準備先host／許可対象／実行元の再起動条件の食い違いを絞る。OSが求めた再起動が未実施ならその操作を案内する。同じ「準備したか」の質問や全ケースの再実行を繰り返さず、次に必要な具体的操作と停止理由を示す。
5. trueになったら、session・display・必要な入力権限・候補所有を確認し、**未完ケースだけ**を新規private runで再開する。以前の成功はbinary identityと関連条件への影響を確認して保持する。変更後の実操作・画像と期待値を判定し、過去のblocked／failは書き換えない。
6. 許可確認や実取得が成功した後のfocus喪失・比較不一致は、権限不足へ戻さない。部分証拠とケース判定を分け、成功済みの権限を再要求しない。

自分で起動・追跡した候補の **PID＋開始時刻＋実行ファイル／bundle identity** を記録する。AX は `AXUIElementCreateApplication(pid)` から辿り、role と実 identifier／label で対象を選ぶ。status-bar／menu-only app は初期窓を要求せず、所有 item とその操作で現れる panel を観測する。

keyboard を全体送信する直前に `NSWorkspace.shared.frontmostApplication` の PID と `AXFocusedWindow` の対象一致を再確認する。`AXMain` や窓が見えるだけでは keyboard focus の証拠にならない。modal sheet／別文書へ対象が変わったら止める。

mouse は所有 AX element の `AXPress` 等を優先する。座標入力が必要なら、送信直前に座標系・倍率・現在の bounds と実 hit target の所有／identityを照合し、別窓の被覆で受け手が変わらないことを確認する。`AXUIElementCopyElementAtPosition` と取得したelementのPID／identity等を使い、前面PIDやfocusだけからpointerの受け手を推測しない。確認不能なら入力せずblocked。status item／非key panelへのpointer操作に存在しないfocused windowを要求しない。

必要な AX 属性の unsupported を成功へ読み替えない。メッセージ timeout・木の深さ／件数・待ちを有限にする。

前面／focusを契約に持つケースが途中で止まったら、保存済みreport／画像／測定と失敗位置を保持する。喪失時の前面PID、**所有候補だけ**のAX focused window identity、bounds／window ID、session／displayを必要な範囲で観測し、未取得の値から原因を断定しない。原因を識別する新しい観測や環境変化が得られてから、新規runで対象ケースを再開する。focus guardを外す、対象を取り戻しながら無条件に試行する、部分画像やfailure cleanupを全ケース／正常Quitの成功にする回避はしない。画像取得にfocusが必要かは各ケース契約による。

旧候補と保存証拠から必要情報を得られない場合は、**通常再開とは別に、1回の診断専用private run**を行える。操作許可範囲・session／display・必要なpreflight・新規候補の所有を確認し、仮説、実在する追加の読取り専用観測手段、有限の待ち上限・停止条件を先に定める。focus／input guardは維持し、喪失時に停止して証拠を保存する。診断の終了自体をケース成功にせず、識別可能な観測や環境変化が得られなければ通常再開は保留する。追加観測のない同じ試行は診断にしない。

## 3. 操作ごとの期待値をチェックする

| 検証対象 | 実観測の例 |
|---|---|
| 設定・menu・選択 | 操作前後の AXValue／選択／チェック状態と必要な backend acknowledgment。単なる click 成功とは分ける |
| focus・scroll・再描画 | `AXFocusedUIElement` と対象 identity、実 viewport 変化、操作中の状態保持 |
| 保存・復旧 | 隔離文書／設定を変更 → 実保存 → close／再起動 → 内容を再読取り。窓が開いたことだけでは復元証拠にしない |
| 窓・panel | 要件どおりの数・同一性・geometry・最小化／再生成。自由 resize や固定サイズを共通要件にしない |
| Quit | 実 menu／shortcut 等の終了経路、owned PID 消失と終了結果。terminate／kill cleanup は正常 Quit の証拠ではない |

非同期は観測可能な acknowledgment／状態を基準に、上限付きで待つ。source・build・unit test、AX 操作、実描画、試聴／質感、物理同期、配布物の検証を別の証拠として扱う。

## 4. スクリーンショットと画像判定

撮影範囲の許可と、使用 API の OS 権限を両方確認する。実画面 capture の preflight=false なら取得 API を呼ばず **blocked**。許可 dialog・TCC／OS／認証設定を変更しない。取得不能を offscreen／export／synthetic 画像で埋めず、それらを使った場合は別の観測種別として記録する。独立した AX ケースはその前提が成立すれば続行する。

許可済みの取得方法だけで候補 PID の実 window ID・geometry・所有を照合し、対象窓／panelのみをローカル保存する。他アプリ・desktopを撮影せず、画像を外部送信しない。機密情報を含む新規文書・profile は使わない。

画像チェックは、事前に定義した **対象領域・sample 時刻・期待関係・許容差** に従う。

- animation は必要領域の変化、静的画面は安定／一致、操作結果は要件どおりの表示を確認する。画面のどこかが変わっただけでは機能の成功にならない。
- clock・cursor・透ける背景等の除外は要件に合う領域／maskとして事前に記録し、検証対象を隠さない。ack 前後の samples を混ぜない。
- decode・寸法・scale／色空間を確認する。空・単色・透明画像は取得失敗か本来の画面かを要件と取得証拠から区別する。正当な空 canvas を無条件に失敗にしない。
- pixel差分は文字の正しさ・音・物理同期・質感まで証明しない。必要なら別の観測手段を用意する。比較器の self-test はアプリ画像の受け入れではない。

## 5. 証拠・判定・再利用

既存 runner の実 schema／exit mapping を読む。このスキルのために既存 format を書き換えたり、exit0だけで PASS にしない。新たに証拠を保存する場合は一意の private run directory（0700）と file（0600）、上書きしない pathを使う。

報告の形：**要件参照・選択ケース → command／macOS・arch・app version/hash → expected と observed → 今回の report／画像／測定／stderr の local path → 判定 → 未実施範囲・人の判断・後始末**。既存 schema にない項目は実取得して補足し、未取得なら未確認と書く。

- **fail**：実チェック不一致、必要証拠の破損／矛盾。ほかの blocked より優先。
- **blocked**：必要権限・環境・観測手段がなく未完。未実施項目を PASS で補わない。
- **pass**：選択した必須チェックすべてが実証済み。任意／未選択の画像チェックや製品全体とは区別する。

他の macOS アプリでは、共通手順はそのまま、**固有レシピだけを置き換える**。根拠のない汎用 flags・固定 UI 名・receipt・画像期待値を持ち込まない。この directory を別プロジェクトの skill directory に置けば利用できるが、グローバル登録・ツール導入・設定変更は別の依頼として扱う。
