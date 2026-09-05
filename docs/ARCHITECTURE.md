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

demux、video decode、audio decode、WASAPI outputは独立workerとし、stream別packet queue、decoded output queue、presentation queueをすべてboundedにする。demuxは満杯の一方のstreamだけで他方を直ちに停止させず、各streamに同じ上限のpending packetを持って空きqueueを先に進める。packetはdropせず、presentation時刻に遅れたdecoded video frameだけをdropする。audio workerの完了はvideo workerと独立してWASAPIへ通知し、末尾audio drain後はvideo-only clockへ切り替える。

通常再生は`IAudioClock`をmasterとする。running中のdevice positionが供給停止で進まない場合に限り、audio clientのstart/stopとpauseを追跡した単調時計を下限にして永久停止を防ぐ。Seekはgeneration更新後に旧workerと全queueを破棄し、`avformat_seek_file`、decode/discard、audio/video primingを新しいpipelineで行う。default render endpoint変更とD3D11 device removalはtyped eventとしてappへ渡し、現在位置と新しいendpointまたはD3D11 deviceでpipeline全体を再構築する。

### M5 image presentation

静止画はEXIF orientation適用後、アニメGIF、WebP、APNGは合成済みRGBA frameと10 ms以上のdeadlineへ変換する。app event loopは次frame時刻までsleepし、期限を過ぎたframeを追いつかせてからegui textureを更新する。画像textureも動画・UIと同じD3D11 deviceとback bufferへ描画し、Presentは一回に保つ。

selectionは元画像に対する正規化矩形として保持し、表示scaleから独立させる。左dragで作成、辺dragで変形、Shift付き作成で画像pixel上の正方形、Shift付き辺dragで現在比率を保持する。右dragはpan、Ctrl+wheelはpointer anchorのzoom、選択範囲clickとCtrl+Yはpixelを変更しないcrop previewである。実crop、undo、save/exportはM6まで開始しない。

reading modeは表示専用で、同じ`FolderSnapshot`から現在画像以降の画像だけをShell view順のまま2～10 page取得する。横・縦配置と表示順反転はpresentation状態だけを変更し、個別画像のselectionや編集状態を作らない。

### M6 non-destructive editing and export

`EditHistory`は適用済みcursorとsaved cursorを別に持つ。新しいoperationをundo位置から追加した場合はredo branchを破棄し、破棄されたbranchにsaved cursorがあれば保存済みidentityも失効する。tab titleとwindow titleの`*`およびstatusのUnsavedは、現在cursorとsaved cursorが一致するまで消えない。folder内移動は同じtabの履歴を破棄するためcloseと同じguard対象だが、tab切替は履歴を保持するためguardしない。

exportはruntimeだけが`ffmpeg.exe`を子processとして起動し、app/coreへFFmpeg型を公開しない。画像filterはoperation順のcrop / transpose / flip、動画filterはそれらとtrim / PTS rate、音声filterはatrim / PTS / atempo / volumeを適用し、metadataを入力からcopyする。2倍を超える、または0.5倍未満のrateは複数の`atempo`へ分解する。video/audio encodeは固定FFmpeg buildのsoftware codecを使い、hardware encodeはM7まで行わない。Save As後のSaveは同じexport先を更新できるが、sourceと同一pathへの出力は拒否してpartial overwriteによるsource破損を避ける。

### H1 export lifecycle

Saveはruntime所有の単一background export jobへimmutableなsource・target・operation snapshotを渡す。appは進捗と完了eventだけを受け取り、再生とUI event loopを継続する。追加exportは現在jobの完了またはcancelまで開始しない。export中の追加編集は保持し、完了時はexportしたoperation列に対応する履歴位置だけをsavedにする。対象tabのclose・detach・folder内移動とprocess終了は、job終了まで保留する。dirty guardからのexportは成功時だけ元の操作を再評価し、cancel・失敗時は編集とguardを保持する。

FFmpegはtargetと同じfilesystemの専用一時directoryへ出力する。成功・非空output・cancel未要求を確認してからrenameでtargetを置換し、失敗・cancelでは既存targetを変更しない。runtimeはFFmpegの進捗pipeとdiagnostic pipeをdrainし、cancel時には子processを終了・回収して一時outputを片付ける。hardware fallbackも同じ一時output内で行う。sourceと同一pathの拒否は維持する。

### M7 advanced presentation and interaction

preview cacheはruntimeがFFmpeg / FFprobeの子processとdisk I/Oを所有し、appへowned RGBA画像とdurationだけを返す。cache keyは正規化path、file size、更新時刻、preview種別と寸法から作り、`%LOCALAPPDATA%\towavue\preview-cache`を64 MiB以内へ古い順に削減する。waveform、duration、hover thumbnailは専用workerで生成し、path付きeventをappへ返すため、古いtabの結果を現在のtabへ適用せずUI threadもblockしない。

grid menuは既存のcommand registryだけをdispatchし、画像・動画・音声ごとの16 commandを`%APPDATA%\towavue\grid.conf`に保持する。cell順は物理keyの`1234/qwer/asdf/zxcv`と固定してclickとkey入力を一致させる。表示・非表示には短いopacity transitionだけを使い、media操作の意味を持つanimationは追加しない。

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

フォルダ変更はoverlapped `ReadDirectoryChangesW`で検知し、150 msのdebounce後に新しいsnapshotを作る。現在項目はShell identity、次にcanonical pathで再対応付けし、位置番号だけで保持しない。ExplorerのSort By変更はmedia load時とfilmstripを開く時の再取得へ反映する。

Shell viewの作成・列挙に失敗してもmedia open自体は失敗させない。その場合だけWindows自然名前昇順へfallbackし、診断ログと一時status messageで縮退を明示する。

### 検証条件

M4ではName、Date modified、Date created、Size、Typeの昇順・降順、同値、複数列sort、Explorerが開いている場合と閉じている場合、sort変更後の再読込を自動またはfixture付きintegration testで確認する。

## 7. ライセンスと配布

本体はMIT OR Apache-2.0。FFmpegはGPL/nonfree componentsを無効化した9.0.1のDLLを動的リンクする。配布時には対応するFFmpeg source、build configuration、変更差分、著作権・LGPL表示、第三者license一覧を同じreleaseから取得可能にする。

FFmpeg binaryやsource archiveは、再現可能なbuild・配布工程を定義するmilestoneまでGitへ入れない。
