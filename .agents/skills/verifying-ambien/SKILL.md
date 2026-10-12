---
name: verifying-ambien
description: Use when Ambien の native E2E、実行元の権限不足・許可反映、フォーカス喪失、雨／岩／深海ノブ・短い連続選択・ひとりで移ると時間設定・音量・消音の受理、予定表示・非表示と再表示、保存・再起動・未来版や破損の保護、設定窓のレイアウト・スクロールバー・縦の伸縮、背景のぼかし・ガラス、演出の強さの見た目、DMG・リリースの検証、または固有の検証コマンドと受け入れ条件を確認するとき。
---

# Ambien 固有の検証レシピ

**必須スキル：verifying-macos-apps**。まず [macOS 共通手順](../verifying-macos-apps/SKILL.md) を読み、このレシピで対象・ケース・観測契約を対応付ける。要件・受け入れ・実施権限の唯一の正本はプロジェクト root の `SPEC.md`。`AGENTS.md`、`docs/development.md`、対象の現存実装も確認する。

## 実在する入口

リポジトリ root で、既存 toolchain による `cargo build --locked` 後に実行する。Rust が PATH にない場合は既存 `~/.cargo/bin` を当該コマンドだけに渡す。導入や全体設定変更はしない。

```sh
/usr/bin/swift scripts/native-e2e.swift --help
/usr/bin/swift scripts/native-e2e.swift "$PWD/target/debug/ambien" \
  --case all --artifacts "$PWD/target/native-e2e"
```

