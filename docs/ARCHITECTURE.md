# towavue アーキテクチャ

## 1. 目的と優先順位

towavueはWindows向けの画像・動画・音声ビューア兼プレイヤーである。設計上の優先順位は次のとおり。

1. 正確で安定した再生、Seek、同期
2. 起動・アイドル・再生時の低オーバーヘッド
3. GPU常駐を維持する単純な表示経路
4. メディアを遮らない最小UI
5. 読めるコードと検証可能な境界

対象はWindows 10 22H2以降のx86-64。Rust 1.98.0、Edition 2024、MSVC ABIを使用する。

## 2. 採用技術

| 領域 | 決定 | 理由 |
|---|---|---|
| Window/Event | winit 0.30.13 | Win32の詳細へ降りられる薄いイベント境界 |
| UI | egui 0.35.0 | 最小UIとカスタム描画を少ないコードで構築できる |
| UI renderer | egui-directx11 0.13.0 | wgpuを介さずD3D11 render targetへ描画できる |
| Windows API | windows 0.62.2 | COM、D3D11、DXGI、WASAPI、Shell APIの公式Rust projection |
| Media | FFmpeg 9.0.1 / ffmpeg-next 9.0.0 | demux・codec・resampleの広い対応範囲 |
| Image | image 0.25.10 + FFmpeg AVIF decode | 静止画、アニメ画像、orientationを安全なRGBA frameへ統一 |
| Graphics | D3D11 + DXGI Flip Model | decode surfaceからpresentationまで同じdeviceを維持できる |
| Audio | event-driven WASAPI Shared | 他アプリと共存しつつ十分に低遅延 |

依存は使用を開始するmilestoneでのみ追加し、`Cargo.lock`へ固定する。`egui-directx11`は薄いadapter内に閉じ込め、必要ならUIやmedia境界を変えずに自前rendererへ交換できるようにする。

`eframe/wgpu`は採用しない。D3D11VAが返すNV12/P010等の外部・マルチプレーナresourceをD3D12側へ安全に取り込む経路が中核リスクになるためである。独自UI toolkitも、文字入力、DPI、accessibility、複数windowまで自前化するコストに見合わない。

## 3. Workspace境界

### towavue-core

OS非依存の値、状態遷移、コマンド、編集履歴を置く。unsafe、COM、FFmpeg、native handleを禁止する。

主な中核型は次のとおり。

- `MediaTime(i64)`: ナノ秒単位の時刻。
- `SessionId`: 開いているmedia sessionの識別子。
- `PlaybackGeneration(u64)`: Seekや再openの前後を識別し、古い結果を破棄する番号。
- `MediaCommand`: open、play、pause、seek、rate、volume、stop、close。
- `MediaEvent`: opened、state、position、ended、fault。
- `MediaInfo`: 種別、duration、stream、codec、解像度、sample rate、色空間。
- `CommandId` / `CommandContext`: menu、palette、shortcutが共有するcommand identity。
- `FolderSnapshot`: Shellが返したfolder identity、ordered media、sort columns、source、generation。
- `TabSet` / `ShortcutBindings`: platform非依存のtab targetとprefix対応key sequence。
- `ImageViewState` / `ReadingSettings`: 正規化selection、zoom・pan・crop preview、2～10 pageの表示軸と反転。
- `EditHistory` / `EditOperation`: source非破壊のcrop、90度回転、反転、trim端点、volume、rateとsaved cursor付きundo/redo。
- `PixelCrop`: preceding visual edit後の整数pixel crop矩形。UIの正規化selectionを確定し、previewとexportで共有する。

### towavue-runtime-windows

FFmpeg、D3D11/DXGI、WASAPI、Windows Shell、worker、queue、clockを所有する。FFI/COMとunsafeはこのcrateから外へ出さず、安全なcommand/event境界へ変換する。

GPU frameはruntime内部のRAII `FrameLease`で保持する。FFmpegの`AVFrame`、`ID3D11Texture2D`、array slice、COM pointerをappやcoreへ公開しない。

### towavue-app

winit event loop、egui、tab、command dispatch、利用者向け状態を所有する。runtimeへcommandを送り、eventと描画結果だけを受け取る。

M4ではmenu、command palette、shortcutの全入口を同じ`CommandId`へdispatchする。画像・動画の外部openはfile単位の新規tab、音声は同じfolderの既存playlist tabを再利用し、明示的な新規openだけ別tabにする。filmstrip、playlist、全種移動、同種移動は一つの`FolderSnapshot`を共有する。

M5ではruntimeがBMP、GIF、JPEG、PNG、TIFF、WebPを`image` crate、AVIFを既存FFmpeg software pathでdecodeし、native handleを含まないRGBA frame列とframe durationだけをappへ返す。appはegui texture、animation deadline、画像操作状態を所有する。画像と動画が共有するのは同じD3D11 device、visual surface、Presentだけであり、画像frameを動画decode queueやVideo Processorへ流さない。

M6では各tabが独立した`EditHistory`と直近export先を持つ。appは画像の履歴をUV meshへ順番どおり適用してpreviewし、動画上のcrop selectionを同じ正規化矩形で保持する。動画・音声のtrim、volume、rateを含む全operationはsource playbackを変更せずstatusへ反映し、runtimeのFFmpeg exportで初めて出力へ適用する。dirtyなtabの移動・closeとprocess終了は、入力を遮るExport / Discard / Cancel modalを必ず通る。

## 4. 再生・表示契約

