# towavue 既知の不足と草案との差

この文書はM7 checkpointとH1での改善を、これから人が触って改善するための基準として整理する。`concepts/concept.txt`は発想の参照元であり、ここに載っている項目も採用決定ではない。優先順位は実際の試用結果、再現性、利用頻度、architecture riskで更新する。

## 1. 試用前に知るべき制約

### 操作とpreviewの不一致

- 映像より音声が長い素材で、再生中に音声だけの区間へSeekすると黒画面になる問題を修正した。最終映像をlate-frame dropから除外して保持し、音声clockは継続する。MP4/MKV・不均等なframe間隔の末尾画像照合と通常D3D11VA表示を確認。ただし、この区間のhover thumbnail取得失敗は別経路の未解決事項。

- 同じframeで一覧と音量表示を跨いだwheelは、各event時点の位置とlayerで選別する。音声playlistのsmooth scrollも専用のegui入力状態へ分離し、音量操作の余韻が一覧へ入る問題を修正した。一覧からpointerが離れてもそのscrollは一覧だけに適用し、modal/focus喪失などでは失効する。30/120fpsの距離保持と通常windowの両方向移動を確認。他のScrollAreaを一括変更したものではなく、物理device/DPIの全入力matrixは未完了。

- 動画面と動画/音声status barの音量表示にwheel音量を追加した。raw縦入力だけを使い、playlist/timelineのscroll・修飾key・drag・modal/menu/overlayとは分離する。移動直後のwheelが古い位置へ届く問題は、固定winitがwheel座標を更新しないことを回帰で再現し、runtimeで各wheelのscreen座標を先行反映して修正した。通常windowの一覧→音量→一覧の即時移動も確認したが、物理device/DPIの全入力matrixではない。

- Mキーの消音解除が必ず100%へ戻る問題を修正した。active tabの適用済み履歴から直前の非zero音量を復元し、未適用redoや別tabの値は使わない。音量をlive/export共通の編集として扱う設計は維持する。

- 単一項目のfolder前後移動でzoomがFitへ戻り、未保存編集があると同じ画像への移動でも保存確認が出る問題を修正した。同じpathへのNavigateは共通入口でno-opとし、現在playlist項目の再clickも再生位置/pauseを保つ。別pathへのguard、明示的なOpen、新規tabは維持する。

- 複数stream素材で再生とthumbnail・filmstrip・waveformが異なる問題をH1で修正した。再生と同じFFmpeg best-streamを明示指定し、旧cacheを失効させる。続いてtrimなしの保存で別映像/音声になる問題も通常releaseで再現し、exportへ同じ選択を適用した。無編集・crop・trim・音声のみの回帰とSave As→再openを確認したが、全container/stream配置の保証や手動stream選択UIはない。

- 非zeroのcontainer開始時刻による黒画面・時刻ずれはH1でinput原点を引くdecode/Seek/exportへ揃えた。Matroskaの長さも補正し、MP4/MKV/TSの0/5秒offsetを回帰比較する。TSは実packet keyframeを確認する段階的なpreroll探索を追加し、短い/長いGOPとMPEG-2 B-frameのSeek・thumbnailを全decode基準へ照合した。通常releaseのoffset TSではhardware Seek・timeline preview・trim exportを確認。4K60 TSの45秒付近Seekは単発277.736 msで、以後899枚をdrop 0で再生した。全形式の破損header・不連続PTS・長いGOP・低速storageに対する精度やlatency保証ではない。
- 動画はH1でbar・timelineを除いた領域へsample aspect ratio込みでaspect-fitし、display matrixの90度単位回転・反転を自動適用する。8通りの向きと編集/保存後の再open、hardware/software表示を検証した。任意角度・scale・shear・射影は非対応として明示errorにする。全containerの動的metadata切替を含むmatrixは未検証。
- 動画・音声のvolume・mute・rateはH1でlive playbackにも反映する。rate変更は現在位置からpipelineを再構築するため短い再primingを伴い、音声を無途切れで連続変速する方式ではない。
- 動画のcrop、rotate、flipはH1でhardware/software両方のlive previewへ反映した。cropは整数pixel矩形をexportと共有し、画像1 pixel・動画偶数pixelに揃える。既定H.264 encoderのため動画は16×16未満を確定しない。PNGの1×1・奇数位置/寸法・回転後の再cropは画素一致を検証したが、動画の圧縮・chroma再構成や全codec/HDRの色一致を保証するものではない。
- trimは`I` / `O`またはtimelineの開始/終了gripでsource端点を指定する。H1で入力検証・timeline範囲表示と単一区間のlive再生を追加した。gripはrelease時に一回だけ編集し、Escape/focus喪失/対象切替で取消。逆転・零長は保持せず、frame単位snapはない。終端で停止し、再Playは範囲開始へ戻る。範囲外Seekはpaused source previewになり、端点を選び直せる。exportの秒丸めを整数tick/sampleへ変更し、空出力を拒否した。ミリ秒PTSのFFV1/PCMでframe選択とsource sample列を検証済み。ただし途中Seek後のlive音声は元のsub-tick sample位相を復元できず差が残る。VFR、欠落/不連続PTS、codec paddingを含む全codec一致は未完了。