| 対象 | case／入口 | SPEC 参照と実観測 |
|---|---|---|
| 窓・menu・最小化・再生成・終了 | `lifecycle` | 「窓」／AX 窓 identity・外枠・終了 PID とコード |
| 雨・風・音量・独立消音・選択・scroll | `controls` | 「設定窓」「消音・スリープ・出力停止」／menu・設定一致、settings/manual revisionと音声receipt、focused AX sliderとviewport。座標入力は実hit PID・所有window identity・最新control boundsを照合 |
| 岩の3ノブ・menu／設定選択・消音中frame | `situations` | 「シチュエーション」「切り替えと途中操作」／登録された岩、岩の量・大きさ・硬さの端点受理、雨→岩の最新手動対象とmenu mark、消音中volume0／10／20・解除20%後再消音、focused-other雨Space→岩Return、capture前の実size100／hardness0受理、端点の所有シーンPNG3枚／隣接2比較に加え、実scene雨選択から2.3秒後に岩へ再選択した直後の`rock-crossing-muted` PNG3枚／隣接2比較、実menu Quit正常0。受理対象は実gainや可聴出力の証拠ではなく、captureノブは品質採用値ではない。画像契約と実gain／物理同期は別証拠 |
| 深海の2ノブ・三環境選択・消音中frame | `deep` | 「シチュエーション」「切り替えと途中操作」／深さ0・空気0→空気100・深さ100、三環境のmenu／settings choiceと独立mute保持、深海へ戻った両端点の保持、exact receipt0/0→11/10。設定close後2.3秒のsetup分離、所有シーンの`deep-muted` PNG3枚／隣接2比較、実menu Quit正常0。receipt・待ち時間・画像は実gain／低い可聴音／気泡頻度／物理的な暗さ・速度／同期を証明せず、端点を品質採用値にしない |
| 三環境の短い連続選択・既に鳴る対象・独立mute／volume | `mixing` | 「切り替えと途中操作」「設定窓」／実menu雨→岩→深海→岩→深海→選択済み深海の開始間隔を各2秒未満で観測（入力cadenceのみ）。実settings pointerで雨→岩→深海→選択済み深海、観測depthの1段編集、消音中volume0→10→20→19→18、実viewport scroll後もfocused volume identity／receipt17/11不変、settings pointer解除18%→19→20とmenu再消音、最新深海22/12。各操作のexact main/audio receipt・排他radio／menu mark・独立mute、所有`mixing-muted` PNG3枚／2比較、設定再生成後の最新値、実menu Quit正常0。DSP settle待ちを置かず、captureが交差中か到着後かは保証しない。receipt・入力間隔・画像は実gain／最大三声RT／再入場tail／波形peak・RMS・click／物理同期を証明しない |
| 自動移動・時間の対補正・手動介入 | `wandering` | 「体験」「時間設定」「切り替えと途中操作」／実menuと設定pointerのon/off、menu岩・settings雨・雨ノブ1段がoff、消音volume1段と時間だけの編集はon保持、main/audio世代・排他menu/radio。focused Home／Right／Leftからmin1→2でmax1→2、max2→1でmin2→1、最終1/1分・自動1秒、実説明「次の滞在から適用」「次の自動切り替えから適用」。最終雨19/4・2.3秒setup分離・on20/4、wall60〜95秒内の別表示と60秒保持、最初の変更からwall60〜95秒内の二回目の別表示、off21/4後65秒最新対象保持、正常Quit0。capture不要、時間短縮hook・仮想時計なし。main／receipt／menuを前後照合し、5秒内にcoherentな排他表示が得られなければFAIL。wall・AX・receiptを生成sample数、gain／到着、PRNG一様性、RT、可聴無音・品質、物理同期の証拠にしない |
| 最小化中のworld/wander・hide/closeと復帰 | `presentation` | 「視覚イベント」「再生予定時刻」「窓」／capture必須。fresh0/0からmenu消音1/0・wander off2/0、focused Homeで滞在1/1分・自動1秒3/0→5/0、on6/0。sceneの実AX最小化を保持し、on開始からwall60〜95秒内の実別対象、unchanged6/0・manual0・排他menu/radio・finite receipt worldの進行を前後照合。最小化restore、own opaque cover3秒の除去、AXHidden=trueを実NS/AX状態で3秒観測してfalseへ、scene close2秒中のtime1→2とreceipt7/0・wander/mute on・現在のcoherent target/world進行、実menu再生成。minimize/unhide/reopenの各stageで所有PNG3枚／隣接2比較（合計9枚／6比較）と正常menu Quit0。coverはrunner自身の非対話NSWindowだけで、他appへ入力・撮影しない。UI/wall/PNGはsample数・old particle同定・Task停止/maxfps・RT・可聴品質・物理同期の証拠にしない |
| 保存・正常再起動・元ファイル保護 | `persistence` | 「保存」「復元」「ボリューム設定」「時間設定」／private HOME の production state.json。初回fixtureの岩80%／mute on、reversed20/10→10/20・2.6秒→3、範囲外ノブclamp、保存過大sizeを採らない固定位置200,100。実volume Home0・岩size Home0・time3本 Home1・menu消音offとscene移動250,150をsettled保存、nested unknown／外したknob／未知situation保持。別processでvolume0／mute off／time1/1/1／knob／位置復元、実再消音・volume1%・深海選択の交差中に正常Quitし最新last=deepをflush。次回深海から実wander on、wall60〜95秒内のcurrent-excluding自動対象をactual JSON last／coherent main/audio1/0・menuで確認し、正常Quit後また別processで自動last／wander on／fresh worldへ復元。future版／backup不能／save不能は元bytes不変、corrupt／unreadableは一意backup directoryのstate.json leafが元bytes同一。実POSIX modeでowned directoryのtemp-create／backup-renameを失敗させ、menu通知の実actionが既存設定を開くこと、独立muteと別の制御受理後のaudio world進行、各候補の正常menu Quit0を観測。capture不要、時間短縮hookなし。receipt／wall／JSONを物理的無音・音質・gain同率・PRNG一様性・RT・P12終了veto/fade・更新gateの証拠にしない |
| 消音中の実シーン窓 frame 変化 | `visual` | 「音と絵」「検証条件」／所有窓 PNG と内部 pixel 比較。背景透過の変化も含み得る |
| 上記すべて | `all`（省略時） | 選択した全ケースの証拠 |
| 値・時間・状態・容量・callback 境界 | `cargo test --locked` | 現存 `src/controls.rs`、`src/audio/`、`src/platform/`、`src/persistence.rs` の対応試験。native smoke の代わりにしない |

`--case` の値を上記から選ぶ。全候補の HOME は今回の private run 内へ隔離する。`persistence` の正常再起動だけ同じ fixture HOME を再利用し、通常 HOME／元 state に故障を入れない。未実装の復旧／更新 case 名を発明しない。

## 権限準備と未完ケースへの復帰

手動の準備先・設定項目・拒否の尊重・再起動条件は、共通スキルの「権限不足の手動準備と検証再開」に従う。許可対象をAmbienやSwiftに固定しない。**Ternが今回のresponsible appと確認された場合だけ**Ternの項目を準備し、別terminal／IDEから起動した場合はその実帰属を使う。実行元の終了・再起動とOS設定操作はユーザー本人が行う。

ユーザーの変更報告後、同じ実行経路で既存SDKの非prompt型確認を一度読む：

```sh
/usr/bin/swift -e 'import CoreGraphics; print(CGPreflightScreenCaptureAccess())'
```