- app event loopがruntimeの安全なfactoryを呼んでD3D11 deviceを作成し、M2ではFFmpegの`AVD3D11VADeviceContext`へ正しいCOM参照寿命で渡す。appへCOM pointerは公開しない。
- decode、D3D11 Video Processor、DXGI presentation、eguiは同じadapter/deviceを使う。
- immediate/video contextの利用箇所を限定し、FFmpegとの共有に必要なmultithread protectionを有効にする。
- SwapChainはFlip Discard、2～3 buffers、frame-latency waitable objectを使う。
- FFmpeg D3D11VA surfaceをVideo Processorへ直接渡す。hardware pathでCPU readbackや再uploadを行わない。
- hardware decodeが成立しない場合だけsoftware decodeへfallbackする。CUDA/QSV decode fallbackは設けない。
- HDR metadataは失わないが、正しいtone mapping/pass-throughが完成するまではHDR対応を宣言しない。

### M1 software path

M1ではFFmpegを動的リンクし、映像をsoftware decodeしてtightly packed RGBAへ変換する。runtimeの`FrameRenderer`が単一D3D11 device、immediate context、2-buffer Flip Discard swap chainを所有し、CPU frameをRGBA textureへuploadしてfull-screen shaderでpresentする。このCPU uploadはsoftware fallbackだけの基準経路であり、M2のhardware pathでは使用しない。eguiは同じdeviceとback bufferへ続けて描画し、1回のPresentに合成する。

音声はsource sample rateのinterleaved stereo `f32`へ変換し、専用MTA thread上のevent-driven WASAPI Shared clientへ渡す。映像queueは2 frame、音声channelは32 chunk、WASAPI手前の蓄積は約2秒へ制限する。M1のdecode workerはdemuxとvideo/audio software decodeを直列実行するが、COM、FFmpeg型、native frame handleはruntime外へ出さない。M3の役割別workerでも、この安全なcommand/event境界を維持する。

### M2 D3D11VA path

`FrameRenderer`が作成したD3D11 deviceのopaqueな`GraphicsDevice`参照をdecode workerへ渡す。FFmpeg用`AVD3D11VADeviceContext`にはcloneしたCOM参照の所有権を移し、`AVCodecContext`が`AVBufferRef`とともに解放する。immediate contextにはmultithread protectionを有効にする。

hardware frameはruntime内部のFFmpeg `AVFrame`がD3D11 texture arrayとsliceを保持し、bounded presentation queueを経て同じdeviceの`ID3D11VideoProcessor`へ渡す。appが受け取るのはpresentation timeとeventだけで、COM pointer、FFmpeg frame、texture handleは公開しない。hardware pathではmap、readback、software scaling、back-buffer uploadを行わない。

codec metadata、device、driverのいずれかがD3D11VAを成立させられず、まだhardware frameを公開していない場合だけ入力を開き直してM1 software pathへfallbackする。最初のhardware frame後のdecode errorはfallbackで隠さずsession faultとする。終了時にadapter LUID、hardware frame count、CPU transfer countを記録する。

### M3 synchronization and recovery

renderer再作成不能時は、GPUを使わない所有window付きnative確認を専用workerで表示する。Retryは失敗前のsource位置と再生/停止状態を使い、Cancelは編集を保持する。以後の終了要求はnativeのExport/Discard/Cancel確認から既存のSave As・background exportへ接続する。export失敗もnative通知にし、未保存編集を消さない。native確認とfile dialogは同時に一つだけとし、確認中の別操作を受け付けない。通常rendererがある場合のegui UIは変更しない。

native確認中にexportが完了した場合、保留された終了/移動は確認を閉じてから現在のdirty状態で再判定する。保存済みの旧tabへ再度保存を求めず、未保存tabが残ればそちらを確認する。file dialog・実行中export・未確認export errorがある間は継続せず、Cancelで取り消したguardを復活させない。

export取消が出力先の置換に間に合った場合だけCancelledとして既存fileを保持する。置換後の取消は出力を巻き戻さず、成功したexport履歴を保持するが、保留中の終了/移動は実行しない。確認中に既に完了していても、nativeの取消選択は自動離脱を止める。表示では保存完了と取消要求を区別する。

graphics recoveryでは旧decode/output workerを停止し、旧rendererの参照を解放してから同じwindowへ新しいflip swap chainを作る。[D3D11の遅延破棄契約](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-flush)に従い、runtime内でClearState/Flushを行う。PresentやUI描画からのerrorもdevice removal理由を確認して同じ復旧経路へ渡す。新rendererにはfont atlasと保持中の画像/reading frameを再送し、再生成できるfilmstrip・waveform・hover previewだけを失効させる。tab・編集・選択・zoom・再生設定は保持する。GPU全体が再作成不能ならpanicや自動再試行loopにせず、Faultedのtitleと診断を残し、上記native確認へ移る。renderer不在でも描画要求は安全に戻る。

動画の`seek_latency_ms`はappの`seek_to`開始から、新generationの映像を描画した最初のPresent成功までとする。同期的なworker停止・再構築、decode待機、UI描画とPresent待機を含め、VideoReady通知では完了しない。失敗・別mediaへの移動・device recoveryで中断した要求と、映像を持たない音声は標本にしない。連続要求では未表示の旧要求を置き換える。rate/trim編集など同じSeek経路を使う再構築も含むため、性能ゲートは操作を限定した試験で測る。物理入力の配送前やDWM/scanoutの実表示時刻はこの計測範囲外である。

demux、video decode、audio decode、WASAPI outputは独立workerとし、stream別packet queue、decoded output queue、presentation queueをすべてboundedにする。demuxは満杯の一方のstreamだけで他方を直ちに停止させず、各streamに同じ上限のpending packetを持って空きqueueを先に進める。packetはdropせず、presentation時刻に遅れたdecoded video frameだけをdropする。audio workerの完了はvideo workerと独立してWASAPIへ通知し、末尾audio drain後はvideo-only clockへ切り替える。

通常再生は`IAudioClock`をmasterとする。running中のdevice positionが供給停止で進まない場合に限り、audio clientのstart/stopとpauseを追跡した単調時計を下限にして永久停止を防ぐ。Seekはgeneration更新後に旧workerと全queueを破棄し、`avformat_seek_file`、decode/discard、audio/video primingを新しいpipelineで行う。default render endpoint変更とD3D11 device removalはtyped eventとしてappへ渡し、現在位置と新しいendpointまたはD3D11 deviceでpipeline全体を再構築する。

