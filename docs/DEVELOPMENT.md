# towavue 試用・開発ガイド

この文書は、M7までの開発版を実際に触り、UI/UXの観察を次の小さな変更へつなげるための入口である。完成品向けの利用説明ではない。既知の制約は[KNOWN_GAPS.md](KNOWN_GAPS.md)、固定された技術境界は[ARCHITECTURE.md](ARCHITECTURE.md)、作業順は[ROADMAP.md](ROADMAP.md)を参照する。

## 1. 最初に試す

### OS text clipboard連携（2026-09-06 20:24 JST）

- ownerがOS clipboardへの試験書込みを明示許可した後に実施。既存内容は読み取らず、試験文字列`towavue clipboard 日本語 café 🎞️`で置換した。PowerShellから書込み、通常releaseのWelcomeでCtrl+Shift+P→Ctrl+Vを行い、検索欄の文字表示をcaptureで確認した。
- Ctrl+A後、OS clipboardを別のsentinelへ置き換えてからCtrl+Cを入力し、外部processの読取りで元のUnicode文字列と完全一致した。再度sentinelへ置き換えてCtrl+Xを入力すると、OS側は同じ文字列、検索欄は空になる。外部から`Open`を書いてCtrl+Vすると候補がOpen file/Open folderへ絞られ、commandは実行せずWelcomeを保持した。最後にclipboardには`Open`を残した。
- featureなしの比較build（所有PID 38864、開始UTC 2026-09-06T11:23:10.6270873Z）では同じUnicode paste後も検索欄が空のままだった。比較後はfeatureを戻してoffline再buildし、Cargo.lockのSHA-256が比較前と同一であることを確認。最終release（PID 29364、開始UTC 2026-09-06T11:24:05.0666986Z）で上記全手順を再実施して通過した。先行feature試験PID 8172を含め、3 windowsとも所有権/foregroundを確認し、正常終了・stderr空、Save/元素材/OS設定の変更なし。
- 最終binary SHA-256: `394617AF866B14D89006F64FF576326C7D0A59F21818CC65077DA6E114628087`。比較binary: `4A243E42168F027EB192429CAA7B16E708B127B51BFC76BEA7A824F3BBC28868`。960×576、現在機のWindows build 26200、注入keyによる試験。captured PNGとlogはignoredの`target/tmp/h1-clipboard-*`。物理keyboard、他IME、clipboard占有時の競合、画像copyの証明ではない。
- 回帰testは同じASCII/Unicodeのpaste・select-all・copy・cut・再pasteをegui入力/outputで検査し、OS clipboardには触れない。240 tests（app 125/core 36/runtime 75/integration 4）とformat・Clippy・両buildが通過。既存live ignore 3件とその他のH1 gateは残る。

### 画像エラーからの継続操作（2026-09-06 19:56 JST）

- 8796ad8の通常release、所有PID 43832（開始UTC 2026-09-06T10:51:42.3549463Z）、960×576で実施。所有folder内に不正なPNG signatureの`01-broken.png`と正常な赤600×800の`02-good.png`を置き、前者から起動した。
- FaultedでもBのreading modeでは先頭errorの位置を保ち、隣の正常画像を表示する。Rightで正常画像へ移動し、Bで通常表示へ戻るとPausedとなり、旧errorは残らない。Leftで不正fileに戻った後、そのscratchだけを正常PNGへ置換してRight→Leftすると、同じprocessで修復済み内容を読み直してPausedになる。Ctrl+Wで最後のtabを閉じたWelcomeにもerror表示は残らなかった。
- 操作/captureは対象PIDの開始時刻とforegroundを確認した。windowはWelcomeから通常終了、Save・元素材・OS設定の変更はない。差替えたscratchは元の不正内容へ戻した。stderrの3件は初回・reading再取得・再訪の意図したdecode失敗で、修復後の追加errorはない。captures/logはignoredの`target/tmp/h1-image-error-*`。
- 自動回帰は隔離folder内の不正BMPと2×1の正常BMPで、実ImageLoaderの完了を待ち、error状態の正常reading page mesh、前後command、修復後の正確なRGBA、旧error解除と最終tab終了を検証する。Shell順は固定snapshot fixtureであり実Explorerの再検証ではない。最初のtestは描画shapeをRectと誤認して失敗したため、実際のMeshを検査するよう修正した。app動作の不具合としては扱わない。
- 挙動修正・architecture変更は不要だった。239 tests（app 124/core 36/runtime 75/integration 4）・format・Clippy・両buildが通過。既存live test 3件のignore、全codec/破損形式、実DPI/入力deviceの未検証をこの結果で埋めない。native試験binary SHA-256は`38C2D611B88C5C47BB087801C735CED6BF315B92C32EA087FD945CD1767427FD`、正常PNGは`5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`。

### 画像端でのShift選択比率（2026-09-06 19:46 JST）

- Shiftで正方形を作る処理が両軸を独立にclampし、画像端で長方形になる問題をcore testと通常release PID 44696で再現した。600×800の赤PNGを960×576で表示し、window内(630,140)→(650,440)をShift dragすると右端の細長い選択になった。Shift付き辺resizeも直交軸だけのclampで比率と中心が変わり、変更前のapp testが失敗した。
- 正方形作成は始点から両方向へ確保できる共通のpixel寸法で制限する。辺resizeは反対側の辺と直交中心を保ち、画像端に収まる最大寸法で止める。drag前の選択比率を使い、一度幅/高さがzeroになっても戻す操作で比率を復元できる。非Shiftの操作、取消、crop/exportは変更しない。release時の既存整数/動画偶数pixel整列による丸めは残る。
- core回帰は縦長/横長画像、全4方向と画像外pointerで正方形・始点・境界を検証。app回帰は全4辺と縦横寸法で、zeroへ縮小後の再拡大でも比率・固定辺・中心・境界を維持する。既存の疎な入力、取消/focus/overlay、1 pixel選択、crop制約の回帰も通過。238 tests（app 123/core 36/runtime 75/integration 4）・format・Clippy・両buildが通過し、3件の既存live ignoreは実device証明ではない。
- 最終通常release PID 47484（開始UTC 2026-09-06T10:45:34.4260795Z）では同じShift dragが右端に収まる正方形になる。Escapeで解除後、(400,80)→(500,180)の通常選択を作り、右辺中央(500,130)→(650,130)のShift dragで上端へ接する正方形のまま止まる。left辺と垂直中心は保持された。前段PID 43360も同形状を確認したが、最終確認はratio基準修正後の47484を使う。
- foregroundを確認し、注入Shiftはfinallyで解除した。全3所有windowはclean titleで通常終了、Save/source/OS設定変更なし。captures/空のstderr logはignoredの`target/tmp/h1-selection-*`。物理pointer/keyboardやmixed-DPI matrixの代替ではない。
- 最終binary SHA-256: `38C2D611B88C5C47BB087801C735CED6BF315B92C32EA087FD945CD1767427FD`。baseline binary: `3AE8785A7B8F5F9907966A0A419EFBBA7482A333F46564A79C035CC7B0753D26`。赤PNG: `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`。

### アニメ画像の長いdeadline遅延（2026-09-06 19:35 JST）

- 既存のframe追従loopは遅れた全周回を一枚ずつ数えていた。1×1の3 frames、delay 10/20/30ms、2日＋35msの時刻差を同じrelease testへ注入すると、追従関数が修正前20.918ms、修正後0.005msだった。実OSスリープ/復帰や画面latencyの試験ではなく、合成時刻でUI側計算を切り分けた単発値である。一周して同じframeへ戻る際の不要なtexture uploadも変更前に回帰testが失敗した。
- 最初の一周から周期を求め、整数nanosecondの剰余で完全な周回を飛ばす。最大二周ぶんのframe探索となり、delay値や位相は変更しない。同じframe番号なら次の期限だけ更新し、texture変換/uploadと画像由来のredrawは行わない。新worker・cache・時計変更はない。
- 回帰は2日/3650日のgap、異なる初期frame、nanosecond端数のある3種類のdelay、deadline直前/一致/直後、完全周期の境界を確認。短い範囲では従来のframe逐次計算を独立した参照として、最終frame・正確な次期限・upload件数を照合する。235/236件目のtestsを含む計236 tests（app 122/core 35/runtime 75/integration 4）、format、Clippy、両buildが通過。3件の既存live ignoreは実device確認ではない。一時計測出力は最終sourceから除去した。
- 通常releaseの所有PID 46052（開始UTC 2026-09-06T10:34:47.2798406Z）で640×360・10 frames/2秒の`page-03.gif`を表示。foregroundを確認した2枚のcaptureでframeが進むことを目視し、clean titleで通常終了した。Save/source/OS設定の変更なし。実スリープ、長時間のOS停止、物理入力/全codecの代替証拠にはしない。
- native binary SHA-256: `3AE8785A7B8F5F9907966A0A419EFBBA7482A333F46564A79C035CC7B0753D26`。GIF SHA-256: `EC4B85BCE7C7E44BD85FD45F7F0FC076B97AFBD5329AB8840C66930AF7A350EB`。captureと空のstderr logはignoredの`target/tmp/h1-animation-*`。

### 初回画像の画面用変換（2026-09-06 19:27 JST）

- 6000×6000 PNGのRGBA→egui変換を一時exampleで比較した。全画像のalpha事前走査は末尾だけ半透明の条件で約31→37msへ悪化したため不採用。行単位のopaque判定では、6回の計測が不透明31.427～32.069→19.261～20.341ms、末尾だけ半透明31.677～33.725→19.409～20.634ms、全体半透明46.548～48.214→39.486～40.456msで、全画素が従来変換と一致した。このexampleは削除し、最終コードは固定Rustのlintに合わせて4-byte配列sliceを使う。
- alpha=255だけの行はbyte値をそのままColor32へ渡し、混在行は既存eguiの変換を使う。独自premultiply/丸め、画質変更、decode/thread/cache変更はない。alpha全256値、複数RGB値、幅1/3/256/257、全opaque・行末の半透明・先頭の透明を従来ColorImage全体へ照合し、変更前後とも一致。現在animation frameとreading pageのgraphics復旧testも通過した。
- native比較は基準機・960×576・通常release。既存6000×6000 PNGを異なる5 pathへコピーし、小画像から一方向に初めて開く。対象HWNDへRightのkeydown/upを一回ずつ送り、2ms間隔でfile名とPaused titleを確認し、各完了後500ms空ける。appのdecode/texture cacheにはないがOS file cacheはwarmであり、cold-storage・初回process起動・GPU Present/物理表示の測定ではない。

| 初訪問の大画像5枚 | 最小 | 中央値 | 最大 |
| --- | ---: | ---: | ---: |
| 5d1d529、PID 372 | 222.005ms | 233.814ms | 235.034ms |
| 最終行単位変換、PID 29536 | 217.178ms | 219.187ms | 219.640ms |

- foreground確認済みの最終画像captureは表示領域498×498＝248,004 pixelsが完全一致。lint調整前の中間PID 24964も一致したが、表は最終binaryの測定だけを使う。private memoryの5点は最終674.33～1136.78MiBで、allocator/texture lifecycleを含む瞬間値。cache上限やメモリ削減、全codec・透明度配置での速度保証ではない。decodeとGPU uploadは依然必要で、初回切替を無停止にする変更ではない。
- 全3所有windowはclean titleで通常終了し、Save・source変更・OS設定変更なし。helper/captures/空のstderr logはignoredの`target/tmp/h1-image-first*`。234 tests（app 120/core 35/runtime 75/integration 4）・format・Clippy・両buildが通過。既存live test 3件はignoreのままで実deviceの証明ではない。
- 最終PID開始UTC: 2026-09-06T10:26:52.4330466Z。最終binary SHA-256: `1336B3EEF661A5DAC75754C4BBCCFEABC84C82D2AD30BC690B5DF2ADB2D6A922`、baseline: `4BDD4B90BC94AEC6B5C11D14E85C5B6F69C577D8384F95E021B5BB001DCEDCA1`。大画像5枚はすべてSHA-256 `7456E01DB2237E3D4F120F9EE9A9B50DCC0019CB87AA31C37C1636FAD6538243`。

