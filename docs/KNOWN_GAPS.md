# towavue 既知の不足と草案との差

この文書はM7 checkpointとH1での改善を、これから人が触って改善するための基準として整理する。`concepts/concept.txt`は発想の参照元であり、ここに載っている項目も採用決定ではない。優先順位は実際の試用結果、再現性、利用頻度、architecture riskで更新する。

2026-09-09のowner指定: 現在は未公開評価版のブラッシュアップを優先し、実際の公開配布は行わない。Windows 10はowner環境に導入せず、合理的な仮想環境検証が難しければ実機確認を省略可とする。対応確認済みとは宣言せず、これを現在のH1停止条件にはしない。下部の日付付き監査で「公開／Win10確認」を残件とした記録は過去の検証範囲であり、現在の優先順位は[ROADMAP](ROADMAP.md)の冒頭を正とする。

## 1. 試用前に知るべき制約

新goalの残件と各checkpointの実績は[UX_IMPLEMENTATION_PLAN](UX_IMPLEMENTATION_PLAN.md)で管理する。tabごとの背景再生・完全な状態保持、timelineの選択編集、repeat／shuffle等は引き続き未完である。tab context menu／一括close／path copy・Explorer表示／path-only reopenは実装したが、window間結合、keyboardからのcontext menu呼出しと全体focus／UIA監査は残る。新しい台帳は過去の「このsliceには含めない」を永久的な却下とは扱わない。

U07の画像側では読み込み済みの画素・texture・zoom／pan／selection・読書設定／ページをtabごとに保持する。開いた画像のsnapshotはcloseまでpinするので、decode／texture cacheの256 MiBはprocess全体の上限ではない。未完了decodeは復帰時に再要求し、未完了resizeも元Arcから再処理する。一覧のscroll／focus、動画・音声sessionの保持と背景再生は未完。device世代の違う復帰画像は再upload対象とするが、この保持経路の実GPU removal／混在DPI監査は未実施である。

