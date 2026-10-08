---
name: verifying-ambien
description: Use when Ambien の native E2E、実行元の権限不足・許可反映、フォーカス喪失、雨／岩ノブ・選択・音量・消音の受理、または固有の検証コマンドと受け入れ条件を確認するとき。
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
| 消音中の実シーン窓 frame 変化 | `visual` | 「音と絵」「検証条件」／所有窓 PNG と内部 pixel 比較。背景透過の変化も含み得る |
| 上記すべて | `all`（省略時） | 選択した全ケースの証拠 |
| 値・時間・状態・容量・callback 境界 | `cargo test --locked` | 現存 `src/controls.rs`、`src/audio/`、`src/platform/` の対応試験。native smoke の代わりにしない |

`--case` の値を上記から選ぶ。保存／復旧／更新を、未実装の追加 case 名で案内しない。

## 権限準備と未完ケースへの復帰

手動の準備先・設定項目・拒否の尊重・再起動条件は、共通スキルの「権限不足の手動準備と検証再開」に従う。許可対象をAmbienやSwiftに固定しない。**Ternが今回のresponsible appと確認された場合だけ**Ternの項目を準備し、別terminal／IDEから起動した場合はその実帰属を使う。実行元の終了・再起動とOS設定操作はユーザー本人が行う。

ユーザーの変更報告後、同じ実行経路で既存SDKの非prompt型確認を一度読む：

```sh
/usr/bin/swift -e 'import CoreGraphics; print(CGPreflightScreenCaptureAccess())'
```

falseなら取得APIや未完の`visual`／`situations`画像段階を実行せず、共通手順で次の具体的な準備を案内する。trueは画面取得の利用可否だけであり、安全入力や画像ケースの合格ではない。現存runnerがsession／display／AX／event／所有を確認する。binary hashと関連条件の影響を照合し、既存lifecycle／controlsの成功が有効なら、準備のためにそれらを繰り返さず、未完の選択ケースだけを新規runで実行する（以下はvisualの例。岩のケースは`--case situations`）：

```sh
/usr/bin/swift scripts/native-e2e.swift "$PWD/target/debug/ambien" \
  --case visual --artifacts "$PWD/target/native-e2e"
```

現存visualの契約は、実menu消音と受理、同じ所有シーン窓・bounds・window ID・消音markとexact foreground／AX focused windowの確認、間隔を置いた実PNG3枚、隣接2組ともhelperのcropped interior比較で変化、実menu Quit正常exit0である。helperのJSONにある所有identity・寸法・crop・測定・限界と今回のreportを照合する。途中で止まったら過去のfail／blockedと保存済みPNG／測定を保持し、2枚取得や1組の変化を全契約の成功にしない。failure cleanupは正常Quitではない。権限成功後のfocus不一致を権限不足へ戻さず、共通スキルの通常再開／限定診断の条件に従う。未実装の診断flagや自動focus回復を案内しない。


## runner 固有の証拠契約

stdout の今回の report path を読む。一意の run directory0700、`report.json`0600。`schema_version:1`、`executable`、`binary_sha256`、`selected_case`、`started_at/finished_at`、`status`、`checks[].id/status/expected/observed/artifacts` を照合する。

退出は fail=1、blocked=77、選択全ケースpass=0、優先順は fail > blocked > pass。capture不足は`visual`と`situations`の画像段階をblockedにし、独立したlifecycle／controlsは続ける。situationsの途中controls受理を全ケースPASSにせず、controls単独passを画像や製品全体の合格にしない。岩・雨の初期ノブ値はP13の品質判断前の候補で、public採用値として固定しない。

画像 helper は通常 runner が呼ぶ。直接使う場合の現存 CLI：

```sh
/usr/bin/swift scripts/native-screenshot.swift capture --pid "$PID" \
  --window-id "$CGWINDOWID" --output "$ABSOLUTE_PNG"
/usr/bin/swift scripts/native-screenshot.swift compare "$IMAGE1" "$IMAGE2"
/usr/bin/swift scripts/native-screenshot.swift --self-test
```

PID／window IDは今回の所有候補から取得する。capture outputは既存の自分所有のprivate directory内の未使用絶対path。helperは空・透明・単色・不変・寸法不一致を拒否する**このシーン向け**の比較器。両scriptは `ambien` executableと固有UI／ケースを前提にし、他アプリへそのまま流用しない。

## 個別に残す受け入れ

音の心地よさ・ガラスの質感はユーザー判断。可聴無音と解除後音量、粒だけの運動、物理同期、高DPI最大負荷、下限OS、署名公証と配布はSPECの別証拠を必要とする。実行記録と未確認範囲は `docs/verification.md`、Issue状態はmikanで確認し、このレシピに複製しない。