### 静止画の再訪texture再利用（2026-09-06 19:18 JST）

下記decode cache試験と同じ素材・通常release・960×576・5往復・title判定で、app側のtexture再利用を比較した。file名とPaused titleの更新はGPU Presentより先に起こりうるため、物理表示完了時間ではない。

| 大画像へ戻る5回 | 最小 | 中央値 | 最大 |
| --- | ---: | ---: | ---: |
| 9be0d14、decode cacheのみ、PID 41676 | 48.423ms | 49.705ms | 49.819ms |
| texture再利用、PID 42824 | 15.695ms | 16.500ms | 18.372ms |

- 別の一時instrumented baseline PID 45724では、6000×6000 textureを含むUI renderからPresentまでが初回68.007ms、再訪5回30.626～71.356ms。純粋な転送時間ではないが、再decode以外の反復作業を確認した。traceは除去して最終releaseをbuildし、最終logにtrace/errorはない。
- 同じDecodedImageのArc identityに限りTextureHandleを再利用する。最大8件・RGBA相当256 MiBのLRUで、animation・容量超過・失敗は保持しない。decode側と画素を共有するがGPU resourceとcacheの所有範囲は別であり、process全体の上限ではない。graphics復旧開始時にはcacheを破棄し、現在画像/reading pageだけ既存経路で再uploadする。
- cache hitでも別textureになる変更前の回帰を確認し、修正後は同じIDかつupload deltaなし。新decodeの同path、LRU、byte/件数制限、animation/容量超過、clear、復旧失敗時のcache破棄と現在画像復元を検証。233 tests（app 119/core 35/runtime 75/integration 4）・format・Clippy・debug/release buildが通過。既存live test 3件のignoreは実device確認の代替ではない。
- foregroundを毎sample確認する単一点screen samplerの暖機後3回では、baseline PID 36344の緑への変化が107.932～151.150ms、固定PID 42824が20.278～36.245ms。GDI/DWM取得自体に待ちがあり、物理入力から表示までの精密測定ではない。最初のbaseline helperは緑255を要求して実色254を取り逃したため、そのtimeoutをapp障害やlatency値に含めない。同じPIDで許容差をRGB各2へ修正して再測定した。
- 最終PIDのcached表示、31回のRight keydown/up後の正しい大画像、所有scratchを赤600×800から青320×240へ置換した後の新しい表示をcaptureで確認。大画像title時のprivate memoryは556.90～557.16MiB（5点）であり、GPU memoryやtransient peakを測ったものではない。初回decode/変換/uploadはこの変更では省かれない。
- 全3所有windowはclean titleで通常終了し、Save・OS設定変更はない。差替えscratchは元の赤PNGへ戻し、原素材は変更しない。helper/log/captureはignoredの`target/tmp/h1-image-present*`、`h1-image-upload*`、`h1-image-texture*`。最終PID開始UTCは2026-09-06T10:11:52.6111128Z、binary SHA-256は`4BDD4B90BC94AEC6B5C11D14E85C5B6F69C577D8384F95E021B5BB001DCEDCA1`。素材hashは下記と同一。

### 静止画の再訪decode cache（2026-09-06 18:56 JST）

基準機・通常release・960×576で、6000×6000 PNG（RGBA 144,000,000 bytes）と600×800 PNGを5往復した。同一素材・同じkeydown/upを対象HWNDへ一回ずつ送り、次のfile名とPaused titleを2ms間隔で確認してから300ms空ける。これは画像decode/画面用変換の完了通知までの比較で、Present・DWM/physical displayまでの入力遅延ではない。

| 大画像へ戻る5回 | 最小 | 中央値 | 最大 |
| --- | ---: | ---: | ---: |
| 3fddd7a、PID 42724 | 218.537ms | 220.651ms | 221.243ms |
| 最終cache実装、PID 41676 | 48.423ms | 49.705ms | 49.819ms |

- 一時的なcomponent計測では大PNGのdecodeが182.907～186.436ms、egui向け画素変換が32.542～35.758ms（各5回）。component計測用exampleは削除し、通常buildで最終測定した。初回decodeやGPU uploadの高速化を証明したものではない。
- 同じimage workerに8件・256 MiBの静止画cacheを置き、Arcでowned RGBAをappと共有する。file size/更新時刻をdecode前後で確認し、変更/取得失敗で旧entryを捨てる。animation・大容量・失敗はcacheしない。cache hitも要求全体の512 MiB判定を通し、古い要求の破棄とwindow/GPUを持たないworker寿命を維持する。
- native PID 37064の試用では、cache済みの所有fixtureだけを赤600×800から青320×240へ差し替えて移動し、新画像を表示した。元fixtureは変更しない。最終PID 41676の初回/cached表示は、foregroundをassertして取得した498×498＝248,004 pixelsが一致した。旧baseline captureはforeground不成立で別windowが写っており、表示比較の証拠から除外した。
- 最終runの大画像title完了時private memoryは694.02～791.10MiB。allocator・UI texture等を含む粗い瞬間値であり、256 MiBはcache自身のRGBA上限に限る。初回cold-storage、他codec、大量画像・長期連続利用は別途評価する。
- 全3試用windowはclean titleで通常終了し、SaveやOS設定変更はない。scratchの小画像は測定元の赤PNGへ戻した。helper/log/captureはignoredの`target/tmp/h1-image-switch*`。232 tests・format・Clippy・debug/release buildが通過し、3件の既存live testはignoreのまま。

最終測定SHA-256:

```text
towavue.exe: 8B3272860DE3CA01D5C579F386291A43880490F7453F2D00C67BD9CACB1CA88A
01-large.png: 7456E01DB2237E3D4F120F9EE9A9B50DCC0019CB87AA31C37C1636FAD6538243
02-small.png: 5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C
```

### Reading modeの見開き連結（2026-09-06 18:45 JST）

- 基準機・960×576の通常releaseで、同じ320×240の赤/緑PNGを開きBを押した。旧PID 20568では中央8pxの隙間と外周の固定余白があり、縦横比が異なる画像の実描画testでは全体の中心もずれた。
- 修正後PID 40264は同じ横並びが隙間なく中央に収まり、Rの縦並び、H反転と480×300へのresizeでもmedia領域内に収まる。folder seekのhoverは赤/緑を連結したpreviewとなり、Fの通常filmstripでも縦横比を保つ。ページ送り・source・編集状態は変えていない。
- 横では高さ、縦では幅を揃え、全体をaspect-fitする同じ配置を本表示とhoverで使用。画像だけのpreview cacheをpaddingなしのv4へ更新し、旧v3の画像を再利用しない。動画/音声のcache・生成経路と64 MiB上限は維持する。極端な縦横比の低解像度previewでは整数pixelへの丸めは残る。
- 変更前に失敗する実描画回帰、縦横/反転/通常・fullscreen flag/3寸法、異なる画像比率、空/1/10枚の配置、失敗page表示、hoverと通常filmstrip、旧cacheからの再生成を確認。230 tests・format・Clippy・debug/release buildが通過。3件の既存live testはignoreであり実device検証の代用ではない。
- 両windowはclean状態で通常終了し、SaveやOS設定変更はしていない。固定版の静止後5秒CPU時間増分は0ms（時計分解能以下）。capture/logはignoredの`target/tmp/h1-reading-joined-*`。測定release SHA-256は`39D3B4E1AD8B3318ACFAB20D8A7EB429D2B3518AF37C7886889CB941A50370B2`。物理入力・mixed-DPIや見開き単位のnavigationの証明ではない。

### 現行releaseの30分性能再確認（2026-09-06 18:30 JST）

51b43bfの通常release（開始時HEAD aa83efd、以降は文書のみ）で、4K H.264/AACの30分連続再生がEOFへ到達した。adapterは00000000:000146b5、D3D11VA、960×576、1倍、アプリ内mute。sourceは事前hash済みでcold-storage試験ではない。再起動・Seek・並行したbuildや重い試験は行っていない。

- 107,771 hardware frames＝107,771 presented＋0 droppedでsourceの全video frameと一致し、CPU transferも0。全区間drop率0%のため、先頭10分も0.1%未満となる。A/V driftはp95 4.808ms・最大32.055msで、30分の40/100ms基準内。アプリのvideo PTSとaudio master時計の差であり、物理display/speaker遅延の測定ではない。
- 約30秒間隔の61 samplesで同一PID/start timeを追跡。開始5分後からEOF直前までの50 samples（309.5～1780.2秒）はprivate memory 221.86～235.78MiB、区間最初222.33・最後223.40MiB。EOF後1810.2秒は178.62MiBで、後の5秒CPU時間増分は0ms（時計分解能以下）。粗いprocess-memory sampleであり、GPU allocationや全瞬間のpeak・無期限の安定性の証明ではない。
- PID 46632、開始UTC 2026-09-06T09:00:31.6472465Zの同一processで完走。最終統計とEndedを確認後、試験用muteを一回Undoし、clean titleを確認して通常終了した。source保存・OS設定変更なし。binary/source SHA-256は前後一致。raw manifest/log/sample/captureはignoredの`target/tmp/h1-current-30m*`にある。
- 17:05の100回Seek測定とは別試験。今回の結果はこの基準機・codec・通常windowでの長時間/dropゲートを再確認したもので、他codec、実device復旧、物理入力、mixed-DPI、配布ゲートの代替ではない。

測定対象のSHA-256:

```text
towavue.exe: 18F57AF2E2E7D1FCF34D64E70154080A79E661F4B02297AEE27491865F404247
m3-4k60-30m.mp4: FEE0E738E7149225A7B4DEA02CDA75AAE6873288CBE5A1077B101829ADFD0C10
```

### 現行releaseの性能再確認（2026-09-06 17:05 JST）

4b7721bの通常releaseを再buildし、同じ基準機・960×576・1倍・アプリ内muteで測定した。preview/末尾Seek変更後の短時間確認であり、30分ゲートの再証明ではない。4b7721bと9fe13a5のCIは成功している。

| 1080p H.264/AAC、5秒Seek | 完了数 | p50 | p95 | 最大 |
| --- | ---: | ---: | ---: | ---: |
| 再生中 | 100/100 | 84.094ms | 102.204ms | 109.049ms |
| 停止中 | 100/100 | 28.255ms | 42.260ms | 55.786ms |

- 120.030729秒、1920×1080/30fpsの同一素材・PID 8668で前進10回/後退10回を5往復した。各操作の完了を待ち、PID/start time/foregroundと完了数を確認。計測はアプリのSeek受付からPresent成功までで、key注入やOS配送・物理display遅延は含まない。p95は昇順95番目。両modeとも300ms基準内。
- 60.006771秒の3840×2160/60fpsをPID 31228で無Seek連続再生。sourceの3,594 framesと表示3,594が一致し、drop 0・CPU transfer 0、drift p95 4.682ms・最大4.979ms。adapterは00000000:000146b5。並行したbuildや重い試験は行っていない。
- private memoryは開始1.3秒で266.83MiB、31.4秒で206.11MiB、EOF後61.4秒で184.02MiB。OS peak paged memoryは283.79MiB。30秒間隔の粗いsampleであり、GPU memoryや長期保持量の証明ではない。
- EOF後5.052秒でCPUが468.75ms増加した。別の同解像度10秒素材・PID 24784ではEOF後の5秒sampleが156.25→78.125→0ms、後の5.060秒も0msとなった。private memoryは184.26→171.01MiBへ減少。持続的busy loopは再現しないが、最初のCPU増加の原因を特定したものではない。
- 三つの試用windowはmuteをUndoしてclean状態で通常終了。sourceのSHA-256は前後一致。source保存・OS変更なし。raw log/sample/captureはignoredの`target/tmp/h1-current-*`にある。

測定対象のSHA-256:

```text
towavue.exe: BCC1B1D4438F8F0EB409DF07F80DEE331291526D7C13EF4F509B8A1C32870F97
m3-1080p-h264-120s.mp4: DC645595A1165506BF5C3E685B14D7EA3B0116BBDFE74839E7DA5834CF60DA0C
m3-4k60-60s.mp4: 455BDB1844E4ACDE00DB792BACBF099CC528038DF38313729BAD2DA95D99C0DA
```