2026-09-09の通常release f91b4498では、120秒1080p H.264の先頭にkeyframeが1枚だけの生成素材で、4条件各100回Seekのp95が872.539～979.008msとなり、300ms目標を超えた。同じ生成条件を2秒間隔keyframeにした対照では40.242～108.103ms。長GOPの待ち時間は未解決であり、対照側の合格を全素材へ一般化しない。[動画保存・Seek比較の条件と範囲](DEVELOPMENT.md#新しい通常releaseの動画保存とseek条件比較2026-09-09-1410-jst)を参照。同じ本体の30分4K再生は107771 framesすべて表示・drop／CPU transfer 0、drift p95 4.812ms／最大17.349msで通過したが、単一基準機／素材の結果である。5分以降のprivateは224.23～240.14MiBで、リーク不在や実環境matrixの証明ではない。詳細はDEVELOPMENTの14:57記録を参照。

### 操作とpreviewの不一致

長GOP側の50秒Seekは同じFFmpeg単体のD3D11VAでも最初の1 frameまで836～936ms（3回）かかり、UI固有の遅延ではないことを確認した。app内部のprofileや最適化余地なしの証明ではない。従来の120 keyframeを持つ固定基準素材では、新本体の4条件各100回Seekがp95 30.411～103.126msで通過した。詳しくはDEVELOPMENTの14:22記録を参照。正確なframe選択を粗いkeyframe表示へ変更してはいない。

- Shift付き正方形作成と比率保持resizeが画像端で長方形になる問題を修正した。両軸共通の上限で止め、resizeの固定辺・直交中心とdrag開始比率を保持する。縦横/全方向/zero縮小後の回帰と通常releaseを確認。確定時の整数/動画偶数pixel丸め、物理入力・DPI matrixの未検証は残る。
- 映像より音声が長い素材で、再生中に音声だけの区間へSeekすると黒画面になる問題を修正した。最終映像をlate-frame dropから除外して保持し、音声clockは継続する。MP4/MKV・不均等なframe間隔の末尾画像照合と通常D3D11VA表示を確認。hover thumbnail/filmstripの空画像も最終選択frameの時刻へ一度だけ再生成し、通常windowと複数stream・TSの回帰で確認した。空preview時の追加decodeは長いGOPや遅いstorageに影響される。

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

- animated imageのdeadlineが長く遅れた場合、過去の全frameを一枚ずつ数える処理を完全周回の省略へ変更した。frameと次期限の位相を保ち、同じframeへ戻る場合の不要なuploadも省く。合成2日gapのrelease単発測定は20.918→0.005msだが、OSの実スリープ復帰試験やGPU upload自体の高速化ではない。
- 長いGOPのSeek中に終了すると、target到達までdecodeを待つ問題をH1で修正した。pipelineごとの取消flagを探索・demux・decoded outputで確認し、target以前のframe破棄中も停止する。1080p60・30秒GOPの通常release単発比較では終了待ち897 msから63 msへ短縮し、音声付き素材の再Seek・再生・tab closeも通過した。worker joinは維持し、進行中のFFmpeg call/OS I/Oの強制中断や、全素材の終了時間保証ではない。
- 画像decodeとreading modeの複数画像loadはH1で単一background workerへ移した。要求・結果は最新1件だけを保持し、古い結果は表示しない。texture化とGPU uploadはUI側に残り、大きい画像の表示切替が完全に無停止とは限らない。
- 初回の画面用変換は行単位のopaque判定で不要なalpha変換を省いた。透明/半透明行の丸めと全画素一致を保ち、通常releaseの未cache大PNG5枚ではtitle完了中央値233.814→219.187ms。decodeとuploadは残るため、cold-storageや初回表示全体の問題を解消したものではない。
- 静止画の再訪は最大8件・256 MiBのdecode cacheで高速化し、RGBAをappと共有する。file size/更新時刻の変更とmetadata失敗で失効する。同じdecode identityのtextureも最大8件・RGBA相当256 MiBで再利用し、graphics復旧時に消す。2026-09-09には移動方向の隣一件を別workerで先読みし、このdecode cacheを共用するようにした。6000×6000 PNGを600ms間隔で初めて開くtitle完了の中央値は220.113→31.840ms。見開きも読込済みページから逐次表示する。低解像度RGBAは別の64件／16 MiB cacheで共用し、元画像の寸法付きentryは通常／readingの原寸読込中にも表示する。cache missのページ寸法は仮置きで、実寸取得時に再配置される。寸法未知のdisk thumbnailは原寸代用せず、未訪問／未cache画像の黒い待機は残る。tab hoverは画像／音声のfilmstrip・動画のseek区間thumbnailを共用するが、recentもfilmstripのpreviewを共用するが、未訪問preview先行生成は未完。初回起動／cold storage、画素変換・GPU upload、連打全般の改善も残る。これらのcache上限はprocess全体のメモリ上限ではなく、title完了は物理表示遅延でもない。
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

- [local評価用Setupとuninstaller](LOCAL_SETUP.md)は実装・生成済み。正確なinstaller sourceの同梱と展開後の再buildに加え、許可されたWindows 11基準機で実導入・更新・shortcut起動・保存と通常自己copyアンインストールを確認した。利用者file／設定と共有VCは保持。Windows 10、VC未導入環境、実製品の中断復旧などは未検証で、公開配布版はない。インストール不要のportable application package、automatic update、file association、Explorer context menuはない。
- 実アンインストール後、NSISの一時自己copy約100 KBが試験TEMPに残る。本体／登録の削除成功とTEMP全消去は別の結果であり、次の再起動で消えるとは保証しない。更新用の復旧資料も意図的に保持する。詳細とupstreamの根拠はLOCAL_SETUPの実lifecycle記録を参照。
- settings画面、session/tab復元、window位置・sizeの保存はない。recent filesは直近40件のpath-only履歴として永続化するが、未保存編集や再生状態は復元しない。
- Explorerからのfile/folder dropはH1で実装した。複数fileは既存Open契約で開き、folderはShell順の先頭mediaを開く。folder要求は最新1件で、複数folderを一括展開するimport queueではない。virtual file、URL、app間tab結合は対象外。
- export errorは確認するまで残る詳細modal、画像load errorは画像領域（readingでは該当page）、動画・音声のplayback errorはFaulted中の中央領域に表示する。壊れたMP4から正常動画をOpenし、元のerror tab、最後にWelcomeへ戻るflowを通常releaseで確認した。他のerrorは主に短時間のstatus messageとterminal diagnosticで、履歴、copy、詳細表示はない。
- OS-level end-to-end UI test、visual regression、複数DPI/monitorの自動matrixはない。accessibilityはheadlessのtree/action回帰とWindows UI AutomationによるWelcome/menu/paletteの手動確認を追加したが、screen readerや全custom widgetの横断matrixではない。現在のUI完了判定には実window操作が必要である。
- 画像100%とzoomはphysical pixel基準へ修正し、100/125/150/200%の描画入力、crop preview・編集後寸法・pointer anchorを自動testした。UI rendererの二重拡大も実windowのpixel照合で修正した。ただし接続中の2画面は両方96 DPIで、異なる実DPI間の移動・切断は未検証。

## 2. UI草案との対応

### Shell、navigation、tab

filmstripを開くと現在項目へfocusし、同じmediaで閉じると呼出元へ戻る。移動後は現在tab（fullscreenではExit、WelcomeではWelcome tab）へ戻し、古い選択辺へ復帰しない。通常画像の取消・移動・fullscreen・dirty Cancelを確認したが、音声playlistを含む全連続flowとscreen readerの横断確認は継続中。

音声playlistのfilmstrip往復と矢印再開も確認した。palette背後のfilmstripへUIAで選曲できた問題は、palette/grid/menu中のdisabled化で修正した。上のpalette/gridを閉じるとfilmstripを操作でき、次のEscapeでplaylistへ戻る。全screen reader・実pointer/物理入力matrixは未完了。

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
| Welcomeのrecent files | 安定した非media tab identity、Open file/folder、永続化した直近40件の可視thumbnail／waveform gridを実装。再起動復元と一覧からのOpenをWindows 11で確認。session復元・未保存backupではない |
| Explorerから開く/新規window context menu | OS登録・配布処理が未実装 |

### Menu、command、status

logo menuのEscape取消後はlogoへfocusを戻し、Enter/Spaceで再openできる。Welcome/画像の親menu・submenuと通常command実行を確認したが、全overlay間の連続操作・screen readerの横断確認は継続中。

menuからpaletteを開いて取消すと、消えた項目へのfocus復帰でnative accessibility consumerがpanicした。command dispatch前のlogoへの引継ぎと、完全root treeのfocus存在検証で修正した。通常Welcome/画像の生存とlogo復帰を確認済み。logo focus中のR不達も非text controlのshortcut配送を修正し、logo/reading button/tabでRとUndoを確認。UI操作・検索入力は優先する。連続menu再openの不成立は、試験helperがEdit categoryと通知文を名前の前方一致で混同した誤判定だった。Button型も照合すると同じ通常binaryで回転/Undoとdirty close/Cancelを繰り返せる。全overlay・screen readerの横断確認まで完了した意味ではない。

| 草案 | 現状 |
|---|---|
| 全機能を一つのlogo menuへ集約 | H1でFile / Edit / Viewへ分類し、関連項目の区切り、現在shortcutの右揃え、window内scrollを実装。keyboard focusを最深menu内に保ち、上下/Tab移動、左右の階層移動、Enter/Space選択とEscape取消に対応。全registry commandの一意配置、無効項目、末尾到達・再openとpointer操作をtestする |
| File/Edit/Viewの3方向drag gestureとSVG logo | logo形状をvector描画。方向gestureは未実装 |
| 黒基調のcompactなwindow shell | 32px title/tab barと30px status、window操作、右寄せ情報をH1で実装。複数DPI/monitorのmatrixは未検証 |
| Command palette | titleの部分一致検索、上下選択、有効候補の巡回、Enter実行、Escape閉じを実装。取消時に直前のfocusへ戻し、画像/動画の四辺を続けて調整できることを通常releaseで確認。command実行時は新しいfocus先を優先する。IME eventと重複keyを分離し、focus再要求で毎文字の変換が取り消される不具合も修正。Windows日本語IMEの候補表示・上下選択・確定・取消と確定後のcommand実行を実windowで確認した。ranking、categoryはない。物理keyboard・他IME・focus/DPIを含む横断matrixは未完了 |
| 日本語filename・文字表示 | Windowsの日本語fontを既定fontの後ろへ追加し、tab/statusの欠字を修正。日本語fontがない環境や全言語のfallbackは未対応 |
| Custom shortcutとprefix key | text設定として実装。GUI editor、競合表示、recording UIはない |
| 検索欄などのOS clipboard連携 | 固定egui-winitのclipboard featureを有効化。日本語・アクセント文字・絵文字のpaste/copy/cut回帰と、通常releaseからOS clipboardへの正確な往復を確認。画像copyは別のruntime workerで実装し、原画／回転／selectionの全画素と透過RGBA、文字入力の優先を確認。clipboard競合・他IME・全入力matrixは未検証 |
| Media別4×4 grid | key/clickとtext設定を実装。H1で列はみ出し、名前/path省略、click後のclose、物理位置対応と修飾key競合を修正。button focus中もEscape一回で取消して元の操作部へ戻り、上のmenuとprefix取消を優先する。paletteとは同時表示せず、切替時は元の復帰先を引き継ぐ。配置編集UI、drag配置、詳細animationはない |
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
| Trim handleと編集mode | I/O端点検証、bracket・ミリ秒表示、範囲内live再生と開始/終了gripのdragをH1で実装。releaseで一回だけ確定し、Escape・focus喪失・対象切替で取消。frame単位snapはない |

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
| Resize/resampleとinterpolation選択 | Ctrl+Rの寸法／比率固定／4補間、非同期処理とUndo/Redo、処理済みcopyを実装し保存PNGとの一致を確認。View／paletteから表示専用Smooth／Nearestも切替可能。画像／animation／読書・cache clone／復旧dataと無編集・copy不変を回帰、WARPと実windowでnearest出力画素を確認。resize固有の実GPU復旧／mixed-DPI追加監査は残る |
| Fullscreen | H1で画像/readingの全領域表示、Escape復帰と最大化状態の保持を実装。複数DPI/monitor matrixは未検証 |
| 左右矢印、Home/End、Page、Backspace、A/D、数指定jump | H1で画像の左右矢印とHome/Endを共有commandへ追加。Shell snapshotの画像順で前後/端点へ移動し、reading mode・dirty guard・custom bindingに対応。現在の端点では再loadせず、paletteの文字編集を優先。動画/音声のSeekとCtrl+左右は維持。Page、Backspace、A/D、数指定jumpは未実装 |
| Reading mode 2～10枚、縦横、反転 | 横は高さ・縦は幅を揃えた隙間なしの連結表示と全体の中央fitを実装。seek hoverにも同じ配置を使い、画像previewのpaddingを除去。ページ送りは一枚ずつで、見開き単位の移動は未実装 |
| Reading表示数のbutton drag、offset調整、設定保持 | 未実装 |
| Marker、text、色調補正 | 草案でも後回しまたは対象外。現在も未実装 |

### 動画・音声

| 草案 | 現状 |
|---|---|
| D3D11VA優先、software fallback | 実装済み。codec/GPU/driverごとの成功は実機依存 |
| Audio master、seek、pause、EOF、late frame drop | 実装済み。基準fixtureで測定済み |
| WASAPI Sharedとdefault endpoint復旧 | 実装済み。hardware/driverの広いmatrixは未検証 |
| Wheel volume、hold中2倍速 | 動画面と動画/音声statusのwheel音量はH1で実装。event時点のtargetで選別し、playlist scrollやmodal入力とは分離する。hold中2倍速は未実装 |
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
| Waveform/thumbnail disk cache | 実装済み。64 MiBを目標に削減。cache directory作成・保存失敗でも生成済みpreviewは返し、利用可能になれば再保存する。生成失敗や取消は別扱い。削減を妨げる権限・共有状態での容量保証、管理UIや手動clear commandはない |

## 3. Codebase上の改善余地

- `crates/towavue-app/src/main.rs`へevent loop、UI描画、input、tab orchestration、preview worker連携、export flowが集中している。UI反復で競合と回帰範囲が広がるため、変更対象が固まった単位から挙動を変えずに分離する価値がある。
- AppのUI testはH1でegui描画・pointer・palette focus・modalの回帰を追加した。確認画面は長文とwindow縮小/復元時の操作labelの非clip・Cancel/OK clickも検証する。実OSのIME・混在DPIやtab dragなどの操作matrix全体を自動testで保証してはいない。
- 現在tabの未保存確認Cancel後は、元の操作部へfocusを戻す。通常画像/動画の四辺、画像の実Save As取消→確認Cancelと、回帰での描画前再確認・隣tabへの古いfocus非復帰を確認した。全modal・全screen readerのfocus横断確認ではない。
- 対応拡張子、file dialog filter、実decoder能力、export codec選択の関係を一つのcapability modelへ統一していない。拡張子を増やすだけでは対応完了にならない。
- Loading、empty、error、unsupported capabilityのstate表現が各所のstatus textへ分散している。UX改善時には表示だけでなくstate transitionをcoreでtest可能にする余地がある。
- 起動時のshortcut／grid記述ミス・読込／初回保存失敗は、その設定だけ既定値へ戻して継続し、原本を保持する。path／理由付きnative警告と修正後Reloadを接続した。隔離した設定fixtureの回帰に加え、通常release f91b4498で警告全文・背景操作の無効化・Enterでの解除・menu操作と修正後Reloadを確認した。cache保存先がfileで塞がれていても画像とfilmstripを表示し、そのfileの除去後は同じprocessでcache保存を再開した。全設定エラー種別の実画面・screen reader・混在DPIを保証するものではない。
- duration・waveform・hover thumbnailはH1で各種類1本の常設worker、実行中1件＋最新待機1件へ制限した。media切替/closeでは未開始要求を破棄し、owned child processも取り消す。種類間の優先度制御や進行中のnative I/Oの強制中断はない。filmstripは可視集合の最新要求を別の単一workerで処理する。
- hover thumbnailは現在表示用のtextureを一つ保持し、20区間のcacheを利用する。失敗した区間は同じload中に再試行せず、media切替/再openで失敗記録を消す。media load世代で旧結果を拒否する。
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

## 6. Launch判断に残る確認（2026-09-06 20:04 JST再監査）

### 2026-09-08の配布候補除外と再build

固定開発FFmpegのChromaprint→GPL FFTW静的リンクをrecipe、当時のlibrary/pkg-config、実avformat DLLで確認した。FFmpegのLGPL自己表示だけでは既定の配布構成を満たさず、旧binaryをinstallerへ入れない。[FFMPEG_REBUILD.md](FFMPEG_REBUILD.md)のKissFFT切替patchと単体8条件のfingerprint一致は確認したが、FFmpeg全体の再build、全機能/性能の比較、対応source/notice、VC runtime、installerとclean-machine gateは残る。以前の「LGPL 3以降」は自己表示の観測であり、推移依存を含む承認ではなかった。

### 2026-09-07 22:18の配布依存監査

[DISTRIBUTION.md](DISTRIBUTION.md)へ現行binaryの同梱候補と不足資料を記録した。補助exeのimportからavdeviceを含む7 DLLが必要で、固定FFmpegはLGPL 3以降である。全bin fileのZIP entry一致を確認したが、対応する第三者source/license一式と再現可能buildの証拠はまだ揃わない。Visual C++ runtime候補のversion・hash・署名は確認しただけで、配布条件やclean-machine起動を合格にしない。実装変更・binary公開・OS操作は行っていない。

### 2026-09-07 22:14の配布方式決定

ownerは「インストールあり」「monapad側のような形」と回答した。monapadの固定commitの設定を確認し、インストール先を選べるSetup.exeをARCHITECTURE §7の配布方針にした。単一fileのportable appは要求しない。以下の22:11以前の形式確認待ちは解消したが、FFmpeg等の同梱条件、開発用環境変数なしの起動、installerの導入/更新/削除、clean-machine検証と公開は未完了。ROADMAPの段階計画に従い、Electron・自動更新・file関連付けは暗黙に追加しない。

### 2026-09-07 22:11の配布希望と依存確認

- ownerはexe形式の配布を希望。インストール不要の単一exe、exeインストーラー、補助fileを伴うexeのどれを意味するかは確認待ちで、単体化・同梱・公開を承認済みとは扱わない。
- 現行release（SHA256 `339046…7FCA2`）のPE importをLLVMで確認すると、FFmpegの`avcodec-63.dll` / `avfilter-12.dll` / `avformat-63.dll` / `avutil-61.dll` / `swresample-7.dll` / `swscale-10.dll`と`VCRUNTIME140.dll`への直接依存がある。`target/release`に隣接DLLはない。preview/exportは`FFMPEG_DIR/bin`にある`ffmpeg.exe` / `ffprobe.exe`を優先し、なければ実行file名だけで起動する。現行exeを単独で渡す形は自己完結した配布物ではない。これは直接依存と探索codeの確認であり、推移依存の全一覧・clean-machine起動試験・同梱license条件の決定ではない。
- `918d0c4`のCI 34125434899/job 101752898227は22:11:06 JSTに全step成功。実装・OS設定・packageは変更せず、この希望と確認結果だけをローカル記録する。H1と実環境・owner受入・配布gateは引き続き未完了。

### 2026-09-07 22:04の再開確認

- 6時間以上後の明示的再開に際し、古い失敗annotationだけでは現在の実行可否を証明できないため、既存workflowを一度だけ再試行した。`196c116`のCI 34091346834はattempt 2/job 101750929926で全step成功（22:03:52 JST完了）。CI起動障害は現在のblockerから外す。課金設定・spending limit・workflowは変更しておらず、アカウント側で何が変わったかは推測しない。
- ローカルのformat・Clippy・268 testsも再通過し、release SHA256は下記の`339046…7FCA2`と一致。製品code/依存に変更はなく、15:35/15:38にローカル保持した監査記録を確認済みcheckpointへまとめる。既存live ignore 3件は合格に数えない。
- 目標は再開したがlaunch完了ではない。実音声出力切替の許可、実環境/owner受入と配布方針は未確認のまま。配布方式は方針確認の質問のみを出し、package作成・公開・課金/OS操作は開始していない。以下の15:35記録は当時の状態であり、最新HEADのCI失敗を現在も継続中とは扱わない。

### 2026-09-07 15:35の引継ぎ状態

- 現在HEADは`196c116`。製品code/依存/toolchainは`a5639e5`以後差分なしで、release SHA256は`339046281C097BE5BA05D91BDA9D73F8501BD433A0A6AA96EF4C77E6DD87FCA2`。core/appのunsafe禁止とcoreの無依存を再確認し、concepts/vendor/target/generated mediaはtrackedでない。
- ローカルの直近format・Clippy・268 testsは通過。既存live ignore 3件は合格ではない。`80108c4`のCI 34090843746は全step成功。一方、最新`196c116`の[CI 34091346834](https://github.com/sheetau/towavue/actions/runs/34091346834)はrunner未割当・stepなしで失敗し、check annotationは支払い失敗またはspending limitによる未開始を示す。コードのtest failureではないが、最新HEADのCI通過とも扱わない。ownerによるアカウント確認が必要で、課金設定変更、上限引上げ、再run、追加pushを暗黙に行わない。
- 同じreleaseで代表PNG/chirp/動画の保存・再open、4条件計400回Seek、30分4K60のdrop/drift gate、同倍率の実2画面での画像・動画・reading/fullscreen/最大化復帰を確認した。DEVELOPMENTの14:28～15:31記録が証拠の範囲であり、全codec・全環境・物理遅延の保証ではない。
- 未解決の技術的観測: 約15分の一時メモリ増加の原因、粗い標本に現れない短いpeak、進行中native I/Oの待ちなど。位置Seekの非再現を「修正済み」や「リークなし」としない。
- 未確認の実環境gate: 物理keyboard/pointer・他IME・screen readerの全体操作、実mixed-DPI、Windows 10 22H2、実endpoint切替/抜き差し/driver reset。現在2画面はともに100%、OSはWindows 11 build 26200。既定音声出力を一時変更する許可は未受領で、clipboard許可や自動goal継続で代用しない。
- owner判断を要するgate: 草案に対する外観/操作の受入とreadingの区切り方、portable/installerの選択、FFmpeg同梱・license条件、clean-machine配布検証。version 0.0.0と開発FFmpeg配置を完成packageと呼ばず、H1から配布作業へ無断で範囲を広げない。recent/sessionや高度な編集など、後回しの追加機能をこれらの代替にしない。

全体のlaunch完了は未証明。15:35時点で全native trialは終了し、OS設定・課金・配布操作は行っていない。この引継ぎ更新はCI再開条件の確認までローカルに保持した（22:04に上記の再試行成功を確認）。

2026-09-07 15:07追記: a5639e5通常releaseの代表PNG/chirp/動画保存・再open、4条件各100回のSeek（p95 30.766～105.332ms）に加え、同binaryの30分再生も確認した。107,771 hardware frames、12 drops、CPU transfer 0、drift p95/max 4.803/36.985ms、先頭10分drop率の保守的上限0.033403%で基準内（DEVELOPMENTの14:28/14:32/15:07記録）。約15分でprivate memoryが約319.59 MiBへ増えて次の標本で約223.41 MiBへ戻る挙動が再現したが、原因は未特定。下表の32967f9とは別の証拠であり、実環境・配布・owner受入gateは引き続き未完了。

15:17の位置切り分けでは、起動後早期に880秒へSeekして899～930秒を2回通過しても約224～228 MiBだった。Seek直後の約418～453 MiBへの増加は300秒でも起き、約10秒後には戻る。source位置だけで連続再生の一時増加を説明する証拠はなく、原因確定や全peak/リーク有無の証明ではない。

15:25の実機確認では横1920×1080・縦1080×1920の2画面がともに100%であり、混在DPIは未検証のまま。通常releaseの画像移動→各画面のfullscreen→元のwindow位置/サイズ復帰、選択保持とaspect-fitは確認した。音声はUSB HIFI AUDIOとNVIDIA Broadcastがactiveで既定3rolesはUSBだったが、既定変更の許可は未受領のため切替試験はしていない。

15:31には同じ2画面で動画pauseとreading（横並び、および縦並び・逆順）の最大化→fullscreen→最大化→通常window復帰も確認。動画は5秒の位置とframeを保ち、再Play後にEOFへ到達した。同倍率の実monitor間移動の証拠であり、混在DPI・物理入力・screen readerや外観のowner受入まで完了したものではない。

H1の個別修正が通ったことと、配布可能な品質の判定を分ける。草案の全機能を初回launchの必須条件にはしないが、以下は未確認のまま完了と扱わない。

| 領域 | 証拠の範囲 | 次に必要な確認/作業 |
| --- | --- | --- |
| 再現可能なbuild・境界 | 9fe5a3aでformat/Clippy/239 testsを再実行し通過。core/appはunsafe禁止、依存とFFmpeg archive hashを固定。concepts/vendor/target/generated mediaはtrackedでない | 3件のlive ignoreは合格へ数えない。CIはwindows-2022であり、実GPUや製品対象OSの代用ではない |
| 編集・保存・終了の安全性 | dirty guard、複数tabの順次保存、export失敗/取消、描画不能時の保存の記録と現行回帰あり。直近の画像error→移動→修復→Welcomeも実windowで通過 | 最終配布候補でも代表的な画像/動画/音声の保存・再open・取消を維持。電源断のdurabilityや全codecの保証ではない |
| 再生性能 | 32967f9の30分logは107758 presented＋13 dropped＝107771、CPU transfer 0、drift p95/max 4.704/30.109ms。先頭10分drop率の保守的上限0.036187%。100回Seekは再生/停止時p95 103.382/33.651ms、UIA tree取得後も103.416/57.750ms。メモリの一時332.19 MiBへの増加と復帰もDEVELOPMENTに記録 | 記録は各binary/基準機限定。最終候補でM3の10分drop<0.1%、30分drift p95≤40/max≤100ms、1080p 100回Seek p95≤300msを確認し、古い測定へ新binaryのlabelを付けない。粗いメモリ標本から全peak・GPU allocation・リークの有無は断定しない |
| device復旧 | 制御faultによる再構築/保存保護とheadless回帰はある | 物理endpoint変更、unplug、実driver/adapter変更は未検証。OSや他appへ影響する試験を暗黙に実行しない |
| 日常操作・草案の外観 | compact shell、palette、menu、filmstrip、tab、reading連結、selection、Welcomeの記録あり。今回Welcome/reading/audioの草案画像も再確認 | pixel完全一致やownerの外観受入は未証明。recent一覧、曲ごとの長さ、見開き送り等の差が残るが一括で必須扱いしない。読書の区切り方はowner回答待ち |
| OS clipboard | egui-winitのclipboard feature、arboard 3.6.1/clipboard-win 5.4.1を既存入力/platform outputへ接続。ownerの書込み許可後、通常releaseでUnicode往復・cut後の空欄・外部変更後の再pasteを確認。変更前は同じpasteが空欄のままだった | clipboardを他processが占有する場合、全形式/IMEのmatrixは未検証。画像copy機能と混同せず、以後の試験でもclipboard内容への影響を明示する |
| accessibility | AccessKitのtree/action配送、Welcome/menu/palette、保存確認中の背景拒否、再生・画像位置・trim端点の値操作を確認。Shell STA停止を修正。tabとplaylist/filmstripの対象ID・名前・Invoke・path説明、playlistの画面外行focus移動/選曲、filmstripの連続Tab時focus・Escapeと未保存Cancelも確認。画像/動画の全体選択・四辺のpixel値/focus操作・crop/Undo・modal拒否、拡大画像の選択辺への最小pan・手動pan保持も通常releaseで検証 | screen reader、全体の読み順/focus、window resize/実mixed-DPIを含む横断focus、画面外一覧項目へ支援技術だけで到達する全操作は未完了。keyboard/native試験をscreen reader・実環境の横断matrixの代替にはしない |
| 対象OS・入力 | 現在の機械はWindows build 26200。日本語IME、注入pointer/key、scale入力の回帰/記録はある | Windows 10 22H2実機/VM、物理keyboard/pointer、他IME、実mixed-DPI、keyboard-onlyの横断確認は未完了 |
| 配布 | versionは0.0.0の開発workspace。owner指定でインストール先を選べるSetup.exeを予定。setup scriptは開発用FFmpegを準備するだけで製品packageではない | FFmpeg等の配布条件と同梱物、開発環境に依存しない起動、installer導入/更新/削除、clean-machine検証と公開は未完了。上記の段階計画で進める |

根拠の詳細は[DEVELOPMENT](DEVELOPMENT.md)の各日付付きscenario、[ROADMAP](ROADMAP.md)のM3/H1 gate、appの`Cargo.toml`と固定dependency source、`.github/workflows/ci.yml`を参照する。20:04監査時のrelease出力SHA-256は`660F60A453A8C8473A2B591B3866AAC64BBE68A80F7FA6000555686EEE5615FE`で、上表の過去30分測定binaryとも、その後のclipboard/accessibility検証binaryとも異なる。全体のlaunch可否は引き続き未証明である。OS text clipboardの通常往復とaccessibility bridgeを確認した後も、custom widget・支援技術・実環境gateは残り、小さな性能改善だけでこれらを完了扱いにはしない。

同一frameの選択/panは固定egui event列で確認し、native traceでも押下～release～後続hoverが同frameに入り正しい選択を保持した。前回の「選択なし」はPNGの読み取り誤りで、実pixelに白い境界と内外の明暗が残ることを再確認した。続く細い選択は同じ左辺を右端へ再dragした結果であり、配送不整合の証拠ではない。別に再現した描画前のEscape取消漏れは保留押下の破棄で修正済み。物理入力・混在DPIのmatrixは引き続き未完了。