falseなら取得APIや未完の`visual`／`situations`／`deep`／`mixing`／`presentation`画像段階を実行せず、共通手順で具体的な手動準備を案内する。trueは取得可否だけで安全入力／画像合格ではない。runnerはsession／display／AX／event／所有を確認し、lock=trueなら起動前blocked、本人unlockによる実変化まで再試行しない。hashと関連条件が有効な既存lifecycle／controlsは繰り返さず、未完caseだけを新規runで選ぶ（以下はvisualの例。岩はsituations、深海はdeep、連打はmixing、予定視覚と非表示復帰はpresentation）。permission不足をAIがOS/TCC設定変更で補わない。

```sh
/usr/bin/swift scripts/native-e2e.swift "$PWD/target/debug/ambien" \
  --case visual --artifacts "$PWD/target/native-e2e"
```

現存visualの契約は、実menu消音と受理、同じ所有シーン窓・bounds・window ID・消音markとexact foreground／AX focused windowの確認、間隔を置いた実PNG3枚、隣接2組ともhelperのcropped interior比較で変化、実menu Quit正常exit0である。helperのJSONにある所有identity・寸法・crop・測定・限界と今回のreportを照合する。途中で止まったら過去のfail／blockedと保存済みPNG／測定を保持し、2枚取得や1組の変化を全契約の成功にしない。failure cleanupは正常Quitではない。権限成功後のfocus不一致を権限不足へ戻さず、共通スキルの通常再開／限定診断の条件に従う。未実装の診断flagや自動focus回復を案内しない。

`presentation` のAXHiddenはpublic AX setterの成功と実NS/AX hidden状態を両方確認する。hide/unhideの便利APIのreturnだけを状態の証拠にしない。新sceneのAX boundsとCG frameはOS開窓animation中に一致しない場合があるため、同じowned window／foreground／固定AX boundsと一意のexact CG matchを既存5秒内で観測してから最初のcaptureを開始する。幅・位置の許容差を広げず、その後は同じID/boundsを毎回確認する。自動対象の更新でretireされたowned AX virtual descendantは既存のPID／同じsettings window検査でsnapshot全体を破棄する。入力・captureのguardはcatchせず、安定した不一致・重複世代を通さない。scopeの異なる一般retryやfake clockは導入しない。

`persistence` は保存例の数値を品質presetとして採らず、fixtureの復元と実production inputの結果を比較する。backupは一意directoryをcreate_dirで予約し、その中の `state.json` へ原本をrenameする。directory自体をDataとして読まず、leafのbytesと新しいdefaultsの保存を区別する。read-onlyなfuture／backup不能と通常write failureでも、正常なcontrol editが通知を消さず音声receiptが進むことを確認する。menu通知は詳細を設定singletonへ開く意味のあるactionであり、GPUI/macOSのaction validationに無効flag／no-opで対抗しない。失敗時cleanupを正常Quitにせず、各正常process終了と新しい0/0の復元を観測する。productionの最短自動滞在を待ち、時間短縮hook／途中DSP再演を使わない。

## runner 固有の証拠契約

stdout の今回の report path を読む。一意の run directory0700、`report.json`0600。`schema_version:1`、`executable`、`binary_sha256`、`selected_case`、`started_at/finished_at`、`status`、`checks[].id/status/expected/observed/artifacts` を照合する。

退出はfail=1、blocked=77、選択全case pass=0、優先順fail > blocked > pass。capture不足はvisual／situations／deep／mixing／presentationの画像段階をblockedにし、独立lifecycle／controls／wanderingは続ける。途中のcontrolsや一組の画像だけを全case PASSにせず、controls単独PASSを画像／製品全体の合格にしない。雨・岩・深海の初期ノブ値はP13品質判断前の候補でpublic採用値として固定しない。

画像 helper は通常 runner が呼ぶ。直接使う場合の現存 CLI：

```sh
/usr/bin/swift scripts/native-screenshot.swift capture --pid "$PID" \
  --window-id "$CGWINDOWID" --output "$ABSOLUTE_PNG"
/usr/bin/swift scripts/native-screenshot.swift compare "$IMAGE1" "$IMAGE2"
/usr/bin/swift scripts/native-screenshot.swift --self-test
```

PID／window IDは今回の所有候補から取得する。capture outputは自分所有のprivate directory内の未使用絶対path。helperは空・全透明・uniformなcropped premultiplied RGBA・不変・寸法不一致を拒否する。透明background上のmonochrome shapeはcoverage/alphaが異なるためuniform interiorではない。alpha差を含むpixel変化も可視品質や粒同定の証拠にしない。両scriptは `ambien` executableと固有UI／caseの専用実装で、他appへそのまま流用しない。

