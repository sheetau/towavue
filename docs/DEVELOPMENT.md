# towavue 試用・開発ガイド

この文書は、M7までの開発版を実際に触り、UI/UXの観察を次の小さな変更へつなげるための入口である。完成品向けの利用説明ではない。既知の制約は[KNOWN_GAPS.md](KNOWN_GAPS.md)、固定された技術境界は[ARCHITECTURE.md](ARCHITECTURE.md)、作業順は[ROADMAP.md](ROADMAP.md)を参照する。

## 1. 最初に試す

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

## 2. 現在試せる操作

何も開かずに起動するとwelcome画面が出る。`Open file`または`Open folder`を使うか、上記の起動引数を使う。画像と動画はfileごとのtab、音声は同じfolderのplaylist tabになる。

代表的なdefault shortcutは次のとおり。

| 操作 | Shortcut |
|---|---|
| Open file / folder | `Ctrl+O` / `Ctrl+Shift+O` |
| Play/pause、5秒seek | `Space`、`Left` / `Right` |
| 同種media移動 / 全種media移動 | `Ctrl+Left` / `Ctrl+Right`、`Alt+Left` / `Alt+Right` |
| Filmstrip | `F`（表示中は`Tab` / `Shift+Tab`でも移動） |
| Command palette / grid menu | `Ctrl+Shift+P` / `G` |
| Tab移動 / close | `Ctrl+Tab`、`Ctrl+Shift+Tab` / `Ctrl+W` |
| Timeline | `T`（音声では最初から表示） |
| Undo / redo | `Ctrl+Z` / `Ctrl+Shift+Z` |
| Save As / 同じexport先へ再Save | `Ctrl+Shift+S` / `Ctrl+S` |

画像では`Ctrl+wheel`または`+` / `-`でzoom、右dragでpan、左dragでselectionを作る。`Shift`付きselectionは正方形になり、辺をdragしてresizeできる。`Ctrl+Y`でcrop、`R` / `L`で90度回転、`H` / `V`で反転する。`B`でreading mode、`Ctrl+[` / `Ctrl+]`で同時表示数を変える。

動画・音声では`I` / `O`がexport用trim端点、`Up` / `Down`、`M`がvolume、`,` / `.` / `/`がrateを編集する。volume・mute・rateは現在の再生と最終exportの両方へ反映する。rateはピッチ維持の0.25～4倍で、変更時は現在位置から短い再primingを行う。trimは現在の再生には反映されない。source fileは変更されない。

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
- Shell snapshot取得の2秒待機と一時的な応答停止は別の未解決課題である。今回の修正を非同期Openの実装として扱わない。

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

同時exportは1件。対象tabのclose・detach・folder内移動とprocess終了は、進行中jobの完了またはcancel後に再操作する。native Save As dialogでの出力先選択は従来どおりmodalである。

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
