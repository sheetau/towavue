# towavue 既知の不足と草案との差

この文書はM7 checkpointとH1での改善を、これから人が触って改善するための基準として整理する。`concepts/concept.txt`は発想の参照元であり、ここに載っている項目も採用決定ではない。優先順位は実際の試用結果、再現性、利用頻度、architecture riskで更新する。

## 1. 試用前に知るべき制約

### 操作とpreviewの不一致

- 動画はH1でbar・timelineを除いた領域へsample aspect ratio込みでaspect-fitするよう修正した。回転metadataによるportrait orientationの自動適用は未検証。
- 動画・音声のvolume・mute・rateはH1でlive playbackにも反映する。rate変更は現在位置からpipelineを再構築するため短い再primingを伴い、音声を無途切れで連続変速する方式ではない。
- 動画のcrop、rotate、flipもedit historyとexportには入るが、再生映像へ最終形をlive previewしない。crop selection overlayだけが見える。
- trimは`I` / `O`で現在位置を端点として記録する方式で、timeline上のrange handleや選択範囲はない。

UI上のcommand名は操作が即時反映される印象を与えるため、live playbackへの適用または表示上の区別が、最初のUX改善候補である。

### UI threadを止める処理

- 画像decodeとreading modeの複数画像loadはH1で単一background workerへ移した。要求・結果は最新1件だけを保持し、古い結果は表示しない。texture化とGPU uploadはUI側に残り、大きい画像の表示切替が完全に無停止とは限らない。
- Shell snapshotはH1で非同期化した。最新1件だけを待機・保持し、古い結果をgenerationで拒否する。実行中のShell APIは強制中断しないため、次の取得がすぐ完了する保証はない。path正規化、file metadata、watcher作成、media probeにはUI側の同期処理が残る。
- native Open file/folder/Save AsはH1で専用STAへ移した。本体入力はmodal制限するが描画・再生を続け、Cancel後は入力とdirty guardを復元する。同じ30秒H.264/AACのOpen Folder→Cancel試験は、修正前の808/900 dropsから修正後0/900 dropsになった。基準機の単発試験であり、複数DPI/monitorや全codecでの保証ではない。
- Save/Save AsはH1でbackground化済み。書き出した時間とcancelを表示し、完了までは一時outputだけを変更する。同時jobは1件でqueueはない。通常export中も再生・tab切替・追加編集ができるが、対象tabのclose・移動とprocess終了はjobの完了またはcancelを待つ。
- waveform、duration、hover thumbnailはworker化済みだが、mediaを切り替えた後も開始済みFFmpeg process自体はcancelせず、返った古い結果を捨てる方式である。
- filmstripは可視項目だけを単一workerで順次読み込み、待機要求・結果・UI textureを最大64項目、各RGBAを240×160に制限する。開始済みprocessの強制cancelやdecoder作業領域の制限ではなく、遅い素材は後続previewを待たせる。失敗項目はNo previewと詳細tooltipで表示する。
- animated imageはframe列を先に保持する。H1で1画像/reading要求のRGBA保持量を合計512 MiBに制限したが、decoder作業領域・GPU texture・切替前の旧画像は別である。超過時はerrorとし、部分animationや低解像度へは自動縮退しない。

### 開発版としての不足

- H1でRedrawRequestedの自己再予約と静止gridの連続描画を除いた。基準機の5秒間CPU時間は静止画・Welcomeで約5.9秒から計測分解能以下へ、音声再生で約5.9秒から0.47秒へ減少した。debug buildの単発process計測であり、GPU消費電力・release性能・長時間負荷を保証する値ではない。

- installer、uninstaller、portable package、automatic update、file association、Explorer context menuはない。
- settings画面、recent files、session/tab復元、window位置・sizeの保存はない。
- Explorerからのfile/folder dropはH1で実装した。複数fileは既存Open契約で開き、folderはShell順の先頭mediaを開く。folder要求は最新1件で、複数folderを一括展開するimport queueではない。virtual file、URL、app間tab結合は対象外。
- export errorは確認するまで残る詳細modal、画像load errorは画像領域（readingでは該当page）に表示する。他のerrorは主に短時間のstatus messageとterminal diagnosticで、履歴、copy、詳細表示はない。
- end-to-end UI test、visual regression、accessibility検査、複数DPI/monitorの自動matrixはない。現在のUI完了判定には実window操作が必要である。

## 2. UI草案との対応

### Shell、navigation、tab

