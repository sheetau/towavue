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
| Window/Event | winit 0.30.12 | Win32の詳細へ降りられる薄いイベント境界 |
| UI | egui 0.35.0 | 最小UIとカスタム描画を少ないコードで構築できる |
| UI renderer | egui-directx11 0.13.0 | wgpuを介さずD3D11 render targetへ描画できる |
| Windows API | windows 0.62.2 | COM、D3D11、DXGI、WASAPI、Shell APIの公式Rust projection |
| Media | FFmpeg 9.0.1 / ffmpeg-next 9.0.0 | demux・codec・resampleの広い対応範囲 |
| Graphics | D3D11 + DXGI Flip Model | decode surfaceからpresentationまで同じdeviceを維持できる |
| Audio | event-driven WASAPI Shared | 他アプリと共存しつつ十分に低遅延 |

依存は使用を開始するmilestoneでのみ追加し、`Cargo.lock`へ固定する。`egui-directx11`は薄いadapter内に閉じ込め、必要ならUIやmedia境界を変えずに自前rendererへ交換できるようにする。

`eframe/wgpu`は採用しない。D3D11VAが返すNV12/P010等の外部・マルチプレーナresourceをD3D12側へ安全に取り込む経路が中核リスクになるためである。独自UI toolkitも、文字入力、DPI、accessibility、複数windowまで自前化するコストに見合わない。

## 3. Workspace境界

### towavue-core

OS非依存の値、状態遷移、コマンド、編集履歴を置く。unsafe、COM、FFmpeg、native handleを禁止する。

将来の中核型は次のとおり。

- `MediaTime(i64)`: ナノ秒単位の時刻。
- `SessionId`: 開いているmedia sessionの識別子。
- `PlaybackGeneration(u64)`: Seekや再openの前後を識別し、古い結果を破棄する番号。
- `MediaCommand`: open、play、pause、seek、rate、volume、stop、close。
- `MediaEvent`: opened、state、position、ended、fault。
- `MediaInfo`: 種別、duration、stream、codec、解像度、sample rate、色空間。
- `CommandId` / `CommandContext`: menu、palette、shortcutが共有するcommand identity。

### towavue-runtime-windows

FFmpeg、D3D11/DXGI、WASAPI、Windows Shell、worker、queue、clockを所有する。FFI/COMとunsafeはこのcrateから外へ出さず、安全なcommand/event境界へ変換する。

GPU frameはruntime内部のRAII `FrameLease`で保持する。FFmpegの`AVFrame`、`ID3D11Texture2D`、array slice、COM pointerをappやcoreへ公開しない。

### towavue-app

winit event loop、egui、tab、command dispatch、利用者向け状態を所有する。runtimeへcommandを送り、eventと描画結果だけを受け取る。

## 4. 再生・表示契約

- app event loopがruntimeの安全なfactoryを呼んでD3D11 deviceを作成し、M2ではFFmpegの`AVD3D11VADeviceContext`へ正しいCOM参照寿命で渡す。appへCOM pointerは公開しない。
- decode、D3D11 Video Processor、DXGI presentation、eguiは同じadapter/deviceを使う。
- immediate/video contextの利用箇所を限定し、FFmpegとの共有に必要なmultithread protectionを有効にする。
- SwapChainはFlip Discard、2～3 buffers、frame-latency waitable objectを使う。
- FFmpeg D3D11VA surfaceをVideo Processorへ直接渡す。hardware pathでCPU readbackや再uploadを行わない。
- hardware decodeが成立しない場合だけsoftware decodeへfallbackする。CUDA/QSV decode fallbackは設けない。
- HDR metadataは失わないが、正しいtone mapping/pass-throughが完成するまではHDR対応を宣言しない。

### M1 software path

M1ではFFmpegを動的リンクし、映像をsoftware decodeしてtightly packed RGBAへ変換する。runtimeの`FrameRenderer`が単一D3D11 device、immediate context、2-buffer Flip Discard swap chainを所有し、CPU frameをback bufferへuploadしてpresentする。このCPU uploadはM1だけの基準経路であり、M2のhardware pathでは使用しない。