audio masterに対して40msを超えて遅れたdecoded frameは、待機スケジュール時だけでなく描画直前のpromotion前にも既存queueから破棄する。待機判断の後にUIが遅延しても古い判断でframeを進めず、破棄数を既存metricsへ含める。PausedのSeek previewとaudio masterがない経路にはこの破棄を適用しない。

音声の正常排出後も残りの動画はpause/resumeできる。WASAPI workerはcontrol receiverを閉じる前に正常終了を公開し、その場合だけ閉じたcontrol channelへのpause/resumeを成功したno-opとして扱う。device変更・失敗・予期しない終了はこの扱いに含めず、既存のerror/recovery経路を維持する。

### M5 image presentation

静止画はEXIF orientation適用後、アニメGIF、WebP、APNGは合成済みRGBA frameと10 ms以上のdeadlineへ変換する。app event loopは次frame時刻までsleepし、期限を過ぎたframeを追いつかせてからegui textureを更新する。画像textureも動画・UIと同じD3D11 deviceとback bufferへ描画し、Presentは一回に保つ。

selectionは元画像に対する正規化矩形として保持し、表示scaleから独立させる。左dragで作成、辺dragで変形、Shift付き作成で画像pixel上の正方形、Shift付き辺dragで現在比率を保持する。右dragはpan、Ctrl+wheelはpointer anchorのzoom、選択範囲clickとCtrl+Yはpixelを変更しないcrop previewである。実crop、undo、save/exportはM6まで開始しない。

reading modeは表示専用で、同じ`FolderSnapshot`から現在画像以降の画像だけをShell view順のまま2～10 page取得する。横・縦配置と表示順反転はpresentation状態だけを変更し、個別画像のselectionや編集状態を作らない。

### M6 non-destructive editing and export

`EditHistory`は適用済みcursorとsaved cursorを別に持つ。新しいoperationをundo位置から追加した場合はredo branchを破棄し、破棄されたbranchにsaved cursorがあれば保存済みidentityも失効する。tab titleとwindow titleの`*`およびstatusのUnsavedは、現在cursorとsaved cursorが一致するまで消えない。folder内移動は同じtabの履歴を破棄するためcloseと同じguard対象だが、tab切替は履歴を保持するためguardしない。

exportはruntimeだけが`ffmpeg.exe`を子processとして起動し、app/coreへFFmpeg型を公開しない。画像filterはoperation順のcrop / transpose / flip、動画filterはそれらとtrim / PTS rate、音声filterはatrim / PTS / atempo / volumeを適用し、metadataを入力からcopyする。2倍を超える、または0.5倍未満のrateは複数の`atempo`へ分解する。video/audio encodeは固定FFmpeg buildのsoftware codecを使い、hardware encodeはM7まで行わない。Save As後のSaveは同じexport先を更新できるが、sourceと同一pathへの出力は拒否してpartial overwriteによるsource破損を避ける。

### H1 trim endpoint feedback

I/Oの端点はsource時刻で保持する。未指定の開始は0、未指定の終了はsource末尾として扱い、duration取得前・負の時刻・範囲外・開始以上でない終了はUIで拒否する。既存の履歴・saved/redo位置・再生位置は変えず理由を表示する。同じ有効範囲の再指定は履歴を増やさない。export境界でも負の端点・零長・逆転を拒否する。

有効な端点を指定したらtimelineを表示し、除外区間を暗く、保存区間をbracketとミリ秒付きsource端点で示す。fullscreenでは通常windowへ戻って表示する。Undo/Redo・tab復帰は履歴から表示を求める。入力検証の初回段階では「Export trim」と明示した。現在は後述のlive範囲再生へ接続し、範囲外の再選択・音声sample境界・Seek・EOFを扱う。range handle、cut/delete、時間軸伸縮は追加しない。

### H1 live trim range

書き出しのtrimも同じsource半開区間を使う。秒文字列はFFmpegでinput time baseへ最近傍丸めされ、境界直前のframeが入るため使わない。runtime workerで再生と同じbest streamを選び、映像はinput tick、音声はsample単位へ端点を切り上げて整数のstart_pts/end_ptsを渡す。trim時はstreamを明示mapし、copytsでsource時刻を維持してfilter適用後に出力PTSを0へ戻す。

低精度containerの音声PTSは、そのままchunk開始へ使うとsampleの重複・欠落を生む。decode worker内でFFmpegのav_rescale_deltaを使い、累積sample数とinput PTSの精度からsample時刻を復元する。前chunk終端の切り上げinput tickを超えるgapでは状態をリセットし、sourceの時刻飛びを保持する。状態はpipeline/Seekごとに独立し、sampleデータやWASAPI clockは変更しない。

極小trimではFFmpegが成功終了してもcontainer headerだけになる。trim出力はstaging内で要求media種別の非空packetを確認してからpublishする。動画要求で音声しか残らない場合も失敗とし、既存保存先と編集履歴を保持する。packet確認は全fileの再decodeやcodec品質保証ではない。

