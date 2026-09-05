# towavue 試用・開発ガイド

この文書は、M7までの開発版を実際に触り、UI/UXの観察を次の小さな変更へつなげるための入口である。完成品向けの利用説明ではない。既知の制約は[KNOWN_GAPS.md](KNOWN_GAPS.md)、固定された技術境界は[ARCHITECTURE.md](ARCHITECTURE.md)、作業順は[ROADMAP.md](ROADMAP.md)を参照する。

## 1. 最初に試す

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
- 開始済みFFmpeg/FFprobeは強制cancelしない。遅い/破損mediaのprocessが終了するまでは同workerの後続項目が待つが、古い結果は適用せずwindow closeもjoinしない。既存64 MiB disk cacheを共有し、表示用RGBAは各240×160、UI textureは可視集合のみ保持する。

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

## 8. UI/UX変更の判断基準

- 実装済みcommandの入口はmenu、palette、shortcut、gridで同じ`CommandId`を共有する。入口ごとに別logicを作らない。
- mediaを覆う常設UIを増やす前に、status、hover、一時overlay、command paletteで解決できるか検討する。
- shortcutだけに頼らず、初見で発見できる入口と現在状態のfeedbackを用意する。
- 操作結果がlive previewへ反映されない場合は明示する。表示とexport結果が違う状態を黙って作らない。
- animationは状態変化の理解を助ける短いものに限定し、再生・seek・入力応答を遅らせない。
- DPI、keyboard focus、mouse hit target、長いpath/file名、empty/error/loading状態を通常状態と同時に設計する。
- backend境界を変える必要が出たら、UI都合でnative objectをappへ漏らさず、先に`ARCHITECTURE.md`のcontractを更新する。