## runner の外の確認（probe）と、そのための準備

`native-e2e.swift` の全ケースは設定窓の AX・メニュー・保存・窓の寿命を見るが、**見た目と一部の操作は見ない**：設定窓のレイアウトとスクロールバー、設定窓の縦の伸縮、背景のぼかし、演出の強さ（控えめ／標準／派手）の描画、DMG。これらは [共通手順の5・6](../verifying-macos-apps/SKILL.md) に従い、次の probe で観測する。結果は runner の PASS と別の証拠として記録する（`docs/verification.md`）。

```sh
/usr/bin/swiftc -O scripts/native-probe.swift -o /tmp/native-probe     # 使い捨てのビルド
/tmp/native-probe windows --owner ambien                              # 窓 ID・bounds・onscreen
/tmp/native-probe drag --window-id ID --edge bottom --by 150          # 縁・タイトルバー（--dx で横にも）
/tmp/native-probe wheel --window-id ID --dy -60                       # ホイール（6回）
/tmp/native-probe press --pid PID --title ぼかし                       # AXPress（既定 role は AXCheckBox）
/tmp/native-probe backdrop --seconds 60 &                             # 全画面の白黒市松（背面に置く）
/tmp/native-probe sharpness shot.png                                  # 隣接画素差の平均（鮮明≒0.06、ぼけ≒0）
```

- **起動と隔離**：`MACOSX_DEPLOYMENT_TARGET=15.0 cargo build --release --locked --offline` の後、検証用 HOME（例 `/tmp/ambien-blur-home`）で `HOME=… ./target/release/ambien`。保存先は `$HOME/Library/Application Support/Ambien/state.json`。固定値の例：`{"version":1,"volume":0.2,"mute":true,"blur":true,"veil":0.0,"visuals":"standard"}`（`visuals` は `calm`／`standard`／`vivid`、`mute` を true にして音を出さない）。トグルは押すたびにこのファイルへ保存されるので、**測定の前に毎回書き戻す**。`just try` は release を一時 HOME で起動する利用者向けの入口。
- **窓を開く**：アプリのメニュー（2番目のメニューバー項目）の「設定…」を System Events で押す。**素の実行ファイルのプロセス名は `ambien`、`.app` にした配布物は `Ambien`**（bundle 名）なので、`process "…"` を取り違えない。シーンは 960×600 の窓、設定窓は幅520で高さ400（内容）〜作業領域。
- **ぼかしの確認**：背面に `backdrop`、前面にシーン。設定窓はシーンの中央に被るので、`drag --edge title --by 300 --dx 1100` で先にずらす。シーンの bounds を `screencapture -R` で撮り、`sharpness` を入／切／入で測る（今回の実測：切 0.058、入 0.0004）。差が出ないなら、設定が効いていない（過去に GPUI の既定素材 `Selection` が背後をぼかさず、入でも 0.058 のままだった）。
- **スクロールバー・縦の伸縮**：設定窓を `drag --edge bottom` で伸縮し、前後の bounds の数値（内容の最小400＝外枠で +タイトルバー分、幅520不変、画面下端までの最大）を記録する。最小の高さの窓を窓単位で撮り、つまみの長さが見える割合に合うことを見る。画面の縁に窓が接していたら、先にタイトルバーで動かす。
- **演出の強さの見た目**：`visuals` を固定して起動し、メニューで岩などを選び、窓単位で複数枚撮って確認する。粒は疎なので、1枚で判断せず数秒ごとに撮り、拡大して見る（派手の岩のきらめきは小さい）。見た目の好み・かっこよさはユーザーが判断する。
- **配布物**：`scripts/package-app.sh` が `dist/` に DMG と `.sha256` を作る。[共通手順の6](../verifying-macos-apps/SKILL.md) の検査を、マウントした `.app` に行う。GitHub Actions の `release.yml`（手動実行は公開せず成果物 `ambien-dmg` だけを残す）が作った DMG は、取得して同じ検査を行う。

probe は入力を物理マウスで送るため、`native-e2e.swift` の実行中は動かさない。AI は見た目の好み・音・Gatekeeper の実際の警告を合格にせず、数値で言える範囲（bounds・測定値・署名情報）だけを記録する。

## 個別に残す受け入れ

音の心地よさ・ガラスの質感はユーザー判断。可聴無音と解除後音量、粒だけの運動、物理同期、高DPI最大負荷、下限OS、Gatekeeper承認と配布はSPECの別証拠を必要とする。実行記録と未確認範囲は `docs/verification.md`、Issue状態はmikanで確認し、このレシピに複製しない。