sample時刻復元は[固定FFmpegのdecoder処理](https://raw.githubusercontent.com/FFmpeg/FFmpeg/n9.0.1/fftools/ffmpeg_dec.c)に合わせるが、途中Seekで元のsample位相を失う低精度PTSは完全復元を保証しない。exportはsource先頭からのdecodeを基準とし、この差は既知の未完項目として扱う。最後のvideo frame長・圧縮音声paddingによりcontainer durationが区間長を越える場合もある。

trimを持つ動画・音声のPlayはsource基準の半開区間[start, end)を再生する。未指定端点は0/自然EOF。範囲終端でEndedになり、Playで範囲開始へ戻る。timeline/shortcutのSeekは全sourceを参照でき、範囲外へのSeekはPausedの素材確認とする。その状態でI/Oを再指定でき、Playは現在範囲の開始へ戻る。範囲内Seekは元のpause状態を保つ。端点変更・Undo/Redo・rate変更は現在source位置でgeneration付きpipeline再構築を行い、位置が範囲外なら素材確認としてpauseする。tab復帰は保存された範囲の開始から再生する。

runtimeのparallel decode収集点で映像PTSを[start, end)へ制限し、音声は開始/終了と重なるchunkのsampleを整数計算で切り出してからtempo/WASAPIへ送る。ナノ秒変換で切り捨てられたchunk PTSを最も近いsample indexへ戻し、開始以上・終了未満になるよう端点をceilする。これによりsample境界上の終端が1 sample増える誤差を防ぐ。各streamの終端を独立通知し、両streamが範囲終端または自然EOFへ達した時点で既存channelを閉じ、workerを回収する。終端まで全fileを無制限にdecode/discardしない。自然EOFの短いstreamとhardware fallbackを維持し、GPU frameをCPUへ戻さない。

終端判定はdecode完了だけでは行わず、最後の映像を表示し音声がdrainしてから行う。動画のみで終端まで残る時間は既存source時計と一回のdeadlineで待つ。位置表示は再生範囲の終了を超えない。素材確認中はこの上限を適用しない。これは単一区間のtrimで、cut/delete、repeat、range dragや新しい時刻軸は追加しない。

### H1 pixel-aligned crop

selectionのdrag中は正規化座標を使い、releaseとcrop確定時に現在の編集後寸法へ丸める。画像は1 pixel、動画は偶数の位置・寸法を使う。各辺を近いgrid境界へ丸め、同じ境界へ潰れた場合は内側の最小1 grid領域とする。現行defaultのlibopenh264は2×2を実際にencodeできず16×16未満を拒否したため、動画cropの確定は16×16以上に限定する。小さすぎる選択は勝手に16×16へ広げず、案内とともにselection・履歴を保持する。非finite・逆転・範囲外の選択と寸法未取得も確定しない。

確定cropは正規化floatではなく、編集時点の整数pixel矩形を`EditOperation::Crop`へ保持する。previewはその矩形からUVと整数寸法を求め、FFmpegへ同じ整数と`exact=1`を渡す。回転/反転/cropの履歴順は変えず、既存の履歴はmemory内だけのため保存形式の移行は生じない。確定時に出力寸法をstatusへ表示し、全領域cropはdirty履歴を増やさない。選択の一時crop previewも同じimage pixel丸めを使う。動画の一般的なresize/paddingやencoder変更は行わない。

crop境界でlinear samplingが選択外の隣接pixelを混ぜないよう、画像meshを端の半pixel帯で分割し、動画shaderでも選択領域内のpixel中心へsample座標をclampする。1×1画像は一色のまま拡大される。textureの再decodeやcrop用CPU copyは追加しない。

### H1 visual filmstrip

filmstripは中央の横scroll overlayとし、背景を暗くして現在項目の白枠・名前、画像/動画thumbnail、音声waveform、取得できたdurationを表示する。Shell snapshot順を維持し、clickは既存のguard付き移動、middle clickは新規tab、Tab/Shift+Tabは既存の全種移動へ渡す。開いた時と現在media変更時は現在項目を中央へ寄せ、wheelは横scrollに使う。

Tabはeguiのfocus traversalより前にfilmstripへ渡す。ただしpalette、grid、modal確認の最中は横取りしない。縦wheelの横変換はfilmstripのscroll領域だけに設定し、他のUIのscroll方向は変えない。

画面内の項目だけを単一runtime workerへ要求する。待機要求は最新1件、最大64項目、RGBAは各240×160以下とし、結果はpath/generationで照合する。UI textureは現在の可視集合だけ保持し、folder snapshot更新・closeでは失効する。diskは既存のmetadata付き64 MiB preview cacheを共有する。古い要求の未開始項目は処理せず、開始済みFFmpeg/FFprobeは完了後の結果を捨てる。window closeでそのprocess完了をjoinしない。これはpreview processの強制cancelやdecoder作業領域の上限を保証する変更ではない。

### H1 external file drop

Explorerからのfile dropはwinitのowned path eventで受け、既存のexternal Openへ渡す。画像・動画は新規tab、音声は同folderの非dirty playlistを再利用し、dirty/書き出し中のplaylistは別tabとして保持する。folder dropは非同期のOpen Folderへ渡し、Shell順の最初の対応mediaを開く。folder要求は従来どおり最新1件で、複数folderを展開・importするqueueは設けない。hover中は描画だけの案内を出し、外へ戻すかdropしたら消す。native picker、dirty guard、export error、guardからのexportの最中はdropを拒否し、確認対象を切り替えない。fileの移動・copy・source変更は行わない。

### H1 compact window shell

日本語filename・入力の欠字を避けるため、runtimeはWindows Fonts内のYu Gothic Medium、Meiryo、MS Gothicの順で読める一つのfont fileを返す。appは起動時にegui既定fontの後ろへ補助fontとして登録し、英数字の見た目を維持する。fontはOSから読み、同梱・download・OS設定変更は行わない。日本語fontがない環境はdiagnosticで明示し、既定fontで継続する。これは全言語fallbackや配布fontの選定ではない。

上部は32 logical pxの単一title/tab bar、下部は30 logical pxのstatus barとし、暗いneutral色でmedia領域を優先する。appはdecorationsなしのwinit windowにlogo menu・tab・window controlsを描画し、移動・resize・minimize・maximizeはwinitのWindows操作へ委ねる。window closeは既存のdirty/export guardを必ず通す。tab幅は等分、最大160 px・最小72 pxとし、収まらない場合は横scrollする。path/名前は省略表示と全文tooltipを使い、右側の状態表示へ専用領域を確保する。menuの方向gestureとtab reorderはこの変更には含めない。

logo menuはFile / Edit / Viewの3分類とし、app内の固定配置で関連commandを区切る。全registry commandを一箇所ずつ配置し、title・有効条件・現在のcustom shortcutは既存registry/bindingsから取得する。shortcutは右揃え、縦に収まらないsubmenuはwindow内でscrollする。commandのdispatch・dirty guardは変更せず、分類のために新commandやruntime処理は追加しない。方向drag gestureは引き続き対象外とする。

### H1 fullscreen viewing

F11をdefaultとする共有Toggle fullscreen commandをView menu・palette・custom shortcutへ登録する。winitのBorderless fullscreenを現在monitorへ適用し、復帰時のwindow位置・寸法・最大化状態もwinitの保存済みplacementに任せる。exclusive display modeやD3D deviceの再作成は行わず、通常のresizeと同じ単一device描画を使う。Enterは確定操作との競合を避けて割り当てない。

最大化から直接入ると、固定winitのWindows経路では旧client領域が残り復帰時のouter boundsも一致しないことを実機で確認した。appは入る前の最大化状態だけを保持し、最大化解除→Borderless、解除→再最大化の順で呼ぶ。通常位置・寸法の保存をappで重複実装せず、display modeやnative placementへ直接触れない。

fullscreenではtitle/tab bar、status、timeline、seek barとwindow resize操作を隠し、media領域をwindow全体へ広げる。画像/readingの外周余白も除く。音声playlistとWelcomeは中央contentとして残す。filmstrip・palette・grid、loading/error、export進捗・dirty guardは明示的な操作/通知として引き続き表示する。status通知とEscapeによる復帰案内は期限付きoverlayとする。timelineの表示設定は復帰まで保持し、fullscreen中にToggle timelineを実行した場合は通常windowへ戻ってtimelineを表示する。

Escapeはmodal/paletteの入力を優先し、次にfilmstrip/gridを閉じ、overlayがなければfullscreenを解除する。解除時に画像selection・編集・再生状態は変えない。edge-hoverによるbar表示とdouble-click割当は別の操作監査とする。

保存確認のEscapeはCancelと同じく離脱要求だけを取り消し、編集を保持する。export失敗のEscapeは最前面のエラー通知だけを閉じ、保留中の保存確認は残す。背景クリックではどちらも閉じず、保存・破棄は明示的なbutton操作に限定する。

保存確認・export通知は現在のegui表示領域に幅を制限する。長いfile名は一行で省略してtooltipに全文を残し、確認buttonは横幅に応じて折り返す。エラー詳細の縦scrollには画面高に応じた上限を設け、確認buttonを詳細の外に保つ。OSのDPIや設定は変更しない。

画像・reading・動画のfullscreen閲覧中だけ、入力が2秒ないとcursorを隠す。windowがactiveでpointerが内側にあることを条件とし、button保持・selection drag、filmstrip/palette/grid、picker・dirty guard・export、loading/error・file hover中は表示する。pointer移動・button・wheel・key入力、focus/入退出の変化で期限をリセットし、fullscreen解除時も表示へ戻す。音声playlistとWelcomeでは隠さない。eguiのplatform outputでcursorを統一管理し、期限をevent loopの既存待機へ統合する。非表示中という理由だけで再描画やpollを追加しない。最小化からpointerを動かさず復帰するとCursorEnteredが届かない場合があるため、focus取得時も既存のpicker復帰と同じclient座標更新を行う。

### H1 seek bar and command palette

timeline非表示時はstatus上端に1 physical pxのseek barを重ね、hover/drag時だけ太くしhandleを表示する。動画・音声はsource時刻、画像は同じShell snapshotの画像だけの順序へ対応付ける。dragはhandle位置を更新し、releaseで一回だけ既存のgeneration付きSeek/guard付き画像移動を行う。動画hoverは既存の20区間thumbnail tooltip、画像hoverは位置とfilenameとし、本画面のscrub previewは含めない。EOFからの位置移動はPausedとし、その位置からPlayできる。停止中のSeekでは音声時計が次のframe時刻へ到達できないため、保持frameがない場合だけ最初のdecode frameを時計待ちせず表示する。

paletteは検索入力を保ち、上下keyで有効な候補を巡回し、Enterで共有commandへdispatch、Escapeで閉じる。eguiの破棄されたlayout passで消費したkeyのactionも保持し、同一frameの同じUI actionは一回だけ実行する。

shortcut prefixは一続きのkey入力だけに有効とし、1秒の期限切れ、Escape、focus喪失、mouse press、別command、file drop・離脱確認で解除する。Escapeはprefix取消をoverlay/fullscreen解除より先に扱う。prefix開始時刻と案内の時刻を共有して通知の所有を識別し、取消ではその案内だけを消して再描画する。後から出た別通知を消さず、正常な複数key shortcutは従来どおり一回dispatchする。

shortcut設定の生成と読込は往復可能にする。`+` keyはmodifier区切りと曖昧にならない`Plus`として保存し、旧版が出力した`+`・`Ctrl++`等も同じkeyとして受け付ける。既存の利用者設定を移行のために上書きしない。

IMEのpreedit中、および確定/取消などIME eventを含むframeでは、paletteの上下・Enter・Escapeのkey eventを消費し、IME eventだけをTextEditへ渡す。確定用Enterをcommand実行やTextEditのfocus解除、取消用Escapeをpalette closeへ二重使用しない。入力欄の固定idへ描画前にfocusを要求し、eguiの上下focus移動による確定文字の取りこぼしを防ぐ。composition状態はpalette resetで解除し、通常の操作は次の独立key入力から再開する。OSのIME状態・keyboard layoutや設定は書き換えない。

### H1 video viewport

動画のdisplay matrixはruntimeで読み、90度単位の回転・反転をownedな四隅の順序へ変換する。stream metadataを初期値とし、frame metadataがあれば優先する。appはsource orientationを編集履歴より先にUV・寸法へ適用し、SARも軸交換に合わせる。source orientation自体は編集でもdirty状態でもない。software/hardware共通で既存UV shaderを使い、hardware frameをCPUへ移さない。平行移動はaspect-fitで相殺するが、任意角度・scale・shear・射影など表現できない行列は明示的なdecode errorにする。画像の既存EXIF適用は変更しない。

FFmpeg exportとthumbnailは既定autorotateがfilterより先にmetadataを適用するため、手動の二重回転を追加しない。編集cropはorientation適用後のpixel座標とする。回転metadata付きfixtureでpreview・保存後の寸法/四隅/再openとUndoを検証する。

固定OpenH264はvflip由来の負のstrideを受け取るとencodeに失敗するため、そのsoftware encoderだけは最後にFFmpeg copy filterで通常のframe bufferへ整える。export worker内の一frame copyであり、再生のD3D11VA経路・CPU transfer契約は変更しない。

動画・音声がFaultedになった場合、理由は期限付きstatusとは別に中央へ保持し、別mediaのload/closeで解除する。非対応matrixなどの理由が通知期限後に消えて黒画面だけにならないようにする。

動画frameを選んでから同frameのUI layoutを確定し、bar・timelineを除いた中央領域にsample aspect ratio込みでaspect-fitする。appは画面上の同じ矩形をselectionと表示に使い、runtimeへphysical pixelのdestination rectと編集UVを渡す。software shaderのviewportと直接表示時のVideo Processorのdestination/output target rectを一致させ、余白はclear色で残す。decode frameのnative ownershipと単一device・1回Presentは維持する。

H1のlive visual editでは、画像と同じ履歴順のcrop・90度回転・反転を動画にも適用する。appは編集後の寸法・sample aspect ratioでaspect-fitし、selectionをその編集後画像に対する正規化矩形として扱う。runtimeへdestination rectとsource UVの四隅だけを渡し、undo/redo・Seek・tab復帰にも現在の履歴を使う。表示変更ではdecode sessionや再生位置を再構築しない。

software動画は既存RGBA textureをUV付きshaderで表示する。hardware動画のUVがidentityでない時だけ、Video Processorの色変換結果をsource寸法のRGBA texture一枚へ出し、同じshaderでback bufferへ合成する。textureはrenderer所有・寸法変更時だけ再確保し、全処理を同じdevice内に保つ。CPU readback/reuploadや別adapterは使わず、未編集時は従来の直接Video Processor表示を維持する。追加GPUメモリはRGBA画素数×4 byte（4Kで約32 MiB、driver領域別）とし、rotation/mirrorのdriver固有能力へ依存しない。HDR変換の能力判定・typed errorは従来どおり適用する。trimのlive範囲再生と動画zoomはこのscenarioに含めない。

software描画はUIから引き継ぐscissor・blend・depth stateを解除する。表示frame数は新frameの描画成功時だけ加算し、UIだけの再描画では加算しない。UIのrepaint deadlineは動画・画像・folderの待機期限と統合し、停止中のlayout・tooltip・animation要求も処理する。

RedrawRequestedはそのevent内で描画し、egui-winitのrepaint応答を次frameの無条件要求へ変換しない。静止したUIは入力・worker完了・必要なdeadlineでのみ描画する。gridのopacity transitionもeguiのanimation期限に従い、表示中という理由だけで連続描画しない。status通知とshortcut prefixの失効も待機期限へ含め、mediaやwatcherがないWelcome画面でも時間どおり処理する。

音声だけ、または動画frameを待たない音声末尾の再生中は、位置表示へ20 ms後のUI repaintを要求する。audio eventのpoll自体は次の描画を無条件予約しない。pause後はこの周期描画を止める。WASAPIとdecodeのthread・clock契約は変更しない。

### H1 live volume

動画・音声のvolumeはedit historyの現在値をlive playbackとexportで共有する。runtimeはWASAPIへ渡す直前のstereo f32 sampleへgainを適用し、decode済みqueueは元の値を保持する。変更時は5 msのrampで不連続を抑え、mute後は正確なzero sampleにする。master endpointや他applicationの音量は変更しない。初期gainはpipeline開始前に設定し、Seek・endpoint復旧・tab再open・undo/redoにも現在値を反映する。trimは別項のH1 live trim range契約に従う。

### H1 live rate

rateは0.25～4倍のedit値を再生・exportで共有する。音声は固定FFmpegのin-process `atempo`を使い、各段を0.5～2倍に保ってピッチを維持する。WASAPI workerはstereo f32 chunkを逐次filterし、出力queueの上限とvolumeの直前適用を維持する。1倍はfilterを通さない。EOFではfilterもdrainする。

速度変更は現在のsource位置でSeekと同じgeneration更新・全queue再構築を行う。pipeline内のrateは不変とし、pause・volumeを保持する。WASAPI経過時間とvideo-only時計をrate倍してsource時刻へ変換し、frame deadlineはrateで割る。Seek、timeline、trim端点は常にsource時刻であり、rateで短縮されたexport時刻と混ぜない。

### H1 image loading

Fitは通常画像・reading pageとも表示領域に入る比率をそのまま使い、2%などの縮小下限を課さない。手動zoomの下限は既存2%と長辺1 physical pixel相当の小さい方、上限は既存64倍を維持する。大きい画像のFitからzoomを始めても2%へ飛ばず、Custom倍率はwindow resizeで変わらない。これは表示倍率の変更で、decode寸法・texture上限・source pixelは変更しない。

画像のActual/100%はsourceの1 pixelを画面の1 physical pixelへ対応させ、Customの倍率も同じ基準にする。appは現在のegui pixels-per-pointでviewportをphysical寸法へ変換してcoreのscale/zoomへ渡し、描画時にlogical寸法へ戻す。Fitは現在のmedia領域、keyboard/menuのzoomは直近表示viewportと編集・crop preview後の寸法を使い、固定window寸法や未編集source寸法を使わない。panとpointer補正はlogical座標のままとし、selection・履歴・source fileは変えない。OS DPI設定の変更や新しいUI scale設定は追加しない。

固定egui-directx11 0.13.0は頂点・clipにcontextのzoom factorを別途掛ける。FullOutputのpixels-per-pointには既にzoomが含まれるため、runtime adapterは渡す値からzoomを一度除き二重拡大を防ぐ。appの入力・media座標・font生成は完全なpixels-per-pointを使い続ける。補正は固定rendererの契約に閉じ込め、依存更新時は実pixel寸法・clipとpointer hit位置を再検証する。

画像decodeとreading pageの取得はruntimeの単一workerで行う。要求と完了結果はそれぞれ最新1件だけを保持し、新しい要求・media切替・closeでgenerationを更新する。workerはframe間と結果公開前にgenerationを確認し、appも現在generationに一致する結果だけをtexture化する。新しい要求は待機中の古い要求を置換し、UI threadでworkerの終了を待たない。codec内部の単一frame decodeは即座に中断できない場合があるが、workerはwindowやGPU objectを参照しない。

一回の画像・reading要求で保持するRGBA frame列は合計512 MiBまでとし、animationを逐次収集しながら上限を確認する。decoderのscratch、GPU texture、表示切替時の旧画像は別であり、process全体の512 MiB上限を意味しない。超過時は画質を落としたりanimationを途中で切ったりせず明示errorにする。readingの先頭は現在画像を再利用し、失敗した後続pageには位置を保ったerrorを表示する。読み込み中もtab操作とwindow操作ができ、loading/errorを画面へ表示する。

textureの一辺の上限はrendererが実際のD3D feature levelから返す。appはegui contextとwinit inputの両方へ起動時に設定し、texture登録前にも寸法を確認してpanicを防ぐ。外部から開くpathはruntimeのShell互換canonical pathへ統一し、相対pathでもsnapshotの現在項目と一致させる。

### H1 export lifecycle

Open file/folderとSave Asは専用STAでnative dialogを表示し、UIはthreadをjoinせず結果eventを受ける。本体windowをownerに指定して通常のmodal入力制限を保ち、workerがwindowの共有所有権を保持してnative handleの寿命を保証する（[IModalWindow::Show](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-imodalwindow-show)）。同時pickerは1件、picker中も描画・再生を継続し、選択前の保留Open Folderは失効させる。Save As結果は開始時のtab/pathと照合し、Cancel・失敗では書き出さずdirty guardを復元する。native dialogを閉じるまで本体の終了操作は受け付けない。

owner handleはUI threadで取得し、COM objectとSTA cleanupはworker内へ閉じ込める。復帰時はruntimeが現在のclient座標を読み取り、appがeguiのpointer位置を更新する。native dialogがcursor eventを消費しても、pointerを動かさずに次のbuttonをclickできるようにする。

Saveはruntime所有の単一background export jobへimmutableなsource・target・operation snapshotを渡す。appは進捗と完了eventだけを受け取り、再生とUI event loopを継続する。追加exportは現在jobの完了またはcancelまで開始しない。export中の追加編集は保持し、完了時はexportしたoperation列に対応する履歴位置だけをsavedにする。対象tabのclose・detach・folder内移動とprocess終了は、job終了まで保留する。dirty guardからのexportは成功時だけ元の操作を再評価し、cancel・失敗時は編集とguardを保持する。

FFmpegはtargetと同じfilesystemの専用一時directoryへ出力する。成功・非空output・cancel未要求を確認してからrenameでtargetを置換し、失敗・cancelでは既存targetを変更しない。runtimeはFFmpegの進捗pipeとdiagnostic pipeをdrainし、cancel時には子processを終了・回収して一時outputを片付ける。hardware fallbackも同じ一時output内で行う。sourceと同一pathの拒否は維持する。

### M7 advanced presentation and interaction

preview cacheはruntimeがFFmpeg / FFprobeの子processとdisk I/Oを所有し、appへowned RGBA画像とdurationだけを返す。cache keyは正規化path、file size、更新時刻、preview種別と寸法から作り、`%LOCALAPPDATA%\towavue\preview-cache`を64 MiB以内へ古い順に削減する。waveform、duration、hover thumbnailは専用workerで生成し、path付きeventをappへ返すため、古いtabの結果を現在のtabへ適用せずUI threadもblockしない。

grid menuは既存のcommand registryだけをdispatchし、画像・動画・音声ごとの16 commandを`%APPDATA%\towavue\grid.conf`に保持する。cell順は物理keyの`1234/qwer/asdf/zxcv`と固定してclickとkey入力を一致させる。表示・非表示には短いopacity transitionだけを使い、media操作の意味を持つanimationは追加しない。

grid入力はwinit PhysicalKeyのDigit1～4とKeyQ/W/E/R/A/S/D/F/Z/X/C/Vへ対応させ、logical文字やIME確定文字を位置として使わない。Shift/Capsによる文字変化は位置を変えず、Ctrl/Alt/Super付きは通常shortcut側へ渡す。gridの対象keyはeguiのfocus処理より先に扱うが、palette・modal中は横取りしない。paletteを開いたらgridを閉じ、入力欄へ集中させる。keyboard layout・OS IME設定は変更しない。

gridの4列・4行は表示領域から求めた同じcell寸法に固定し、長いcommand名で列を拡張しない。keyと名称はcell内の行数で折返し・省略し、無効項目もhoverで全文を示す。高さが小さいcellではfontを11pxにしてkeyと名称の2行を保つ。設定pathも幅制限付き省略と全文tooltipを使う。通常cellの110×52 logical pxを上限とし、小さいwindowやUI拡大時は縮める。clickもkeyと同様に一回実行して閉じ、fade-out中のcell入力は無効にする。command配置・shortcut・有効条件は変更しない。

window外へdropしたtabは、appが同じexecutableへ現在pathを引数として渡して別processを起動し、起動成功後だけ元tabを閉じる。edit historyをprocess間で暗黙移送せず、dirty tabは既存guardを通す。別windowへの再結合を行うprocess間protocolは設けない。

hardware exportはruntimeがFFmpegのMedia Foundation H.264 encoderへ`hw_encoding=1`を要求し、成功時だけhardware利用として返す。encoderまたはcontainerが非対応なら、同じrequestをM6 software codecで再実行する。appは設定値ではなく`ExportOutcome`の実結果を表示する。

hardware frameはFFmpegのPQ / HLG transfer metadataをruntime内で保持する。rendererは`ID3D11VideoProcessorEnumerator1::CheckVideoProcessorFormatConversion`でsource color spaceからSDR swap-chain color spaceへの変換を確認し、対応時だけ`ID3D11VideoContext1`へ入力・出力color spaceを設定する。未対応時はtyped errorとし、metadataを無視した表示やHDR対応の宣言をしない。10-bit HDR pass-throughは、HDR displayと10-bit UI合成を含む別のarchitecture decisionなしには有効化しない。

## 5. スレッド・同期契約

- demux、video decode、audio decode、WASAPI outputを役割ごとのworkerに分ける。
- packet queueはboundedかつblockingとし、packetを便宜的にdropしない。
- audio sampleは通常再生中にdropしない。decoded video frameだけをpresentation時刻に基づいてdropできる。
- 音声が存在して再生中なら`IAudioClock`をmasterとし、running中にdevice clockが停止した区間だけ単調時計を下限にする。動画のみ、priming中、audio drain後は単調時計とanchorを使う。
- Seekはgeneration更新、demux停止、全queue破棄、`avformat_seek_file`、decoder flush、目標時刻までのdecode/discard、audio/video primingを一つのtransactionとして扱う。
- すべてのworker結果へgenerationを付け、不一致のpacket、frame、eventを破棄する。
- device removal、audio endpoint変更、decode failureはtyped session errorとして扱い、復旧可能な場合だけpipelineを再構築する。

## 6. Explorerのフォルダ並び順

### 意味

「Explorerの並び順」は自然名前順ではない。対象フォルダに対してWindows Explorerが現在適用または保存しているSort Byの列、方向、複数列条件を指す。Name、Date modified、Date created、Size、Typeと、Shell拡張が提供する列を同じ契約で扱う。

### 取得優先順位

1. mediaを開いた時点で、同じfolder PIDLを表示しているExplorer windowを`IShellWindows`から探す。
2. 複数ある場合はforeground window、次に直近active windowを選ぶ。
3. そのwindowのactive Shell viewを`IServiceProvider`、`IShellBrowser`経由で取得し、`IFolderView2`へqueryする。
4. 一致するwindowがなければ、専用STA Shell worker上で非表示`IExplorerBrowser`を対象folderへnavigateする。独自property bag名を設定せず、Explorerと同じ保存済みfolder view/default templateの解決を利用する。読み取りによって設定を書き換えないよう`EBO_NOPERSISTVIEWSTATE`を使う。
5. `GetSortColumnCount`と`GetSortColumns`でsort metadataを取得し、Shell viewが返すitem順を正とする。

registryのExplorer Bagsを直接解析しない。これは非公開の保存形式へ依存し、現在適用中だが未保存のviewとも一致しないためである。

### FolderSnapshot

`FolderOrderProvider`は専用STA worker上で次の情報を持つimmutable `FolderSnapshot`を返す。

- canonical folder PIDLとfilesystem path
- Shell item identityとfilesystem pathを持つordered media items
- `PROPERTYKEY`とascending/descendingを持つsort columns
- source: `LiveExplorerView`、`PersistedShellView`、`Fallback`
- capture generationとtimestamp

Shell viewの列挙順を取得してから、対応mediaだけをfilterする。filmstrip、全種移動、同種移動、audio playlistは同じsnapshotを共有し、個別に再sortしない。

Open Folderで対応mediaが見つからなかった場合はstatusだけを更新し、表示中mediaのsnapshot・tab・編集を保持する。別folderのsnapshotは実際のmedia load時にだけ現在のnavigationへ反映する。

H1ではUIからのsnapshot取得を非同期要求へ変更する。専用STAは最新1件の待機要求と完了結果を持ち、appはgenerationと現在pathで古い結果を拒否する。取得中もmedia表示・tab操作を継続し、同folderなら既存snapshotを保持する。Open Folder要求をwatcherの背景refreshで上書きせず、media切替・最後のtab closeでは古い要求を失効させる。終了時は結果公開を止め、実行中のShell API完了をUIでjoinしない。COMの解放とSTA終了は所有worker上で行う。

フォルダ変更はoverlapped `ReadDirectoryChangesW`で検知し、150 msのdebounce後に新しいsnapshotを作る。現在項目はShell identity、次にcanonical pathで再対応付けし、位置番号だけで保持しない。ExplorerのSort By変更はmedia load時とfilmstripを開く時の再取得へ反映する。

Shell viewの作成・列挙に失敗してもmedia open自体は失敗させない。その場合だけWindows自然名前昇順へfallbackし、診断ログと一時status messageで縮退を明示する。

### 検証条件

M4ではName、Date modified、Date created、Size、Typeの昇順・降順、同値、複数列sort、Explorerが開いている場合と閉じている場合、sort変更後の再読込を自動またはfixture付きintegration testで確認する。

## 7. ライセンスと配布

本体はMIT OR Apache-2.0。FFmpegはGPL/nonfree componentsを無効化した9.0.1のDLLを動的リンクする。配布時には対応するFFmpeg source、build configuration、変更差分、著作権・LGPL表示、第三者license一覧を同じreleaseから取得可能にする。

FFmpeg binaryやsource archiveは、再現可能なbuild・配布工程を定義するmilestoneまでGitへ入れない。