UI上のcommand名は操作が即時反映される印象を与えるため、live playbackへの適用または表示上の区別が、最初のUX改善候補である。

### UI threadを止める処理

- 長いGOPのSeek中に終了すると、target到達までdecodeを待つ問題をH1で修正した。pipelineごとの取消flagを探索・demux・decoded outputで確認し、target以前のframe破棄中も停止する。1080p60・30秒GOPの通常release単発比較では終了待ち897 msから63 msへ短縮し、音声付き素材の再Seek・再生・tab closeも通過した。worker joinは維持し、進行中のFFmpeg call/OS I/Oの強制中断や、全素材の終了時間保証ではない。
- 画像decodeとreading modeの複数画像loadはH1で単一background workerへ移した。要求・結果は最新1件だけを保持し、古い結果は表示しない。texture化とGPU uploadはUI側に残り、大きい画像の表示切替が完全に無停止とは限らない。
- Shell snapshotはH1で非同期化した。最新1件だけを待機・保持し、古い結果をgenerationで拒否する。実行中のShell APIは強制中断しないため、次の取得がすぐ完了する保証はない。path正規化、file metadata、watcher作成、media probeにはUI側の同期処理が残る。
- native Open file/folder/Save AsはH1で専用STAへ移した。本体入力はmodal制限するが描画・再生を続け、Cancel後は入力とdirty guardを復元する。同じ30秒H.264/AACのOpen Folder→Cancel試験は、修正前の808/900 dropsから修正後0/900 dropsになった。基準機の単発試験であり、複数DPI/monitorや全codecでの保証ではない。
- Save/Save AsはH1でbackground化済み。書き出した時間とcancelを表示し、完了までは一時outputだけを変更する。同時jobは1件でqueueはない。通常export中も再生・tab切替・追加編集ができるが、対象tabのclose・移動とprocess終了はjobの完了またはcancelを待つ。
- waveform、duration、hover thumbnailはH1で各種類1 worker、実行中1件＋最新の待機1件へ制限した。media切替/最後のtab closeで未開始要求を捨て、owned FFmpeg/FFprobeも停止する。2時間音声のwaveform生成中に本体を閉じると旧実装では子processが残ったが、修正後の通常releaseでは終了時と500 ms後に残存なしを確認した。実行中のfilesystem I/O/native probeの強制中断や個別decoderのメモリ上限は保証しない。open番号とsession内Seek/recovery世代による旧結果の拒否も維持する。
- filmstripは可視項目だけを単一workerで順次読み込み、待機要求・結果・UI textureを最大64項目、各RGBAを240×160に制限する。表示範囲の変更・clear・closeで同じpreview取消を使うが、native probeや同一要求内の遅い素材は後続previewを待たせる。失敗項目はNo previewと詳細tooltipで表示する。
- waveformの全音声frame保持をPCMの逐次集計へ変更した。640列では和の保持量が最大5 MiB、入力bufferは64 KiB。短い音声は従来と画素一致、長い音声/急変の回帰ではbar高さの差1 pixel以内を確認した。2時間AACの単発通常release試験では子processのピークprivate bytesが1,360,474,112から41,574,400へ減り、本体との合計ピークは190,844,928 bytes。生成観測時間も3,315から2,405 msへ短縮したが、全codecやcold storageを保証する結果ではない。
- animated imageはframe列を先に保持する。H1で1画像/reading要求のRGBA保持量を合計512 MiBに制限したが、decoder作業領域・GPU texture・切替前の旧画像は別である。超過時はerrorとし、部分animationや低解像度へは自動縮退しない。