### Frame内のwheel音量配送（2026-09-06）

- 旧コードのdraw_ui回帰は「一覧上でwheel→音量表示へ移動」で音量変更を発行して失敗する。逆順、一覧と音量の両方でwheelを含む列も、各eventの位置へ割り当てる修正後に通過する。
- 前frame末尾を次frame先頭のwheel位置に使い、PointerGoneで失効する。動画面/status相当の複数targetを一回合算し、重複領域を二重計上しない。eguiのdiscard後のpassではwheel eventが空になることを実測し、最初のpassのactionを保持する既存render経路を維持した。
- 通常release PID 42956へ一覧の下2単位と音量表示の下1単位を続けてqueueし、音量が対象分だけの90%となることを確認。Undo一回で100%へ戻り、paused 01/30を保持して通常終了した。native入力が何frameに分かれたかは測定していないため、同一frame配送の厳密な証拠はheadless回帰とする。source保存・OS変更なし。
- 一覧のsmooth scroll自体は別経路で、複数領域を跨ぐframeでの配送は引き続き監査する。

### Wheel直前のpointer移動（2026-09-06）

- 旧通常release PID 2764でforegroundを確認し、paused音声の一覧から音量表示へ移動直後にwheelを送る。一覧だけがscrollし、volumeは100%のままになることを再現した。単なるforeground不成立ではない。
- 固定winit 0.30.13のWM_MOUSEWHEEL/HWHEEL処理はlParam座標を使わない。既存のhidden owned-window testへ縦/横wheelを追加すると、両方が最後のbutton座標(-20,-30)を使い、期待した(300,320)/(-40,-50)にならず失敗した。runtime hookでsigned screen→client変換後のmoveを先行dispatchして通過する。
- 修正版PID 596では同じ移動直後のwheelで100%→90%。音量表示から一覧へ即時移動したwheelは90%を保って一覧をscrollし、表示へ戻る即時wheelは80%へ変更する。停止位置は01/30のまま。2編集をUndoし、両windowとも通常終了した。source保存・OS設定変更なし。
- appの240/480/960px回帰もPointerMovedとMouseWheelを同じframeへまとめて通過する。実機mixed-DPIや全deviceを証明する試験ではない。

### Wheel音量操作（2026-09-06）

- 旧通常release PID 37152では動画面のwheel後も100%だった。修正版PID 7712では下1ノッチで90%となり、Undoで元へ戻る。音量表示をstatus barの再生controlsの隣へ分離した。
- 同じwindowで24曲の音声folderを開き480×300へ縮小。一覧上のwheelは曲一覧だけをscrollし100%を維持した。音量表示へpointerを移した直後のwheelは音量を変えず、同じwindowでhoverを確立して再送すると90%になった。最初の試行は成功の証拠に含めず、入力境界の未解明点として残す。
- 回帰はLine/Page/Point、30/120fps、raw eventの合算とsmooth tailの非発火、Undo、0/200%端点、target tabの照合、修飾key・横wheel・focus・button保持・modal/menu/overlayの遮断を確認。240/480/960pxの実draw_uiで音量labelだけが変更を発行することも確認した。
- 両windowは通常終了、試用の音量編集はUndo、source保存・OS音量設定変更なし。live gain経路は既存実装を使用し、今回のnative観測はUI状態の確認でloopback測定ではない。

### 消音解除時の音量（2026-09-06）

- 旧通常release PID 40604で30秒音声を50%へ下げ、M→Mを実行すると100%へ戻った。修正版PID 34076では50%→0%→50%を表示し、再生を継続する。停止後のUndo/Redoは18秒の位置を保ったまま0%/50%となった。
- 自動回帰は音声/動画で初期値、35%/160%/20%、消音中の別編集、Undo/Redo、履歴branch、tabごとの復元、連続zero編集、再loadなしを確認する。旧コードでは35%復元のassertionが100%となり失敗する。
- 両試用windowは7編集をUndoして通常終了した。sourceの保存・OS音量変更なし。既存runtimeのgain/ramp経路は変更しておらず、今回のnative観測はUI音量と再生状態の確認で、loopback sample測定ではない。

### 音声playlistの現在行追従（2026-09-06）

- 旧通常release PID 28620で24曲目を直接開いても一覧が1曲目からのままで、現在行が見えないことを確認。通常終了後に修正した。
- 修正版PID 40276では直接openで24曲目全体が下端に見える。volume変更後のCtrl+Rightは保存確認となり、確認中とEscape後も現在行とscroll位置を維持する。Undo後のCtrl+Rightは先頭曲へ移動して表示する。
- 再生中にwheelで一覧を下へ送り、位置更新中も手動scrollが保持されることを確認。別画像tabをOpenし、音声tabへ戻ると現在の1曲目が再び見える。両windowは通常終了し、sourceの保存・OS設定変更なし。
- 自動回帰は1万曲で未取得snapshot、初回末尾、手動scroll、先頭/既に可視の隣接行、Shell reorder、tab再表示相当のclear、現在項目の消失/再出現を描画で確認。offsetを0に制限すると初回表示回帰に失敗する。

### 音声playlistの行操作（2026-09-06）

- 24曲のignored fixtureで通常release PID 16488を開き、番号・現在曲の強調・32pxの行高を確認。曲名より右のx=900/y=88でも2曲目へ切り替わった。480×300へ縮小すると既定tooltipの右端が切れたため、通常終了して幅制限を追加した。
- 再build後のPID 7288では480×300でも日本語を含む全文tooltipがwindow内に収まる。x=440/y=88で2曲目を選択でき、wheelで16曲目以降へscroll、18曲目の行の右端clickで同じ項目がactiveになり再生を開始した。通常終了し、sourceは保存していない。
- 自動回帰は240/960px幅、1万音声と混在画像、Shell順、現在色、32px間隔、一行省略、全文tooltipの水平境界、全幅click、scroll後の選曲、空一覧を確認。描画行数は12未満で、旧label相当の幅64pxへ戻すmutationはクリック回帰に失敗する。metadata列・自動scroll・物理入力の全matrixは対象外。

### 単一項目の前後移動（2026-09-06）

- 旧通常release PID 40588で1枚だけのfolderのPNGを100%表示にし、右矢印でFitへ戻ることを確認。回転編集後の右矢印では、行先が同じ画像なのに保存確認が出た。Cancel/Undoして通常終了する。
- 修正版PID 42056では回転後の右矢印が未保存状態と画像を保持し、確認を出さない。最初のまとめたキー送信後に100%状態を確認できなかったため、同じwindowでCtrl+H→capture→右矢印を分けて行い、100%保持を確認した。再起動で試験を取り直していない。
- 同じwindowへ30秒音声をOpenし、停止中の現在playlist項目をclickして01/30・pause・波形位置が変わらないことを確認。最初のclickはlabel外だったため証拠から除き、label内x=28/y=75で確認した。試用windowは通常終了し、生成PNG/audio・capture/logはignoredの`target/tmp`内。sourceの保存・変更なし。

### 画像folderのHome/End（2026-09-06）

- 3枚の赤・緑・青PNGをignoredの`target/tmp/h1-boundary-folder`へ生成し、中央の緑を開く。旧通常release PID 43708でHome/End後も2/3の緑に留まることを確認した。
- 修正版PID 43980ではHomeで1/3の赤、reading modeのEndで3/3の青へ移動する。reading解除・回転編集後のEndは青と未保存状態を保ち、Homeだけが保存確認を出す。Escape/Undo後にpaletteを開き、`image`の前後へHome/Endで文字を挿入して`first image in folder`にでき、編集中は青のままだった。Enterで共有commandを実行すると赤へ移動した。
- 両windowは通常終了し、sourceは保存・変更していない。自動回帰は非filenameのShell順、画像以外の除外、端点/missing/空snapshot、reading、取消時の編集・世代保持、設定の往復・旧設定への既定値追加・custom chordを確認する。物理keyboard/IMEの全matrixではない。

### 画像folderのSeek preview（2026-09-06）

- 旧通常release PID 40772で既存PNGを開き、bar hoverは`15 / 20 towavue.png`だけで画像が出ないことを確認。修正版PID 44740では同じhover位置に移動先の画像を表示し、本画面は現在画像のままになる。
- Bでreadingへ切替後は移動先の2 pageと`15–16 / 20`、R/Hでは本画面と同じ縦並び・反転をpreviewにも確認した。Fでfilmstripへ移るとSeek previewを隠し、中央の一覧を表示。workerは共用で、追加threadを作らない。
- readingを解除して回転編集を加え、別画像へのbar clickで保存確認が出ることを確認。Escapeで取消、Undo後に移動するとpreviewで見た画像が本画面に出た。破損した既存JPEGのhoverはNo previewとなり、現在画像は保持する。両windowは通常終了、sourceは保存・変更せず、capture/logはignoredの`target/tmp`内。
- 自動回帰は実FFmpegで生成したPNGを用い、filename順と異なるShell snapshot、音声項目の除外、単画像/readingの非同期完了、hover中のpath/編集保持、離脱/overlay、dirty guardを検証。preview経路を一時的に無効化すると3秒の完了assertが失敗した。別の描画testは縦横・反転順、取得済み/失敗の再要求抑制とclearを確認する。

### Seek previewの位置と縦長素材（2026-09-06）

- 旧通常release PID 2932で30秒動画の薄いseek barのx=720をhoverすると、thumbnailが左端に出ることを確認した。修正版PID 13952では同じ位置の上へ160×96で表示し、captionも中央になる。草案のhover地点とpreviewの対応を優先した変更で、本画面scrubではない。
- 同じwindowのOpenから108×720の縦長動画を開き、右端hoverで高さ108以内・画面内のpreviewを確認。320×240へ縮小しtimelineを表示すると、previewは空き高さに縮みcaptionもtrackより上に収まった。音声なしの既存案内は維持される。両windowは通常終了し、fixture/capture/logはignoredの`target/tmp`内。
- 回帰は100%/200%の描画入力、960×576/320×240、横長/縦長、薄いbar/96px timeline、左右端/中央の48組を描画する。旧表示サイズと、小画面で高さ制限だけを入れた場合のtrack重なりをそれぞれ検出し、修正後は通過した。物理mixed-DPIや全tooltipの検証ではない。

### 複数streamのpreview（2026-09-06）

- 赤320×240の先頭映像、青160×96の既定映像、無音の先頭音声、880 Hzの既定音声を持つ4秒MKVをignoredの`target/tmp`へ生成する。旧通常release PID 39968では青い本画面に赤いhover thumbnail、空のwaveformとなった。起動直後の注入keyは未反映だったため、同じwindowを再確認・前面化してtimelineを表示した。再起動で試験を取り直していない。
- 修正release PID 28992は同じpathのv3 cacheで青い本画面・thumbnailと非空waveformが一致。D3d11va、120 hardware/presented frames、drop/CPU transfer 0、drift p95 3.899 ms/最大4.203 ms。両windowは通常終了した。
- 自動回帰は同構成の1秒素材で再生の映像寸法・音声sampleを確認し、thumbnail・filmstripの青色と波形を照合する。旧コードでthumbnailのassertが失敗し、修正後は通過した。全container/stream配置やexportの選択一致を保証する試験ではない。

### 複数streamのSave As（2026-09-06）

- 上記素材を青→赤、tone→無音の順にremuxし、既定指定を全て外す。旧通常release PID 41696では青160×96を表示するが、native Save As→再openで赤320×240になった。保存音声もmonoからstereoへ変わる。既定指定を残した素材のCLI出力は一致していたため、それだけでは差を検出できなかった。
- 修正release PID 45184は同じ素材・同じSave As操作で青160×96/monoを保存し、同じwindowへ再openして青い映像を確認した。元素材と保存物の各再生はD3d11va、120 frames、drop/CPU transfer 0。再openのdrift p95 3.671 ms/最大6.600 ms。両windowは通常終了し、sourceは編集せず、出力・capture・logはignoredの`target/tmp`内。
- 自動回帰は青/赤とtone/無音の4streamで、無編集・crop・trim・音声のみの保存をdecode比較する。音声の第2streamにはhearing_impairedを付け、channel数だけでは選択が一致しない場合も含める。hardware/softwareの引数に同じindexが入り、copyts/start_at_zeroはtrim時だけであることを確認する。hardware encoderの実行成功そのものを引数testで証明するものではない。

