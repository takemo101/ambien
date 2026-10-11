# Ambien 詳細設計

## 文書の位置づけ

2026-10-08 時点の、承認済み [SPEC](../SPEC.md) を実装へ落とす設計である。製品要件・既定値・依存改訂の唯一の正本は SPEC。本書は実現方法、Interface の意味、所有、失敗、時計を定め、[実装計画](implementation-plan.md) は P01〜P18 の順序・作業・受け入れを定める。作業 Status は複製しない。[概念構成](architecture.md) と [検証手順](verification.md) はそれぞれ関係の説明と観測方法を扱う。

- **確認済みソース**：固定改訂の一次資料で確認したライブラリの Interface・Implementation。節内の URL が根拠である。
- **計画 Interface／設計**：製品全体の型・関数・規則を示す代表断片。実際に作成した部分は以下の現存ソースへの参照と区別し、断片をコンパイル済みコードの転載として扱わない。
- **限定検証**：固定二窓は build・幾何試験・実 GPUI/NSWindow の状態観測を実施し、2x主画面でガラス・シーン／設定操作・最小化と復帰・system appearance・全OSタイリングの成功回答を得た。診断を除去した shipping source に外部 AX／foreground CGEvent の E2E runner を実行し、設定・窓の lifecycle と、両窓からの ⌘Q による実 PID 消失・exit0 を確認した。過去の physical 終了報告と host 生存の不一致の原因特定とは区別する。別1x画面・倍率切替・負荷、音質、RT、物理同期、署名・公証・更新へ一般化しない。証拠は [検証記録](verification.md) に置く。