### 開発版としての不足

- WASAPI APIの`AUDCLNT_E_DEVICE_INVALIDATED`も既存endpoint再作成へ接続した。実clientのworkerへ同エラーを一度だけ与え、再生中/停止中の位置・rate・mute・回転保持と再Playを確認した。trim開始前previewの復旧失敗も修正し、位置と表示画素の保持を確認した。停止sessionの切断callback登録とgeneration付きUI wakeを追加し、folder pollなしでも入力を待たず復旧することを制御試験で確認した。物理抜き差し・default device変更・audio service停止は実行しておらず、OS由来の切断通知を含む実機matrixは未完了。

- graphics recoveryのswap chain解放順序とUI/image texture再送を修正した。再作成不能時はnativeのRetry/Cancelと終了時のExport/Discard/Cancelへ接続し、描画なしの保存失敗・再保存・終了、停止位置でのRetryを実windowで確認した。複数dirty tabの順次保存と、export中/保存完了後のnative取消も確認した。完了後の取消は保存出力を残し、自動終了・移動を止める。ただしこれは所有process内で故障状態を作る試験であり、実driver reset・adapter切替・endpoint切替のmatrixは未完了。

- H1でRedrawRequestedの自己再予約と静止gridの連続描画を除いた。基準機の5秒間CPU時間は静止画・Welcomeで約5.9秒から計測分解能以下へ、音声再生で約5.9秒から0.47秒へ減少した。debug buildの単発process計測であり、GPU消費電力・release性能・長時間負荷を保証する値ではない。

- installer、uninstaller、portable package、automatic update、file association、Explorer context menuはない。
- settings画面、recent files、session/tab復元、window位置・sizeの保存はない。
- Explorerからのfile/folder dropはH1で実装した。複数fileは既存Open契約で開き、folderはShell順の先頭mediaを開く。folder要求は最新1件で、複数folderを一括展開するimport queueではない。virtual file、URL、app間tab結合は対象外。
- export errorは確認するまで残る詳細modal、画像load errorは画像領域（readingでは該当page）、動画・音声のplayback errorはFaulted中の中央領域に表示する。壊れたMP4から正常動画をOpenし、元のerror tab、最後にWelcomeへ戻るflowを通常releaseで確認した。他のerrorは主に短時間のstatus messageとterminal diagnosticで、履歴、copy、詳細表示はない。
- end-to-end UI test、visual regression、accessibility検査、複数DPI/monitorの自動matrixはない。現在のUI完了判定には実window操作が必要である。
- 画像100%とzoomはphysical pixel基準へ修正し、100/125/150/200%の描画入力、crop preview・編集後寸法・pointer anchorを自動testした。UI rendererの二重拡大も実windowのpixel照合で修正した。ただし接続中の2画面は両方96 DPIで、異なる実DPI間の移動・切断は未検証。

## 2. UI草案との対応

### Shell、navigation、tab

| 草案 | 現状 |
|---|---|
| Explorerの実際のSort By順を全navigationで使う | 実装済み。live Explorer view、保存済みShell view、明示fallbackの順で取得 |
| 全media filmstripと同種/全種移動 | H1で中央のthumbnail列、音声waveform・duration、現在項目の枠・名前、wheel横scrollを実装。Tab / Shift+Tabのfocus競合も修正 |
| 画像/動画はfile tab、音声はfolder playlist tab | 実装済み |
| 音声playlistの番号付き一覧・現在曲・曲長 | H1で32pxの全幅行、現在曲の明るい表示、名前省略とwindow幅内の全文tooltip、可視行描画を追加。open/選曲/tab再表示/順序変更時は現在行へ最小scrollし、通常の再描画では手動scrollを保持。曲長の列は未実装 |
| Filmstrip middle-clickで新規tab | 実装済み |
| Explorerからfile/folderをdropして開く | H1で実装。hover案内、複数file、dirty編集保持、modal中の拒否を確認 |
| Tabをwindow外へdrag | 別process起動として実装。dirty editの移送はせずguardする |
| 別windowへtabをdragして結合 | 未実装。process間protocolもない |
| Filmstrip itemをwindow外へdrag | 未実装 |
| Tabの並べ替え、drop indicator、等分幅 | 等分幅（72～160 logical px）と横scroll・名前省略、release時の並べ替え・挿入線をH1で実装。Escapeとbar外・window内dropは取消。drag中の端での自動scrollはない |
| Welcomeのrecent files | H1でwordmark・START・Open file/folder・drop案内を中央columnへ整理。現在のshortcut、狭い画面のscroll、hover/focus表示を追加。recent listは未実装 |
| Explorerから開く/新規window context menu | OS登録・配布処理が未実装 |