### 破損mediaからのOpen回復（2026-09-06）

- aedb74bの通常releaseを、mediaではない短いtextを入れたローカル`.mp4`で起動する。中央に`Could not play media`とprobeの原因が残り、短時間statusのduration失敗とは区別できた。
- 同じprocessでCtrl+Oから既存30秒H.264/AACを選択し、別tabでPlaying、D3d11va、映像表示を確認する。pause後にCtrl+Wで正常tabを閉じると元のFaulted tabと原因へ戻り、さらにCtrl+WでWelcomeへ戻る。各段階のwindow応答probeは6～8 msだったが、これはOpen latencyの測定ではない。
- 所有PID 43708は通常終了し、正常sourceのSHA-256は試験前の基準値と一致した。編集・export・OS設定変更なし。capture/log/破損fixtureはignoredの`target/tmp`内。96 DPIの注入入力による単発試験で、遅いstorage・全codec・物理入力のmatrixを保証しない。

### Release長時間再生の再検証（2026-09-06）

描画直前のlate discard再確認を入れた通常release版は、基準機の30分再試験で同期・dropゲートを満たした。以下は不合格だったbaselineからの比較であり、全codec/deviceや実DPI matrixまで完了したものではない。

- dbf13b4のrelease版で既存の30分4K60 H.264/AAC素材を連続再生した。960×576、1倍、アプリ内muteのみ。再起動・Seek・OS設定変更・並行した重い試験は行っていない。adapterは00000000:000146b5。
- 107,771 hardware frames、CPU transfer 0、107,754 presented＋17 droppedでsourceのframe数と一致した。全区間drop率は0.015774%。drift p95は4.725msだが、最大281.340msで100ms上限を超えた。**30分ゲートは未達**であり、旧M3の合格結果を現在のreleaseへ流用しない。集計logだけでは外れ値の位置・原因を特定できない。
- Playing中の30秒間隔private memoryは224.07～309.07 MiB。15分付近のpeakは次のsampleで約237 MiBへ戻った。EOF idleの10.053秒間CPU時間は15.625ms、private memoryは176.98 MiBへ減った。GPU memoryや全codec/deviceでの保証ではない。
- 既存Seek時間は同期pipeline再構築の後からVideoReadyまでで、操作受付から映像表示までを測っていない。この境界を修正してから100回Seekを再測定する。まずは最大driftの発生位置とlate frame処理を切り分ける。
- 続く1分4K60の通常試験は最大9.739msだった。試験用にsource 2秒でUIを300ms止めると、待機時の判断のまま複数の古いframeをpromotionし、最大283.758msを再現した。描画直前にも既存queueのlate discardを行う修正後は27.039msとなった。両方とも3,578 presented＋16 dropped＝3,594、CPU transfer 0で整合する。元の30分runでUIが遅れた原因・時刻そのものを特定したわけではない。
- 遅延・traceコードと環境変数を除去し、142 tests・Clippy・通常release build、pause中のSeek表示を確認した。その通常buildで30分再試験を完走し、107,750 presented＋21 dropped＝107,771、CPU transfer 0、drift p95 4.803ms・最大37.416msとなった。全区間drop率は0.019486%。先頭600秒の35,925 frameへ全21 dropsを割り当てても0.058455%以下で、10分の0.1%ゲートも満たす。これは先頭10分のdrop率の保守的上限であり、正確な区間drop数ではない。
- 修正版のPlaying中private memoryは30秒間隔sampleで220.40～235.48 MiB、OSのpeak paged memoryは343.42 MiBだった。粗いsampleだけではpeakを捉えきれない。EOF idleは10.042秒でCPU 0ms（計測分解能以下）、private memory 174.41 MiB。muteのUndoを確認して通常終了し、source/OS設定は変更していない。次は別件のSeek計測境界を修正し、100回の再測定を行う。

### Shortcut prefixの取消

- Ctrl+Kの1秒待ちが切れた後も4秒のstatus通知が残るbaselineを確認した。修正後は入力状態とその案内を同時に解除する。後から出た別の通知は消さない。Escape、mouse press、focus喪失、別command、file drop・離脱確認でも待ちを解除する。
- 自動testは期限前の保持、正しいCtrl+K Ctrl+S解決、別command・確認画面・期限切れでの解除、後から出た通知の保持を検証する。実windowでは通常sequenceで再読込通知を確認した。同じ注入方法でCtrl+K→別の所有windowへfocus→復帰→Ctrl+Sを402msで行うと、再読込ではなくnative Save dialogになった。Cancelし、fileを書いていない。
- 期限後captureでprefix案内が消えることも確認した。基準機の注入入力試験であり、全keyboard/IMEの証拠ではない。各試用windowは通常closeし、生成物はignoredのtarget/tmpに残す。

### 入力注入とcaptureの完了確認

- `SendKeys.SendWait`や送信helperの終了を、本体が全keyを処理した証拠にしない。まとめた「save」の直後captureが「sav」になる現象を、window event・query更新・Present完了の一時計測で切り分けた。capture完了18:41:16.235 UTCより後の16.337に最後のEが本体へ到着し、16.346にPresentが完了していた。本体queryから文字を落とした事例ではない。
- 同じ4文字のWindowEvent到着→Present完了は7.829/7.335/15.045/8.502 ms。注入前の待ちやDWM表示完了を含まない基準機debug buildの単発値で、物理keyboard・IMEの遅延保証ではない。Windows/input backendのどの層が到着を遅らせるかまでは特定していない。
- 短いASCII試用labelはkeyを分割して送り、対象windowがforegroundであることと、期待文字列が全文表示されたことを確認してから次の操作へ進む。長いsequenceは各stepの到着を確認する。一定秒数のsleepだけで成功と判断しない。過去の不完全captureをcommandや文字欠落の成功/失敗証拠へ流用しない。
- 調査用入力logは所有するfixture windowだけで一時的に有効化し、計測後はコードも環境変数も除去した。最終通常buildでも「save」全文を確認し、format・Clippy・138 tests・buildを再実行した。製品へkey loggingや再描画の回避策は追加していない。試験log/captureはignoredのtarget/tmpに限る。

### 小さいwindowのgrid menu

- 入力監査: grid上のCtrl+SがSaveでなく時計回り回転となりdirtyになるbaselineを確認した。修正後はnative Save dialogを開き、Cancel後も向き・clean状態を保つ。Shift+Sは物理cellを一回実行して閉じ、Undoでcleanへ戻る。Ctrl+Shift+Pではgridが閉じ、検索文字はpaletteへ入る。
- 全16物理codeのindex、numpad/未知codeの除外、Shift保持、Ctrl/Alt/Superと組合せ、palette・guard・pickerの優先を自動testする。対象keyはegui focus処理前へ渡す。OS layoutやIME設定は変更しないため、非QWERTYの実keyboard/candidate操作の成功を主張しない。送信文字列の末尾がcaptureへ反映されない試行もあり、最終palette captureは「sav」での検索・無編集を確認した証拠に限る。

- 480×300でPNGを開きGを押す。旧実装は長い名前が列幅を広げ、左右列と設定pathが画面外へ切れた。修正後は全16 cellを固定4×4で表示し、名前は折返し・省略、設定pathは省略し、hoverで全文を示す。
- 960×576、480×300、320×200 logical pointsを画像・動画・音声で自動検査する。長い設定pathでも全16のkey/nameが2行以上で画面内に収まり、同じ位置のpointer clickで期待commandを一回だけ返して閉じ、fade-out中は追加実行しない。最初のLayoutJobだけではButtonが行数を再設定してはみ出したため、bounded galleyを渡す方式へ修正した。
- 実windowでZoom inのcellをclickし、一段拡大してgridが閉じることを確認した。既存eguiのUI倍率を上げた状態でも4列とkeyを保持し、省略名を表示する。これは注入入力・基準機での確認で、極端なUI倍率や全OS keyboard layoutのmatrixではない。最終word-wrap/disabled-tooltip buildも同じ480×300で再確認した。

### 画像zoomとDPIのH1確認

- 大画像境界: 512×16,384 PNGの上端1,024pxを赤、下端を緑にしたfixtureを480×300でFitする。旧2%下限では両方が切れ、修正後は222pxの表示高に両端が収まる。同じ2枚を横/縦readingにしても端を保持する。Zoom outは0.8倍、最終statusは1.08%になる。手動下限は2%と長辺1 physical pixel相当の小さい方、上限64倍。小さいFitからの操作が2%へ飛ばないことを回帰testする。
- 基準機の静止2ページreadingは5秒CPU時間15.625 ms。画像は約32 MiBのRGBA一枚で、試験はdecode/GPU上限を緩めていない。注入入力による試験で、最初のpalette送信はEnterまで反映されず、独立送信後のcaptureと最後の直接shortcut送信を最終証拠にした。

- 480×300のwindowで8×8 PNGをfitし、paletteでZoom inを選ぶ。修正前は固定960×576から計算して6400%まで飛んだ。修正後は表示中の222px角に対して1.25倍の約278px角（3469%）となり、viewport外はclipする。
- 自動描画testは100/125/150/200%と100%への復帰、100%実pixel寸法、fitからの1.25倍、crop preview、crop後回転、Ctrl+wheelのpointer anchorを確認する。fractional DPIのegui座標丸めには0.1 physical pixel未満の許容を使う。
- native試験では既存egui keyboard zoomでUIだけを拡大した。最初は8×8 PNGの有色領域が12×11、画像計算だけの修正でも10×9だった。固定egui-directx11のzoom二重適用をadapterで除き、最終captureでは元と同じ8×8、同じ中心位置になった。タイトル・status・window controlsのはみ出しも解消し、拡大UIのclickで最大化→元の40,40,520,340へ復帰→closeを確認した。
- H.264/AACの拡大UIもbar外へaspect-fitし、60 presented / 0 dropped / 0 CPU transfersでEOFに到達した。実monitorは2画面とも96 DPIで、異なるOS DPI間の移動・monitor切断は未検証。OS設定やdisplay modeは変更していない。capture・fixture・diagnosticはignoredのtarget/tmp内に保持する。

### 日本語表示とpalette入力のH1確認

- 日本語filenameのPNGでtab/statusが欠字になるbaselineを確認した。WindowsのYu Gothic Medium（なければMeiryo、MS Gothic）を既定fontの後ろへ追加し、最終windowで日本語の名前が読めることを確認した。fontは起動時に一度読むだけで、同梱・downloadやOS設定変更はしない。未導入環境ではdiagnosticを確認し、glyph testの明示skipを成功証拠にしない。
- 回帰testはpreedit中の上下/Enter、Commitと同frameのEnter、取消とEscape、次の独立key、英語・日本語の確定文字を確認する。最初のaction-only検査では確定文字欠落を見逃したため、queryの完全一致を追加した。TextEdit描画前のfocus固定と重複key消費で通過した。
- 実windowでは通常のzoom検索→Down→Enterで一回Zoom outしpaletteが閉じ、再open→Escapeも通過した。まとめたkey送信は不完全だったため、foreground確認後の分割送信を最終証拠とした。WM_CHARによる日本語query注入は反映を確認できず、IME成功の証拠には含めない。
- paletteを閉じた静止PNGの5秒CPU時間は0 ms（計測分解能以下）。基準機debug buildの単発観測で、起動時間・release性能・実IME候補window・物理keyboard・複数DPI/monitorのmatrixを完了したものではない。

### 必要な環境

- Windows 10 22H2以降のx86-64 PC
- Visual Studioの`Desktop development with C++` workloadとWindows SDK
- LLVM（標準インストール先の`%ProgramFiles%\LLVM\bin\libclang.dll`を使用）
- PowerShellとGit

Rust 1.98.0、rustfmt、Clippy、MSVC targetは`rust-toolchain.toml`に固定されている。FFmpegはリポジトリへ同梱せず、セットアップスクリプトがchecksumを検証したLGPL shared buildを`vendor\ffmpeg`へ展開する。

