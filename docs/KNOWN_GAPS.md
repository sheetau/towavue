# towavue 既知の不足と草案との差

この文書はM7 checkpointとH1での改善を、これから人が触って改善するための基準として整理する。`concepts/concept.txt`は発想の参照元であり、ここに載っている項目も採用決定ではない。優先順位は実際の試用結果、再現性、利用頻度、architecture riskで更新する。

## 1. 試用前に知るべき制約

### 操作とpreviewの不一致

- 動画・音声のvolume、mute、rateは非破壊edit historyとexport filterには入るが、現在再生中の音量・速度を変えない。
- 動画のcrop、rotate、flipもedit historyとexportには入るが、再生映像へ最終形をlive previewしない。crop selection overlayだけが見える。
- trimは`I` / `O`で現在位置を端点として記録する方式で、timeline上のrange handleや選択範囲はない。

UI上のcommand名は操作が即時反映される印象を与えるため、live playbackへの適用または表示上の区別が、最初のUX改善候補である。

### UI threadを止める処理

- 画像decodeとreading modeの複数画像loadは同期処理であり、大きい画像、animation、多数pageでwindowが応答しにくくなり得る。
- Save/Save AsはH1でbackground化済み。書き出した時間とcancelを表示し、完了までは一時outputだけを変更する。同時jobは1件でqueueはない。通常export中も再生・tab切替・追加編集ができるが、対象tabのclose・移動とprocess終了はjobの完了またはcancelを待つ。
- waveform、duration、hover thumbnailはworker化済みだが、mediaを切り替えた後も開始済みFFmpeg process自体はcancelせず、返った古い結果を捨てる方式である。
- animated imageはframe列を先に保持するため、長い・大きいanimationのmemory上限を定義していない。

### 開発版としての不足

- installer、uninstaller、portable package、automatic update、file association、Explorer context menuはない。
- settings画面、recent files、session/tab復元、window位置・sizeの保存はない。
- Explorerからwindowへのfile drag-and-dropはない。tabのwindow外dropだけが実装されている。
- export errorは確認するまで残る詳細modalを表示する。他のerrorは主に短時間のstatus messageとterminal diagnosticで、履歴、copy、詳細表示はない。
- end-to-end UI test、visual regression、accessibility検査、複数DPI/monitorの自動matrixはない。現在のUI完了判定には実window操作が必要である。

## 2. UI草案との対応

### Shell、navigation、tab

| 草案 | 現状 |
|---|---|
| Explorerの実際のSort By順を全navigationで使う | 実装済み。live Explorer view、保存済みShell view、明示fallbackの順で取得 |
| 全media filmstripと同種/全種移動 | 基本実装済み。filmstripはfilenameのtext listでthumbnailはない |
| 画像/動画はfile tab、音声はfolder playlist tab | 実装済み |
| Filmstrip middle-clickで新規tab | 実装済み |
| Tabをwindow外へdrag | 別process起動として実装。dirty editの移送はせずguardする |
| 別windowへtabをdragして結合 | 未実装。process間protocolもない |
| Filmstrip itemをwindow外へdrag | 未実装 |
| Tabの並べ替え、drop indicator、等分幅 | 未実装。横scrollの単純なbutton列 |
| Welcomeのrecent files | Open file/folderだけ実装。recent listは未実装 |
| Explorerから開く/新規window context menu | OS登録・配布処理が未実装 |

### Menu、command、status

| 草案 | 現状 |
|---|---|
| 全機能を一つのlogo menuへ集約 | 全commandを一つの`towavue` text menuへ列挙 |
| File/Edit/Viewの3方向drag gestureとSVG logo | 未実装 |
| Command palette | titleの部分一致検索を実装。ranking、category、keyboard selectionの磨き込みはない |
| Custom shortcutとprefix key | text設定として実装。GUI editor、競合表示、recording UIはない |
| Media別4×4 grid | key/clickとtext設定を実装。配置編集UI、drag配置、詳細animationはない |
| Statusへpath、位置、zoom、解像度、size、modified等 | filename、parent path、folder内位置、size、画像解像度・zoom、編集値などを部分実装。modified日時、詳細codec/stream情報はない |
| 常時1px seek bar、hover時展開 | 未実装。timelineを閉じると動画位置をbarで直接操作できない |
| Fullscreen時はUIを隠す | fullscreen自体が未実装 |

### Timeline

| 草案 | 現状 |
|---|---|
| 動画/音声のwaveform timeline | 96px固定panel、waveform、CTI、click/drag seekを実装。音声はdefault表示 |
| 動画hover thumbnailと低負荷scrub | 20区間のcached thumbnail tooltipを実装。thumbnailを本画面へ出すscrub previewは未実装 |
| 画像のfolder位置seekとthumbnail | 未実装 |
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
| Fullscreen | 未実装 |
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
| J/K/L、frame step | 未実装。`,` / `.`はframe stepではなくexport用rate変更 |
| Live playback volume/rate | 未実装。現在はexport用edit |
| Track selection、delete、cut、range playback | 未実装 |
| Repeat、shuffle | 未実装 |
| Video zoom、fullscreen、resize/resample | 未実装 |
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
- Preview workerはtask単位にthreadを起動する単純構成で、優先度、同時数、cancel、重複排除を持たない。
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
2. Volume/rate/trim/cropなど「見える結果」と「export結果」の不一致を解消する。
3. 大きい画像の応答停止を解消する。長いexportのworker、進捗表示、cancelと既存target保護はH1で検証済み。
4. Timeline、tab、filmstrip、menuを実際の利用頻度に基づいて磨く。
5. DPI、keyboard-only、長いfile名、error/loading state、accessibilityを横断確認する。
6. その後にrecent/session復元、file association、packagingを決める。

この順序は固定milestoneではない。試用で再現性の高いdata loss、crash、再生破綻が見つかった場合は、それを最優先する。