### Menu、command、status

| 草案 | 現状 |
|---|---|
| 全機能を一つのlogo menuへ集約 | H1でFile / Edit / Viewへ分類し、関連項目の区切り、現在shortcutの右揃え、window内scrollを実装。keyboard focusを最深menu内に保ち、上下/Tab移動、左右の階層移動、Enter/Space選択とEscape取消に対応。全registry commandの一意配置、無効項目、末尾到達・再openとpointer操作をtestする |
| File/Edit/Viewの3方向drag gestureとSVG logo | logo形状をvector描画。方向gestureは未実装 |
| 黒基調のcompactなwindow shell | 32px title/tab barと30px status、window操作、右寄せ情報をH1で実装。複数DPI/monitorのmatrixは未検証 |
| Command palette | titleの部分一致検索、上下選択、有効候補の巡回、Enter実行、Escape閉じを実装。IME eventと重複keyを分離し、focus再要求で毎文字の変換が取り消される不具合も修正。Windows日本語IMEの候補表示・上下選択・確定・取消と確定後のcommand実行を実windowで確認した。ranking、categoryはない。物理keyboard・他IME・focus/DPIを含む横断matrixは未完了 |
| 日本語filename・文字表示 | Windowsの日本語fontを既定fontの後ろへ追加し、tab/statusの欠字を修正。日本語fontがない環境や全言語のfallbackは未対応 |
| Custom shortcutとprefix key | text設定として実装。GUI editor、競合表示、recording UIはない |
| Media別4×4 grid | key/clickとtext設定を実装。H1で列はみ出し、名前/path省略、click後のclose、物理位置対応と修飾key競合を修正。paletteとは同時表示しない。配置編集UI、drag配置、詳細animationはない |
| Statusへpath、位置、zoom、解像度、size、modified等 | filename、parent path、folder内位置、size、画像解像度・zoom、編集値などを部分実装。modified日時、詳細codec/stream情報はない |
| 常時1px seek bar、hover時展開 | H1でstatus上端に実装。動画・音声はduration取得後、timeline非表示時に使える。drag終了時に一回だけSeekする |
| Fullscreen時はUIを隠す | H1でF11/View menuのborderless fullscreenと通常bar/timeline非表示を実装。下端hoverでstatus/seek/解除buttonを重ね、dragはreleaseまで保持。画像・動画・readingでは2秒idleでcursorを隠し、入力・操作部・overlay・modal時は表示。上端tab/menu表示、double-clickは未実装 |

### Timeline

| 草案 | 現状 |
|---|---|
| 動画/音声のwaveform timeline | 既定96pxの高さ変更可能panel、waveform、CTI、click/drag seekを実装。音声はdefault表示 |
| 動画hover thumbnailと低負荷scrub | 20区間のcached thumbnail tooltipを実装。H1でhover位置の上へ中央揃え・画面端制約、最大160×108 logical pxと空き高さへのaspect-fitを追加。縦長・小windowでtrackを覆わない。thumbnailを本画面へ出すscrub previewは未実装 |
| 画像のfolder位置seekとthumbnail | Shell snapshotの画像順seekと移動先preview・位置・filenameを実装。readingは現在の枚数・縦横・反転でpage群を縮小表示する。filmstripのworker/cacheを共用し、離脱・overlay・snapshot更新で失効。移動はdirty guardを通し、hoverだけでは現在画像を変更しない |
| Range selection、範囲内再生、delete/cut | I/Oと開始/終了gripによる単一trim範囲をH1で実装。track上の任意範囲選択、複数区間、delete/cutは未実装 |
| Rubber bandでtrack volume | 未実装 |
| Range伸縮でrate編集 | 未実装 |
| 上端dragでtimeline高さ変更 | H1で実装。既定96px・下限64px、上限はbarを除く残り高さの60%（小さいwindowは下限も縮小）。再生位置・編集は変えない |
| Trim handleと編集mode | I/O端点検証、bracket・ミリ秒表示、範囲内live再生をH1で実装。drag handleは未実装 |