### 初回セットアップと起動

リポジトリのルートをDeveloper PowerShellで開き、同じPowerShell session内で実行する。

```powershell
$ffmpegDir = .\scripts\setup-ffmpeg.ps1
$env:FFMPEG_DIR = $ffmpegDir
$env:LIBCLANG_PATH = Join-Path $env:ProgramFiles 'LLVM\bin'
$env:PATH = "$(Join-Path $ffmpegDir 'bin');$env:PATH"

cargo run -p towavue-app
```

引数にfileまたはfolderを渡して直接起動することもできる。

```powershell
cargo run -p towavue-app -- 'C:\path\to\media.mp4'
cargo run -p towavue-app -- 'C:\path\to\media-folder'
```

一度buildした後は、同じ環境変数を設定したsessionから次のように短時間で再試用できる。

```powershell
.\target\debug\towavue.exe 'C:\path\to\media.mp4'
```

現時点ではinstaller、portable package、file association、Explorerの「プログラムから開く」登録はない。`towavue.exe`だけを別の場所へ移してもFFmpeg DLLを発見できないため、この方法を配布手順として使わない。

### 対応拡張子

| 種類 | 認識する拡張子 |
|---|---|
| 画像 | avif, bmp, gif, jpeg, jpg, png, tif, tiff, webp |
| 動画 | 3gp, avi, m2ts, m4v, mkv, mov, mp4, mpeg, mpg, mts, ogv, ts, webm, wmv |
| 音声 | aac, aiff, alac, flac, m4a, mp3, oga, ogg, opus, wav, wma |

これはfolder navigationで認識する拡張子のlistであり、すべてのcodec、profile、bit depth、破損file、DRM付きfileの動作保証ではない。互換性は実fileで確認し、失敗した組み合わせを記録する。

### H1で確認したvideo metadata orientation scenario

- 固定FFmpegの`-display_rotation 90`付きH.264を開く。変更前は横640×360のまま、FFmpegのautorotate参照は縦360×640だった。変更後は同じ緑/黄/赤/青の四隅になり、90 frames / 0 drops / 0 CPU transfersで終了する。旧`-metadata:s:v rotate=90`では今回の固定buildに行列が付かなかったため、そのfixtureは再現証拠から除外した。
- Rで手動回転、Undoでmetadataの向きへ戻る。縦画面上のselectionをcropすると214×392で表示され、native Save Asも同寸法で成功した。保存先を再openして二重回転しないこと、dirty解除を確認する。
- FFV1/SAR 3:2の90度matrix付きMKVは、軸を交換した縦横比3:8で表示し、90 frames / 0 drops / 90 software transfersで終了する。
- 45度matrixはFaultedにして理由を表示する。最初はstatus期限後に黒画面だけになったため、再生failure理由を永続表示へ変更した。6秒後の実windowと最小window/fullscreenのpaint test、別file Open時の解除を確認する。最初のerror captureは別trialに隠れており、foregroundを確認した再captureで判断した。
- 自動testは回転4通り×反転有無の8 matricesを実MP4へ付け、無編集とcrop/回転/反転後の16出力をUV・寸法・RGB平均誤差12未満・再open時のidentity metadataで検証する。OpenH264が上下反転由来の負strideで失敗する問題も再現し、そのencoder直前だけcopy filterを追加した。これはexport内のcopyであり、再生のGPU-only経路は変更しない。
- 任意角度・scale・shear・射影は対応外。streamとframeのmetadataは安全な値へ変換するが、全containerでの動的metadata・HDR・物理keyboard/DPI・長時間性能gateまで検証したものではない。

### H1で確認したlive trim scenario

途中Seekのsample位相差は、今回の48 kHz/ミリ秒PTS fixtureの1.067秒開始で-8 samples（約-0.167 ms）だった。testはsource列中の一致位置を探索して差を記録し、このfixtureで0.5 msを越えないことを確認する。全file・全Seekの上限保証ではなく、先頭からの無制限prerollは追加していない。

- 境界監査ではミリ秒PTS・30 fpsのFFV1/PCMから33.4–99.6 msを選び、変更前のexport 2 frames / live 1 frameを再現した。整数PTS trim後は1 frameになり、67,000,001–100,000,001 nsなどのframe色と非圧縮音声sample列もsourceからの切り出しに一致する。5秒のsource PTS offsetでも同じ結果を確認する。
- 低精度音声PTSの丸めで生じた16 samples差はsample累積時刻の復元で修正した。44.1/48 kHzの3,000 chunks、missing PTS、前後の時刻飛びをtestする。ただし途中Seek後のsub-tick位相差は残るため、全source/Seekのbit一致とは扱わない。
- 1–2 nsの動画/音声trimと、音声だけが残る動画trimは失敗し、既存保存先を保持する。実windowの約2.984秒H.264/AACは90 frames / 0 drops / 0 transfersでEnded、Save As成功・dirty解除を確認した。出力videoは90 frames / 3秒、audio durationは約2.984秒。最後のframe長・codec paddingと端点選択を区別する。

- 30秒H.264/AACをpauseしてOを指定しPlay。変更前は約2.946秒の終端を越えて7秒台も再生した。変更後は範囲開始から再生し、終端でEndedになる。約2.95秒で89 frames / 0 drops / 0 CPU transfersを確認した。
- 範囲外の約10秒へtimeline SeekするとPausedのsource previewになる。Oで終了を広げ、約5秒へSeekしてI。2倍速で約5～10秒を再生して終端へ停止し、Playで再開する。151 frames / 0 drops / 0 CPU transfersを確認した。範囲内Seekはsource時刻とpause状態を保つ。
- 音声なしFFV1/SAR 3:2は約2.874秒の範囲を約2.85秒で再生し、最後のframeを保持してEndedになった。87 frames / 0 drops / 87 software transfers。
- WAVの約2.858秒範囲は0.25倍速で約11.4秒。Undoでtrimを消し、Redoで範囲を戻して再Playできる。終端idleの5秒CPU時間は15.625 msだった（単発debug計測）。
- 自動testは部分音声chunkのsample値・個数、mono/stereo音声のみ、音声/映像の長さが異なるsourceでの両終端通知、caller cancel、半開区間と範囲外Play方針を検証する。基準機の注入入力試験であり、物理keyboard・DPI・全codecのexport境界一致や長時間性能gateとは別である。

### H1で確認したtrim endpoint scenario

- 30秒H.264をpauseし、同じ位置でI→O。変更前は両方を受理してSave Asで失敗した。変更後はOを即時拒否し、開始→source末尾の有効範囲を保持する。
- timeline上で別の終了位置へSeekしてO。除外区間の暗転、白いbracket、ミリ秒付きsource端点が現れ、Save Asが成功する。実試用の2.954～7.690秒指定は約4.736秒のMP4になった。frame/sample境界や圧縮による全formatの厳密一致を保証する試験ではない。
- WAVでOから指定し、暗黙の開始0を確認する。Undoで範囲・dirty印が消え、通知はTrim clearedへ更新される。Redoで戻る。480×300でも範囲labelが読める。
- fullscreenの動画でIを指定すると通常windowのtimelineが開く。再生位置・pauseは保持する。
- 入力・表示の初回段階ではexport専用だった。現在は上記live trim scenarioへ接続している。試用は基準機への入力注入で、物理keyboard/IME/DPI matrixではない。

## 2. 現在試せる操作

何も開かずに起動するとwelcome画面が出る。`Open file`または`Open folder`を使うか、上記の起動引数を使う。画像と動画はfileごとのtab、音声は同じfolderのplaylist tabになる。

代表的なdefault shortcutは次のとおり。

| 操作 | Shortcut |
|---|---|
| Open file / folder | `Ctrl+O` / `Ctrl+Shift+O` |
| Play/pause、5秒seek | `Space`、`Left` / `Right` |
| 同種media移動 / 全種media移動 | `Ctrl+Left` / `Ctrl+Right`、`Alt+Left` / `Alt+Right` |
| Filmstrip | `F`（表示中は`Tab` / `Shift+Tab`でも移動） |
| Fullscreen | `F11`（Escapeはoverlayを閉じた後にwindowへ復帰） |
| Command palette / grid menu | `Ctrl+Shift+P` / `G` |
| Tab移動 / close | `Ctrl+Tab`、`Ctrl+Shift+Tab` / `Ctrl+W` |
| Timeline | `T`（音声では最初から表示） |
| Undo / redo | `Ctrl+Z` / `Ctrl+Shift+Z` |
| Save As / 同じexport先へ再Save | `Ctrl+Shift+S` / `Ctrl+S` |

画像では`Ctrl+wheel`または`+` / `-`でzoom、右dragでpan、左dragでselectionを作る。`Shift`付きselectionは正方形になり、辺をdragしてresizeできる。`Ctrl+Y`でcrop、`R` / `L`で90度回転、`H` / `V`で反転する。`B`でreading mode、`Ctrl+[` / `Ctrl+]`で同時表示数を変える。

動画・音声では`I` / `O`がtrim端点、`Up` / `Down`、`M`がvolume、`,` / `.` / `/`がrateを編集する。trim・volume・mute・rateは現在の再生と最終exportの両方へ反映する。rateはピッチ維持の0.25～4倍で、変更時は現在位置から短い再primingを行う。範囲外Seekはpaused source previewになり、Playはtrim開始へ戻る。source fileは変更されない。

## 3. Explorer順を確認する

towavueの「Explorer順」はfilename順の別名ではなく、そのfolderで利用者がExplorerの`Sort by`から選んだ実際の列・方向・複数列条件である。

1. Explorerで試験folderを開き、`Sort by`をName、Date modified、Date created、Size、Typeなどへ変更する。
2. そのExplorer windowを開いたまま、folder内のmediaまたはfolder自体をtowavueで開く。
3. `F`のfilmstrip、音声playlist、`Ctrl+Left/Right`、`Alt+Left/Right`の順序を確認する。
4. Explorer側のsortを変更し、towavueで別mediaを読み込むかfilmstripを開き直して再取得させる。
5. status右側の情報へhoverし、`Explorer live order`、`Explorer saved order`、または`Natural-name fallback`を確認する。fallback時は通常表示にも`Name fallback`が付く。

同じfolderを表示するExplorerがある場合はそのlive viewを優先し、ない場合はShell viewが解決する保存済み状態またはfolder templateを使う。取得に失敗したときだけWindows自然名前順へ縮退し、statusに明示する。Explorerの非公開registry Bagsは解析しない。

## 4. 設定と一時data

| Path | 内容 | 扱い |
|---|---|---|
| `%APPDATA%\towavue\shortcuts.conf` | commandごとのshortcut | 初回起動時にdefaultを生成 |
| `%APPDATA%\towavue\grid.conf` | image/video/audio別の4×4 grid | `1234/qwer/asdf/zxcv`順に16 commandを記述 |
| `%LOCALAPPDATA%\towavue\preview-cache` | waveformとhover thumbnail | path・size・更新時刻key、最大64 MiB |
| `tests\generated` | test用media fixture | Git対象外、scriptで再生成 |
| `vendor\ffmpeg` | local FFmpeg development build | Git対象外、scriptで再取得 |
| `target` | Rust build出力 | Git対象外 |

設定fileを編集した後は、menuの`Reload keyboard shortcuts`または`Ctrl+K Ctrl+S`でshortcutsとgridを再読込する。設定UIはまだない。壊れた設定を初期化するときは、custom内容を退避してから対象`.conf`を削除し、アプリを再起動してdefaultを再生成する。

## 5. 人が触るときの確認matrix

一度に「全機能を試す」のではなく、次の単位で一周してから気付きをissue化する。