ユーザーの追加依頼に基づき Cargo、P01 の固定二窓、P02 の雨の合成・標準出力・視覚境界、P03 の雨ノブ・音量・独立消音・受理通知、P04 の岩と最終形の共通手動交差、P05 の深海合成と登録を作成した。P02 の雨・左右の光・シーン閉鎖中の音継続・再表示、P03 のフォーム操作・独立消音にはユーザーの確認を得た。P04 は実標準出力の寄与gains・途中再選択・消音中の世界とImpactを観測した。P05 の実factory端点波形と同じ気泡のevent記録はoffline観測であり、native／CPAL・試聴の受け入れとは分離する。新しい選択フォームのnative観測とその限界は検証記録で区別する。その他の予定配置は、必要な成果の開始時に実装する。実装・Git・鍵生成・署名・公開の権限は [次に進む条件](../SPEC.md#仕様の管理と次に進む条件) に従う。両窓の連続 resize・zoom・fullscreen、GPUI サイズ制御パッチ、新しい内部 resize 操作は採らない。最新の [固定サイズと内部描画](../SPEC.md#固定サイズと内部描画) が窓設計の基準である。

## 予定ファイル構成

変更理由が同じ Implementation を同じ Module に置く。登録・合成以外を `situations/mod.rs` に集めず、すべてを trait にもしない。必要な成果を作る作業でファイルを追加し、空 Module を一括生成しない。

| 予定ファイル | 所有・変更理由 |
|---|---|
| `src/main.rs` | GPUI host 起動、Assets、明示的な quit mode。保存・DSP を実装しない |
| `src/app.rs` | `AppModel` の所有 root、Ely 初期化、メニュー actions、設定端点、窓 handles、失敗表示、各 Module の接続 |
| `src/controls.rs` | `Normalized`、登録 index、`Revision`、`ControlState` と UI 操作 Reducer。viewport は制御に混ぜない |
| `src/situations/mod.rs` | `Registry`、`SituationDefinition`、`KnobDefinition`、`Voice`、`PreparedVoice`、登録リスト |
| `src/situations/rain.rs`・`rock.rs`・`deep.rs` | 各声の固有合成、曲線、見え方、試聴で確定するノブ初期値 |
| `src/situations/synthesis.rs` | 固定粒 pool、包絡、Poisson、色付き揺らぎの実際に共有する Implementation。外部 plugin framework ではない |
| `src/audio/mod.rs` | `AudioController` worker、標準出力、config、build/play/pause、停止・復旧 |
| `src/audio/render.rs` | `PreparedEngine`、型付き CPAL render、合成・混合・出力形式変換 |
| `src/audio/world.rs` | `WorldClock`、`Movement`、滞在・PRNG・停止再開・表示と保存用の選択 |
| `src/audio/mix.rs` | 現存 `GainRamp` のN声slerp。手動／自動durationはrender側が選び、進行中の秒はrampが保持。現存volume／muteと独立した復旧gainの `OutputRamp` はrender私有。正常終了rampは対応作業で実装 |
| `src/audio/transport.rs` | `ControlMailbox`、`StatusMailbox`、`VisualRing` と固定容量の数値転送 |
| `src/audio/lease.rs` | `EngineLease` の排他所有だけ。unsafe とその安全証明を閉じる |
| `src/audio/timing.rs` | `StreamEpoch`、Mach 時刻、CPAL 再生予定時刻、DSP 因果遅延 |
| `src/rt_alloc.rs` | 現存する `cfg(test)` 専用の Rust allocator 観測器。callback／固定 pool の区間だけ thread-local に数え、native backend 全体の観測とは分ける |
| `src/ui/mod.rs` | `SceneView`／`SettingsView` の生成・再表示 |
| `src/ui/scene.rs` | `VisibleParticles`、直接 paint、可視時だけの更新 Task |
| `src/ui/settings.rs` | controlled Ely フォーム、stable id、操作・状態・失敗の表示 |
| `src/ui/appearance.rs` | システム palette、motion、transparency の反映 |
| `src/persistence.rs` | `StateDocument`、`LoadedState`、`LoadOutcome`、`SaveWorker`、正規化・unknown 保持・退避・atomic replace・`SaveReceipt` |
| `src/lifecycle.rs` | `TerminationCoordinator`、`AttemptId`、最新状態確定・flush・fade・終了／更新の調停 |
| `src/platform/mod.rs`・`macos.rs`・`updates.rs` | 借用 AppKit、OS 通知、Sparkle Adapter。独自 host／設定 UI を作らない |
| `native/ambien_bridge.h`・`.m` | 必要な ObjC protocol・copy した blocks・KVO・終了 selector の接続 |
| `Cargo.toml`・`Cargo.lock`・`rust-toolchain.toml` | 最初の実装時に SPEC の依存と実解決版を固定 |
| `build.rs`・`resources/Info.plist`・`scripts/package-app.sh` | native 接続・実 bundle metadata・署名／公証／DMG が必要になった作業で追加 |

依存方向は UI／App → controls／registry、audio → controls／registry、persistence → registry／controls、platform → App／lifecycle とする。Voice は GPUI・ファイル・Sparkle に依存しない。UI は Voice を借用しない。復旧や保存に voice 名の分岐を追加しない。順序とファイルの追加時点は [実装計画](implementation-plan.md) を参照する。

## 共通型と登録のInterface

### 値と単位

以下は **計画 Interface**。newtype の field は Module 内だけで構築する。外部入力は検査後に変換し、callback で文字列や JSON を解釈しない。

```rust
pub struct Normalized(f32);       // finite、0 <= value <= 1
pub struct SituationIndex(usize); // Registry の 0..N
pub struct KnobIndex(usize);      // 平坦な 0..総ノブ数
pub struct Revision(u64);         // 同一プロセス内の操作／保存世代
pub struct StreamEpoch(u64);      // 出力時刻対応の世代。世界時刻ではない
pub struct WorldSeconds(f64);     // finite、生成 sample の論理経過秒
pub struct MachNanos(u64);        // mach_absolute_time の timebase 変換後
pub struct LogicalPixels(f32);    // finite、非負。device pixel ではない
pub struct VoiceLatencyFrames(f64); // finite、非負、当該voiceのdry経路遅延frames
pub struct AttemptId(u64);        // 終了調停の一意世代
```

`Normalized` への UI 入力は finite を検査し clamp、保存入力は後述の欠落／型不正規則へ、非有限な native 寸法は採用しない。index は `Registry` が解決し、長さ不整合は engine 準備時のエラーにする。実行中 registry は不変、N≥1、situation ID は一意、同一声内の knob ID は一意。revision・epoch・serial・attempt は異なる意味であり相互比較しない。加算は同一プロセス中に再利用しない checked counter とし、枯渇を公開成功として wrap させない。

### Registry、Voice

```rust
// 計画 Interface（ライブラリの API ではない）
pub struct KnobDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub initial: Normalized,
    // 値域・表示順は registry、音への曲線は声 Module が所有する
}
pub struct SituationDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub knobs: &'static [KnobDefinition],
    pub visual: VisualStyle,
    prepare: fn(VoicePreparation) -> Result<Box<dyn Voice>, VoicePreparationError>,
}
pub struct VoicePreparation {
    pub sample_rate: u32,          // 実 client rate、0 は不可
    pub seed: u64,                 // worker で用意、声ごとに独立
    pub grain_capacity: usize,     // worker で固定 pool を準備
}
pub struct VoiceFrame {
    pub world: WorldSeconds,
    pub delta_seconds: f64,        // 1 / actual_fs
    pub sample_rate: u32,
}
pub struct PreparedVoice {
    pub voice: Box<dyn Voice>,
    pub dry_causal_delay_frames: VoiceLatencyFrames,
}
pub trait Voice: Send {
    fn advance_world(&mut self, frame: VoiceFrame,
                     knobs: &[Normalized], events: &mut VisualWriter);
    fn render_sample(&mut self) -> [f32; 2];
    // gainの0/正の境界だけ。固定poolのscalar／lazy validityで古い音を無効化し、世界は保持。
    fn set_contributing(&mut self, active: bool);
    // worker排他中だけ。世界・PRNGを保持し、DSPのrate依存storageを再準備。
    fn prepare_rate(&mut self, sample_rate: u32)
        -> Result<VoiceLatencyFrames, VoicePreparationError>;
}
// Registry::definitions(&self) -> &[SituationDefinition]
// Registry::resolve(&self, id: &str) -> Option<SituationIndex>
// Registry::knob_range(&self, index: SituationIndex) -> Range<usize>
// Registry::prepare_voice(&self, index: SituationIndex, config: VoicePreparation)
//     -> Result<Box<dyn Voice>, VoicePreparationError>
```

`Registry::prepare_voice` は登録された private factory を index で呼び、構築・rate 設定・確保・reset・因果遅延の確認まで worker で行う。ID 名の match 分岐を増やさない。`PreparedVoice` は engine 内の準備済み voice と経路 metadata の保持型であり、この戻り型を変更しない。`Voice::prepare_rate` の返す `VoiceLatencyFrames` を worker が metadata に保持する。初回にもこのメソッドで最終 rate と遅延を確定する。`VoicePreparationError` は `InvalidRate`、`InvalidDefinition`、`InvalidParameters`、`UnresolvedCausalDelay` とし、未知の遅延を適当な 0 に置換しない。因果遅延なしと経路を確認した場合だけ 0 を記録する。

全声・全 pool は開始前に保持する。`advance_world` は粒の論理寿命・包絡、息、天候、ノブの追従を進め、`render_sample` はその準備済み状態を DSP で tick する。可視化 writer は当該声の index・epoch・時刻を設定した借用であり、声が窓へ直接通知する Interface ではない。両者を分けることで消音でも世界を進められる。寄与ゼロの声はサンプルを生成しないが、世界の平均・揺らぎ・寿命は論理秒で進める。寄与ゼロの声の event は見せず、消音・音量0を寄与ゼロと取り違えない。

rate 再準備では worker が engine を move して保有し、`PreparedEngine` の世界・PRNG・ノブ追従値・論理包絡／粒寿命・混合を保持したまま、各 `Voice::prepare_rate` を呼ぶ。このメソッドは **非RT専用の計画 Interface** であり callback から呼ばない。世界側 PRNG・粒の進行・天候／息は保持し、noise node 内部 phase／DSP の位相・残響の完全保持は保証しない。失敗した voice の準備完了を偽装せず、engine は worker 所有の Frozen のまま維持する。通常追加は Module と登録1行だけで、registry と `VisualStyle` を読んだ設定・保存・選択が自動的に増える。[SPEC のシチュエーション](../SPEC.md#シチュエーション) を参照する。

`set_contributing` は engine が寄与の0↔正の境界で tick 前に呼ぶ計画 Interface。inactive へ出ると音の遅延状態を無効化し、再入場は現在の論理粒・息・天候から始める。世界・PRNG・ノブ・粒寿命を reset しない。後述の固定 scalar 状態と lazy-valid delay だけを扱い、graph の再準備・大規模 buffer clear・確保・破棄を callback に持ち込まない。

## 操作のcoherencyと通知

### UI の正本と intent

現存 [`ControlState`](../src/controls.rs) の数値部分を代表断片として示す。登録から準備する private な knob owner 配列は断片に含めない。main が設定端点の唯一の writer、audio が実行状態の所有者である。時間・自動移動も同じ部分操作reducerから公開し、menu／設定は受理済みの表示対象を読み、pending manual intentを直ちに優先する。

```rust
pub struct ControlState {
    pub volume: Normalized,
    pub mute: bool,
    pub wander: bool,
    pub dwell_min_minutes: u32,
    pub dwell_max_minutes: u32,
    pub transition_seconds: u32,
    pub knobs: Box<[Normalized]>, // registry と同じ平坦配列、開始前に確保
    pub manual_target: SituationIndex,
    pub manual_revision: Revision,
    pub wander_revision: Revision,
    pub settings_revision: Revision,
}
```

`settings_revision` は保存対象の control 変更全体の世代。`manual_revision` は選択／ノブ編集だけ、`wander_revision` は wander 端点変更だけを示す。main の Reducer は一つの操作で相互依存する field をすべて変更してから snapshot を公開する。menu と設定は同じ Reducer を呼ぶ。

| 操作 | Reducer の意味 |
|---|---|
| 手動選択 | target を変更、manual revision 更新、wander=false。wander が変わった場合はその revision も更新 |
| `edit_knob(index, value)` | knob を変更、その knob の声へ target 設定、manual revision 更新、wander=false |
| volume／mute | 独立した最新端点を更新。manual intent にしない |
| wander on/off | wander revision を更新。現在の交差を手動選択へ変換しない |
| min を max より上へ操作 | min と max を同値へ一回で更新 |
| max を min より下へ操作 | max と min を同値へ一回で更新 |
| 時間変更 | settings revision だけ。進行中の予定を変更しない |
| 窓表示、設定閲覧、移動／fit、focus | 音声 intent を発行しない。矩形変更だけ保存世代へ |

現存 `ControlEdit` はvolume／mute／wander／最小・最大滞在分／自動交差秒／手動選択／ノブの部分操作で、`AppModel` が最新端点へ適用してから全snapshotを公開する。invalid index・時間範囲外・revision exhaustionは変更前に失敗させ、途中のfieldを残さない。対の時間補正も一操作であり、manual／wander世代を進めない。同じ対象の明示選択・ノブ操作は新しいmanual intentとする。Elyのstable idはmetadataとfieldの意味から固定し、設定窓の制御cacheは生成時だけ確保、離散更新時に既存storageへ値を写す。Slider／Switchはprocessor、現存Radio choiceのmouseとclean Enter／Spaceのkey pairは個別wrapper listenerで配送する。選択済みの再選択も含め、そのchoiceが捕捉したtyped targetへ一度だけ発行する。pending keyのtarget・実focus handle・blur subscriptionは設定窓が短く所有する。blurはdraw時の観測であり、同一frame内の任意のfocus ABAを保証したとはしない。Radioの実role／label／選択値と、metadata・現在端点・受理文・実Caption付き時間説明の意味付きnodeを使い、別のtesting routeは設けない。

### ControlMailbox

```rust
// 計画 Interface
// publish は main のみ。try_read_into は audio、凍結中はその代わりの worker。
fn publish(&self, state: &ControlState);
fn try_read_into(&self, dst: &mut ControlState) -> bool;
```

全 payload を `AtomicU32`／`AtomicU64` にする。bool・index・整数は整数値、f32/f64 は bits で格納する。sequence **も payload もすべて SeqCst**。scratch の Box は engine 準備済みで、読み取りで構築・clone・resize しない。

```text
single writer:
  seq := odd（書込開始）
  全 numeric field と全 knob の atomic.store(SeqCst)
  seq := 次の even（公開）
reader（試行は一回）:
  before := seq.load(SeqCst)
  odd なら false
  scratch へ全 payload.load(SeqCst)
  after := seq.load(SeqCst)
  before == after && even なら scratch を採用、それ以外は前の整合 state を維持
```

scratch と committed state は事前確保した二組。失敗 snapshot を committed に上書きせず、成功したときだけ準備済み state を swap する。読み取りは callback 区間開始時だけで、毎 sample 全 field を読まない。読み取り失敗でも spin・retry・待ちなし。全 atomic の SeqCst 順序で accepted read が書込前後を混ぜないことを設計条件にし、plain payload＋`UnsafeCell` の seqlock を混ぜない。簡易モデルの interleaving 観測は Rust 実装のメモリ検証の代わりにならない。

受領後はまず最新 volume・mute・時間端点を反映し、未適用 manual revision ならその場のゲインから一度 retarget、最後に snapshot の最新 wander 端点を反映する。手動選択の後で wander を再び on にした二操作が一 snapshot にまとまっても、古い manual に付随する false を別途適用して巻き戻さない。手動 target を sample ごとに再発行しない。knob の細かな連続変更も queue にせず最新端点が勝つ。

### StatusMailbox と失敗

`StatusMailbox` は同じ全numeric SeqCst snapshot方式の逆方向である。全体の **計画payload** は `ack_revision`、`ack_manual_revision`、`epoch`、`world`、`display_dominant`、`resume_target`、移動phase、出力状態、終了fadeのattempt／deadline。文字列やgains全体は入れず、gainsは時刻付き `WorldVisualState` に入れる。writerはengine所有中のcallback、または凍結中のworkerのどちらか一人だけ。mainは一回読み、失敗時は直前状態を保持する。

現存 `ControlReceipt` payloadは `epoch`、`ack_revision`、`ack_manual_revision`、`display_dominant`、`resume_target`、`world`。手動の表示／再開対象は受領した最新target、自動の表示はg²最大で同率は以前の表示、自動resumeの同率はdepartureとする。移動phaseは `WorldVisualState` にあり、終了fadeはまだない。未受理mailboxは `None`、一回の整合読取だけを採用する。callbackが新しい制御を採用すると、その世代をまだ公開していない印を立てる。以降のcallback途中失敗でも、排他移譲後の凍結workerが当該世代を一度公開でき、旧受理を最新の受理に見せない。settings／menuは現在epochかつmanual ackの一致した表示対象を読む。ack前のmanualはmainの最新targetを即表示し、受理の世界時刻を常時進む表示にはしない。

CPAL error callback は engine を借りず、**その stream epoch 専用の別 cell** にある固定 `ErrorCode` の atomic bitset を更新するだけ。複数の呼出元があり得るので snapshot writer と共用しない。worker は現在採用中の epoch の cell だけを消費し、旧 cell を新 stream に再使用しない。旧 monitor が遅れて DeviceChanged 等を発行しても、新出力の状態を変えない。worker が bitset と OS の現在状態を読んで出力状態を決める。`DeviceChanged`、`DeviceNotAvailable`、`StreamInvalidated`、`UnsupportedConfig`、`Xrun`、その他を固定分類し、RT で `format!`・ログ・UI・rebuild を行わない。エラーメッセージの生成は worker／main。確認済み：[CPAL ErrorKind](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/error.rs)、[error delivery](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/host/error_emit.rs)、[旧monitorの配送と寿命](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/host/coreaudio/macos/mod.rs#L274-L365)。依存内の try_lock を「依存も完全 lock 無し」と説明しない。

main への status 通知は非RT worker が mailbox を読み、output／dominant／control receipt／failure／termination結果などの **離散変更だけ**送る。gain・breath・world秒の進行を settings notify の理由にしない。native と worker→main の coalesced signal にのみ async-channel を使い、audio data／error callback から send／wake しない。scene は自身の可視 Task で VisualRing を読む。worker の状態確認 timer は初回実装20ms、通知 burst は容量1へまとめ、現在の numeric status をmainで再読する。

## 世界と移動の状態遷移

現存 `WorldClock` は生成した sample についてだけ `1 / actual_fs` 秒進む。streamのframe counter、Mach、UI Taskの経過時間とは別である。秒はf64、長時間加算は補償加算を使う。現存 `Movement` は下記の3状態を持ち、`GainRamp` が出発vectorと交差の論理秒を所有する。`target()` は到着先、`display_target()` と `resume_target(gains)` は異なる同率規則、`phase()` は滞在0／手動1／自動2。滞在deadline／到着後の抽選待ちは `WanderTimer` がprivateに持ち、保存consumerはP09以後である。

```rust
pub enum Movement {
    Staying { current: SituationIndex },
    ManualCrossing { target: SituationIndex },
    AutomaticCrossing {
        departure: SituationIndex,
        target: SituationIndex,
        display: SituationIndex,
    },
}
```

手動の多声出発混合は `GainRamp` が保持し、`departure` を一声へ音として丸めない。`departure` は自動抽選・保存同率の意味を持つ index。時間要件・初期設定は [体験](../SPEC.md#体験)、[時間設定](../SPEC.md#時間設定) を参照する。

| 現在／入力 | 次の規則 |
|---|---|
| Staying、wander on | 今から新しい滞在を抽選 |
| Staying、期限到達、N>1 | 現在以外から一様選択、Automatic Crossing 開始 |
| N=1 | 自動交差を発行しない。busy loop や自分への交差にしない |
| Crossing 到着 | target の one-hot、到着を滞在起点にする。wander on ならここで次の滞在を抽選 |
| Crossing 中 wander off | 現在の交差を完了、到着先で滞在。途中で片側へ飛ばさない |
| Crossing 中 wander on | 到着まで交差維持、到着時に新抽選 |
| 任意 phase、manual revision 更新 | 現在の混合から最新 target へ Manual Crossing。既に one-hot なら交差不要 |
| 時間端点変更 | 進行 deadline／duration は不変、次回抽選／次回自動交差で利用 |
| mute／volume0 | 世界、滞在、交差、event は通常どおり進む |
| 出力不能／sleep | lease を凍結し、世界・PRNG・残り時間は進めない |
| 新 rate／wake | checkpoint の秒と残りを維持し、新 rate で以後の sample を進める |

一様なindex抽選はrejection samplingで `[0,N-1)` を選び、現在indexを飛ばす。`DrawBudget` は `render_samples` のframe loopより外で一回だけ作り、dwellを含めCPAL callback全体のraw drawを最大8回に制限する。拒否が続くと `WanderTimer` の期限到達済みprivate状態を保ち、現在のone-hotを鳴らしたまま次callbackへ継続する。sample／scratch chunkごとのbudget再開始、偏ったmodulo／固定相手のfallbackはない。wander off／最新manual intentはpendingを取消す。min=maxは乱数なし、それ以外のdwellはraw drawの53bitから `[min,max)` 秒を作り、丸めによる上端到達を `next_down` で除く。既存SplitMix64のraw primitiveを共有し、声のopen-interval `unit()` の系列を変えず、移動は別instanceを所有する。seedはworkerで与え、callbackのOS entropy呼出しはない。交差完了sampleの世界時計を進めた後に到着起点を記録し、次の抽選で最新時間端点を使う。進行中のdeadlineと `GainRamp` durationは時間編集で書き換えない。睡眠・出力復旧のengine checkpoint／rate再準備はP10の別経路である。

表示と保存用の選択は別の出力である。Automatic の `display_dominant` は g² 最大、同率なら直前の表示 index を保持する。Manual は受領直後に target を表示し、交差中その表示を維持する。`resume_target` は Manual なら最新 target、Automatic なら g² 最大、同率なら departure。終了の snapshot はこの保存用値を使う。再起動はその声の one-hot から始め、滞在を新しく抽選し、途中割合・瞬間天候を JSON に保存しない。

## 多声slerpと出力ゲイン

**現存P04 Interface**：`GainRamp::retarget(&mut self, target: SituationIndex, duration: Duration)`、`advance_frames(&mut self, frames: usize, rate: u32)`、`gains(&self) -> &[f64]`。N 個の current／g0／接平面 vector は事前確保する。durationは手動／自動の規則を呼出側がSPECから選び、声の名前を判断しない。現存consumerはmanual revision更新時だけSPECの手動durationを渡し、volume／muteではretargetしない。view公開時だけ事前確保のf32配列へ射影する。

非負の unit vector g0 から target の one-hot e_j へ、θ=acos(clamp(g0[j],0,1)) として

\[
g(u)=\frac{\sin((1-u)\theta)}{\sin\theta}g_0+
      \frac{\sin(u\theta)}{\sin\theta}e_j,\quad 0\le u\le1.
\]

θ=0 は既に target だけなので不要交差を開始しない。開始は **その瞬間の g**、第三声を選んでも一声へ畳まない。等パワー二声は θ=π/2 の特殊ケース。最新選択で毎回 duration を取り直し、旧選択を待たせない。終端は正確に e_j、非 target は正確に0へ確定してサンプル生成を止める。

retarget時に `v=(e_j-cosθ*g0)/sinθ` を作り、`g=g0*cosφ+v*sinφ` を使う。現存実装はcurrentをその場で変えず、複写した出発vectorのnormをintentごとに一度補正する。近いtargetで `acos(1)` の丸めにより不要交差と誤認しないよう、`c=g0[j]`、`s=hypot` で求めた非target成分のnorm、`θ=atan2(s,c)` とする（数学的に上式と同じ）。`s==0` だけが不要交差。接平面は `v[j]=s`、`v[i≠j]=-c*g0[i]/s` と計算してtargetの減算cancellationを避ける。sampleのφ増分についてcosΔ／sinΔと `1/rate` を区間／rate準備時に一度計算し、rotation recurrenceで進める。毎sampleの三角関数・sqrtによる全vector再正規化はしない。固定区間境界でφからrecurrenceの誤差を補正し、末端でone-hotにする。丸めに由来するごく小さい負値だけ0へ戻し、操作に由来する正の寄与をtoleranceで先に捨てない。rate変更では経過・残り秒を保持して新Δを計算し、旧framesを新rateで読み替えない。`advance_frames(0,rate)` は不変、rate0は準備失敗で到達不可。

出力は次の独立 gain を掛ける。

```text
voice sum = Σ situation_gain[i] * calibrated_voice_sample[i]
output = voice sum * volume_gain * mute_gain * recovery_gain * shutdown_gain
```

volume／mute は別端点を持つ有限長 ramp、0・1へ正確に到達する。mute 中の volume 操作は解除にならない。復旧 gain は世界と mute を変えず出力だけ滑らかに戻す。shutdown gain は正常終了成功後だけ下げ、保存失敗では開始しない。voice calibration は試聴で決め、設定 volume として保存しない。

現存 `OutputRamp` は `PreparedEngine` 内の私有部品で、actual rateに応じた20msのsmoothstepを混合後のvolume／muteに独立適用する。これは初回実装定数であり製品の新しい数値要件ではない。同じ端点はrampを再開せず、途中の編集は現在値から追従し、有限sample数で端点を正確に確定する。出力が0でも声の世界・視覚イベント・寄与gainは進める。N声交差は別の `GainRamp` が所有する。現存の復旧gainは別の `OutputRamp` として同じengine内に保持し、epoch再準備時に無音から戻す。正常終了rampは未実装であり、volume／muteや復旧gainをその代用品にしない。

Σg²=1 は独立信号の power の数学的規則であり、波形相関・実 peak・聴感一定の証明ではない。混合は f64 積算、最終出力は finite を保つ音源設計・level 校正で収める。整数出力変換時は許容範囲へ clamp するが、この clamp を強い peak の音質修正や合格根拠に使わない。非有限発生を黙って通常音扱いせず固定 DSP fault を通知し、該当出力を無音として停止・調査する。これは未成立の DSP を隠す fallback ではない。

初期設計時の親のthrowaway数学モデルはslerpのnorm／連続retargetを観測したが、当時アプリは未実装であり実波形・RT・音質の証拠ではなかった。現存P04の数学境界・実声callback・実CPALの観測と限界は [検証記録](verification.md) に分けて残す。実三環境の重なり・最大三声の統合受け入れは [実装計画のP06](implementation-plan.md#p06-現在の多声混合から手動選択へ滑らかに寄せる)、規則は [SPECの切り替え](../SPEC.md#切り替えと途中操作) に従う。

P06の `render.rs` consumer試験は登録された実三声を使い、latest snapshotの一回適用、千回の短いretarget、現在vector保持、微小正寄与と終端のsample生成停止、mute中のDSP／event履歴を別に観測する。微小gainの初期vectorは現存rampの長いduration操作で数値境界だけを作り、productionの固定手動durationを変更しない。rate境界fixtureは非callbackで別rateの実engineを準備し、進行中の実 `GainRamp` と `Movement` だけを所有移譲して残り秒を消費する。これはrendererの数値契約であり、世界／DSP checkpoint、実stream再構成、P10復旧経路の実装や成功ではない。

実声のprivate fixtureは離脱前に音の履歴を作り、非寄与中の論理age／PRNGと古いaudio eligibilityを保ったまま、新しい衝突／気泡を決定的に入場させる。同じ時計と乱数状態のclean-history実声との波形比較、独立した新しいdry励起のonset、実reverbのwet到来を分ける。気泡の連続低音は新しい励起の証拠にせず、f32の混合後DC出力から低音を引いた値を理想的な線形重ね合わせとして厳密比較しない。raw励起と混合後の分解能を区別し、数値試験を可聴click・物理delay・音質へ拡張しない。

## 合成とノブ曲線

### 共通世界と粒

`Situations/synthesis` の共有対象は fixed pool、有限包絡、Poisson 発生、bounded 色付き揺らぎである。sample を録音ファイルから読むことや FFT を使うことはない。[音と絵](../SPEC.md#音と絵) の粒・息・天候を同じ `WorldClock` で進める。

Poisson は exponential な次の到来間隔を PRNG で抽選し、論理時間で減らす。頻度が変わる間は accumulated hazard を使い、ノブ変更ごとに乱数列を再 seed しない。間隔抽選の log は発生時だけ。各 sample の birth 採用数と fixed pool に上限を持ち、rate／密度の上端でも callback の無制限 while-loop を作らない。full ならその楽器の新規発声採用を止める。採用された粒は fixed slot に励起値・包絡・中心周波数・pan・寿命を設定するだけで Box を作らない。VisualRing full はこの音粒採用に影響しない。

pan は event と共通の normalized x から equal-power stereo を作る。y・大きさ・strength は同じ粒の印象を表すが、表示側で新乱数を作らない。包絡は有限 attack／decay とゼロ終端。天候は各ノブの平均の近傍に bounded に漂い、range と明示0 gateを越えない。息・天候は white／pink を lowpole で帯域制限した色付き変動で、正弦 LFO の単独繰返しにしない。係数／天候更新は固定の control cadence で行い、その間を補間する。世界の粒包絡は sample 時刻で更新する。

### 声固有の組合せと曲線

以下は **設計した合成モデル**。関数形を固定し、係数の聴感値は P13 の試聴で確定する。未試聴 candidate を初期プリセットとして公開しない。`x` は追従後の knob、`lerp(a,b,x)` は線形、`log_lerp(a,b,x)=a*(b/a)^x` は正の頻度／時間の対数補間。物理係数は具体声内に閉じ、UI・保存は normalized だけを扱う。

| 声／ノブ | 曲線と合成の意味 | 0 と独立性 |
|---|---|---|
| rain amount | 粒頻度は `λmax*x`、帯域制限 white／pink の連続雨成分は `rain_level*sqrt(x)`、厚みは cutoff／dry-wet の線形補間。短い noise 励起の共鳴粒を近い雨粒にする | x=0 へ到達後、雨 birth と連続雨を正確に0。既存粒／wet 余韻は終了まで続く |
| rain wind | 独立した低帯域 noise の level と流れを線形増加、ゆっくりした cutoff 揺れを wind に比例 | wind0 は風だけ0、rain amount を変更しない |
| rock amount | rolling noise の励起密度と衝突 Poisson 頻度を線形増加。疎な noise impulse＋複数の固定 resonator＋包絡 | amount0 で新規 rolling／衝突なし。既存衝突の減衰は残す |
| rock size | 中心周波数は `log_lerp(high,low,x)`、減衰時間は `log_lerp(short,long,x)`、logical radius は線形。励起エネルギーは level 校正して重さを音量増だけで作らない | x=0 は最小の岩であり mute ではない |
| rock hardness | attack は `log_lerp(soft_attack,hard_attack,x)`、余韻は `log_lerp(long,short,x)`、励起の高帯域比率は線形。短い attack に対して peak normalization を同時に適用 | x=0 は柔らかい衝突、発生密度を0にしない |
| deep depth | lowpass cutoff を対数下降、低い響きの比率・暗さは線形、motion speed は正の範囲で対数下降。pink／brown の低帯域＋固定低共鳴 | x=0 は浅い深海、基底音は残す |
| deep air | bubble Poisson 頻度と高い成分を線形増加。短い共鳴／pitch 下り包絡を bubble に用いる | air0 は bubble と高い air 成分だけ0、低い響きは独立して残す |

全声は dry 経路と開始前に準備した stereo `RestartableReverb` の wet 経路を持つ。量0では、声自体が寄与中なら既存 tail を自然に終え、不連続に捨てない。声の寄与が正確に0へ到達したときだけ、無音の状態で音の履歴を無効化する。Knob smoothing は有限時間の滑らかな補間（端点の傾き0）で、SPEC の目安に沿う実装 ramp を使う。asymptotic smoother の微小な非ゼロ値で永遠に birth を続けない。量0では追従が終わった sample から gate が0、天候／息も0 gateを解除できない。共通 volume と mute も有限到達する。

現存P04の岩は固定24衝突slot、各slotの低い三つの `Resonator<f64,U3>` と有限包絡、左右独立の色付きgated rolling noiseを持つ。低周波／高rateでpole係数を失わないようmodal計算をf64にし、FunDSPの第三入力へ `Q=center/bandwidth` を渡す。短い部屋は既存 `RestartableReverb`、DC除去とdry遅延の確認は実声の準備経路で行う。factoryは対応rateを検査し、変更時はworkerがrate依存storageを再準備する。量・大きさ・硬さの曲線はrock私有、UI／menuは登録metadataとtyped indexだけを使う。係数・ノブinitialはP13未確定のcandidateである。

現存P05の深海は固定24気泡slot、左右独立のpink／brown相当低帯域noiseと86／143Hzの固定低共鳴、depthのlowpass／暗さ／寿命、airの高い成分とPoisson密度を持つ。air0へ追従後は新規気泡と高いair成分を正確に0にし、低い連続音と既存粒／wet尾は残す。気泡の `Resonator<f64,U1>` は `Q=center/bandwidth`、中心周波数は有限包絡中に下降し、tone係数50Hz／pitch係数200Hzのcadenceで更新する。eventのx・strength・radius・寿命は実入場気泡から渡し、depthは音とSlowGlow双方の強さと寿命へ作用する。対応rateのworker準備時に実dry routingの遅延を検査する。既存synthesis primitiveとI02／I07を再利用し、production consumerへdeep名の分岐や新UI経路を追加しない。係数・initialはP13未確定のcandidateであり、offline eventを物理的な光の暗さ・速度・同期の証拠とは呼ばない。

非寄与の雨／岩／深海は新規Poisson入場を止め、既存粒の論理age・ノブ追従・息／天候の時間更新とPRNG状態を保持する。寄与境界で古い粒のaudio eligibilityとDSP履歴を無効化し、再入場で古い音を復活させない。global mute／volume0はこの非寄与状態へ変換しない。settingsは現存Ely Radioの実role／label／選択状態を持つ個別choiceを登録metadataから生成し、mouseとSpace／Enterのreleaseはそのchoiceのtyped targetへ届く。受理表示は「手動の表示対象」であり、g²最大・実聴感dominantとは呼ばない。

### 音質の確定方法

音色係数・voice level・ノブ `initial` は、まず実音を作るための candidate として各 voice Module 内で明示的に扱う。保存 JSON 例を既定値の根拠にしない。candidate の値と変更理由を試聴記録へ残し、各声、端点、最大粒、最大重なり、連続 retarget の finite／peak／RMS／クリックを実経路で観測する。統計上 finite でも心地よいとは呼ばない。

ユーザーが [成功条件](../SPEC.md#成功条件) と [音と操作の検証](../SPEC.md#音と操作) の試聴を行い、差異・穏やかさ・ループ感・強い peak・切替の聴感を評価する。試聴結果を AI の係数調整に戻し、採用値だけを各 `KnobDefinition.initial` と SPEC へ同時反映する。未採用候補を alias／旧 preset として残さない。これは実装情報の穴ではなく、仕様で選ばれた音質決定 gate である。現在は値の確定も音質成功も未観測。

## DSP準備とリアルタイム経路

### 確認済みの FunDSP と採用経路

固定版の [AudioNode](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/audionode.rs) は `set_sample_rate(f64)`、`allocate()`、`reset()`、`tick(&Frame)` を持つ。[AudioUnit](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/audiounit.rs) は object-safe で `tick(&[f32], &mut [f32])` を持つ。`An<X>` の inherent tick と AudioUnit の slice tick を混同しない。声内は具体的静的 node、異種 voice の登録 seam だけ Box を使用する。

[prelude32 の constructor](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/prelude32.rs) の実関数は `noise()`／`pink()`／`brown()`、`lowpole_hz(f)`、`lowpass_hz(f,q)`、`resonator_hz(center,第二値)`、`dcblock_hz(cutoff)`、`reverb_stereo(room_size,time,damping)`。選定理由と制約は次のとおり。

- noise／pink は連続成分と励起、lowpole は息・天候、lowpass は音色、dcblock は DC 対策、resonator は短い粒に使う。残響は下記の準備済み `RestartableReverb` とし、非寄与期間の古い tail を復活させない。
- `resonator_hz` の第二引数は宣言の名称に反し実装では **Q**。帯域 Hz でモデル化するなら `Q=center/bandwidth` へ明示変換し、正の有限値かつ実 rate で Nyquist を越えないパラメータへ準備する。[Resonator 実装](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/biquad.rs#L301-L368)
- composite `process` が使う `BufferArray::uninitialized` に `assume_init` の問題があるため **tick のみ採用**。allocate や性能測定でその UB を安全にできたとは呼ばない。[BufferArray](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/buffer.rs#L325-L351)
- `reverb3` は pre delay を rate/reset で巡回しない固定実装なので採らない。`reverb_stereo` は FDN の準備手段を持つが、tick を止めると delay 履歴も止まり、reset は delay 全体を clear する。非寄与声を無計算にしながら古い tail を戻さず、callback で大規模 clear もしないため、どちらも本経路には採らない。[reverb3 実装](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/reverb.rs#L153-L265)、[FDN](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/feedback.rs)、[Delay の reset](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/delay.rs#L96-L110)
- Slot は交差中の更新を次の交差へ待たせるので latest 即時 retarget と異なる。Net／Sequencer も可変 graph／event、返却失敗時 drop、message drain を抱えるため callback に入れない。[Slot](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/slot.rs)、[Sequencer](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/sequencer.rs)

worker が全 graph を構築→actual sample rate 設定→allocate→reset→入出力長・係数・dry 因果遅延を確認→engine 設置する。latency 分析は worker のみで、意図的 delay を別 metadata に足す。`latency()` は全 reverb tail の到来時刻を返さない。未解決の dry delay は準備成功にしない。

### 非寄与声の再入場と残響

`RestartableReverb` は `situations/synthesis.rs` 内の私有な合成部品。各 stereo channel に4並列 feedback comb と2直列 allpass を持つ Schroeder 型を使う。delay 長・damping・wet level は声ごとの試聴 candidate、feedback は worker で `10^(-3*delay_seconds/T60)` を計算して絶対値1未満、allpass 係数も絶対値1未満に検査する。tick で log／pow や長さ変更をしない。comb は遅延値を低域減衰して input＋feedback を書き、allpass は `y=delayed-a*x`、`write=x+a*y` とする。これらは設計した式であり、FunDSP の constructor を仮装しない。

各固定 delay は worker が準備した `Box<[f32]>` と `cursor`、`filled` を持つ。read は `filled == len` の時だけ現在 cursor の値、それ以前は0。write 後 cursor を一つ進め、filled は len まで飽和加算する。inactive 化は cursor=0／filled=0 と固定個数の damping state=0 **だけ**。先頭から一巡するまでに旧値はすべて現在の値で上書きされるので、全 buffer clear、cell ごとの世代 tag、新しい buffer、非寄与中の tick は不要である。len は正値、変更は worker のみ。

再入場時、音の dry filter／粒 resonator の履歴も固定 scalar 状態だけ0に戻す。固定実装で O(1) と確認した Lowpole／DCBlock／Biquad／FixedSvf／Resonator の個別 reset に限定し、composite／Delay／FDN や PRNG の reset を callback から呼ばない。noise generator、世界用低域揺らぎ、論理粒の年齢は保持し、現在の包絡から励起する。[scalar reset](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/filter.rs#L53-L61)、[Biquad](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/biquad.rs#L171-L185)、[FixedSvf](https://github.com/SamiPerttu/fundsp/blob/8479a70d813d539c67a908f88e65f6b0b344122e/src/svf.rs#L979-L987)

engine は `set_contributing(false)` を gain 終端で、`true` を次の正の寄与前に一度だけ呼ぶ。同じ active 値の反復は不変。消音／volume0をこの境界にしない。rate 再準備は worker が delay storage を再構築できる。離脱→長い非寄与→再選択を実 DSP 回帰に含め、旧 impulse の tail 不復活、新 impulse の正しい delay、世界 PRNG／寿命保持、leave/re-enter の最大 callback 時間を観測する。

### CPAL config と render

確認済み：[CPAL traits](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/traits.rs)、[config 型](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/lib.rs)、[macOS device](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/host/coreaudio/macos/device.rs)。旧 CPAL の `SampleRate(..)` や `&StreamConfig` を持ち込まない。

```rust
// 確認済みライブラリ呼出し形式。render_f32/render_i16 は計画関数。
let supported = device.default_output_config()?;
let mut config = supported.config();  // StreamConfig の値
config.buffer_size = cpal::BufferSize::Default;
let rate: u32 = config.sample_rate;
// SampleFormat::F32:
let stream = device.build_output_stream::<f32, _, _>(
    config, render_f32, report_error_code, None,
)?;
// SampleFormat::I16 も同じ config 値形式で i16 の typed callback を構築。
stream.play()?;
```

default host の `default_output_device()` をそのまま使う。列挙で同 ID の Device を選び直すと HalOutput になり、DefaultOutput の reroute 意味を失う。default config の F32／I16 を採り、未知形式・rate0・channel0 は `UnsupportedConfig`、default の None は `NoDefaultOutput`。Some だけで正常出力と断定せず config/build/play の Result を扱う。buffer は Default、device-level buffer size を書き換えない。

callback は最初に全 output を silence にし、lease 取得失敗／inactive epoch ならそのまま返す。実 frame は `data.len()/channels`。partial channel の余りは silence のまま、固定 callback 長を仮定しない。必要な固定 scratch 単位へ分割するが、world frame offset は全区間で連続。mono は `(L+R)/2`、stereo は L/R、3ch 以上は最初の L/R だけで残り silence。機器設定と OS volume を変えない。I16 変換は `FromSample<f32>` の型付き経路を使う。

現存P04の各sampleは、区間の最新snapshot適用後にslerpを1sample進め、実gainsの正／0境界をvoiceへ通知し、同じsampleの世界時刻でノブ・粒包絡・eventをadvanceする。正の寄与voiceだけtickし、f64で混合して独立volume／muteを掛け、形式変換と時刻付きevent／state公開を行って世界clockを1sample進める。終了sampleとgain端点をこの順序で揃える。P10の独立復旧gainを含め、世界・寄与と出力gainを混同しない。正常終了gainの追加は対応作業で扱う。

非寄与 voice は world update だけで tick しない。音の履歴は上記 `set_contributing` の境界で無効化し、数分後の再選択で以前の残響を再生しない。消音・volume0では world／イベントを必ず進め、寄与 voice の wet／filter tick を維持して tail の論理時間を止めず、出力計算／変換の不要な部分だけ省く。寄与と消音を同じ active flag にしない。

RT では Box／Vec の構築・clone・drop、resize、DSP の rate／allocate／latency 分析、汎用 graph reset／大規模 buffer clear、CPAL device/config/build/play/pause、保存、network、UI、文字列整形、ログ出力、lock 待ち、spin を禁止する。上記のソース確認済み固定 scalar reset／lazy-valid delay の無効化だけは寄与境界に用いる。声・固定配列は callback に **借用**するだけ。FunDSP の coefficient 更新が tan／exp を含むことも負荷測定の対象であり、準備済みという名前だけで安価と主張しない。RT の no-alloc／no-drop と最悪時間は初回 callback・最大 pool・最大同時声・leave/re-enter・control burst・ring full・停止競合を含む実経路の gate。

## EngineLeaseと資源寿命

### 実行場所と寿命

| 所有者 | 寿命と所有するもの |
|---|---|
| main の app owner | `AppModel`、Registry、窓 handles、root Subscription、foreground Tasks、native observer owner、TerminationCoordinator、Sparkle Adapter。両窓 close でも app owner は生存 |
| AudioController worker | Host／Device／Stream、epochごとに別の EngineLease Arc と gate、復旧 listener、checkpoint、DSP の構築／再準備／最終 drop |
| CPAL closure | **その epoch 専用の lease Arc と gate Arc**、固定 numeric 転送 handle。PreparedEngine の唯一の owner ではない |
| callback が取得した guard | その callback 中だけ engine の排他的借用。終了後に参照を持ち出さない |
| SaveWorker | 直列 I/O と in-flight／latest pending snapshot、終了時の receipt |
| scene owner | 固定 cache、可視 Task。close で解放、音声 engine に影響しない |

### 最小unsafeの安全証明

以下は **現存する `src/audio/lease.rs` の内部 protocol**。payload seqlock とは別物である。ownership container は **stream epoch ごとに別 cell** を作り、旧 cell を新 engine で再使用しない。P10 の復旧 worker は同じ PreparedEngine を保持してこの protocol を反復する。

```rust
struct EngineLease {
    owner: AtomicU8,                    // Idle / Callback / Worker
    engine: UnsafeCell<Option<PreparedEngine>>,
}
struct EpochGate {
    epoch: StreamEpoch,
    enabled: AtomicBool,               // 同一 gate は一度閉じたら二度と開かない
}
```

`PreparedEngine: Send` を準備の型条件にする。`unsafe impl Sync for EngineLease` の根拠は以下の排他 protocol だけ。cell を触る private 関数は guard の所持を要求し、`&mut PreparedEngine` を guard の lifetime 以上へ返さない。外部へ raw pointer や cell を公開しない。owner／gate は単純さを優先して SeqCst に固定する。

```text
callback:
  output を silence にする
  gate.enabled=false なら return
  owner.compare_exchange(Idle, Callback, SeqCst, SeqCst) を一回
  失敗なら return（retry 無し）
  CallbackGuard を作る
  gate.enabled を再確認、false なら guard を戻して return
  cell 内の Some engine を排他的に借用して render
  全借用を終了してから guard Drop が owner=Idle を公開

worker freeze:
  現在 gate.enabled=false を公開（永続的 close）
  非RTで Idle→Worker を獲得、実行中 callback の guard 返却を待つ
  WorkerGuard の下で checkpoint／engine.take、PreparedEngineをworkerへmove
  旧cellは空、旧ownerはWorkerのまま永久固定（guardをIdleへ返さない）
  旧 stream の pause/drop を非RTで実施（所有権の根拠にはしない）

worker start:
  worker が独占保持する engine に必要な Voice::prepare_rate と最新制御を適用
  新 epoch の別 cell に engine を設置し、owner=Idle
  新 epoch 専用 gate だけを enable（旧 gate は再び開かない）
  新 stream の play。失敗ならその gate をcloseしてworkerがengine.take
```

安全性は次のケースで成立する。

1. owner の CAS により二つの callback／worker が同時に cell を触れない。Idle 公開は engine の全借用終了後。
2. gate の一回目の確認後に freeze された callback も二回目で拒否される。二回目を通過した callback は owner=Callback を持ち続け、worker は終了まで cell に触れない。
3. 古い callback が遅れて CAS しても古い gate は再び true にならず、旧ownerはWorkerで固定されて取得できない。旧cellにはengineが無く、新cellの描画権を一瞬奪うこともない。新engineへ旧epochのアクセス／無音gapを持ち込まない。
4. engine の take／drop は worker 上だけ。新cellへ返すWorkerGuardと旧cellを永久閉鎖するguard終了を区別する。callback guard の Drop は scalar owner を戻すだけで DSP を drop しない。
5. build 失敗で closure が依存内に残っても、worker が保有する lease Arc で engine を take できる。残る古い closure は cell にアクセスしない。空の lease cell が最後に drop されても DSP drop は起きない。
6. unexpected panic を通常制御に使わない。private guard は unwind でも借用が終了した後 owner を返す RAII とし、callback の precondition は準備時に検査する。guard を forget／clone しない。

この protocol のメモリ安全性と実時間完了は別である。実行中 callback が永久に返らない場合、worker が排他獲得を完了したと報告してはならない。pause 成功、次 callback が無いこと、closure Drop、一定 sleep のいずれも所有移譲 ack ではない。

実 `Cargo.lock` は SPEC 選定の CPAL 0.18.2 と transitive coreaudio-rs 0.14.2 を解決した。実際の依存ソースの [CPAL macOS Stream／monitor](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/host/coreaudio/macos/mod.rs#L374-L457) は stream drop で monitor を join しない。[coreaudio-rs 0.14.2 の render callback 登録](https://docs.rs/crate/coreaudio-rs/0.14.2/source/src/audio_unit/render_callback.rs) は Box を raw にしてから native property を登録し、成功後にだけ所有 pointer を記録するため、その property 登録失敗の closure 回収を保証しない。実解決 rlib での独立 probe は未再生の DefaultOutput AudioUnit の登録→drop と本当の format mismatch の各8回で capture Drop を各1回観測したが、raw 移譲後の native property 失敗を再現していない。別の未再生 generator は callback 登録前の constructor で InvalidElement になり、その経路の証拠にはならなかった。アプリの DSP 回収と依存の closure／AudioUnit／monitor の全回収は別。詳細な実行結果・追加の反復停止観測と未確認範囲は [検証記録](verification.md) に置く。

### 出力停止と復旧

AudioController の出力状態は `Starting → Running(epoch) → Freezing → Frozen(reason) → Preparing → Running(new_epoch)`。reason は sleep／unavailable／reconfigure／shutdown。build/play／準備失敗は Frozen と明示失敗へ。window と control は生存する。Xrun は発生を表示可能にするが即時の「機器がない」と同一視しない。

worker は stream とは独立の CoreAudio property listener を所有し、default output・device alive・nominal rate の変更を固定 signal として受け、非RTで現在 default/config を読み直す。stream を捨てた後も復旧通知を受ける。登録解除は worker shutdown、listener 内で engine／App を操作しない。[公開 property listener](https://developer.apple.com/documentation/coreaudio/audioobjectaddpropertylistener(_:_:_:_:))

`DeviceChanged` は既存 DefaultOutput が reroute した可能性があり、失われた機器と即断しない。ただし旧 latency の単調 clamp を持ち越さないため、freeze→現在 config 検査→新 stream epoch と対応を作る。rate／形式不変なら DSP は再準備不要、rate が変わるときだけ worker で準備する。どちらも論理 checkpoint は維持する。出力復旧では最新 control を適用して mute を維持、recovery gain だけ上げる。無音を user mute 解除で隠さない。

現存の `OutputStatus` は起動・実行中・通常スリープ・再準備・明示失敗を分ける。freeze 自体は worker の内部所有移譲であり UI 状態に仮の成功を置かない。NSWorkspace observer の登録失敗でも設定 metadata／control transport は保持し、出力を成功に見せない。native listener は default output・alive・nominal rate に加え出力 stream/config/format を監視し、登録と解除の失敗を返す。旧 native 通知は scalar signal だけなので、worker が現在 fingerprint を再検査し、不変なら不要な epoch を作らない。凍結中の latest receipt は発行した epoch を記録し、失敗した候補世代の receipt が現在世代の操作を隠さない。rate 準備の一部が失敗した場合は、古い rate へ戻る場合でも全声を再検査する。

復旧 listener の設置と出力 availability／config 検査は分ける。constructor は独立 global listener だけを保持し、最初の query が失敗しても worker／登録済み listener を破棄しない。native fingerprint は現在 device ID・nominal rate・実 buffer layout・stream ID列・各 stream の virtual／physical AudioStreamBasicDescription を含み、CPAL の rate／channel／sample型が同じでも stream／format が変われば新 epoch を作る。初期 engine は起動時の整合済み ControlState から準備し、失敗すると途中まで書き換わり得る mailbox 読取をその committed state へ直接適用しない。最新操作は engine が既存 scratch へ読み、成功した snapshot だけを受理する。

`OutputListener::remove` は native 解除成功の `ListenerRetirement::Unregistered` と、解除先の AudioObjectID がもう存在しない `ObjectInvalidated` を区別する。CoreAudio の `kAudioHardwareBadObjectError`（`!obj`）だけは、再解除不能な旧 guard を引退させて現在 default／stream の再読へ進む。実 device／stream の変更時も shutdown 時も同じ扱いにし、それ以外の解除失敗は guard を保持して明示失敗を返す。登録・property query の `!obj` を成功にはしない。引退はアプリ所有の RcBlock 参照を解放する意味であり、HAL 所有／queued Block copy の全解放を保証しない。遅延 callback は所有 Arc の固定 signal だけへ届き、worker が現在状態を再検査する。監視の失敗を worker gate の無条件解除・timer retry・world 再生成・user mute 解除で隠さない。


## 三つの時計と再生予定時刻

現存 [`timing.rs`](../src/audio/timing.rs) が `WorldSeconds`、epoch内sample offset、`MachNanos` を分離する。`presentation_time` はactual rateでframe offsetとdeclared dry delayを検査してCPAL playbackへ加え、`MachClock` は起動時に準備したtimebaseをmainで使う。物理同期やP10の復旧checkpointを、これらのsource存在から保証しない。

| 時計 | 進行／比較できる範囲 |
|---|---|
| WorldClock | 実際に生成した sample の論理秒だけ。sleep／unavailable は停止、UI timer で増やさない |
| epoch 内 frame／CPAL StreamInstant | 現 stream 内の callback／playback と offset。異なる stream の instant 差を取らない |
| MachNanos | main の現在描画時刻と、同 epoch の CPAL host 時刻を対応。wall clock／日時ではない |

確認済み：macOS CPAL は mHostTime を timebase numer／denom で ns にし、`Stream::now()` も `mach_absolute_time` を使う。[host 変換](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/host/coreaudio/mod.rs#L68-L77)、[now](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/host/coreaudio/macos/mod.rs#L442-L445)。main は macOS Adapter で同じ `mach_absolute_time` を採り、timebase を起動時に保持し、`u128(ticks)*numer/denom` を検査して `MachNanos` へ変換する。std::Instant を CPAL timestamp と無条件に同一視しない。

```text
event.present_mach_ns = playback.as_nanos()
                       + ((frame_offset + dry_causal_delay_framesのframes値)
                          / actual_fs) をDuration／nsへ明示変換
```

`OutputCallbackInfo.timestamp()` の callback と playback は同じ host 基準。`frame_offset` は該当 callback の先頭からの位置。dry 粒の対応 attack を同期基準にし、必須の因果遅延＋意図的 pre-delay は `VoiceLatencyFrames` で準備時計算した metadata。裸の f64 を Mach 時刻へ渡さず、当該 rate で Duration／ns へ明示変換する。採用 node の route／latency と実波形観測で dry 経路を確認し、reverb tail 全体や filter のすべての群遅延が一値で同時になるとはしない。overflow／非有限 offset は timing fault、古い値で表示しない。

CPAL の playback は device buffer／latency／safety offset に基づく推定で、query 失敗に fallback があり、reroute 後は前回 playback へ単調 clamp され得る。[playback 推定](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/host/coreaudio/macos/device.rs#L877-L924)、[単調補正](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/host/mod.rs#L284-L298)。main の now が playback より小さいのは正常。`StreamInstant` の差の saturate を signed offset 計算に使わない。[timestamp Interface](https://github.com/RustAudio/cpal/blob/e1612d5d98152f8dc2a62e1b51ef7cbf4f7f26b7/src/timestamp.rs)

新 stream、route 変更、rate 変更、wake のたび epoch を変え、世界秒は維持して物理時刻の対応と表示 cache を張り直す。今の Mach 時刻から sleeping time を world へ加算しない。未来の packet を即表示せず、今の playback に対応する state を読む。CPAL 推定、Bluetooth、CoreAudio 変換、WindowServer／表示フレームの遅延を含む [同期目標](../SPEC.md#再生予定時刻) は実機 gate。自動対応を手動 offset 設定で代用しない。

## 視覚容量と描画復帰

### packet と固定slot

以下は現存I07の代表断片。実visibility cacheとraw atomic historyの所有を分ける。

```rust
pub struct VisualEvent {
    pub epoch: StreamEpoch,
    pub world: WorldSeconds,
    pub present: MachNanos,
    pub situation: SituationIndex,
    pub x: Normalized,
    pub y: Normalized,
    pub strength: Normalized,
    pub radius: LogicalPixels,
    pub shape: VisualShape,        // Point / Impact / SlowGlow
    pub lifetime: Duration,
    pub serial: u64,
}
// WorldVisualState: epoch / world / present / phase / N個のgainsとbreath_intensity
// VisualRing::new(event_capacity, registry_count)
// push_event(event) / push_state(WorldVisualState)
// read_live_into(epoch, now, &mut preallocated_cache)
// read_state_into(epoch, now, gains, breath, next_gains, next_breath)
//     -> Option<WorldVisualBracket { current, next }>
```

VisualRing は queue を reader が空けるまで producer が待つ方式にしない。固定 slot を serial modulo capacity で上書きし、**全 payload atomic＋各 slot sequence＋global publish serial** を SeqCst で公開する。state の N 配列も開始前確保した atomic slots。reader は slot の before／payload／after が同一 even、かつ必要な serial・epoch に一致する場合だけ採用する。slot 読取中の上書きは視覚だけ省略。unsafe な plain-data seqlock は使わない。

callbackのstate cadenceはworld sampleで数え、通常の視覚間隔または移動phase変更sampleで公開する。全sampleのstate queueはない。`read_state_into` は一回捕捉したglobal serial範囲とepochで、最大eligible `present <= now` と最も近いfuture `present > now` を選ぶ。同時刻は最新serial、futureの最短packetが違うphaseならcurrentだけを返し、その先の同phaseを探さない。選んだraw gain／breath配列はcallerの4本の事前確保scratchへ単発整合読取し、currentを最後にも再検査する。途中上書き／不正payloadならbracket全体Noneで、partially changed scratchは採用しない。

mainの `VisibleParticles` はcomplete bracketだけを採用し、同phaseの前後時刻からg²の寄与と息を線形補間する。amplitudeを先に混ぜて二乗せず、phase境界・未来だけのstateを先取りせず、最後のpacketの先へ外挿しない。粒opacityと薄い色はこの寄与を使い、global volume／muteは掛けない。寄与0はfade／GPU計算前に省く。bracket欠落は受け入れ済みの色・粒・fade時刻を保持し、freezeでも同じframeを維持する。未来birthはevent ringに保持して到来までlive cacheへ入れない。viewportが空・ゼロ・非有限ならpaintを休み、現在boundsでnormalized位置と論理的な円形radiusを計算してclipする。世界や音声資源を再構築しない。

### 初回実装容量とfullの意味

次は要件値ではなく **初回の実装容量**。容量測定で見直すときは同じ固定方式の値だけを変更する。

| 保持物 | 初回容量／full時 |
|---|---|
| event ring | 2048 slot、old slot 上書き。音を待たせない |
| state history | 128 atomic slot、old state上書き。mainはbounded scanで前後を選び、registry-sized4本のscratchから採用。history全N配列を毎frameコピーしない |
| VisibleParticles | 512、期限切れを先に解放、その後弱い順に採用を減らす |
| 音粒 pool | rain64／rock32／deep16、各楽器の新規発声採用を停止。visual overflow と無関係 |
| registry／knob 配列 | N／総ノブ数に応じて起動前確保。callback resize 無し |

音粒上限、transport overflow、表示上限、遅延により保持が必要な event は別に観測する。最大 event rate×lifetime＋出力遅延に対して固定容量を実測し、ring 一周以上 main が止まったときの欠落を音の欠落にしない。画面面積・倍率によって容量を増やさない。

### hiddenと復帰

scene hidden／最小化／完全occlusion／closeではowned scene Taskを止め、audioは上書き公開を続ける。GPUIの `is_visible`／visibility observerはpinned macOS hostの `NSWindow.occlusionState` を使う。visibleかつRunningのsceneだけが上限intervalのtimerを持ち、receiptだけのaudio更新はscene notifyにしない。復帰時はtimerの最初のpaint前にrefreshし、古いcursor差分を全drainせず、現epochのslotをbounded scanして `present <= now < present+lifetime` のlive eventだけを採用する。futureはringに保持、古いepochは捨てる。Task取消や30fpsの実測はUI/PNGから推定せず、対象経路の観測と区別する。

同じ world event を再読しても serial で重複表示しない。UI close 後の再作成も同じ規則。出力 Frozen 中は直前の描画状態を静止し、Mach 経過で粒が勝手に消えた世界を作らない。復旧の新 epoch では旧 event を消し、新時刻 state から再開する。mute／volume0は world を進めるので Frozen と区別する。

## 固定GPUI窓とEly設定

### 確認済みhostと窓Interface

[GPUI platform entry](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui_platform/src/gpui_platform.rs)、[Ely init](https://github.com/ZacharyZhang-NY/Ely-GPUI-Components/blob/7e39a9383dc2fd7404751d02cb166e95861ea3d6/src/lib.rs) の呼出し順を使う。

```rust
// 固定 host API の呼出し形式。実際の entry/root は src/main.rs と src/app.rs。
gpui_platform::application()
    .with_assets(ely::Assets)
    .with_quit_mode(gpui::QuitMode::Explicit)
    .run(|cx| {
        ely::init(cx);
        // Entity<AppModel> と所有rootを作り、open_windowでSceneViewを開く。
    });
```

`App::open_window(WindowOptions, ...)` で root `Entity<SceneView>`／`Entity<SettingsView>` を作る。両窓とも `WindowBounds::Windowed`、`titlebar:Some(...)`、`is_resizable:false`、`show:false`、`focus:false`、tabbing 無し。open 前 fit、同期 AppKit 設定の後 `activate_window()` で表示する。[WindowOptions](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui/src/platform.rs#L2174-L2236)、[native style／renderer 初期化](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui_macos/src/window.rs#L990-L1107)

scene は Blurred、透明 titlebar・タイトル文字なし、OS 信号ボタンを保持。空いた領域の drag を GPUI の titlebar drag 機構で有効化し、scene にボタン／label を置かない。設定は別通常窓、項目が収まらなければ scroll。window handle と root Subscription を保持し、close で handle／window-specific observer を解除する。root の解放を音声停止へ接続しない。「シーンを表示」は live handle なら前面、closed なら新しい root と Task を作る。設定も重複窓を作らず、既存 handle を前面へ。menu の登録済み選択は registry 順、wander／mute は model と同じ check 状態、quit は Coordinator 経由。

現存経路は [`src/main.rs`](../src/main.rs) → [`app::start`](../src/app.rs) → `SceneView::new`／`SettingsView::new` である。App global に `Entity<AppModel>`、独立した `AudioController` と制御 state を保持し、close subscription は窓 ID を照合して handle と window-specific observer を解除する。global action は active window の借用後に `App::defer` で適用する。scene は `Window::start_window_move` と直接 canvas を使い、native delegate は変更しない。設定は system appearance を Ely Theme に反映した出力状態・試聴候補の説明表示で、未接続の操作部品は置かない。通常 quit は GPUI の明示 quit だが、globals の Drop に依存せず保持した `Context::on_app_quit` で worker の停止・engine 回収・join を完了する。P12 の fade／保存 Coordinator を実装済みとはしない。

P02 の声 factory は worker で rate・DSP・固定 delay を準備し、`Voice::prepared_delay` で検査済み dry metadata を取り出す。同じ初期 rate を二重に準備しない。CPAL の f32/i16 closure は [`audio::render`](../src/audio/render.rs) の実 entrypoint に入り、全 data を silence にした後に lease を一回だけ CAS で借り、checked playback ns・frame offset・voice dry delay を対応づける。実行中は sample 世界時計と全声の world を進め、寄与声だけを tick する。全体 volume／mute は世界と視覚を止めない。数値 state と粒は SeqCst atomic slot／sequence、error は epoch 別 bitset。worker の出力通知だけが非 RT の `AudioShared`／bounded wake を使う。故障は worker が gate を閉じて回収・pause し、設定を維持する。自動復旧は P10。

`VisibleParticles` は固定 event 配列、registry に合わせた gains/breath と scratch、整合 header、受入済み frame の Mach 時刻を持つ。失敗時は受入済み cache と fade 時刻を変更せず、新しく開いた凍結シーンだけ worker の checkpoint 時刻で初期化する。epoch 変更では旧 cache を消し、固定 event 配列は再利用する。registry arrays の確保は境界だけで、paint／timer のたびに粒を clone しない。

### シーンpaintと更新

`canvas` の paint callback で normalized x/y を **そのフレームの最新 bounds** へ写す。radius は論理 px のまま、縦横別 scale や Retina 二重倍率を掛けない。`paint_quad`／`paint_drop_shadows(&[BoxShadow])` と content mask を使用し、円・柔らかな影を現在 bounds で clip する。[GPUI Window paint](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui/src/window.rs)

雨は点、岩は衝突、深海は SlowGlow を `VisualShape` で描く。色は registry の VisualStyle、強さは時刻対応した situation gain／breath に従い、全体 volume／mute は絵に掛けない。空またはゼロ領域は paint を休む。viewport の更新は scene の配置を変えるだけで control revision を変えない。window size に依存した shader／履歴画像を持たず、window 全面の offscreen を毎 frame 生成しない。

`request_animation_frame` だけでは上限 cadence にならない。visible scene だけが timer Task を保持し、期限の notify で必要な更新を要求する。`window.is_visible()`／`observe_window_visibility` を使い、完全 occlusion／最小化時は task を停止、復帰時は live 読取を一度行ってから task を再開する。cadence は SPEC の最大 fps から決め、OS の resize／露出による redraw は animation と別。settings を同じ毎 frame Task に入れず、control／dominant／output／receipt／failure／appearance の離散変化だけ notify。gain／breath／world秒で設定窓を常時 notify しない。

### Ely実Interfaceとcontrolled form

確認済みの [Slider](https://github.com/ZacharyZhang-NY/Ely-GPUI-Components/blob/7e39a9383dc2fd7404751d02cb166e95861ea3d6/src/forms/slider.rs)、[Switch](https://github.com/ZacharyZhang-NY/Ely-GPUI-Components/blob/7e39a9383dc2fd7404751d02cb166e95861ea3d6/src/forms/switch.rs)、[SegmentedControl](https://github.com/ZacharyZhang-NY/Ely-GPUI-Components/blob/7e39a9383dc2fd7404751d02cb166e95861ea3d6/src/buttons/segmented.rs) を使う。

```text
Slider::new(id, value:f64).range(min,max).step(step)
  .on_change(Fn(f64, &mut Window, &mut App))
Switch::new(id, on:bool)
  .on_change(Fn(bool, &mut Window, &mut App))
SegmentedControl::new(id, selected:SharedString)
  .segment(value,label,None)
  .on_change(Fn(&SharedString, &mut Window, &mut App))
FormField.child(control)
```

Slider／Switch の値 event は `cx.processor` または Entity update closure で Reducer へ渡し、segment の borrowed SharedString は `cx.listener` または Entity update 内で Registry 解決する。全 control は render 時の model 値を入力とする controlled form。stable id は `volume`／`mute`／`wander`／時間 field、knob は situation ID＋knob ID の組。viewport／fit で Entity・input state・focus を作り直さない。knob step0.01 と volume1% は UI 実装値、時間は SPEC の整数単位。メニューと同じ端点を操作する。

UI は [設定窓](../SPEC.md#設定窓) の順に配置し、時間の「次回適用」を説明する。自動の dominant 更新と manual の即時 target 表示を区別し、単に閲覧して wander を止めない。出力・保存 failure の表示に必要な Ely 部品も使い、4種類限定の framework 制約を作らない。

## AppKit座標と範囲補正

### raw借用と固定制約

`gpui::Window::window_handle()` の inherent method は論理 handle であり raw native handle ではない。`raw_window_handle::HasWindowHandle::window_handle(window)` を完全修飾し、`RawWindowHandle::AppKit` の `ns_view` から所属 `NSWindow` を **main の同期 scope だけ**借用する。raw pointer の retain-from-raw／release はしない。typed `NSView::window()` が返す一時 Retained は getter の正常所有としてその scope で終了させる。raw NSView、NSWindow、delegate を await、worker、voice、observer block へ渡さない。[NSView getter](https://docs.rs/objc2-app-kit/0.3.2/objc2_app_kit/struct.NSView.html#method.window)

AppKit Adapter が collectionBehavior の既存 bits から FullScreenPrimary／Auxiliary／AllowsTiling だけ除去し、FullScreenNone／FullScreenDisallowsTiling を加える。他の bits を捨てない。`standardWindowButton(NSWindowButton::ZoomButton).setEnabled(false)`、`contentMinSize==contentMaxSize==補正済み固定 content`。Ambien の menu／actions に zoom・fullscreen・tile・resize を登録しない。GPUI の window delegate は置換しない。

[FullScreenDisallowsTiling](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/fullscreendisallowstiling) は fullscreen／Split View の flag であり、desktop edge／Option drag／menu／keyboard tiling 全経路を禁止する万能 flag ではない。`is_movable=false` は位置移動も奪うので採らず、SPEC の drag を維持する。nonresizable＋zoom 無効＋fullscreen flags の実装後、desktop tiling 経路でも **固定 content が拡大しない**ことを実機 gate にする。未成立なら P01／P11／受け入れを完了にせず、未承認 delegate／renderer patch を勝手に加えない。[flags と button](https://docs.rs/objc2-app-kit/0.3.2/src/objc2_app_kit/generated/NSWindow.rs.html#127-175)

### screen、outer、content、top-left

現存 Adapter は [`src/platform/macos.rs`](../src/platform/macos.rs) の `prepare_window(WindowRole, DisplayId, Option<&WindowPlacement>) -> Result<PreparedWindow, WindowGeometryError>`、`capture_placement(&Window) -> Result<(WindowPlacement, DisplayId), WindowGeometryError>`、`constrain_window(&Window, &PreparedWindow) -> Result<(), WindowGeometryError>`。`WindowPlacement` は所有 screen 相対 outer top-left と logical content 寸法、保存 UUID を持つ。capture は同じ借用 NSWindow／NSScreen から位置と実 display ID を読み、main が display 変更時だけ UUID を解決する。GPUI の display list を drag ごとに確保しない。saved size は現在の固定基準として採らず、現在の workarea と AppKit の実 frame/content conversion で補正する。GPUI の f32 dimensions は縮小側へ、outer top-left は外枠の収まる表現可能範囲へ丸める。画面変更通知・外観・accessibility は P11 の別 gate。

1. 現在 displays の UUID を保存 UUID と照合し、無ければ現在 primary display。UUID の取得失敗を一致と扱わない。display 未取得は最後に確認した finite geometry／制約を維持し、架空の無限領域を作らない。初回から有効 display が無い場合は窓生成を延期して明示失敗とし、取得後に通常 fit を行う。
2. AppKit NSScreen の global bottom-left `frame` と `visibleFrame` を取得する。GPUI の display bounds／visible_bounds は display 内 top-left 論理座標なので区別する。[GPUI MacDisplay 変換](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui_macos/src/display.rs#L112-L150)
3. **open 前**に GPUI と同じ style（Titled／Closable、採用した Miniaturizable／FullSizeContentView、Resizable 無し）で `NSWindow::contentRectForFrameRect_styleMask(visible_outer,style,mtm)`、`frameRectForContentRect_styleMask(...)` を使う。固定 titlebar 高さを引かない。[geometry helpers](https://docs.rs/objc2-app-kit/0.3.2/src/objc2_app_kit/generated/NSWindow.rs.html#529-542)
4. 基準 content を利用可能 content の各寸法以内へ補正し、その content の outer frame を visibleFrame へ clamp する。GPUI の初期 bounds.size は content、bounds.origin は display 相対 outer top-left。renderer が open 内で作られるので、初回サイズを open 後に縮めるだけで過大確保の予防としない。
5. raw Adapter の固定 min/max を設定後に窓を表示する。画面移動・撤去・作業領域変更で今の画面を再読し、必要なら固定寸法を再 fit。前の狭い画面の縮小を任意可変サイズとして引き継がず、現在基準から再計算する。

座標の代表式（全値は logical points）を固定する。screen global frame を S、window outer の高さを H、保存相対 top-left を (x,y) とすると、AppKit outer origin は

```text
outer.minX = S.minX + x
outer.minY = S.maxY - y - H
保存 x = outer.minX - S.minX
保存 y = S.maxY - outer.maxY
```

clamp は global visibleFrame で外枠全体について行い、最後に相対座標へ戻す。負の global 座標の screen も正常。content size と outer size は static helpers／実窓の geometry で変換する。保存 `window.width/height` は content、`x/y` は outer top-left。[SPEC の復元](../SPEC.md#復元) に従い、落ち着いた矩形と UUID を一 snapshot にする。move 通知ごと・毎 frame に disk 書込みしない。

### appearanceとnative通知

起動時と変更時に system appearance、reduce motion／transparency を再読する。Ely の既定テーマをそのまま根拠にせず `Theme::set_mode_now(Mode::from(appearance),cx)`。motion は GPUI `App::set_reduce_motion`、Ely `Theme::update` 内 reduced_motion、scene の motion flag に別々に適用。scene は漂い・急点滅を抑え、静止した柔らかい明滅へ。音声 control は変更しない。reduce transparency のみ system palette の opaque 背景へ置き換え、解除時は Blurred／透明へ戻す。[アクセシビリティ](../SPEC.md#アクセシビリティ)

通知登録は NSWorkspace 自身の `notificationCenter` など元の center を owner が保持する。native 同期 callback 内に App borrow を作らない。`async_channel::bounded::<()>(1)` の Send な Sender だけを capture し `try_send`、Full は再読 pending の coalesce、Closed は終了済み。AsyncApp／ForegroundExecutor／raw NSView を Send block に capture しない。root 所有の foreground Task が `recv().await → AsyncApp::update → App::defer → WeakEntity::update` の順で、その時点の native 値を再読して notify する。古い矩形や通知 pointer を queue に積まない。[spawn／defer](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui/src/app.rs#L2034-L2074)、[AsyncApp lifetime](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui/src/app/async_context.rs#L16-L32)

登録後に初回再読 signal を入れる。window observer は close 時解除、global observer は app owner が所有。shutdown は **元 center で observer 解除→channel close→Task drop**。Retained token の drop だけで登録が解除されたとしない。Task を detach しない。

現存経路は [`src/ui/appearance.rs`](../src/ui/appearance.rs) の `AccessibilityState`（reduce motion／transparency）、`Motion`（粒の包絡・形の拡大・背景 glow の膨らみ）、容量1の `change_channel`（`Queued`／`Coalesced`／`Closed`）と、[`src/platform/macos.rs`](../src/platform/macos.rs) の `EnvironmentMonitor`（workspace の `notificationCenter` の accessibility 表示設定変更と、既定 center の画面構成変更を各 center の token で登録）、`read_display_options`、`read_screen_layout`、`refit_window` である。ブロックが捕捉するのは `Send + Sync` の通知関数だけで、App／NSView は持たない。`AppModel` が monitor・notifier・Task を所有し、起動時に一度読んで GPUI／Ely の reduced motion を設定してから窓を開く。通知後は owner Task が最新値を再読し、変化した accessibility だけを反映する。scene は `set_accessibility` で背景を Opaque／Blurred に切り替え、Opaque では Ely palette の `bg` を塗る。音声 control・revision・粒 cache は変えない。system の light／dark は両窓の `observe_window_appearance` が Theme mode へ反映し、シーンの再表示前にも再読する。

窓の再 fit は、画面構成（各 screen の frame／visibleFrame）が前回と異なる通知、またはシーン／設定窓が別 display へ移ったときだけ行う。accessibility の切替だけでは、利用者が置いた窓を動かさない。`refit_target` は現在の窓寸法ではなく役割の固定基準から計算し、狭い画面で縮めた寸法を引き継がない。`refit_window` は変更前に取得・検査をすべて終え、画面を取得できないときは最後の有限寸法と制約を保持する。AppKit の `setFrame` は resize／move callback を同期的に呼ぶが、GPUI の handler は呼出し元が保持する App borrow により更新を捨てるため、frame を変更したときは同じ `handle.update` 内で `Window::bounds_changed` を呼び、viewport・scale・display と bounds observer を最新にする。`App::defer` は同じ borrow の中で実行されるため、この問題の回避策にならない。

### 高DPIと確保コスト

固定 content でも物理 surface は倍率で変わる。paint の logical radius に倍率を再乗算せず、GPU／GPUI が物理 pixel へ変換する。画面面積に比例して音粒・event・表示粒を増やさない。GPUI の BGRA8 surface、最大3枚の drawable、パス用 intermediate、MSAA、atlas、旧資源寿命、WindowServer、Blur は別の実資源として観測する。[Metal resize](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui_apple/src/metal_renderer.rs#L390-L435)

サイズ／倍率 observer より前に GPUI が再確保する経路があるため、通知後 fit を確保前 prevention と説明しない。API の寸法上限と面積分の memory 確保成功も別。任意巨大解像度への安全保証や GPUI patch を追加せず、[固定窓の既知コスト](../SPEC.md#基盤のサイズ制約と既知のコスト) と [高DPI](../SPEC.md#高dpiと描画負荷) の対応範囲を実測する。実確保失敗を透明性／資源成功として報告せず、native geometry error と renderer の技術 gate を区別する。

## 保存と読取り保護

### 読取Interfaceと正規化

**現存 Interface**：[`src/persistence.rs`](../src/persistence.rs) の `StateDocument` は元 `serde_json::Value` を worker 内だけで保持する。`LoadedState` は Registry 解決済み control 端点と `Option<WindowPlacement>`、`WritePolicy` は `Writable` または `ReadOnly(reason)`。`SaveStartup { settings, notice }` を bounded channel で最初の stream／窓より前に main が受け取り、復元した volume／mute／last／knobs を最初の sample から渡す。metadata Registry は起動時に一度構築し、音声 worker の声準備と保存 worker が共有する。追加の direct 依存は既存解決版 `serde_json = "=1.0.151"` だけ。

```rust
pub enum LoadOutcome {
    Loaded { settings: LoadedState, document: StateDocument,
             write_policy: WritePolicy, notice: Option<String> },
    Defaults { reason: LoadReason, document: StateDocument,
               write_policy: WritePolicy },
}
pub struct SaveReceipt {
    pub revision: Revision,
    pub result: Result<(), SaveError>,
}
```

パス／形式は [保存](../SPEC.md#保存) を参照する。file 不在は正常 defaults＋Writable。root object と version を検査し、known version では値を正規化する。既知 numeric が finite なら範囲端へ clamp、時間は整数単位へ四捨五入して許可範囲へ、保存 min>max は入替。UI の相互追従と読み込み入替を混同しない。欠落・型不正／非有限な既知値は既定値、欠落ノブはその Module の初期値、未登録 last は registry 先頭。起動時に不正 field の修復を知らせるが、未知 field を不正だから削除しない。

known 値だけを元 object に上書きする。unknown top-level key、未知 situation、外した knob、window の未知 key は保持。known `situations.rain` のような object 内も既知 knob を merge し、object 全体を置換しない。既知 field の object 型自体が不正な場合だけ、known schema の object に修復する。表示 label を保存 ID として使わない。

未対応の未来 version は元ファイルを保護して ReadOnly、defaults 起動＋明示 failure。未知 schema を書込み可能な空 document と取り違えない。migration／互換 shim を追加しない。parse 不能／unreadable／root または version の型不正は、一意の backup 名へ **rename で退避**を試みる。元を読めない場合でも rename の Result を独立に扱い、退避成功後だけ Writable defaults。退避不能は ReadOnly、その回の保存は明示 failure。未来 version を corruption として退避しない。

### 保存世代、I/O、flush

SaveWorker は単一の直列 worker。`in-flight=1` と `latest pending<=1` で、過去 snapshot を全部 queue にしない。App の保存 generation は control の settings revision、再開対象の離散変更、有限窓位置変更から発行し、audio ack の制御 revision と意味を区別する。main の autosave debounce は500ms、一つの取消可能 Task を差し替え、落ち着いた後だけ `SaveSnapshot { controls, resume_target, window }` を作る。window bounds observer は閉じる前の最新位置も保持し、元 window ID と現在の scene handle を照合して退役通知を捨てる。flush は debounce を取消し、明示 retry として新しい保存世代を発行して最新 snapshot を submit する。audio から保存を呼ばない。

保存は同 directory 内の一意 temp を create_new→write_all→file.sync_all→rename(temp, state.json)→parent directory sync。rename 成功前に old file を消さない。temp の衝突は別名、失敗は `CreateTemp`／`Write`／`SyncFile`／`Replace`／`SyncDirectory`／`ReadOnly` の段階を含む SaveError。rename 後の directory sync failure は「置換されたが durability 未確認」という結果であり成功 receipt にしない。error cleanup は temp のみ、old や backup を消さない。

`SaveReceipt { revision, result }` は実 I/O 完了後に latest mailbox と bounded-one notification で返す。submit は同一 process で厳密増加の revision を要求する。flush は指定した pending／in-flight／保持済み generation の実完了を待ち、上書きされた pending・未提出・失効した世代は `UnknownRevision`、重複／逆順 submit は `InvalidRevision` を返して無期限に待たない。待機済み flush はその generation の completion を所有する。main は最新世代と一致した receipt だけを扱い、古い成功で最新保存失敗を消さない。通常 failure は音を止めず、詳細を既存設定窓へ表示する menu action と設定冒頭の semantic label に通知する。通常の有効 control edit では保存通知を消さない。GPUI/macOS の menu validation は action availability を返すため、保存通知に無効 flag や no-op action を使わない。次の明示変更／flush で再保存できるが ReadOnly を解除しない。終了は worker を join し、thread panic／startup channel loss／停止を明示失敗にする。最新 flush と audio shutdown は接続済みだが、終了 veto／fade／更新の調停は P12 以後の gate。

保存 snapshot は knobs の端点、volume／mute／wander／時間、resume_target の ID、安定した矩形だけ。world／DSP 位相／粒／gains は書かない。unknown を保持する StateDocument は読み込み後に一度だけ SaveWorker へ所有移譲し、以後 worker が正規化した既知 field を merge する。毎 snapshot に元 JSON 全体を clone せず、main は端点と矩形だけを渡す。document を callback に渡さない。atomic replace は POSIX の rename と sync の契約に従う設計で、電源断を含む万能な durability や新 version の解釈成功を主張しない。

## 終了と更新の調停

### 通常終了と最新snapshot

**計画 Interface**：`TerminationCoordinator::request(reason: NormalQuit | UpdateInstall) -> AttemptId`。同一 active attempt に属する save／audio ack／fade／native termination／Sparkle continuation を束ねる。⌘Q・menu、OS termination、Sparkle は別々の終了実装を持たない。

```text
Idle → AwaitLatestState → Saving → Fading → AwaitPlaybackDeadline → Committing
             └ 失敗／取消 → Idle（更新取消は別barrierへ）
```

request を受けた main は設定 edit を停止し、直前までの最新 control generation を公開する。Running なら `StatusMailbox.ack_revision` がその control を反映するまで非RTで待ち、audio が同じ時点の `resume_target` を返す。Frozen／無出力なら worker が lease 所有下で最新 control を適用して checkpoint と保存用 target を確定する。callback が来るという理由だけで待ち続ける設計にせず、停止状態へ合流する。安定した最新窓 geometry と endpoints を組み合わせ、SaveWorker へ最新 generation を flush する。

保存成功前に fade・App::quit・Sparkle Install reply を出さない。失敗ならその終了要求を cancel、edit を再許可、音量／mute／shutdown gain を変えず failure 表示。古い AttemptId の結果は新しい intent に適用せず、所有された temp／blocks／worker 結果を適切な thread で回収する。

保存成功後だけ shutdown gain の有限 ramp（初回実装100ms）を開始する。最終 fade sample の **予定 playback deadline** を返し、main／worker はその Mach deadline まで stream を保持する。最後に生成した時刻だけで pause して fade の後半を切らない。deadline 経過→worker freeze／engine take／stream 停止→通常なら App::quit。sleep／出力なしは lease の凍結 ack が根拠となり fade 待ちなし。deadline 中の出力消失も凍結 ack へ合流し、聞こえない final callback を永久に待たない。

### GPUI delegateを保ったOS終了gate

確認済みの固定 GPUI の existing application delegate に不足する `applicationShouldTerminate:` selector を追加する小さな native bridge を使う。既存 delegate を replace／swizzle しない。起動時に selector 不在を実際に確認し、`class_addMethod` で **不足 selector だけ**追加する。既存実装がある／追加失敗／型 encoding 不一致なら `TerminationIntegrationFailure` とし、誤って通常終了を許可しない。[GPUI macOS host](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/crates/gpui_macos/src/platform.rs)、[class_addMethod](https://developer.apple.com/documentation/objectivec/class_addmethod(_:_:_:_:))

```objc
// 計画するnative bridgeの関数形。AppKit selector は実API。
static NSApplicationTerminateReply ambien_applicationShouldTerminate(
    id self, SEL command, NSApplication *sender);
// Coordinator が未commitなら NSTerminateLater、取消中なら NSTerminateCancel。
// 正当なactive AttemptIdのcommit許可後だけ終了を許可する。
```

native 同期 callback は App へ再入せず、token を main の defer 経路へ渡す。OS の pending request に対し、完了時は同じ token で `replyToApplicationShouldTerminate:YES`、保存失敗なら NO。通常 App action は最初に Coordinator、App::quit は commit 後だけ。OS shutdown／Sparkle Apple event と内部 Quit の重複は同一 active attempt に合流し、複数 save／fade／reply を作らない。[AppKit termination](https://developer.apple.com/documentation/appkit/nsapplicationdelegate/applicationshouldterminate(_:))、[reply](https://developer.apple.com/documentation/appkit/nsapplication/reply(toapplicationshouldterminate:))

### 現存の終了経路（P12）

[`src/lifecycle.rs`](../src/lifecycle.rs) の `TerminationCoordinator` は純粋な状態機械で、`AwaitLatestState → Saving(revision) → Fading → Committing` を持つ。Quit・⌘Q・メニューと OS の終了要求は同じ `request()` に合流し、既に attempt があれば `Joined` になる。保存結果は最終 snapshot の revision と完全に一致したものだけを採り、古い revision の成功は無視する。失敗は attempt を取消して edit・autosave・音を戻し、取消済みや古い `AttemptId` の結果は新しい attempt に作用しない。`NormalQuit` だけを実装し、`UpdateInstall` は P15 以降で追加する。

`AppModel` は attempt の間 edit と autosave を止め、10ms の poll Task で進める。音声側が最新 control を ack する（epoch 一致かつ `ack_revision >= settings_revision`、source が無ければ待たず、最長500ms）のを待って最終 snapshot を submit し、その receipt を `save_updated` が coordinator へ渡す。成功後だけ `ControlMailbox::request_shutdown` で100msの独立した shutdown gain（`render.rs` の4つ目の `OutputRamp`）を開始する。callback は最終 sample の予定 playback 時刻（`presentation_time(playback, frame_offset + 1, 0 遅延)`）を同じ mailbox へ publish し、main は `MachClock::now()` がそれに達するまで stream を保持する。出力が Running でない間（sleep・機器喪失・失敗）は凍結済みで待たず、callback が deadline を報告しない場合は500msで打ち切る。commit は App の borrow の外で行い、保存済みの終了では `on_app_quit` の再保存を省く。

native gate は計画の `native/*.m`・`build.rs` を使わず、`objc2::ffi` の `class_getInstanceMethod` で不足を確認し `class_addMethod` で `GPUIApplicationDelegate` へ `applicationShouldTerminate:`（`Q@:@`）だけを追加する純 Rust 実装である（Cargo.toml／Cargo.lock の変更なし）。既存実装の置換・wrap・swizzle はせず、実装が既にある・追加失敗・二重 install は `TerminationGateError` として設定の通知に表示する。この場合も app 内の Quit は coordinator を通る。IMP は commit 許可前なら `NSTerminateLater` を返して `try_send` で main へ知らせるだけで、attempt の完了時に同じ経路で `replyToApplicationShouldTerminate:` を返す。取消は NO、commit は YES で、内部の `cx.quit()` は許可後にだけ `NSTerminateNow` を得る。

保存できない状態は終了を妨げない。保護された旧版・新版の状態（`ReadOnly`）と、保存 worker が無い・停止した状態（`Spawn`／`StartupChannel`／`WorkerStopped`／`WorkerPanicked`）は、何も書けないため終了を止めても何も守らず、利用者を閉じ込めて OS の logout も止める。一時ファイル作成・書込み・置換のように直して再試行できる失敗だけが終了を取り消し、失敗を表示して音を続ける。

### Sparkle標準driverの保存beforeInstall

確認済み Sparkle は [SPUUpdater](https://github.com/sparkle-project/Sparkle/blob/eef1a539a373c1f1a320624b1130fc5de7b2e100/Sparkle/SPUUpdater.h) と [SPUStandardUserDriver](https://github.com/sparkle-project/Sparkle/blob/eef1a539a373c1f1a320624b1130fc5de7b2e100/Sparkle/SPUStandardUserDriver.h) を直接組み合わせられる。`SPUStandardUpdaterController` 単体では reply の保存 gate を挟めないため採らず、標準 user driver の薄い subclass で **ready と Installing stage の再開 reply** を wrap する。確認・download・error・authorization 画面は標準のまま。main run loop は GPUI。

```objc
// 確認済みSparkle selector。AmbienStandardUserDriverは計画するsubclass。
- (void)showReadyToInstallAndRelaunch:
    (void (^)(SPUUserUpdateChoice))reply;
- (BOOL)updater:(SPUUpdater *)updater
    shouldPostponeRelaunchForUpdate:(SUAppcastItem *)item
    untilInvokingBlock:(void (^)(void))installHandler;
```

ready では original reply を copy し、super の標準 ready UI に wrapper reply を渡す。ユーザー Install を wrapper が受けたら Coordinator(UpdateInstall) を開始し、original Install を保存 receipt まで保留する。成功後に original reply(Install) を一度だけ呼ぶ。delegate の `shouldPostponeRelaunch...` では copy した installHandler を保持して YES、同じ attempt の fade playback deadline 後、main で block を一度呼ぶ。Postpone は保存失敗を cancel する場所ではない。

保存失敗は **original ready reply(Skip) を一度**呼び、この回の install／restart を cancel し音を続ける。Dismiss、delegate veto、driver dealloc、cycle finished を complete cancel と呼ばない。ready Dismiss は後の host 終了時 install を許すため、Ambien の ready wrapper は Dismiss を取消 intent として Skip へ写し、同じ agent barrier を使う。標準画面を改造せず reply の意味だけを制御する。[SPUUserDriver ready の Install／Dismiss／Skip 契約](https://github.com/sparkle-project/Sparkle/blob/eef1a539a373c1f1a320624b1130fc5de7b2e100/Sparkle/SPUUserDriver.h)

標準 `showUpdateFound...state:reply:` で Installing stage の Install が ready を迂回する再開経路も同じ wrapper で扱う。Install は同じ保存 gate、**Dismiss は original reply に Skip を渡す**取消 intent、Skip はそのまま取消 intent とし、両者に同じ cancel agent barrier を適用する。Installing の Dismiss をそのまま転送すると UI abort だけで cancelUpdate が送られないため禁止する。[実分岐](https://github.com/sparkle-project/Sparkle/blob/eef1a539a373c1f1a320624b1130fc5de7b2e100/Sparkle/SPUCoreBasedUpdateDriver.m#L306-L316)。保存 gate を通らない Install の continuation は一つも発行しない。download を開始する通常 stage の Install はそのまま標準に渡す。

Blocks は main の Adapter owner が copy／release し、one-shot の consumed flag と AttemptId で二重呼出し・古い continuation を防ぐ。driver／updater delegate は weak なので app owner が delegate・driver・updater を strong 保持する。失敗中の owner dealloc を cancel ack としない。終了／abort では active continuation を解決または取消 protocol へ移し、参照 cycle を残さない。bridge callback の context pointer は owner の登録解除まで有効、Rust panic を ObjC／C の外へ unwind させない。

### Cancelと通常Quitの競合barrier

**ソースに基づく条件付き設計、実署名版未検証**。Skip 直後の通常 Quit が agent の host 終了監視を通じて install を再開しないよう、実 progress agent の終了を観測する。

1. ユーザー update check 開始前から `NSWorkspace.runningApplications` の KVO を登録する。現在 bundle の実 Sparkle framework 内 `Updater.app` の canonical bundle／executable URL を NSBundle で求め、一致する一意 candidate を追跡する。bundle 名や Sparkle 共通 ID だけで同定しない。
2. ready より前にその **同じ NSRunningApplication instance** を strong 保持し、`terminated` の Initial＋New KVO を登録する。PID の再検索／launchDate 必須／cancel 後「見当たらない」判定を使わない。同じ host の別 instance が無く、agent が自分を対象とする前提を確認する。
3. 保存失敗または ready 取消で Skip を発行し、`CancellingAwaitAgent`。host は生存、音・通常 edit を維持し、Install／resume／別 update attempt を発行しない。この間の通常 Quit は **その要求を Cancel**、後から自動で終了しない。
4. retained agent の terminated=true を、AttemptId と identity を照合して main へ通知し `CancelledAndAgentGone`。通常 Quit を再解禁し、ユーザーの次の明示 Quit から通常保存・終了を行う。
5. URL が取れない、candidate0／複数、ready 前に捕捉できない、host 複数、KVO 不能、agent が終了しない場合は **技術的未解決**。sessionInProgress=false／cycle finished／固定 sleep を substitute ack とせず、永久 Quit 禁止を完成仕様としない。P16 の統合 gate 未達として公開しない。

確認済み：[progress agent 起動と URL](https://github.com/sparkle-project/Sparkle/blob/eef1a539a373c1f1a320624b1130fc5de7b2e100/InstallerLauncher/SUInstallerLauncher.m#L478-L545)、[cancel 送信と早い abort](https://github.com/sparkle-project/Sparkle/blob/eef1a539a373c1f1a320624b1130fc5de7b2e100/Sparkle/SPUInstallerDriver.m#L562-L571)、[installer cleanup](https://github.com/sparkle-project/Sparkle/blob/eef1a539a373c1f1a320624b1130fc5de7b2e100/Autoupdate/AppInstaller.m#L832-L862)、[agent の host KVO](https://github.com/sparkle-project/Sparkle/blob/eef1a539a373c1f1a320624b1130fc5de7b2e100/Sparkle/InstallerProgress/InstallerProgressAppController.m#L315-L348)、[複数 host の制約](https://github.com/sparkle-project/Sparkle/blob/eef1a539a373c1f1a320624b1130fc5de7b2e100/Sparkle/InstallerProgress/InstallerProgressAppController.m#L291-L304)。公開 [runningApplications](https://developer.apple.com/documentation/appkit/nsworkspace/runningapplications) と [isTerminated](https://developer.apple.com/documentation/appkit/nsrunningapplication/isterminated) を使う。LSUIElement の agent には [didTerminate 通知](https://developer.apple.com/documentation/appkit/nsworkspace/didterminateapplicationnotification) が出ないため、それを ack にしない。

この barrier は installer 全 cleanup 完了の証明ではない。正しい host が生存し、Install を出しておらず、正しい agent が死亡したという条件下で、以後の通常 Quit→agent の host completion→install の経路が閉じるという推論である。private IPC、kill、launchd job 削除はしない。実同梱署名 helper／隔離した旧新版で、保存失敗→取消→連打 Quit→agent gone→再明示 Quit→旧 bundle が置換されないところまで確認して初めて成立とする。

### 手動更新policy

`SPUUpdater` を構築後、`startUpdater:` 前に automaticallyChecksForUpdates=false、automaticallyDownloadsUpdates=false、sendsSystemProfile=false を setters で適用する。既存 defaults の true を放置しない。Info.plist に SUEnableAutomaticChecks=false、SUAutomaticallyUpdate=false、SUAllowsAutomaticUpdates=false、profile false。permission prompt delegate は NO、`mayPerformUpdateCheck:error:` は UserInitiated だけ YES。menu は `canCheckForUpdates` と Coordinator／barrier 状態で validate し、`checkForUpdates` のみ。background／information check、独自 telemetry は呼ばない。[SPUUpdaterDelegate](https://github.com/sparkle-project/Sparkle/blob/eef1a539a373c1f1a320624b1130fc5de7b2e100/Sparkle/SPUUpdaterDelegate.h)

実 feed／public key／bundle ID が無い場合は `UpdateConfigurationMissing` として未準備を示し、仮 URL・仮鍵で start 成功としない。初回確認・download・検証の failure は Sparkle 標準画面、保存・終了調停 failure は app の既存状態表示にも反映する。チェック中／download中の音は継続、置換 failure で万能 rollback を保証しない。

## 配布の前提と成果物

[配布前提](../SPEC.md#配布前提) は実 credentials／権限／下限OS環境の gate であり、本書で確保済みにしない。現時点で有効な Developer ID が準備済みとは扱わず、契約・公証認証・Ed25519 鍵・実 URL／公開権限を別々に確認する。文書管理用 private repository は配布用 public Releases／Pages の代用品ではない。秘密鍵・認証は repo／Issue／docs に記録しない。

実装時の bundle は arm64、最低 OS、CFBundleIdentifier、増加する CFBundleVersion と表示用 short version、NSHighResolutionCapable、SUFeedURL、SUPublicEDKey、上記 policy、`Contents/Frameworks/Sparkle.framework`、`@executable_path/../Frameworks` rpath を実配布構成で設定する。URL／ID／key は承認された実値のみ。native bridge は標準 framework headers へリンクし、GPUI が NSApplication を所有する。

配布作業は内包 helper／XPC→framework→app の順で Developer ID／Hardened Runtime／timestamp を付け、必要な entitlement をその実内包物に限定する。`codesign --deep` で一括署名して正しいとしない。app を含む DMG を公証・staple し、その **同じ最終 DMG bytes** を Sparkle Ed25519 署名して、full update の appcast を `generate_appcast --maximum-deltas 0` で作る。appcast は増加 version／表示 version／minimum OS／実 DMG URL／length／Ed25519 signature／実 release notes URL を対応させる。別の未公証 DMG を更新だけに使わない。[Sparkle 導入](https://sparkle-project.org/documentation/)、[publishing](https://sparkle-project.org/documentation/publishing/)、[Apple 公証](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)

HTTPS Pages と Releases redirect、公開権限、実初回取得、隔離2versionの手動更新・保存復元・署名不一致・download／replace failure は別の観測。signed feed追加、delta、beta、sandbox、新 telemetry は範囲外。鍵・認証が無いのは準備 gate、toolchain／ABI／framework link が成立しないのは技術 gate、署名／公証／HTTP failure は操作の失敗であり区別する。許可が揃う前に外部準備を実行しない。

## SPEC責務の対応表

行は要件の複製ではなく、正本の全見出しから設計と作業・将来証拠への案内。P の手順・依存・進行は [実装計画](implementation-plan.md)、観測記録の形式は [検証手順](verification.md) に置く。現在の証拠は固定ソース調査だけで、下表の実アプリ証拠は未取得。

| SPEC 見出し | 本書の設計節 | 対応作業／必要な証拠 |
|---|---|---|
| [文書の状態](../SPEC.md#文書の状態) | 文書の位置づけ | 実装と文書承認を分離、計画全体の開始条件 |
| [成功条件](../SPEC.md#成功条件) | 合成・窓・時計・配布、未検証gate | P13・P17・P18／ユーザー試聴、実同期、下限OS signed DMG |
| [対象にしないもの](../SPEC.md#対象にしないもの) | Module 分離、DSP、固定窓、手動更新policy | 全作業／除外を増やさず対象外を実装しない review |
| [体験](../SPEC.md#体験) | 操作のcoherency、世界と移動 | P03・P07／UI 操作・virtual time |
| [切り替えと途中操作](../SPEC.md#切り替えと途中操作) | 多声slerp、Voice 準備 | P04・P06／最終手動規則と二声実選択、第三声・連打・端点・実音と無確保経路 |
| [窓](../SPEC.md#窓) | 固定GPUI窓、scene paint | P01・P08／ガラス、close／reopen、可視 Task |
| [固定サイズと内部描画](../SPEC.md#固定サイズと内部描画) | 固定GPUI、AppKit fit、最新 bounds | P01・P11／両窓固定・scroll・ゼロ領域・world 不変 |
| [高DPIと描画負荷](../SPEC.md#高dpiと描画負荷) | 高DPIと確保コスト、視覚容量 | P11・P18／対応倍率の物理寸法・memory・WindowServer |
| [基盤のサイズ制約と既知のコスト](../SPEC.md#基盤のサイズ制約と既知のコスト) | raw制約、open前fit、確保コスト | P01・P11・P18／desktop tiling固定・GPUI再確保の観測 |
| [復元](../SPEC.md#復元) | AppKit座標、保存 | P09・P11／UUID喪失・画面外・過大保存の補正 |
| [メニュー](../SPEC.md#メニュー) | AppModel／Reducer、終了、policy | P01・P03・P04・P07・P12・P15／menu と設定の一貫性 |
| [音と絵](../SPEC.md#音と絵) | 合成、world、時計 | P02・P04・P05／実合成、独立成分、同一event |
| [視覚イベント](../SPEC.md#視覚イベント) | VisualRing、full、hidden復帰 | P02・P08／音と視覚fullの独立、mute中・再表示 |
| [再生予定時刻](../SPEC.md#再生予定時刻) | 三つの時計 | P08・P10・P18／epoch、DSP metadata、実音光差 |
| [シチュエーション](../SPEC.md#シチュエーション) | Registry／Voice、予定配置 | P02・P04・P05／module＋登録、名前分岐なし |
| [初期のシチュエーションとノブ](../SPEC.md#初期のシチュエーションとノブ) | 合成とノブ曲線、音質確定 | P04・P05・P13／端点、試聴してinitialを確定 |
| [設定窓](../SPEC.md#設定窓) | controlled Ely、dominant | P01・P03・P04・P07／実フォーム、閲覧はintent無し |
| [ボリューム設定](../SPEC.md#ボリューム設定) | Reducer、独立出力gain、保存 | P03・P09／mute独立・復元・クリックなし |
| [時間設定](../SPEC.md#時間設定) | Reducer、Movement、保存正規化 | P07・P09／相互追従、次回適用、同値 |
| [消音・スリープ・出力停止](../SPEC.md#消音スリープ出力停止) | EngineLease、復旧、世界時計 | P10／sleep・機器切替・残り秒・rate再準備 |
| [アクセシビリティ](../SPEC.md#アクセシビリティ) | appearance、native通知 | P11／起動・変更・3 motion反映・opaque例外 |
| [保存](../SPEC.md#保存) | LoadOutcome、unknown、atomic、flush | P09・P12／isolated real I/O、破損・ReadOnly・最新receipt |
| [配布と更新](../SPEC.md#配布と更新) | 配布の前提と成果物 | P14・P17／実識別子・署名・公証・配信 |
| [アプリ内更新](../SPEC.md#アプリ内更新) | 標準driver、保存beforeInstall、cancel barrier | P15・P16・P17／実標準UI・保存失敗・agent gone・2version |
| [配布前提](../SPEC.md#配布前提) | credentials と技術gateの分離 | P14／実権限・下限OS・公開鍵、準備は今回実行無し |
| [構成と Interface](../SPEC.md#構成と-interface) | 予定配置、全計画 Interface | 全作業／責務・所有・失敗の review |
| [依存の選定](../SPEC.md#依存の選定) | DSP／CPAL／GPUI実Interface、根拠 | P01・P02・P15／実lock・link・採用経路 |
| [スレッドと資源](../SPEC.md#スレッドと資源) | coherency、RT、EngineLease | P02・P04・P06・P08・P10・P18／所有競合・alloc/drop/wait/I/O |
| [検証条件](../SPEC.md#検証条件) | 対応表、未検証gate | P18／全要件の実証拠と残る制約 |
| [音と操作](../SPEC.md#音と操作) | 合成、slerp、coherency | P13・P18／各声試聴・最大実経路 |
| [窓・高DPI・同期](../SPEC.md#窓高dpi同期) | 窓、時刻、容量、復旧 | P11・P18／対応画面・資源・音光・停止復帰 |
| [保存と配布](../SPEC.md#保存と配布) | 保存、lifecycle、配布 | P09・P12・P16・P17・P18／実fail・署名版更新・復元 |
| [調査根拠](../SPEC.md#調査根拠) | 各節のprimary URL、根拠 | 固定一次ソースと実測を混同しない |
| [仕様の管理と次に進む条件](../SPEC.md#仕様の管理と次に進む条件) | 文書の位置づけ、権限 | [開発手順](development.md)／別の実装許可・Issue化判断 |

## 根拠と未検証gate

### ソース確認と設計の違い

本文の primary URL は SPEC の固定改訂を指す。GPUI／Ely は宣言と Implementation の読取り、CPAL／FunDSP は実 config・timestamp・tick・所有の確認、Sparkle は実 headers と cancel／agent の因果経路を確認したもの。Library 名だけで no-alloc、所有回収、標準UI integration、安全な更新取消を保証しない。

native 通知の補助依存は `async-channel=2.5.0`（native→main だけ）と `block2=0.6.2` を候補とするが、未使用の P01 では直接依存に追加しない。現存 P01 の借用 Adapter は `objc2=0.6.4`、`objc2-app-kit`／`objc2-foundation=0.3.2`、`raw-window-handle=0.6.2` を使う。gpui は default-features=false＋stacker、gpui_platform は false＋font-kit、Ely は false＋runtime_shaders。rev の正本は [SPEC](../SPEC.md#依存の選定)、実使用 features と解決版は [`Cargo.toml`](../Cargo.toml)／[`Cargo.lock`](../Cargo.lock)。現在の screen geometry には NSGraphics、NSNumber には Foundation の NSValue feature が必要である。通知・アクセシビリティ・Sparkle の features はそれぞれを実装する時に限定して追加する。

[Zed manifest](https://github.com/zed-industries/zed/blob/1a28cff4b409169bac058bca40dfbfeb7621d19b/Cargo.toml#L704-L755)、[Ely manifest](https://github.com/ZacharyZhang-NY/Ely-GPUI-Components/blob/7e39a9383dc2fd7404751d02cb166e95861ea3d6/Cargo.toml#L1-L23) と実使用 API に合わせて P01 の lockfile を作成し、現行開発環境で GPUI/Ely/native の build と実窓の経路を確認した。両 project lock の完全一致や、未実装の CPAL/FunDSP/Sparkle の ABI・下限 OS の成立は主張しない。依存更新を勝手な互換 shim で吸収しない。

### 完了を止める具体的gate

| gate | 現在の状態と、未成立時の扱い |
|---|---|
| 実 GPUI／Ely／native／CPAL／FunDSP の build と ABI | 現行環境で固定依存を build し、実 GPUI 窓・標準 CPAL stream・FunDSP tick 経路を実行した。外部 native E2E と雨のユーザー確認を得た。下限 OS・署名版や他機器へ一般化しない |
| 固定窓と desktop tiling | 現行一次画面で実 NSWindow の外枠・制約・flags を観測し、2x主画面の全OSタイリングにユーザーの成功回答を得た。native quit の識別と最終候補の状態は検証記録を参照。別1x画面・倍率切替・負荷へ一般化せず、位置固定や GPUI patch へ無断変更しない |
| 高DPI資源と透明性 | 対応画面・倍率で未実測。固定粒／30fpsだけでGPU一定・確保成功を主張しない |
| RTとEngineLease | 競合・旧 epoch・閉鎖 gate・worker 回収と Rust callback／満杯 pool の scoped heap を試験した。native backend 全失敗経路・最悪 timing・最大三声・依存 monitor の残留は独立に確認する |
| Mach／CPAL／物理同期 | Mach 変換・予定時刻 fixture と実標準 stream を確認した。latency 推定精度・Bluetooth・WindowServer・SPEC の物理同期目標は未測定 |
| 音質・initial値 | 雨の試聴候補の smoke にユーザー確認を得た。三環境の心地よさ・初期値は未確定で、P13 でユーザー試聴と SPEC／Module を同時更新する |
| 保存・終了 | 実I/O、latest ack、directory sync failure、fadeの予定deadline、OS termination追加selectorは未検証 |
| Sparklecancel／agent同定 | 条件付きソース推論。実agent事前一意同定・host一意性・terminated KVO・取消後通常QuitがP16で成立しなければ技術未解決、公開不可 |
| 署名・鍵・配信権限 | SPECの前提が未準備。仮値で代用せずP14、人の承認と実credentialsが必要 |
| 下限OSとsignedDMG／2version更新 | 未観測。現在OS・文書検査・mock helperを代用品にしない |

固定二窓の build・幾何試験・実 GPUI/NSWindow の限定観測と2x主画面でのユーザー回答、shipping source の外部 native E2E による menu／両窓 foreground ⌘Q の PID 消失・exit0 の履歴を保持する。P02 では標準 stream の build/play と sample 世界時計、Rust callback／満杯 pool の scoped heap 試験、出力 error bitset への注入、実 GPUI host quit の worker join を観測した。ロック中の失敗履歴と、その後の外部 native E2E 成功・雨のユーザー確認は別の観測として記録する。三環境の音質・初期値確定、物理同期、native backend の全 RT 失敗経路・最悪 timing、別1x画面・倍率切替・負荷、保存・更新・署名・公証・公開は未確認である。証拠は [検証記録](verification.md) で区別し、[実装計画](implementation-plan.md) と [検証手順](verification.md) に従う。文書検査や build から未観測の合格を作らない。