### 画像

| 草案 | 現状 |
|---|---|
| Static/animated imageとAVIF | 実装済み。ただし互換性とmemory上限は限定的 |
| Cursor基点zoom、pan、actual、fit | 実装済み。H1でCtrl+wheelがeguiのscroll→zoom変換後に無反応となる抜けを修正。実wheel eventの倍率・frame間隔・cursor基点とoverlay遮断を回帰testし、通常releaseで拡大/縮小を確認 |
| Selection作成、正方形、辺resize、ratio保持 | 基本実装済み。H1で押下位置を保持し、release位置の反映漏れを修正。画像/動画の疎なevent列・逆方向・辺hit・範囲外開始・click previewを回帰test。移動直後のrelease注入がclick扱いになった例は、Windows button座標の先行反映で解消し通常releaseで確認。物理入力・mixed-DPIの全matrixは未完了 |
| Selectionの移動 | 未実装。内部clickはcrop previewになる |
| 選択drag・panの取消 | H1で開始前の範囲/位置を保持し、Escape・focus喪失・modal/overlay・別commandで復元。取消後のmove/releaseで再開しない。回帰testと通常releaseのEscape、focus往復、dirty guard保持を確認 |
| 指定aspect ratio | 未実装 |
| Crop、90度rotate、flip、undo/redo、export | 実装済み |
| 自由回転 | 未実装 |
| Clipboard copy | 未実装 |
| Resize/resampleとinterpolation選択 | 未実装 |
| Fullscreen | H1で画像/readingの全領域表示、Escape復帰と最大化状態の保持を実装。複数DPI/monitor matrixは未検証 |
| 左右矢印、Home/End、Page、Backspace、A/D、数指定jump | H1で画像の左右矢印とHome/Endを共有commandへ追加。Shell snapshotの画像順で前後/端点へ移動し、reading mode・dirty guard・custom bindingに対応。現在の端点では再loadせず、paletteの文字編集を優先。動画/音声のSeekとCtrl+左右は維持。Page、Backspace、A/D、数指定jumpは未実装 |
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
| Track selection、delete、cut、range playback | 単一trimのrange playbackのみH1で実装。track selection、delete、cutは未実装 |
| Repeat、shuffle | 未実装 |
| Video zoom、fullscreen、resize/resample | fullscreenはH1でhardware/software共通のaspect-fitと復帰を確認。zoomとresize/resampleは未実装 |
| Video crop/rotate/flipのlive preview | H1で同じdevice内のUV表示を実装。回転後のSAR・selection、Undo/Redoとexport照合を検証。trim live範囲再生とは別 |
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
| Export progress/cancel/失敗後のpartial target処理 | 書き出し済み時間とcancelを表示。成功後だけ一時outputをtargetへ置換し、失敗・置換前cancelで既存targetと編集を保持。置換後の取消は保存済み出力を残して自動離脱を止める。残り時間予測とqueueはない |
| PQ/HLGのHDR→SDR | D3D11 Video Processorが変換を保証したhardware frameだけ許可。基準adapterは非対応error |
| HDR displayへの10-bit pass-through | 未実装、対応を宣言しない |
| Waveform/thumbnail disk cache | 実装済み。64 MiB固定で管理UIや手動clear commandはない |

## 3. Codebase上の改善余地