| 観点 | 最低限のscenario |
|---|---|
| 起動 | 引数なし、file引数、folder引数、非対応拡張子、空folder |
| Window | resize、最小化復帰、100%以外のDPI、複数monitor、tabをwindow外へdrop |
| 画像 | 静止画、GIF/WebP/APNG、AVIF、EXIF回転、zoom/pan、selection、reading、export後の再open |
| 動画 | H.264、HEVC、VP9、software fallback、pause、連続seek、EOF、timeline hover、音声あり/なし |
| 音声 | playlist順、play/pause/seek/EOF、default output device変更、waveform |
| Navigation | Explorerの各Sort By、filmstrip、同種/全種移動、folder内容の追加・rename・削除 |
| 編集 | 各operation、順序、undo/redo、dirty indicator、media移動・close・終了guard、Save/Save As |
| 入力 | mouse、wheel、default shortcut、prefix shortcut、command palette、gridのkey/click |
| 異常系 | 読めないfile、書き出せない場所、FFmpegをPATHから外した状態、HDR source |

報告には次を残す。

- Windows version、GPU、driver、display scale、media種別とcodec/container
- 再現手順、期待した結果、実際の結果、再現率
- towavueを起動したterminalのdiagnosticと、必要なら画面capture
- Explorer順の問題なら、対象folder、Sort By列・方向、Explorerを開いていたか、statusに出たsnapshot source
- 性能の問題なら、fileの解像度・frame rate・durationと、何秒後に重くなったか

private mediaをrepositoryやissueへ添付しない。再現fixtureを作る場合は権利上問題のない小さな生成fileを使う。

### H1で確認したcompact shell scenario

- 960×576から480×300へ端dragでresizeし、logo・tab・window controls・下部操作が残ることを確認する。
- 上部の空白をdragして移動、最大化・復元、最小化から復帰する。画像・動画の両方でbarが残ることを確認する。
- 長い名前の画像を開き、Ctrl+Oで2枚目を追加する。狭い幅で等分tab、省略名、active表示とclose buttonを確認する。
- 動画EOF後に左下Playで先頭から再開し、通常再生中は同じbuttonでpause/resumeする。
- Mで編集を作り右上closeを押す。Unsaved edits確認が出て、Cancelならwindowとdirty履歴が残る。
- 基準機の実windowでは上記操作が通過した。複数DPI/monitor・大量tabのmatrixは未検証。直近captureのUI欠落という目視判定はpixel照合で否定され、hardware/software各10回のtimeline開閉でもtabとcontrolsのpixel数が一致した。古い途中buildの欠落captureとは区別する。

### H1で確認したempty-folder Open scenario

- 画像2枚のfolderから1枚目を開き、Ctrl+Shift+Oで空folderを選択する。修正前は元画像が残る一方でfolder位置とseek barが消え、navigationが効かなくなる。修正後は「No supported media」のstatusだけが変わり、bar右端で2枚目へ移動できる。
- 自動testは別processの一時APPDATA/LOCALAPPDATAで起動設定を隔離し、空folderと非対応fileだけのfolderでpath、tab、snapshot、未保存編集が保持されることを確認する。修正前のsnapshot不一致を検出済み。
- その後のH1でShell snapshot待機も非同期化した。native folder pickerを閉じた直後の応答probeは修正前が1秒timeout、修正後が8 msだった。Opening folder中のbar移動は古いOpenを失効させ、待機中のwindow closeも63 msで完了した（いずれも基準機の単発観測）。
- folder起動後のreading 2枚表示、watcher更新による2→3件のsnapshot反映、別folderをOpenした後のreading表示を確認した。runtimeの同期APIを使う実Explorer sort matrixも別途実行し、skipなしで通過した。
- runtime testは中間要求の置換、古い結果・完了済みslotの失効、実行中のcloseと結果抑止を検証する。app testは背景refreshが明示Openを上書きしないこと、別mediaを開いた後や最後のtab close後の失効も検証する。

### H1で確認したpixel crop scenario

- 8×8の四象限PNGを960×576 windowで開き、(400,180)から(420,200)へdrag→Ctrl+Y→Save As。変更前はpreviewを表示できたのにFFmpegが幅0・高さ0で失敗した。変更後はCrop 1×1 pxと表示し、同操作で保存後のPNGも1×1だった。sourceは変更しない。
- 整数化の初回実装でも、1 pixelを拡大すると選択外の色がlinear samplingで右側へにじんだ。画像meshの半pixel帯と動画shaderのsample clampを追加し、最終1×1 previewが一色になることを実windowで確認した。動画の元のchroma再構成や再圧縮までbit一致させる処理ではない。
- H.264の2×2 cropは固定libopenh264が16未満を拒否した。動画の小さなselection→Ctrl+Yでは最小16×16の案内を出し、selectionと非dirty状態を保持する。十分な範囲へ作り直した18×18 cropは同寸法で表示・保存された。16×16の最小出力は実export・全frame再decodeの自動testでも確認する。
- 自動testは非finite・逆転・範囲外、奇数source端、zero寸法、grid往復、1×1 meshの一定UV、回転したcropのsample範囲、全領域no-opと動画最小寸法の拒否、寸法未取得時の保持を確認する。16,384 pixel画像を64倍zoomした1 pixel selectionもrelease後に保持する。PNGの1×1・奇数位置/寸法・回転後再cropはRGB画素を厳密照合し、不正な直接export要求で既存targetを守るtestも維持する。
- これらは基準機の注入入力と固定fixtureによる試験で、全codec、EXIF/display orientation、HDR、DPI・物理keyboardのmatrixを完了したという意味ではない。trial outputはignoredのtarget/tmp内に置く。最初のSave Asは既存dialogの記憶したh1-seek-imagesへ保存されたため、最終試験では明示pathを指定した。
- 再生中のH.264/AACをcropした30秒試験は900 presented / 0 dropped / 0 CPU transfers、drift p95/max 4.769/14.131 msで完了し、crop済みEOFの5秒間CPU時間は0 ms（計測分解能以下）だった。FFV1・SAR 3:2でも300×208 cropを表示し、90 presented / 0 dropped / 90 transfersを確認した。長時間performance gateの再実行ではない。

### H1で確認したvideo visual edit scenario

- 四象限を赤・緑・青・黄、外周を白とした640×360 H.264を開き、EOF後にRを押す。変更前はdirty表示だけが変わり、色の位置と横長表示は変わらなかった。変更後は時計回りの色配置と縦長aspect-fitになり、H→中央領域のselection→Ctrl+Yで編集後の画面をcrop、Ctrl+Zでcrop前へ戻った。
- 同じfixtureをFFV1・SAR 3:2に変換し、software経路でL→Vを確認した。90度回転後はSAR 2:3、display aspect 3:8となり、通常windowとfullscreenの両方で色配置・四辺・縦横比を保持した。各fixtureは90 presented / 0 dropped、hardwareは0 CPU transfers、softwareは90 transfersだった。
- 約1分4K60 H.264/AACでは、約15秒からRの回転表示へ切り替え、3,594 presented / 0 dropped / 0 CPU transfers、drift p95/max 4.943/10.439 msでEOFに到達した。EOFの静止表示は5秒間CPU時間0 ms（時計分解能以下）。追加GPU textureはsource画素数×4 byteで4K約32 MiBだが、GPU allocationそのものの実測ではない。単発のdebug試験であり、10分・30分の再benchmarkや他device/HDRの証明ではない。
- EOF後のUndoで未編集の直接表示へ戻り、Redoで編集表示へ戻ることを確認した。別の30秒動画をpause→R→seek bar中央clickすると、15秒frameを回転したまま保持し、ログにもpipeline再構築とSeek latency 195.525 msが出た。先のarrow key注入ではSeek受領を確定できなかったため、その操作をSeek成功の証拠にはしていない。入力試験は物理keyboard/IMEのmatrixとは別である。
- 自動testはrotation/flipの合成とSAR、共有履歴のUndo/Redo・active tab・selection解除・pause/generation保持を検証する。160×96のfixtureへR→H→中央half cropを適用したUVと、同じ履歴の実export・再decodeの48×80 RGB画素を照合する（圧縮を許容して平均誤差12未満）。全pixel/chroma境界・極小cropの一致までは保証しない。trim範囲のlive再生、動画zoom、metadata orientationは別の未完項目である。

### H1で確認したfullscreen scenario

- 通常位置40,40、960×576の画像windowでF11を押す。修正前は変化しなかった。修正後は1920×1080のborderless表示となり、上下bar・seek・外周余白がなくなり、案内が4秒で消える。Escape後のouter boundsは40,40,1000,616へ戻った。View menuからの起動と2画像readingの全高さ表示も確認した。
- 最大化から直接fullscreenへ入る初回実装では、下端に48pxの旧work-area由来の余白が残り、復帰後のouter boundsも元の-8,-8,1928,1040ではなく0,0,1920,1080となった。入る前の最大化解除と戻る際の再最大化を追加し、四辺の表示とbefore/after bounds一致、さらに通常sizeへの復帰を確認した。monitorは最大化解除前に取得して固定する。複数monitor/DPIやmonitor切断のmatrixは未検証。
- fullscreen中のfilmstripとpaletteを表示し、Escapeでoverlayを閉じてもfullscreenを保つ。palette後の一回目Escapeのboundsは0,0,1920,1080、二回目は通常windowだった。画像を回転しwindow closeを要求すると中央にdirty guardが出た。当時Escapeは無反応だったが、後続のmodal監査で編集を保持するCancelへ変更した。key注入を含むため物理keyboard/IMEの証明ではない。
- modal監査では生成PNGを回転しCtrl+Wで保存確認を出す。確認画面の表示をcaptureで確かめてからEscapeを送ると、修正前は残り、修正後は閉じて回転・未保存状態・tabが残った。自動testではexport失敗→保留保存確認をEscapeで一枚ずつ閉じ、背景clickが解除を起こさず、fullscreen・編集・tabを保持してexportも開始しないことを確認する。
- 長いfile名の保存確認を480×300で開き、eguiのCtrl+NumpadAddでUIを拡大する。修正前は左端とExport buttonが切れた。修正後は名前を省略して全buttonが残り、さらに拡大すると折り返す。実windowでCancel click→画像へ復帰、Undo→正常closeを確認した。自動testは960×576→480×300→320×200→240×150→元sizeを同じcontextで描画し、保存確認・エラー・通常export・離脱前export・別export中の確認の5状態で見出し/操作の非clipと取消/OK clickを検証する。native export失敗や実OS mixed-DPIの試験を代替するものではない。
- 30秒H.264/AACでfullscreen、pause/resume、F11往復、Tによる通常window＋timelineへの復帰を行い、900 presented / 0 dropped / 0 CPU transfers、drift p95/max 4.038/34.290 msでEOFへ到達した。非正方形pixelのFFV1はsoftware経路で全四辺とaspect-fitを保ち、60 presented / 60 transfers / 0 dropsだった。単発の基準機debug trialであり、全codec・HDR・DPI/monitorの保証ではない。
- 静止したfullscreen readingの5秒間CPU時間は0 ms（時計の分解能以下）。自動testは画像meshが960×576全体へ達すること、barの非表示と復帰、modal/overlay優先、selection・pause・generationの保持、Tの復帰、古いshortcut設定へのF11補完とcustom prefixを検証する。native最大化/placementはheadless testではなく実windowで検証した。
- このfullscreen初回変更にはcursor auto-hideを含めず、続く試験で下記を追加した。edge-hoverでのcontrols表示とdouble-click割当は未実装。音声playlistとWelcomeは中央contentとして残し、通常timeline設定は復帰まで保持する。

### H1で確認したfullscreen cursor scenario

- 変更前のPNG fullscreenでpointerを内部へ移し4秒待つと、Windows `GetCursorInfo`の表示flagは1のままだった。変更後は2秒の入力idleで0、移動・wheel・key入力で1になることを実windowで確認した。通常のscreen captureはcursorを含まないため、画像から非表示を推測していない。
- 左button保持中は表示されたが、初回実装では右buttonを3秒保持すると消えた。固定egui-winitの公開button状態はtouch模擬時だけ更新されるため、egui本体のpointer状態へ切り替えた。左右button保持→release後のidle、palette→Escape、F11でのwindow復帰を再試験し通過した。自動testはprimary/secondary/middleの保持・releaseも確認する。
- filmstrip/grid、native Open picker、dirty close guardの表示中は3秒待ってもcursorを維持し、Cancel後は再び隠れる。最小化中は表示へ戻る。pointerを動かさない復帰ではCursorEnteredが届かず表示のままとなるケースを再現したため、既存picker復帰の座標更新をfocus取得時にも再利用した。同手順の復帰後idleで非表示になることを確認した。
- 最終buildの静止PNG fullscreenは5秒間CPU時間0 ms（時計の分解能以下）。期限の到達時だけ表示状態を変え、非表示後は追加deadlineを残さない。自動testはmedia種別、normal/fullscreen、操作overlay・modal・loading/error・reading error・selection drag・file hoverの抑止と待機期限を確認する。基準機での注入入力試験であり、物理keyboard・touch・複数monitor/DPIのmatrixは未検証。
- 30秒H.264/AACでも再生中・pause中・EOFのidle非表示、Spaceでの表示復帰とpause/resume、Escapeでの通常window復帰を確認した。900 presented / 0 dropped / 0 CPU transfers、drift p95/max 3.744/4.044 msで完了した。単発のdebug trialであり、codec/device全体の保証ではない。