音声はsource sample rateのinterleaved stereo `f32`へ変換し、専用MTA thread上のevent-driven WASAPI Shared clientへ渡す。映像queueは2 frame、音声channelは32 chunk、WASAPI手前の蓄積は約2秒へ制限する。M1のdecode workerはdemuxとvideo/audio software decodeを直列実行するが、COM、FFmpeg型、native frame handleはruntime外へ出さない。M3の役割別workerでも、この安全なcommand/event境界を維持する。

### M2 D3D11VA path

`FrameRenderer`が作成したD3D11 deviceのopaqueな`GraphicsDevice`参照をdecode workerへ渡す。FFmpeg用`AVD3D11VADeviceContext`にはcloneしたCOM参照の所有権を移し、`AVCodecContext`が`AVBufferRef`とともに解放する。immediate contextにはmultithread protectionを有効にする。

hardware frameはruntime内部のFFmpeg `AVFrame`がD3D11 texture arrayとsliceを保持し、bounded presentation queueを経て同じdeviceの`ID3D11VideoProcessor`へ渡す。appが受け取るのはpresentation timeとeventだけで、COM pointer、FFmpeg frame、texture handleは公開しない。hardware pathではmap、readback、software scaling、back-buffer uploadを行わない。

codec metadata、device、driverのいずれかがD3D11VAを成立させられず、まだhardware frameを公開していない場合だけ入力を開き直してM1 software pathへfallbackする。最初のhardware frame後のdecode errorはfallbackで隠さずsession faultとする。終了時にadapter LUID、hardware frame count、CPU transfer countを記録する。

### M3 synchronization and recovery

demux、video decode、audio decode、WASAPI outputは独立workerとし、stream別packet queue、decoded output queue、presentation queueをすべてboundedにする。demuxは満杯の一方のstreamだけで他方を直ちに停止させず、各streamに同じ上限のpending packetを持って空きqueueを先に進める。packetはdropせず、presentation時刻に遅れたdecoded video frameだけをdropする。audio workerの完了はvideo workerと独立してWASAPIへ通知し、末尾audio drain後はvideo-only clockへ切り替える。

通常再生は`IAudioClock`をmasterとする。running中のdevice positionが供給停止で進まない場合に限り、audio clientのstart/stopとpauseを追跡した単調時計を下限にして永久停止を防ぐ。Seekはgeneration更新後に旧workerと全queueを破棄し、`avformat_seek_file`、decode/discard、audio/video primingを新しいpipelineで行う。default render endpoint変更とD3D11 device removalはtyped eventとしてappへ渡し、現在位置と新しいendpointまたはD3D11 deviceでpipeline全体を再構築する。

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

将来の`FolderOrderProvider`は、次の情報を持つimmutable `FolderSnapshot`を返す。

- canonical folder PIDLとfilesystem path
- Shell item identityとfilesystem pathを持つordered media items
- `PROPERTYKEY`とascending/descendingを持つsort columns
- source: `LiveExplorerView`、`PersistedShellView`、`Fallback`
- capture generationとtimestamp

Shell viewの列挙順を取得してから、対応mediaだけをfilterする。filmstrip、全種移動、同種移動、audio playlistは同じsnapshotを共有し、個別に再sortしない。

フォルダ変更は`ReadDirectoryChangesW`で検知し、debounce後に新しいsnapshotを作る。現在項目はShell identity、次にcanonical pathで再対応付けし、位置番号だけで保持しない。

Shell viewの作成・列挙に失敗してもmedia open自体は失敗させない。その場合だけWindows自然名前昇順へfallbackし、診断ログと一時status messageで縮退を明示する。

### 検証条件

M4ではName、Date modified、Date created、Size、Typeの昇順・降順、同値、複数列sort、Explorerが開いている場合と閉じている場合、sort変更後の再読込を自動またはfixture付きintegration testで確認する。

## 7. ライセンスと配布

本体はMIT OR Apache-2.0。FFmpegはGPL/nonfree componentsを無効化した9.0.1のDLLを動的リンクする。配布時には対応するFFmpeg source、build configuration、変更差分、著作権・LGPL表示、第三者license一覧を同じreleaseから取得可能にする。

FFmpeg binaryやsource archiveは、再現可能なbuild・配布工程を定義するmilestoneまでGitへ入れない。