- `crates/towavue-app/src/main.rs`へevent loop、UI描画、input、tab orchestration、preview worker連携、export flowが集中している。UI反復で競合と回帰範囲が広がるため、変更対象が固まった単位から挙動を変えずに分離する価値がある。
- AppのUI testはH1でegui描画・pointer・palette focus・modalの回帰を追加した。確認画面は長文とwindow縮小/復元時の操作labelの非clip・Cancel/OK clickも検証する。実OSのIME・混在DPIやtab dragなどの操作matrix全体を自動testで保証してはいない。
- 対応拡張子、file dialog filter、実decoder能力、export codec選択の関係を一つのcapability modelへ統一していない。拡張子を増やすだけでは対応完了にならない。
- Loading、empty、error、unsupported capabilityのstate表現が各所のstatus textへ分散している。UX改善時には表示だけでなくstate transitionをcoreでtest可能にする余地がある。
- filmstrip以外のpreview workerはtask単位にthreadを起動する単純構成で、優先度、同時数、cancel、重複排除を持たない。filmstripは可視集合の最新要求を単一workerで処理する。
- hover thumbnailは同一media内で一つだけ取得し、失敗した区間は再openまで再試行しない。media load世代で旧結果を拒否する。
- 映像2秒・音声30秒のH.264/AACで判明した音声先行蓄積/黒画面は、独立input/demuxと、映像確認後に一度だけ音声を開始する構成へ変更した。同じfileの通常releaseで表示60・drop 0・CPU transfers 0、停止Seekと300msの制御開始遅延、hardware成立前のsoftware fallback/成立後のfaultを確認した。二系統読取の通常releaseによる30分4K60再試験も107,746枚表示・25枚drop・CPU transfers 0で完走し、drift p95 4.806ms・最大37.785msだった。先頭10分のdrop率は保守的上限でも0.069589%で基準内。ただし基準機とこのfixtureの測定であり、低速storageや全codecの保証ではない。
- Exportは同じ保存先volumeの一時outputへ書き、成功後だけtargetを置換する。失敗・置換前cancelの既存target保護は回帰test済みだが、電源断時のdurabilityまでは保証していない。

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

### 2026-09-06のlaunch監査整理

- 同期probe単体の局所測定では1.5 GB MP4が約20～22 ms、4K60 TSが約45～49 msだった。Open全体・cold storageのlatencyではなく、worker化の完了とも扱わない。この監査で見つけた時刻原点・duration・TS Seek不一致は上記の修正と試験へ進めた。長いGOPや遅い読み取りでのSeek・切替応答は別途測定が必要である。
- 物理keyboard/pointer、混在DPI、実endpoint切替・driver resetは、注入入力や所有process内の制御faultとは別の未完了gate。ユーザーのOS設定や他applicationへ影響する操作を暗黙に実施しない。
- 配布方式、FFmpeg同梱・license条件、clean-machine起動は未決定・未検証。portable/installerの選択前にpackageや公開を始めない。recent/session、方向gesture、複数区間編集などの追加機能は、これらのgateを満たす代替にはならない。

この順序は固定milestoneではない。試用で再現性の高いdata loss、crash、再生破綻が見つかった場合は、それを最優先する。

## 6. Launch判断に残る確認（2026-09-06）

H1の個別修正が通ったことと、配布可能な品質の判定を分ける。草案の全機能を初回launchの必須条件にはしないが、以下は未確認のまま完了と扱わない。

| 領域 | 現在の証拠と次の確認 |
| --- | --- |
| 編集・保存・終了の安全性 | dirty guard、複数tabの順次保存、export失敗/取消、描画不能時の保存を自動testと所有windowで確認。今後のUI変更でも同じflowを維持する |
| 再生性能・復旧 | 基準機の30分4K60と100回Seek測定は通過。制御故障による復旧と、実driver/endpoint変更の証拠は別であり、後者は未完了 |
| 日常操作と草案の外観 | compact shell、filmstrip、menu、tab並べ替え、空のWelcomeからのOpen/Cancel/復帰を確認。timeline操作と狭いwindowでの発見性を引き続き評価する。Welcomeのrecent/session復元や別window結合は未実装だが一括追加しない |
| 環境・入力 | 日本語font・scale入力の回帰とWindows日本語IMEの基本候補操作は確認済み。物理keyboard・他IME・focus、異なる実DPI間の移動、keyboard-only/accessibilityの横断matrixは未完了 |
| 配布 | portable ZIPかinstallerかはownerへ確認中。FFmpeg binary/licenseの配布決定、clean machine起動確認、package作成・公開は未実施。H1と分けて計画する |

同一frameの選択/panは固定egui event列で確認し、native traceでも押下～release～後続hoverが同frameに入り正しい選択を保持した。前回の「選択なし」はPNGの読み取り誤りで、実pixelに白い境界と内外の明暗が残ることを再確認した。続く細い選択は同じ左辺を右端へ再dragした結果であり、配送不整合の証拠ではない。別に再現した描画前のEscape取消漏れは保留押下の破棄で修正済み。物理入力・混在DPIのmatrixは引き続き未完了。