| 草案 | 現状 |
|---|---|
| Explorerの実際のSort By順を全navigationで使う | 実装済み。live Explorer view、保存済みShell view、明示fallbackの順で取得 |
| 全media filmstripと同種/全種移動 | H1で中央のthumbnail列、音声waveform・duration、現在項目の枠・名前、wheel横scrollを実装。Tab / Shift+Tabのfocus競合も修正 |
| 画像/動画はfile tab、音声はfolder playlist tab | 実装済み |
| Filmstrip middle-clickで新規tab | 実装済み |
| Explorerからfile/folderをdropして開く | H1で実装。hover案内、複数file、dirty編集保持、modal中の拒否を確認 |
| Tabをwindow外へdrag | 別process起動として実装。dirty editの移送はせずguardする |
| 別windowへtabをdragして結合 | 未実装。process間protocolもない |
| Filmstrip itemをwindow外へdrag | 未実装 |
| Tabの並べ替え、drop indicator、等分幅 | 等分幅（72～160 logical px）と横scroll・名前省略をH1で実装。並べ替えとdrop indicatorは未実装 |
| Welcomeのrecent files | Open file/folderだけ実装。recent listは未実装 |
| Explorerから開く/新規window context menu | OS登録・配布処理が未実装 |

### Menu、command、status

| 草案 | 現状 |
|---|---|
| 全機能を一つのlogo menuへ集約 | H1でFile / Edit / Viewへ分類し、関連項目の区切り、現在shortcutの右揃え、window内scrollを実装。全registry commandの重複・欠落をtestする |
| File/Edit/Viewの3方向drag gestureとSVG logo | logo形状をvector描画。方向gestureは未実装 |
| 黒基調のcompactなwindow shell | 32px title/tab barと30px status、window操作、右寄せ情報をH1で実装。複数DPI/monitorのmatrixは未検証 |
| Command palette | titleの部分一致検索、上下選択、有効候補の巡回、Enter実行、Escape閉じを実装。ranking、categoryはない。物理keyboard・IMEのmatrixは未検証 |
| Custom shortcutとprefix key | text設定として実装。GUI editor、競合表示、recording UIはない |
| Media別4×4 grid | key/clickとtext設定を実装。配置編集UI、drag配置、詳細animationはない |
| Statusへpath、位置、zoom、解像度、size、modified等 | filename、parent path、folder内位置、size、画像解像度・zoom、編集値などを部分実装。modified日時、詳細codec/stream情報はない |
| 常時1px seek bar、hover時展開 | H1でstatus上端に実装。動画・音声はduration取得後、timeline非表示時に使える。drag終了時に一回だけSeekする |
| Fullscreen時はUIを隠す | H1でF11/View menuのborderless fullscreenとbar/timeline/seek非表示を実装。画像・動画・readingでは2秒idleでcursorを隠し、入力・overlay・modal時は表示。edge-hover controls、double-clickは未実装 |

### Timeline

| 草案 | 現状 |
|---|---|
| 動画/音声のwaveform timeline | 96px固定panel、waveform、CTI、click/drag seekを実装。音声はdefault表示 |
| 動画hover thumbnailと低負荷scrub | 20区間のcached thumbnail tooltipを実装。thumbnailを本画面へ出すscrub previewは未実装 |
| 画像のfolder位置seekとthumbnail | Shell snapshotの画像順seekと位置・filename tooltipを実装。移動はdirty guardを通す。thumbnailは未実装 |
| Range selection、範囲内再生、delete/cut | 未実装 |
| Rubber bandでtrack volume | 未実装 |
| Range伸縮でrate編集 | 未実装 |
| 上端dragでtimeline高さ変更 | 未実装。高さは固定 |
| Trim handleと編集mode | 未実装。現在位置を`I` / `O`で記録するだけ |

### 画像

| 草案 | 現状 |
|---|---|
| Static/animated imageとAVIF | 実装済み。ただし互換性とmemory上限は限定的 |
| Cursor基点zoom、pan、actual、fit | 実装済み |
| Selection作成、正方形、辺resize、ratio保持 | 基本実装済み |
| Selectionの移動 | 未実装。内部clickはcrop previewになる |
| 指定aspect ratio | 未実装 |
| Crop、90度rotate、flip、undo/redo、export | 実装済み |
| 自由回転 | 未実装 |
| Clipboard copy | 未実装 |
| Resize/resampleとinterpolation選択 | 未実装 |
| Fullscreen | H1で画像/readingの全領域表示、Escape復帰と最大化状態の保持を実装。複数DPI/monitor matrixは未検証 |
| Home/End、Page、Backspace、A/D、数指定jump | 未実装。現在は共通navigation shortcutのみ |
| Reading mode 2～10枚、縦横、反転 | 基本実装済み |
| Reading表示数のbutton drag、offset調整、設定保持 | 未実装 |
| Marker、text、色調補正 | 草案でも後回しまたは対象外。現在も未実装 |

### 動画・音声