### H1で確認したcategorized logo menu scenario

- 画像を開いて左上logoをclickする。修正前は46 commandの縦列で、回転・exportへはscrollが必要だった。修正後はFile / Edit / Viewの3項目からsubmenuを開き、関連commandの区切りと現在のshortcut右揃えを確認できる。
- EditのRotate clockwiseで画像が横向きになりdirty表示が付く。FileのClose tabで既存dirty guardが出る。Cancelで保持し、EditのUndo editで元の向きとclean状態へ戻ることを最終buildの実windowで確認した。
- 480×300へ縮め、View内をwheelで末尾までscrollし、Show command paletteをclickする。最終項目まで画面内で選択でき、menuが閉じてpaletteが開くことを確認した。異なるDPI/monitorやkeyboard-only menu traversalは未検証。
- 自動testでは全registry commandが一箇所だけにあること、custom prefix shortcutの表示、mediaなしでExport asがdispatchされないこと、Open fileの一回dispatchとmenu tree終了、480×300でView末尾へscrollしてpalette commandを選べることを確認する。disabled項目clickでもeguiがpopupを閉じる既存挙動は変更していない。
- 静止画＋Edit submenuで5秒間CPU時間は0 ms（時計の分解能以下、仕事がないという意味ではない）。最初の複数captureではclick受信を確認できなかったため成功とは扱わず、診断を追加して受信とpopupを確認し、診断を除いた最終buildでも再試験した。menu整理によるruntime・commandの意味・dirty guardの変更はない。

### H1で確認したvisual filmstrip scenario

- PNG 2枚・30秒H.264/AAC・180秒WAVを同folderへ置いて`F`を押す。修正前の下部filename button列から、中央の画像/動画thumbnail・音声waveform・duration、現在項目の白枠と名前、暗い背景へ変わることを確認した。portrait画像は枠内にaspect-fitする。最小480×300でも列と名前が見える。
- 通常clickで同tab移動、middle clickで新規tab、未保存回転後の別項目clickでdirty guard、Cancelで編集保持を確認した。Tab / Shift+Tabは当初eguiのfocus移動に消費され、通常wheelも横移動しなかった。filmstripの入力優先と局所scroll設定を修正後、実windowで前後移動・wheel横移動を確認した。key送信を含む試験であり、物理keyboard/IME matrixではない。
- 開いているfolderへ壊れたPNGを追加するとwatcherが列を更新し、該当項目だけNo previewとなった。可視項目以外は要求しない。5万件の仮想snapshot testで960×576の要求数は9件以内となり、現在項目変更、primary/middle click、wheel、範囲外texture破棄を確認した。worker testは最大64件、段階的通知、古い未開始項目の省略、実行中結果の失効、close時の非待機を確認する。5万fileの実Explorer測定ではない。
- 表示が落ち着いた静止画＋filmstripの5秒間CPU時間は15.625 ms。別pathのcold preview cacheで30秒H.264/AAC再生中に開くと、900 presented / 0 dropped / 0 CPU transfers、drift p95/max 4.138/4.557 msでEOFに到達した。いずれも基準機の単発debug trialで、全codec・大規模folder・DPI/monitorの性能保証ではない。
- 表示範囲の変更・clear・closeは開始済みのowned FFmpeg/FFprobeも停止する。native probeや同じ要求内の遅いmediaは後続項目を待たせる場合があるが、古い結果は適用せずwindow closeもjoinしない。既存64 MiB disk cacheを共有し、表示用RGBAは各240×160、UI textureは可視集合のみ保持する。

### H1で確認したExplorer drop scenario

- Explorerで生成PNGをつかみ、Welcome画面の中央へdropする。修正前は何も開かなかった。修正後はhover案内が表示され、drop後に新規tabと画像が出る。ドラッグ中にEscapeで取り消すと案内が消え、現在tabは変わらない。
- 画像を回転して未保存にし、別画像をdropする。元tabの`*`と回転結果を保持したまま新規tabが開く。未保存tabをcloseして確認中に別画像をdropすると、tab数・確認対象・編集を変えず拒否statusを表示する。
- 実Explorerの2画像同時選択からのdropで2つのtab追加、folder dropで配下の先頭画像、MP4/H.264/AACとWAVのdropで再生開始を確認した。Windowsの実OLE drag/dropであり、path eventを直接注入した試験ではない。回転用keyは対象windowへ送信しており、物理keyboard/IME matrixの証拠ではない。
- headless testは画像のdirty保持、native picker/dirty guard/export error中のfile・folder拒否、unsupported/missing fileの非破壊な失敗、音声playlist再利用とdirty playlistの保持、非同期folder Openを検証する。
- 複数folder要求は既存Open Folderと同じlatest-onlyで、全folderを展開するimport機能ではない。virtual file・URL・異なる権限レベルからのdropや複数DPI/monitorは未検証。

### H1で確認したnative picker scenario

- 30秒H.264/AACの再生中にOpen Folderを開き、約2秒待ってCancelする。Shell非同期化だけのbuildでは92 presented / 808 dropped、drift最大3123.274 msだった。pickerを専用STAへ移したbuildでは900 presented / 0 CPU transfers / 0 drops、drift p95/max 3.862/3.977 msとなった。picker表示中にも動画内のframe counterが進むことをcaptureで確認した。
- picker中は本体windowへの入力をnative modalで制限する。本体を閉じるには先にpickerを閉じる。Open fileで画像選択、Open Folderで画像folder選択、Cancel後の本体入力復帰を実windowで確認した。
- 画像をRで回転してtabを閉じ、Unsaved editsのExport and continueからSave Asを開く。EscapeでCancelしたら同じguardと未保存編集が戻る。pointerを動かさず同じExport buttonを押しても再度Save Asが開く。新しい出力先へ保存すると800×600の回転画像ができ、成功後にtabが閉じることを確認した。
- runtime testは呼出元が待機しないこと、選択・Cancel・失敗・worker panicの結果通知を検証する。app testはpicker中のcommand/exit抑止、Cancel・失敗のguard復元、選択中にsourceが変わった場合のexport拒否を検証する。native ownerとpointer復帰は実window試験であり、headless testだけでは証明しない。
- 上記数値は基準機の単発観測。複数DPI/monitorやcodecのmatrix、静止画・pause中のidle redraw負荷は引き続き監査する。

### H1で確認したidle / periodic repaint scenario

- 同じdebug build条件、960×576の基準window、起動後3秒の待ちを挟み、対象processのTotalProcessorTimeを5秒差分で測る。修正前は静止PNGが5906.25 ms、Welcomeが5953.125 msだった。RedrawRequestedの処理で次のRedrawRequestedを無条件予約していたため、入力なしでも描画が連鎖していた。
- 修正後のPNG・Welcome・表示したままのgridは各0 ms、一時停止したH.264/AACは15.625 msだった。0はprocess CPU時計の分解能以下という意味であり、folder watcherの定期pollまでなくしたという意味ではない。
- 音声だけの再生にもscheduleごとの無条件再描画があった。180秒WAVの5秒間CPU時間は5890.625 msから468.75 msへ減少した。音声出力pollは残し、位置表示の更新をUIの20 ms deadlineと統合する。最終buildのpauseでは31.25 ms。eguiのpredicted frame時間によるdelay短縮を避け、この周期はapp側のdeadlineとして計算する。
- 実windowでgridの開閉、pause中のtimeline開閉、動画再開、8-frame GIFの時間更新、音声のpause/resume・末尾Seek・EOFを確認した。headless testはgridが開閉animation後にrepaintを止めることと、音声周期・より早いUI期限・pause時の周期解除を検証する。grid testは修正前の無条件requestを戻すと失敗した。
- 最終の30秒H.264/AAC再生は900 presented / 0 dropped / 0 CPU transfers、drift p95/max 3.725/3.993 msだった。WelcomeでCtrl+K Ctrl+Sを押した通知は、idle化直後のbuildでは6秒後も残ったため、statusとprefixの失効を待機期限へ追加した。修正後は入力なしで6秒待ったcaptureで通知が消えた。4秒の期限選択はmediaなしのtestでも確認した。
- CPU数値はprocessの全threadの合算で、全CPUに対する百分率ではない。debug buildの単発観測。release、GPU負荷、複数monitor/DPI、長時間稼働の評価は別途必要である。

### H1で確認したseek bar / palette scenario

- 30秒H.264/AACをpauseし、status上端の中央をclick → 15.000秒。75%までdragして離す → 22.500秒。各操作につき一回だけpipelineが再構築される。
- 2秒H.264/AACのEOF後にbar中央をclick → Paused / 1.000秒。timelineを開き、frame境界の間へdragする → 最初のdecode frameを表示したまま停止し、Playで残りを再生する。修正前は約1.508秒へのSeekで黒画面、修正後は1.533秒のframeを保持した。
- 同じfolderの生成画像2枚でbar両端をclickする。2枚目をRで回転して1枚目へ移動するとUnsaved editsが出る。Cancelは画像とdirty履歴を保持し、Discardは移動する。480×300でも両端へ移動でき、pointerを離したbarはpixel照合で1行だった。
- paletteでzoomを検索し、Down → EnterでZoom outを一回実行する。基準機ではwindow宛てkey/text messageで確認した。global key注入は安定しなかったため、物理keyboard・IMEの成功証拠にはしない。狭いwindowのpalette layoutも確認した。
- 自動testは複数layout passにまたがる検索・上下選択・Enter、無効候補と空結果、Escape、folder位置の端点、停止中の非frame境界Seekを検証する。動画thumbnailはhover tooltipのみで、本画面のscrubと画像thumbnailは未実装。

### H1で確認したvideo viewport scenario

- 白枠付き240×320 H.264と320×240 / SAR=2のFFV1を生成し、960×576で四辺と縦横比を確認する。
- Tまたはwaveform buttonでtimelineを開閉し、同じframeが残りの中央領域へ収まることを確認する。480×300へのresizeと最大化・復元も確認する。
- 縦長動画内をdragし、selectionが映像に重なり、letterboxを選択範囲へ含めないことを確認する。
- 基準機ではH.264はD3D11VA / 60 frames / 0 CPU transfers / 0 drops、FFV1はsoftware / 60 transfers / 0 dropsでEOFへ到達した。再表示でframe数を加算せず、EOFからのH.264再開も60 framesだった。
- UI repaint deadlineは停止中も処理する。小さいiconの目視判定だけで欠落とせず、元captureのpixelとlayout矩形を照合する。

### H1で確認したaudio drain後のpause scenario

- 12秒・2 fpsのH.264と0.5秒の無音AACを組み合わせ、音声終了後に左下pauseを押す。修正前はFaultedと`the audio output thread stopped`、修正後はPausedとなる。
- 2秒待って映像が動かないことを確認し、resumeしてEOFまで進める。基準機では停止中の映像sample差分0、再開後24 frames / 0 CPU transfers / 0 dropsだった。
- `cargo test -p towavue-runtime-windows live_drain_keeps_pause_and_resume_valid -- --ignored --nocapture`で実WASAPIの排出完了後のcontrolを検証できる。device不在は明示skipであり、成功の証拠としない。

### H1で確認したlive rate scenario