| 草案 | 現状 |
|---|---|
| D3D11VA優先、software fallback | 実装済み。codec/GPU/driverごとの成功は実機依存 |
| Audio master、seek、pause、EOF、late frame drop | 実装済み。基準fixtureで測定済み |
| WASAPI Sharedとdefault endpoint復旧 | 実装済み。hardware/driverの広いmatrixは未検証 |
| Wheel volume、hold中2倍速 | 未実装 |
| J/K/L、frame step | 未実装。`,` / `.`はframe stepではなくrate変更 |
| Live playback volume/rate | H1で実装。編集値を再生・exportで共有し、rateは0.25～4倍のピッチ維持 |
| Track selection、delete、cut、range playback | 未実装 |
| Repeat、shuffle | 未実装 |
| Video zoom、fullscreen、resize/resample | fullscreenはH1でhardware/software共通のaspect-fitと復帰を確認。zoomとresize/resampleは未実装 |
| Video crop/rotate/flipのlive preview | 未実装。selectionとexportはある |
| Audio-only export、normalize、stereo/mono変換 | 未実装 |
| Track/codec/subtitle selection | 未実装 |
| Exclusive WASAPI | 意図的にdefaultへ採用しない。将来optionを検討可能 |

### Export、HDR、cache

| 項目 | 現状 |
|---|---|
| Source非破壊export | 実装済み。同一source pathへの出力を拒否 |
| Metadata保持 | FFmpegの`-map_metadata 0`を使用。formatを越えた完全保持や個別編集UIは保証していない |
| Hardware encode | H.264 Media Foundationを強制要求し、失敗時software fallback。基準adapterではhardware成功を確認できていない |
| NVENC/AMF/QSV encode選択 | 未実装 |
| Codec、quality、bitrate、containerの選択UI | 未実装。拡張子別の固定codec |
| Export progress/cancel/失敗後のpartial target処理 | 書き出し済み時間とcancelを表示。成功後だけ一時outputをtargetへ置換し、失敗・cancelで既存targetと編集を保持。残り時間予測とqueueはない |
| PQ/HLGのHDR→SDR | D3D11 Video Processorが変換を保証したhardware frameだけ許可。基準adapterは非対応error |
| HDR displayへの10-bit pass-through | 未実装、対応を宣言しない |
| Waveform/thumbnail disk cache | 実装済み。64 MiB固定で管理UIや手動clear commandはない |

## 3. Codebase上の改善余地

- `crates/towavue-app/src/main.rs`へevent loop、UI描画、input、tab orchestration、preview worker連携、export flowが約2,800行で集中している。UI反復が始まると競合と回帰範囲が広がるため、変更対象が固まった単位から挙動を変えずに分離する価値がある。
- AppのUI logicに対するtestは純粋helper中心で、pointer gesture、focus、modal、tab drag、timelineを直接検証していない。
- 対応拡張子、file dialog filter、実decoder能力、export codec選択の関係を一つのcapability modelへ統一していない。拡張子を増やすだけでは対応完了にならない。
- Loading、empty、error、unsupported capabilityのstate表現が各所のstatus textへ分散している。UX改善時には表示だけでなくstate transitionをcoreでtest可能にする余地がある。
- filmstrip以外のpreview workerはtask単位にthreadを起動する単純構成で、優先度、同時数、cancel、重複排除を持たない。filmstripは可視集合の最新要求を単一workerで処理する。
- Exportはtargetへ直接`-y`で書き込む。sourceは保護されるが、失敗・cancelを含むtarget側のatomicityは定義されていない。

これらは一括refactorの指示ではない。実際のUX課題を直す際に、必要な範囲だけ同時に改善する。

## 4. 現時点で採用しないもの

- CUDA/QSV等をD3D11VAの追加decode fallbackにはしない。現在のcontractはD3D11VAからsoftwareへの一段fallbackである。
- WASAPI Exclusiveをdefaultにしない。Shared modeで他applicationとの共存を守る。
- Explorer registry Bagsを解析しない。公開Shell view APIから実際の順序を取得する。
- HDR metadataを無視した表示や、未検証の10-bit pass-throughを「HDR対応」と呼ばない。
- Plugin system、database、telemetry、network accessをUI改善のついでに追加しない。
- 草案にあるmarker、text、色調補正は、基礎的な閲覧・編集UXが安定するまでscopeへ入れない。

## 5. 次に検証する順序

最初の人手評価では、次の順序が費用対効果とriskの釣り合いがよい。

1. Open、folder navigation、Explorer順、play/pause/seek、画像zoom/panという日常flowの摩擦を記録する。
2. Trim/cropなど「見える結果」と「export結果」の不一致を解消する。Volume/rateはH1でlive反映を検証済み。
3. 大きい画像のtexture化・uploadやShell取得の応答を追加測定する。画像decodeのworker化・保持量上限と、長いexportのworker・進捗・cancel・既存target保護はH1で検証済み。
4. Timeline、tab、filmstrip、menuを実際の利用頻度に基づいて磨く。
5. DPI、keyboard-only、長いfile名、error/loading state、accessibilityを横断確認する。
6. その後にrecent/session復元、file association、packagingを決める。

この順序は固定milestoneではない。試用で再現性の高いdata loss、crash、再生破綻が見つかった場合は、それを最優先する。