- 4秒のstereo 440 Hz toneを1024 frameずつfilterし、0.25・0.5・1・1.5・2・4倍で長さと音程、左右の位相関係、EOF drainを検証する。1倍はbyte一致する。
- 実endpointの時計は次の明示testで確認する（無音sampleを使う）。0.6秒のwall時間に対して0.25・0.5・2・4倍のsource位置は約0.15・0.30・1.20・2.40秒進み、pause中は不変だった。device不在によるskipは成功証拠にしない。

```powershell
cargo test -p towavue-runtime-windows live_rate_clock -- --ignored --nocapture
```

- 30秒H.264/AAC素材を2倍で再生 → D3D11VA、CPU transfer 0、変更後850 frameをdropなしでEOFまで表示し、source時刻基準のA/V driftはp95 9.019 ms・最大35.380 msだった。
- 30秒video＋先頭5秒だけaudioの素材を4倍で再生 → 音声終了後も映像と位置が進みEOFへ到達した。変更後852 frame中2 frameをdrop、source時刻基準のdriftはp95 28.900 ms・最大68.240 msだった。これは短い実機trialであり、全codec・高解像度の速度別性能保証ではない。
- 音声なし動画をpauseし、2倍へ変更して2秒待つ → 同じframeを保持する。pause中の5秒Seekとresume後の倍速進行も確認した。

### H1で確認したlive volume scenario

- 48 kHz stereo AACの440 Hz toneを再生し、対象towavue processだけのWASAPI session meterを読む。100%でpeak約0.0885、Down 5回の50%で約0.0442、Mのmuteで0になることを確認した。音声の録音とmaster endpointの音量変更は行わない。
- Undoで50%へ戻し、Redoでmuteへ戻る。mute中のSeek後も0を保持する。pause中にmute解除し、resume後のpeakが元に戻る。
- 純粋なsample testでstereo比率、bufferをまたぐ5 ms ramp、正確なzero、200% gain、初期mute時に100%の音が出ないことを確認する。
- 生成したmono PCM WAVは旧buildで`Input changed`となったが、未指定channel layoutの補完後は再生・muteできた。mono/stereoのmaskなしPCM WAVをtest内で生成し、直列/並列decodeの480 frame保持とstereo sample値を検証する。

### H1で確認したimage scenario

- 生成した6000×6000 PNGを相対pathで起動 → 旧buildではeguiの初期2048px制限でpanic、新buildでは原寸textureをfit表示する。decode中のwindow応答probeは約8 msだった（upload全体のlatency保証ではない）。
- 同じfolderの正常画像2枚・17000×2画像・破損PNGをreadingで4枚表示 → Shell順を保持し、失敗pageにも専用の位置を残す。Hで結果全体を反転し、再decodeしない。
- GPU上限超過画像を単独で起動 → processは継続し、利用中deviceの寸法上限を画像領域に表示する。
- 回帰testで連続要求の中間skip・古い結果の破棄・close後のworker終了・複数pageの共通budget・animationの超過拒否を確認する。

1要求の保持RGBA上限は512 MiB。codec内部の1frame処理は即時中断できず、作業領域とGPU memoryはこの上限に含めない。生成画像とcaptureはignoredな`target/tmp/`へ置く。

### H1で確認したexport scenario

2026-09-05、Windows上の1920×1080 H.264/AAC・120秒fixtureで比較した。旧版はSave中のwindow応答確認が1秒でtimeoutした。background化後はFFmpegの稼働中も約10 msで応答し、書き出し済み時間の更新と再生継続を実windowで確認した。

- Save後にCancel exportをclick → 既存targetのSHA-256が一致し、FFmpeg子processと一時directoryが残らない。
- 編集後にCtrl+W → Export and continue → Cancel export → tabとdirty表示が残り、元の確認画面へ戻る。成功時はtabが閉じる。
- 書き出し中にさらに回転 → 成功後もdirty。Undoで書き出した履歴位置へ戻るとsavedになる。
- 既存targetを別processで排他openしたままexport → error詳細が残り、targetのSHA-256は一致。確認後も編集とclose guardを保持する。

同時exportは1件。対象tabのclose・detach・folder内移動とprocess終了は、進行中jobの完了またはcancel後に再操作する。native Save Asの出力先選択は専用STAで行い、本体入力へのmodal制限を保ちながら描画・再生を継続する。

## 6. 開発の具体的な進め方

今後は「人が触った一つのscenario」を最小の開発単位にする。UI全体の一括作り直しや、草案の項目を上から機械的に実装する進め方は取らない。

1. 観察を一文の問題へする → verify: 再現手順と期待結果が一意である。
2. 成功条件を決める → verify: before/afterを人が同じ手順で比較できる。
3. 所有layerを選ぶ → verify: core/runtime/appの境界を越える理由が説明できる。
4. stateや純粋logicの変更には先に回帰testを置く → verify: 修正前に失敗し、修正後に通る。
5. 最小差分を実装する → verify: 対象scenario以外の挙動とarchitecture contractが変わらない。
6. focused testと実windowで確認する → verify: 自動testと目視・操作の両方に結果がある。
7. 完全checkを行う → verify: format、Clippy、全testが通る。
8. `SESSION_LOG.md`と必要な文書を更新しcheckpointをpushする → verify: CIも通る。

完全checkは次のとおり。

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

fixtureがない初回だけ先に次を実行する。

```powershell
.\scripts\generate-m1-fixtures.ps1
```

UI変更では自動testだけを完了証拠にしない。実windowで対象DPI、入力方法、media種別を操作し、変更前後のcaptureまたは観察結果を残す。逆に、見た目の変更に再生workerやD3D11 lifetimeのrefactorを混ぜない。

## 7. Path構造と編集先

```text
towavue/
├─ crates/
│  ├─ towavue-core/             OS非依存のdomain contractと純粋state
│  ├─ towavue-runtime-windows/  Windows、FFmpeg、D3D11、WASAPI、Shell、worker
│  └─ towavue-app/              executable、event loop、UI state、command dispatch
├─ docs/                         architecture、roadmap、試用・既知制約
├─ scripts/                      local/CI setupとfixture生成
├─ concepts/                     Git対象外の草案。実装契約ではない
├─ tests/generated/              生成fixture。Git対象外
├─ vendor/ffmpeg/                local FFmpeg。Git対象外
├─ target/                       build出力。Git対象外
├─ AGENTS.md                     毎回守る実装規約
├─ SESSION_LOG.md                新しい順のcheckpoint記録
├─ Cargo.toml                    workspace設定
├─ Cargo.lock                    固定dependency graph
└─ rust-toolchain.toml           固定Rust toolchain
```

変更内容から編集先を選ぶ目安は次のとおり。

| 変更したいこと | 最初に見るfile | 関連file |
|---|---|---|
| Window、bar、tab、status、timeline、palette、modal、入力 | `crates/towavue-app/src/main.rs` | `chrome.rs`、`seekbar.rs`、`palette.rs`、`commands.rs`、`shortcuts.rs`、`grid.rs` |
| Command名、利用可能media、shortcut解決 | `crates/towavue-core/src/commands.rs` | appのdispatchとdefault設定 |
| Shortcut設定形式/default | `crates/towavue-app/src/shortcuts.rs` | `commands.rs` |
| Grid配置/default | `crates/towavue-app/src/grid.rs` | `commands.rs`、`main.rs` |
| Media拡張子の認識 | `crates/towavue-core/src/media.rs` | runtime decoderとfile dialogも実対応を確認 |
| Tab/playlistの純粋な挙動 | `crates/towavue-core/src/tabs.rs` | appのopen/activate/close処理 |
| Explorer順のmodel/navigation | `crates/towavue-core/src/navigation.rs` | runtimeの`shell.rs`と`watch.rs` |
| Zoom、selection、reading state | `crates/towavue-core/src/image.rs` | appの描画とpointer処理、runtimeの`image.rs` |
| Edit operation、dirty、undo/redo | `crates/towavue-core/src/edit.rs` | app preview、runtimeの`export.rs` |
| Image decode / latest-only worker | `crates/towavue-runtime-windows/src/image.rs` / `image_loader.rs` | coreの`media.rs`、appのtexture化 |
| Video/audio decode、seek、codec fallback | `crates/towavue-runtime-windows/src/decode.rs` | `playback.rs`、`audio.rs`、`renderer.rs` |
| Worker、queue、generation、recovery | `crates/towavue-runtime-windows/src/playback.rs` | `decode.rs`、`audio.rs` |
| D3D11/DXGI描画、HDR判定、resize | `crates/towavue-runtime-windows/src/renderer.rs` | `decode.rs`、appのrender loop |
| WASAPI、endpoint変更、live rate | `crates/towavue-runtime-windows/src/audio.rs` | `playback.rs`、`tempo.rs` |
| Explorer Sort By取得 | `crates/towavue-runtime-windows/src/shell.rs` | `watch.rs`、coreの`navigation.rs` |
| Waveform、duration、thumbnail cache | `crates/towavue-runtime-windows/src/preview.rs` | appのworker event/timeline |
| Export filter/codec/fallback | `crates/towavue-runtime-windows/src/export.rs` | coreの`edit.rs`、appのSave flow |
| Open/Save dialog | `crates/towavue-runtime-windows/src/dialog.rs` | app command dispatch |

`towavue-core`へWindows型、FFmpeg型、native handle、unsafeを入れない。`towavue-runtime-windows`はそれらをsafeな値/eventへ閉じ込める。`towavue-app`はCOM pointerやFFmpeg frameを受け取らず、UIとorchestrationに集中する。

現在の`crates/towavue-app/src/main.rs`は約2,800行あり、今後のUI反復で最も衝突しやすい場所である。ただし、先に大規模分割だけを行うのではなく、実際に変更するまとまりが明確になった時点で、例えばtop bar、timeline、image interactionのような単位を一つずつ移す。移動と挙動変更を同じ差分へ混ぜない。

### Compact paletteのnative IME再確認（2026-09-06）

- 51b43bfの通常release、Windows build 26200、既存の日本語IMEで実施。binary SHA-256: `18F57AF2E2E7D1FCF34D64E70154080A79E661F4B02297AEE27491865F404247`。code変更・event注入用app hookはない。
- 所有Welcome windowのpaletteで通常のIME mode keyとkey入力を使用し、960×576と480×300の検索欄直下に候補を確認。小窓ではSpaceで変換候補を開き、Downでニホンゴ、Upで日本語へ戻し、Enterで確定してもpaletteが残ることを確認した。
- preeditへの最初のEscapeは入力だけを取り消し、次のEscapeでpaletteを閉じる。`open`のcompositionをF10でLatinに変換し、確認Enterではpaletteを保持、次の独立したEnterでnative Open pickerが開く。pickerはCancelし、fileを開かない。
- 別の所有Welcome windowへfocusを移し、元へ戻った後も文字列を保持し、Escapeでpaletteを閉じられる。focus先は両processのwindow handleで確認した。
- 試験間のforeground再取得は変換を中断しうるため、その途中結果を候補操作の成功証拠に含めない。最終の候補移動・確定/取消は、一続きのforeground確認済み入力列で検証した。
- 両windowは正常終了。設定file・source・OS全体のIME設定は変更せず、captures/logsは`target/tmp/h1-compact-ime*`へ保持。これは注入keyによる現在のIME/layoutの確認であり、物理keyboard・別IME・実mixed-DPI matrixの代替ではない。

## 8. UI/UX変更の判断基準

- 実装済みcommandの入口はmenu、palette、shortcut、gridで同じ`CommandId`を共有する。入口ごとに別logicを作らない。
- mediaを覆う常設UIを増やす前に、status、hover、一時overlay、command paletteで解決できるか検討する。
- shortcutだけに頼らず、初見で発見できる入口と現在状態のfeedbackを用意する。
- 操作結果がlive previewへ反映されない場合は明示する。表示とexport結果が違う状態を黙って作らない。
- animationは状態変化の理解を助ける短いものに限定し、再生・seek・入力応答を遅らせない。
- DPI、keyboard focus、mouse hit target、長いpath/file名、empty/error/loading状態を通常状態と同時に設計する。
- backend境界を変える必要が出たら、UI都合でnative objectをappへ漏らさず、先に`ARCHITECTURE.md`のcontractを更新する。
