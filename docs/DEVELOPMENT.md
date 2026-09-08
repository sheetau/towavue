# towavue 試用・開発ガイド

この文書は、M7までの開発版を実際に触り、UI/UXの観察を次の小さな変更へつなげるための入口である。完成品向けの利用説明ではない。既知の制約は[KNOWN_GAPS.md](KNOWN_GAPS.md)、固定された技術境界は[ARCHITECTURE.md](ARCHITECTURE.md)、作業順は[ROADMAP.md](ROADMAP.md)を参照する。

## 1. 最初に試す

### 限定ZVBI候補と現行コードの最適化版（2026-09-08 19:52 JST）

1504bee時点のアプリcodeを、限定ZVBI headerで再生成したFFmpeg候補に対して通常release buildした。旧試用releaseを置き換えず、別のRust targetに生成したexeは10287104 bytes、SHA256 `03125262C28C0DF0190B6D0DFE7B0EFB1DFBCEB55946C4AF7E40E4AF0AAD43F3`。format／全target Clippy／270 testsも通過した。以下は短時間の操作試験であり、旧候補の30分性能結果を引き継ぐものではない。

候補runtime全94 filesとexeを、新しい`日本語 viewer & tools`へcopyした。FFMPEG_DIRなし、PATHはSystem32のみ、無関係のcwd、専用config／cache。PID 41708、開始UTC `2026-09-08T10:48:48.8404304Z`の実windowで、隣接した直接6 FFmpeg DLLを確認した。30秒H.264/AAC source（SHA256 `F929E6FA18AA010BEBFBB3400539A4F090F7864887F4044E20B4E0C95C5B2F54`）の再生終了、waveform、native Save As、保存物のOpen、filmstrip thumbnailを確認した。画面captureはPID／開始時刻／foregroundを検査し、最初のforeground取得失敗では保存せず、所有windowへのUIA focus後に取得した。

保存物は2072076 bytes、160×96 H.264／48 kHz AAC／30.065960秒、SHA256 `6B983DFBADE1EE4A24802CB0825FF2CBC289A4FBAFE809A533DBB3E83E1DA43C`。同梱helperでprobe・全decodeを通過した。実windowの最初の再生はD3D11VA 892 frames／CPU transfers 0／presented 892／dropped 0、drift p95 4.714 ms／max 5.213 ms。保存物を開いた再生は892／0／890／2、drift p95 4.786 ms／max 30.257 msだった。**後者の2 dropを隠さず、操作を含む短時間試験の観測として残す。** 高負荷・長時間の定常性能やHW encodeの証明ではない。

両tabがcleanなwindowを正常終了し、元media／exe／94 runtime filesとcopyのhashが不変であることを確認した。旧release `94AF9E14...`も不変。証拠はignored `scoped-candidate-release-smoke-20260908`のidentity、stderr、owned captures、保存物。Setup.exe、対象Windowsでの導入／更新／削除、新候補の長時間・高負荷・Seek比較、物理環境とowner受入は未完了。

### 補助exeの配置と探索（2026-09-08 19:18 JST）

previewと保存の探索をruntime内へ統一した。本体と同じdirectoryにffmpeg.exe／ffprobe.exeのいずれかがあれば、その配置を使う。片方が不足してもFFMPEG_DIRやPATHの別版を混ぜない。両方ともない開発配置だけFFMPEG_DIR/binを使い、未設定／不足なら期待pathを含むerrorにする。絶対pathを子processへ渡し、アプリのcwd／PATHは変更しない。

[RustのWindows探索規則](https://doc.rust-lang.org/std/process/struct.Command.html#platform-specific-behavior)には本体exeのdirectoryも含まれるため、旧処理でも環境変数なしの隣接配置は動作した。再現した問題は、別FFMPEG_DIRが同梱helperより優先されることだった。実helperをコピーした隔離子processの試験は旧処理でその条件だけ失敗し、修正後は無設定・競合環境・片方欠落・両方欠落の全4条件が通過した。実FFmpegでduration／thumbnail／waveform／保存・decodeを確認し、欠落時にはPATHの有効な別helperを使わず、既存出力を保持する。

通常debug exe SHA256 `489AEFF19F9C4F6E130171EEEC42675C3689A310782370AA7864702AAE632F58`と限定ZVBI候補の94 runtime filesを、隔離した`日本語 viewer & tools`へ配置。FFMPEG_DIRなし、PATHはSystem32のみ、別cwd・専用設定/cacheから実windowを起動した。30秒H.264/AACの再生、filmstrip／waveform、native Save As、保存物のOpen、tabをwindow外へdragして分離した子windowの再生／previewを確認した。保存物は160×96 H.264／48 kHz AAC、30.065960秒で、元sourceはhash不変。親子の直接6 FFmpeg DLLは全て同じ配置からloadされ、正常終了した。test用app hookは追加していない。

270 tests・format／Clippyと通常debug buildが通過し、候補runtimeの原本／copy全94 hashも不変。これは現在のWindows 11上の局所検証で、最適化releaseの性能、開発環境のない対象Windows、Setup.exeの導入／更新／削除、物理入力や配布監査の完了証明ではない。

### Native FFmpeg再生成候補のrelease確認（2026-09-08 14:38 JST）

3e17046の手順で再生成したFFmpegを使い、別Rust targetで通常releaseをbuildした。アプリsourceの変更はない。binary SHA256は`94AF9E14CDAC00339A0AB830BD728E22EE94B7BAB9FDE66770D9A09DE80B2BC2`。各試験のPID／開始UTC、exe／media hash、実際にloadされた6 FFmpeg DLLのpath／hashを記録した。全DLLは再生成prefixに一致する。設定とcacheは各case専用、cwdは別directory、FFMPEG_DIRを明示し、PATHは同prefix/binとSystem32のみ。これはSetup.exeや環境変数不要の起動試験ではない。

#### Seek

1080p H.264/AACの120秒素材、960×576、1倍、アプリ内mute。5秒前進10回／後退10回を5往復し、前のPresent成功logを待って次の入力を送る。各条件100 indicesに欠落・重複なし、各processの完了logは200件。p50／p95は昇順50／95番目で、全条件が300msゲート内だった。

| 経路・状態 | 完了数 | p50 | p95 | 最大 |
|---|---:|---:|---:|---:|
| 通常・Paused | 100/100 | 28.031ms | 33.528ms | 43.209ms |
| 通常・Playing | 100/100 | 89.546ms | 100.880ms | 110.654ms |
| UIA tree取得後・Paused | 100/100 | 27.128ms | 31.764ms | 36.153ms |
| UIA tree取得後・Playing | 100/100 | 88.203ms | 101.288ms | 108.283ms |

通常PID 48444（開始UTC `2026-09-08T05:25:12.9066966Z`）、UIA PID 37772（`05:26:12.2985531Z`）。各入力前にprocess identityとforegroundを照合した。UIA条件は準備後に15 descendantsのtreeを取得し、同じprocessで測定した。物理keyboard／OS配送／DWM走査表示までの遅延や常駐screen readerの試験ではなく、条件差を純粋なUIA負荷の差とは解釈しない。測定中の並行build/testはない。両windowをmuteのUndoでcleanに戻して正常終了し、exeとsource（SHA256 `DC645595A1165506BF5C3E685B14D7EA3B0116BBDFE74839E7DA5834CF60DA0C`）の不変を確認した。

#### 保存と再open

- PNG、PID 49160（開始UTC `05:27:51.1251530Z`）: 非対称64×48 sourceからleft/right=5/42、top/bottom=7/36をcrop、時計回り回転、Undo/Redo、Save As。29×37のdecoded RGBAは参照と完全一致（SHA256 `EBAE29BC543E282A86CDE62820FEB22884ACC885CB642EA00E1B2C82E5CB13C2`）。続く水平反転と同じexport先へのCtrl+Sも一致（`EFFB5419B11125B40410F13E5F807D4932B1AE16A87F355FF97829C17E6F1BC1`）。同windowで再openし、clean表示とUIAの最大29×37、実画像表示を確認した。元sourceを上書きしていない。
- 画像の再open後、選択sliderがないという補助試験の失敗があった。画面とUIAは正しい29×37を示していた。ignoredなowned-key helperがwindow helperをdot-sourceし、同名のKeys引数をnullへ上書きしてキーを送っていなかったことを確認した。要求keyを別変数へ保持する修正後、同じアプリへCtrl+Aを送り、両sliderの値と上限が29／37になることを確認した。製品の寸法不具合として扱わない。
- 音声、PID 5396（開始UTC `05:30:39.5609360Z`）: 8秒／48kHz mono chirpへtrim 2..6秒、50%、1.25倍を適用。参照filterは`atrim=start_pts=96000:end_pts=288000,asetpts=PTS-STARTPTS,atempo=1.2500,volume=0.5000`。保存PCMは307316 bytes／153658 samplesで完全一致（SHA256 `BCE492EFD2B806F3C44EBB088C92F1232B345FBD79AEA1F649B538A9D1274190`）。最初は別folderの保存先を再openしたため新tabとなった。続けて同じ試験folderへ原本copyを置き、同folder tabで編集・新規保存・再openも実行した。2 tabsのままで同tabが新sourceへ変わり、100%／1.00倍、trim 0..3.201208、cleanでEndedとなった。二回目の保存PCMも同じ参照と一致し、編集の二重適用はない。
- 動画、PID 42684（開始UTC `05:33:41.7331703Z`）: 30秒1080p H.264/AACへtrim 5..9秒、crop 960×540 at 100,100、回転、Undo/Redo、Save As。保存物はH.264 540×960／120 frames／4.000000秒、AACも4.000000秒。元sourceへのtrim/crop/rotation参照に対する120 frame平均SSIM Allは0.998861783で、losslessの主張ではない。同windowで再openして実映像とEndedを確認。再生統計はD3D11VA、120 hardware/presented、CPU transfer 0、drop 0、drift p95/max=3.566/3.606msだった。AAC discarded-sample timestamp警告は残る。

全保存windowをcleanな状態で正常終了した。元PNG／chirp／動画のhashはそれぞれ`15B8DA68F777D7CAAAB816EDE7DC7364CC979CC9BFFAAB4E93D39E24B624D49D`／`4BAF8F02E0F8028C6C87349FB385F0036D27FB4C594C302E797B22198AFA1AA6`／`24FD0CE978C4BD51877A49D2FBE301C39B70E6FB2E19071A6A092651A8D3A4F6`で不変。生成物・identity・stderr・Seek sampleは専用`release-eval-20260908`以下、操作helperはignored `target/tmp`に保持した。clipboard・OS設定・旧開発DLL／releaseは変更していない。保存後のformat／Clippy／268 testsも通過し、既存3 live ignoresは未実行。CI 34190302408は成功した。

#### 30分試験の完走確認（2026-09-08 15:08 JST）

上記の全体check完了後、同じreleaseで4K60の30分試験を開始した。PID 21784、開始UTC `2026-09-08T05:37:37.6633752Z`、sourceは既存`m3-4k60-30m.mp4`（SHA256 `FEE0E738E7149225A7B4DEA02CDA75AAE6873288CBE5A1077B101829ADFD0C10`）。exe／source／DLLを起動前にhash確認し、実module pathも照合した。960×576、1倍、アプリ内mute、UIA tree取得なし。監視session 11573は同じPID／開始時刻を30秒ごとに確認し、再起動・Seek・pauseなしで1801.635秒のEndedを観測して正常終了した。アプリも後述の手順で通常終了した。

- adapter `00000000:000146b5`、D3D11VA、hardware／presentedとも107,771 frames、CPU transfer 0、drop 0。A/V drift p95 4.808ms・最大30.042msで30分基準内。
- 再生終了後のFFprobeは3840×2160 H.264、stream duration 1800.005729秒／107,771 framesを確認。先頭600秒を再decodeして35,925 framesを数えた。全30分のdropが0なので、先頭10分も0%で0.1%未満の基準内。probe session 84751は成功した。
- 全61標本、5分以降のPlayingは50標本（300.926～1771.616秒）。private memoryは区間最初224.91／最後226.64 MiB、範囲222.63～238.92 MiB。最大標本1711.576秒の238.92 MiBは次標本で226.60 MiBへ戻った。process lifetimeのOS PeakPagedMemorySize64は287.64 MiB。旧runの15分前後の約320 MiB増加は今回の標本／OS peakでは再現しなかったが、原因解明、GPU memoryやリーク不在の証明ではない。
- EOF後、UIA・終了操作より前の5.053秒idleでCPU時間増分0秒（計測分解能以下）、private memory 180.27 MiB。最初の終了helperはforeground検査でUndo送信前に停止した。同じPID／開始時刻とEndedを再確認し、既存のforeground確認付きhelperからUndoを一度だけ送り、cleanを待ってCloseMainWindowで通常終了した。強制終了・保存・source変更なし。
- 終了後にexe／source／6 DLLの全hash不変を照合した。再生中の並行local build・重いtest・大きなdownload・別UI trialはなく、小さなrecipe／文書／設定fileの監査だけを行った。対応する未解決の配布条件は[NATIVE_RUNTIME_AUDIT.md](NATIVE_RUNTIME_AUDIT.md)を参照し、性能合格を配布承認へ読み替えない。
- 通常終了とprobeの完了後に、同じnative prefix／別Rust targetでformat、全target Clippy、全268 testsを再実行して成功した（session 44068）。既存3 live ignoresは未実行で、hardware exportの条件付きtestをhardware成功の根拠にはしない。試験済みrelease exeは再buildしておらず、hashも不変。

短時間の代表flowとSeek gateを、長時間・実device／mixed-DPI／物理入力・全screen reader、対象OS、配布条件、Setup.exe、ownerの外観受入の完了へ拡張しない。H1全体は継続中である。

### 動画・readingの最大化/fullscreen復帰を実2画面で確認（2026-09-07 15:31 JST）

前項の画像window往復に続き、同じ100%倍率の横1920×1080と縦1080×1920で、通常window→最大化→F11→Escapeで最大化へ復帰→通常サイズへ復帰を確認した。各段階のGetWindowRectとIsZoomedを照合し、fullscreen captureを目視した。production変更なし。

- 通常video PID 40472（開始UTC 2026-09-07T06:28:50.8800169Z）、1080p H.264/AACの5秒でpause・アプリ内mute。両画面の往復後も5秒・Pausedを保持し、画像内timecodeも00:00:05.000。portraitでは上下余白付き、landscapeでは全画面へ16:9を保持して表示した。再Play後にEndedへ到達し、750 hardware/presented、0 CPU transfers/drops、drift p95/max=3.584/4.266ms。AAC discarded-sample timestamp警告は残るためstderr空とは記録しない。これは短時間flowであり長時間性能gateの代替ではない。
- Reading PID 45156（06:29:44.7606807Z）。既存owned PNG2枚（64×48と29×37）を新規ignored folderへcopyして読み取り専用の試験素材とした。横並び2枚で両画面を往復し、次にR/Hで縦並び・逆順として再度両画面を往復。captureは横並びの左source/右editedから、縦並びの上edited/下sourceへ切り替わり、同一方向への寸法合わせと全体fitを保持した。ownerが求める見開き送りの区切り方を承認した証拠ではない。
- 縦画面: 通常rect（2020,-250,800,600）、fullscreen（1920,-418,1080,1920）、最大化復帰（1912,-426,1096,1936）。横画面: 通常（40,40,960,576）、fullscreen（0,0,1920,1080）、最大化復帰（-8,-8,1936,1048）。最大化rectは移行前と復帰後が一致し、通常へ戻すと最初のrectも一致した。最大化の外側8pxはWin32のwindow境界であり、fullscreenにそのinsetを残したものではない。
- binary SHA256 `339046281C097BE5BA05D91BDA9D73F8501BD433A0A6AA96EF4C77E6DD87FCA2`、動画 `24FD0CE978C4BD51877A49D2FBE301C39B70E6FB2E19071A6A092651A8D3A4F6`、PNG `15B8DA68F777D7CAAAB816EDE7DC7364CC979CC9BFFAAB4E93D39E24B624D49D` / `B1C8975D52FE62D6F5C1A4DA9C98809948CFA9E0510D0624C06C5B8062AA3BEF`は前後一致。動画muteをUndoし、両windowをcleanな状態で正常終了。reading stderr空。Save/clipboard/OS設定変更なし。captures/fixtures/logs/helperはignored `target/tmp/h1-monitor-max-a5639e5`など。

format・Clippy・268 testsを再実行して通過（既存live ignore 3件は未実行）。a85e3b7のCI 34090227262成功、80108c4のCI 34090843746は確認時in_progress。画像・動画・readingの実2画面での同倍率fullscreen/復帰の証拠が揃ったが、実混在DPI、物理keyboard/pointer、screen reader、Windows 10、実endpoint切替/driver reset、配布とowner受入は未完了。既定音声出力を変える許可はまだ受領していない。

### 実機の読み取り確認と同倍率2画面fullscreen（2026-09-07 15:25 JST）

音声出力切替の許可は未受領のため、OS設定を変えずに現状を読み取った。Windows 11 Home build 26200、横1920×1080（0,0）と縦1080×1920（1920,-418）の2画面で、GetScaleFactorForMonitorはいずれも100%。混在DPIの実機gateを満たす構成ではない。

- 固定wasapi 0.24.0の読み取りAPIで、active render endpointは`Speakers (2- USB HIFI AUDIO)`と`Speakers (NVIDIA Broadcast)`の2件。Console/Multimedia/Communicationsの既定はいずれもUSB HIFI AUDIOだった。既定変更・endpoint無効化・音量変更・audio client起動は行わない。隔離したinventory toolはignored領域でoffline buildしたもので、appの依存/lockは変更していない。Broadcastは仮想endpointであり、将来の切替試験も物理unplugの証明とは分ける。
- 通常release PID 16976（開始UTC 2026-09-07T06:23:48.4273100Z）で64×48 testsrc PNGを表示。縦画面上のwindow（2020,-250,800,600）→F11（1920,-418,1080,1920）→Escapeで元のrectへ復帰。続いて横画面上のwindow（40,40,960,576）→F11（0,0,1920,1080）→Escapeで元のrectへ復帰。各rectはGetWindowRectで照合した。
- 最初のhelperはSelect Allを移動後に作ったため、選択保持の証拠にはせず、移動前に作成するよう修正して同processで再確認。両往復後も選択値left/right/top/bottom=0/64/0/48を保持した。owned window captureを目視し、縦画面では1080×810の画像を上下中央、横画面では1440×1080を左右中央へaspect-fitし、選択枠も対応していることを確認。画像データ全pixel一致の試験ではない。
- binary SHA256 `339046281C097BE5BA05D91BDA9D73F8501BD433A0A6AA96EF4C77E6DD87FCA2`、PNG SHA256 `15B8DA68F777D7CAAAB816EDE7DC7364CC979CC9BFFAAB4E93D39E24B624D49D`は前後不変。cleanな状態で正常終了、stderr空。Save/clipboard/OS設定変更なし。captures/logsはignored `target/tmp/h1-monitors-a5639e5`、inventory/helpersもignored。

製品変更なし。format・Clippy・268 testsが通過（既存live ignore 3件は未実行）。2857878のCI 34089534210は成功、a85e3b7のCI 34090227262は確認時in_progress。同倍率の実2画面での画像window移動/fullscreen復帰の証拠であり、動画/reading、最大化状態との組合せ、物理入力、混在DPI、Windows 10、実device復旧や外観のowner受入は未完了。

### 15分付近のメモリ変動をsource位置から切り分け（2026-09-07 15:17 JST）

30分試験の約909秒での一時増加について、同じ通常releaseの新規processから880秒へSeekして900秒付近を通過させ、300秒へのSeekを挟んで880秒から再通過した。PID 46480、開始UTC 2026-09-07T06:10:40.5918300Z、960×576、1倍、アプリ内mute、UIA RangeValueによるSeek。約500msごとにprocess memoryとUIAの再生位置を記録し、3条件計312標本を取得した。これはUIAなしの連続再生と同じ条件ではなく、原因確定の試験でもない。

| Seek先・観測 | 標本数 | 観測private memory最大 | Seek後10秒以降の範囲 |
| --- | ---: | ---: | ---: |
| 880秒から75秒 | 146 | 436.64 MiB | 225.76～228.14 MiB |
| 300秒から25秒 | 49 | 452.65 MiB | 226.02～226.31 MiB |
| 880秒から再度60秒 | 117 | 418.20 MiB | 224.02～227.06 MiB |

- 899～930秒の区間は各通過60標本で、最初225.76～228.14 MiB、再通過224.02～227.06 MiB。連続再生の約319.59 MiBへの増加はここで再現しなかった。単純なsource位置だけの説明は支持されないが、連続decodeの履歴、起動後の時間、driver/OS等のどれが原因かは未証明。
- Seek直後の増加は300秒でも発生して約10秒後には戻る。初回880秒では約0.56～5.69秒に414～437 MiBを観測した。古い30秒標本では捉えられない短いpeakがあるため、標本最大を全allocation上限やリークなしの証明にしない。今回の3回の4K Seekは143.123/361.814/129.582msであり、1080p 100回のp95 gateと混同しない。
- binary SHA256 `339046281C097BE5BA05D91BDA9D73F8501BD433A0A6AA96EF4C77E6DD87FCA2`、source SHA256 `FEE0E738E7149225A7B4DEA02CDA75AAE6873288CBE5A1077B101829ADFD0C10`は前後一致。sourceは前項と同じ4K60 30分MP4。muteをUndoしてcleanな状態で正常終了し、Save/clipboard/OS設定変更なし。ログはD3D11VA選択と3件のSeek完了。raw evidenceはignored `target/tmp/h1-memory-a5639e5`。
- 前の終了helperが150msで早期判定したことを踏まえ、ignored helperだけを一度のUndo送信後に最大3秒pollするように変更した。再送や強制終了はしない。製品コードは変更せず、終了後のformat・Clippy・268 testsを通過（既存live ignore 3件は未実行）。2857878のCI 34089534210は確認時in_progress。

一時増加の原因は未確定として残す。実endpoint切替は制御fault試験と別であり、OSの既定音声出力へ触る前にownerの明示的許可が必要。今回の観測を実device復旧、物理入力、対象OS、配布や外観受入の代替にはしない。

### a5639e5 releaseの30分再生再測定（2026-09-07 15:07 JST）

14:32のSeek測定と同じ通常releaseで4K60 H.264/AACをEOFまで連続再生した。PID 516、開始UTC 2026-09-07T05:34:16.1145904Z、EOF観測15:04:26 JST（起動後1810.1秒）。adapter `00000000:000146b5`、D3D11VA、960×576、1倍、アプリ内mute、試験中UIA tree取得なし。Seek・pause・再起動・並行build/重い試験なし。監視session 78186は同じPID/開始時刻を30秒ごとに照合し、Endedで正常に完了した。

- EOF logは107,771 hardware frames＝107,759 presented＋12 dropped、CPU transfer 0。全区間drop率0.011135%。A/V drift p95 4.803ms・最大36.985msで、30分の40/100ms基準を満たす。値はアプリのvideo PTSとaudio master時計の差であり、display/speakerの物理遅延ではない。
- 終了後に固定FFprobeの`-select_streams v:0 -read_intervals %600 -count_frames`で先頭600秒を再decodeし35,925 frames、headerで全107,771 frames・1800.005729秒を確認。全12 dropsを先頭10分へ割り当てても12/35,925×100＝0.033403%以下で0.1%基準内。これは正確な区間drop数ではなく保守的上限である。
- 30秒間隔61標本中、起動5分以降のPlayingは50標本。private memoryは最初222.63 MiB、最後224.37 MiB、範囲222.44～319.59 MiB。909.65秒の319.59 MiBは次の939.66秒に223.41 MiBへ戻った。OS PeakPagedMemorySize64は319.59 MiB。旧32967f9測定でも約913秒で増えて約943秒に戻っており、近い時刻の一時増加が再現した。原因は未特定で、30秒標本から全peak・GPU allocation・リーク有無を断定しない。
- EOFのprivate memoryは175.11 MiB。UIA取得や終了操作より前の5.055秒idleでCPU時間増分0.015625秒。終了helperはUndo送信150ms後にまだdirtyを観測して停止したが、同じprocessの後続確認ではclean・100%へ戻っていた。Undoを再送せず、Endedとidentityを再照合してCloseMainWindow→5秒以内の終了を確認した。再生試験の再実行や強制終了はない。
- binary SHA256 `339046281C097BE5BA05D91BDA9D73F8501BD433A0A6AA96EF4C77E6DD87FCA2`、source `tests/generated/m1/m3-4k60-30m.mp4` SHA256 `FEE0E738E7149225A7B4DEA02CDA75AAE6873288CBE5A1077B101829ADFD0C10`は試験前後で一致。事前hash済みでcold-storage試験ではない。raw evidenceはignored `target/tmp/h1-soak-a5639e5/playback.stderr.log`と`playback.samples.jsonl`。Save・clipboard・OS設定変更なし。
- 再生終了後にformat・Clippy・268 testsを再実行して通過（既存live ignore 3件は未実行）。a5639e5のCI 34086933988とde1f9d0のCI 34087264518も成功。production変更なし。

この基準機/codec/binaryでSeekと30分性能gateの証拠が揃った。今後変更したbinary、実device復旧、screen reader・物理入力・mixed-DPI、対象OS、配布とownerの外観受入へ結果を流用しない。次は約15分の一時メモリ増加がsource位置依存か起動後の時間依存かを切り分ける。

### a5639e5 releaseのSeek再測定（2026-09-07 14:32 JST）

保存修正後の通常releaseで1080p H.264/AAC、960×576、1倍、アプリ内muteを測定した。5秒前進10回/後退10回を5往復し、各要求のPresent成功logを待つ。各条件100 indicesに欠落・重複なし、各processの完了log200件と一致。p50/p95は昇順50/95番目で、全条件がM3のp95≤300msを満たす。

| 経路・状態 | 完了数 | p50 | p95 | 最大 |
| --- | ---: | ---: | ---: | ---: |
| 通常・Paused | 100/100 | 27.774ms | 31.365ms | 36.079ms |
| 通常・Playing | 100/100 | 80.829ms | 105.332ms | 118.910ms |
| UIA tree取得後・Paused | 100/100 | 26.800ms | 30.766ms | 39.137ms |
| UIA tree取得後・Playing | 100/100 | 73.231ms | 99.931ms | 108.820ms |

- 通常PID 46396（開始UTC 2026-09-07T05:29:22.9987071Z）、UIA PID 49868（05:30:17.4972159Z）。後者の最初の準備はforeground guardで入力前に停止。同じprocessのmenu Invokeで前面化した後、menuへ復帰したfocusでSpaceがmenuを開いたため、Escapeで閉じてPlayback positionへfocusしてからpause/muteした。未完了Seekの再送やprocess再起動はなく、測定開始前の準備差として記録する。
- binary SHA256 `339046281C097BE5BA05D91BDA9D73F8501BD433A0A6AA96EF4C77E6DD87FCA2`、source `tests/generated/m1/m3-1080p-h264-120s.mp4` SHA256 `DC645595A1165506BF5C3E685B14D7EA3B0116BBDFE74839E7DA5834CF60DA0C`。終了後も両hash一致。両windowはmuteをUndoしてcleanな状態で正常終了、Save/clipboard/OS設定変更なし。raw logとsampleはignored `target/tmp/h1-seek-a5639e5`。
- 各入力前にPID/開始時刻/foregroundを照合。測定境界はアプリのSeek受付からPresent成功までで、物理入力・OS配送・DWM表示時間を含まない。UIAはtree取得後であり、常駐screen readerではない。開始source位置と試行順が異なるため、条件差を純粋なUIA負荷の差と解釈しない。測定中はbuild/testを走らせず、終了後にformat・Clippy・268 testsを再実行して通過（既存live ignore 3件は未実行）。a5639e5のCI 34086933988は確認時in_progress。

この基準機・codec・binaryのSeek gateの証拠であり、旧binaryの30分測定を今回の結果へ混ぜない。同binaryの長時間drop/drift、実環境・入力・配布・owner受入は未完了。

### 保存物の再openと音声tabのsource別編集（2026-09-07 14:28 JST）

通常releaseで画像・音声・動画のSave As→再openを監査した。音声folder tabの再利用時に、保存済みのtrim/volume/rateとexport先が別sourceへ残る不具合を修正した。source変更時だけ既存navigationの履歴・保存先resetへ通し、同source再open、dirty保護、明示的新規tabと背景画像の編集保持を回帰試験で確認する。

- baseline ffb3597、binary `F5C51CF089A80CF4FFD91B591A5C2F728D5A657BDE39F0E14917BBFFBC5D8C55`。画像PID 34784（開始UTC 2026-09-07T05:03:30.4792446Z）と動画PID 33276（05:09:46.7949618Z）は保存・再openを通過。音声PID 42144（05:11:29.4067796Z）はchirp保存物の再open後も50%・1.25倍・trimが残り、約3秒の末尾でもPlaying表示を維持。headless回帰もsaved historyが残るassertionで修正前に失敗した。
- 最終binary `339046281C097BE5BA05D91BDA9D73F8501BD433A0A6AA96EF4C77E6DD87FCA2`。音声PID 49592（05:23:29.3570880Z）、画像PID 37308（05:24:30.1946506Z）、動画PID 25084（05:25:37.5126639Z）。すべて通常release、UIAとowned native dialogで操作し、各保存物を同windowで再openした。全windowをcleanな状態で正常終了した。
- PNG: 非対称64×48 testsrcからleft/right=5/42、top/bottom=7/36をcrop、時計回り回転、Undo/Redo、Save As。29×37のdecoded RGBAはFFmpeg参照と完全一致（SHA256 `ebae29bc543e282a86cde62820feb22884acc885cb642ea00e1b2c82e5cb13c2`）。さらに水平反転→Ctrl+Sで新規export先のみ更新し、再び完全一致（`effb5419b11125b40410f13e5f807d4932b1ae16a87f355ff97829c17e6f1bc1`）。再openもclean・29×37、stderr空。
- 音声: 8秒/48k mono PCMの時間変化するchirpへtrim 2..6秒、50%、1.25倍を適用。参照filterは`atrim=start_pts=96000:end_pts=288000,asetpts=PTS-STARTPTS,atempo=1.2500,volume=0.5000`。保存PCMとSHA256 `bce492efd2b806f3c44ebb088c92f1232b345fbd79aea1f649b538a9d1274190`が一致。153658 samples / 3.201208秒で、atempo出力のため理想3.2秒とsample数は同一ではない。再openは同tab・100%・1.00倍・trimなしでEndedへ到達。stderrはSoftware decode選択と正常終了統計。先行constant-tone試験はtrim位置の証明が弱く、初回参照のvolume/atempo順も誤っていたため、最終証拠にはchirpと実装順の参照を使う。
- 動画: 30秒1080p H.264/AACをtrim 5..9秒、crop 960×540 at 100,100、回転、Undo/Redo、Save As。540×960・120 frames・4.000000秒、AACも4秒。trim/crop/rotation参照に対する映像SSIM All=0.998862（losslessの証明ではない）。再openはEnded、D3D11VA 120 hardware/presented、0 CPU transfers/drops、drift p95/max=3.790/3.899 ms。AACのdiscarded-sample timestamp警告は残るため、stderr空とは扱わない。短時間の代表例であり性能gate全体ではない。
- 生成物とhelper/logはignored `target/tmp/h1-release-save-20260907-1403`などに限定。元PNG SHA256 `15B8DA68F777D7CAAAB816EDE7DC7364CC979CC9BFFAAB4E93D39E24B624D49D`、chirp `4BAF8F02E0F8028C6C87349FB385F0036D27FB4C594C302E797B22198AFA1AA6`、動画 `24FD0CE978C4BD51877A49D2FBE301C39B70E6FB2E19071A6A092651A8D3A4F6`は不変。clipboard書込許可は受領したが使用せず、OS設定も変更していない。native filenameはEditではなくPaneとして公開されるため、owned dialogのfocus IDと入力後の完全pathを確認して送信した。

全268 tests、format、Clippy、debug/release buildが通過。既存3 live ignoresは実行していない。今回の代表保存flow以外のformat/長時間性能、完全なscreen reader・物理入力・IME・混在DPI、実device遷移、配布とownerの外観承認は引き続き未完了。

### 最前面overlayと背後のfilmstrip入力を分離（2026-09-07 13:59 JST）

palette/grid/menuの背後にあるfilmstripをdisabledにし、取得済みUIA action・pointer・keyboard操作とhover説明を止める。既存opacityとpath由来ID、限定描画は維持する。Escapeはpalette/gridを先に閉じ、filmstripを残して操作を再開する。modal中の非表示契約は変更しない。

- baseline 2e376b6通常release `0B84119A7B77955D23DD7CF62FF46173007FF42BE61C89789BBBE2A8C57BC7B8`、音声PID 45064（開始UTC 2026-09-07T04:49:30.4896715Z）。playlist行→filmstrip→Escape→行focus/下矢印は通過したが、filmstrip→menuからpaletteを開いた後も背後の02.wavがenabledで、取得済みInvokeにより検索中に01→02.wavへ変わった。gridへ切替えてEscapeするとfilmstripまで消えることも確認。二つの回帰を変更前の失敗として記録した。
- 最終通常release `F5C51CF089A80CF4FFD91B591A5C2F728D5A657BDE39F0E14917BBFFBC5D8C55`、音声PID 37824（04:57:29.4349497Z）では背後の項目がdisabledとなり、取得済みInvokeはElementNotEnabledExceptionで拒否、曲は01.wavを保持した。palette取消後に同じ項目から02.wavへ選曲でき、filmstrip取消後にplaylistへ戻る。01.wavへ戻した次のflowでもgrid Escape後は三つのfilmstrip項目が残り、次のEscapeで元のplaylist行へ戻った。
- 自動回帰はpalette/grid/menu各条件のcached Click/Focus拒否、同一IDの再有効化と選曲、最前面だけの取消を確認。filmstrip単体ではdisabled前のfocus解除、左/middle click・Enter/Spaceの非実行、従来のpath安定性・現在項目no-opを確認した。267 workspace tests・format・Clippy・両buildが通過。既存live ignore 3件は未実行。2e376b6 CI 34084472949も成功。
- 両windowを通常終了し、試験に編集・Save・clipboard・OS設定変更はない。最終stderrは音声のSoftware decode選択の通常diagnostic 3件のみ。最終試験前後の01/02/03.wavはすべて同じSHA256 `0E0CD597CC65B8A0C05633352D0F8985D0F9FF02554C166771C72D6BDFF96020`で不変。helper/logはignoredの`target/tmp/h1-filmstrip-overlay*`と`h1-filmstrip-grid-baseline.log`。
- native証拠はUIA/SendKeysによる音声flowで、実pointer・全screen reader・物理入力matrixの完了ではない。次は最新binaryの代表保存/再openへ進み、最終候補性能、実IME/DPI/device、配布と外観受入を含むH1 gateを継続する。

### filmstrip開閉と移動後のfocusを接続（2026-09-07 13:46 JST）

filmstripを開く際に既存の現在項目focus要求を出し、呼出元widgetとmedia読み込み世代を保持する。同世代で閉じると呼出元へ戻し、別mediaへ移った場合は現在tab、tabのないWelcomeではlogo、fullscreenでは既存Exit操作部へ一回だけ戻す。palette/gridからの呼出はその元の復帰先を引き継ぎ、gridから開く時はgridを閉じる。選曲・Shell順・編集・runtime・外観配置は変更しない。

- baseline f41a1f9通常release `502F5E34DFCDD6768D591316D3B168E7A3BE1A1CC5F2A89296BF481343279347`、画像PID 48488（開始UTC 2026-09-07T04:37:41.6256287Z）で選択左辺→F→現在filmstrip項目へUIA Focus→Escape。focusが消え、Tabで02.pngへ移って閉じる場合もfocusがなかった。新しい回帰は開いた現在項目のfocusで失敗した。
- 最終通常release `0B84119A7B77955D23DD7CF62FF46173007FF42BE61C89789BBBE2A8C57BC7B8`、画像PID 35236（04:44:04.0350662Z）では、現在項目への自動focus、取消後の選択左辺復帰と1 pixel矢印調整を確認。01→02.png移動後は現在tab、02→03.pngのfullscreen移動後はExit fullscreenへ戻り、Enterで通常windowへ復帰した。
- 同じ最終windowの03.pngを回転して全体選択→filmstrip→Tabで未保存確認→Escape取消。現在項目と編集を保持し、次のEscapeで左辺へ戻って調整できた。試験回転はCtrl+ZでUndo。両windowを通常終了し、最終stderrは空。01.png SHA256 `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`は不変。Save・clipboard・OS設定変更なし。
- 回帰は現在項目への初期focus、palette/gridからの引継ぎ、dirty guard取消、読み込み世代変更後の現在tab、復帰後の手動focus保持、fullscreen/Welcome fallbackを確認。266 workspace tests・format・Clippy・両buildが通過。既存live ignore 3件は未実行。f41a1f9 CI 34083748633も成功。native試験はSendKeys/UIAによる画像flowで、物理入力や全mediaのscreen-reader matrixではない。
- helper/logはignoredの`target/tmp/h1-filmstrip-return*`。最終候補の保存・性能gateは再測定していない。次は音声playlistのfilmstrip往復と重なったoverlayの入力・読み順を監査し、実入力/IME/DPI/device・配布・外観受入を含むH1 gateを継続する。

### grid取消の一回Escapeとfocus復帰（2026-09-07 13:34 JST）

grid buttonのfocus中にEscapeがUIへ消費され、gridが残る問題を修正した。draw開始時にmenu/modal/paletteの優先状態を確認して取消を処理し、進行中prefixの取消も優先する。同時表示しないgrid/paletteの復帰先を一件だけ共有し、両者の切替でも最初の操作部を保持する。通常commandの実行は復帰先を破棄し、command自身のfocusを優先する。filmstripの復帰契約やruntimeは変更しない。

- baseline 1f941c9通常release `6263CBEBE36210EC2CF6A26B12DD069F775AB4795B689CC80819851900982711`、画像PID 29240（開始UTC 2026-09-07T04:21:34.5454668Z）で全体選択→左辺focus→G→grid回転buttonへUIA Focus→Escape。gridが残り、左辺focusもない。追加回帰も一回取消のassertionで失敗した。
- 中間release `CC174DCC7789F6D6FC23A7AD5D6E527FCD30731D4139C31CC145E4C797863CCB`、PID 41316（04:26:38.4022882Z）は単独gridの取消を通過したが、上のlogo menuを閉じるとgridまで閉じた。menu描画後に判定していたためで、同じ失敗を回帰へ追加してdraw開始時の判定へ移した。prefixの優先も追加回帰の失敗を確認してから修正した。
- 最終通常release `502F5E34DFCDD6768D591316D3B168E7A3BE1A1CC5F2A89296BF481343279347`、PID 32256（04:32:47.2101420Z）でgrid取消→左辺への復帰→矢印1 pixel調整を確認。四辺とも取消前後の値不変・focus復帰が成立し、bottomではlogo menuだけを先に閉じ、次のEscapeでgridを閉じた。gridのSによる回転→grid close→Ctrl+Zも成立した。SendKeysによる通常window確認であり、物理keyboard matrixの完了ではない。
- 同じ最終windowで既存paletteの四辺focus復帰・検索文字の隔離・矢印調整・command固有focusを再確認。準備用Select helperも旧`Edit *`検索で通知を拾って一度失敗したため、実treeを再確認しButton型に限定して再試験した。このhelper失敗は製品の退行として数えない。
- 自動回帰はgrid/palette切替、grid上のmenu、prefix、custom gridのpalette/自身のtoggle/回転commandを含む。265 workspace tests・format・Clippy・両buildが通過。既存live ignore 3件は未実行。前回1f941c9と4d75355のCIも成功。custom grid設定は隔離test内だけで変更し、利用者設定は変えない。
- 三つのwindowを通常終了、試験回転はUndo済み、最終stderrは空。PNG hash `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`は不変。Save・clipboard・OS設定変更なし。helper/logはignoredの`target/tmp/h1-grid-*`。性能・最終保存gateの再測定ではない。
- 次はfilmstripの取消と選曲後のfocusを監査する。全screen reader・物理入力/他IME・実DPI/device・最終候補保存/性能・配布・外観受入を含むH1 gateは継続する。

### 連続menu操作の誤判定を訂正（2026-09-07 13:19 JST）

前二項で残したcategory focus/展開の不成立は、native helperの対象選択に原因があった。`Edit *`を名前だけで検索すると、回転後の通知Text `Edit added (source unchanged)`を先に取得する。実際のEdit buttonはfocusを持っていても、通知へのHasKeyboardFocus検査・SetFocus・Invokeは失敗する。対象のButton型も照合し、アプリ側のmenu処理は変更しない。

- clean HEAD 4d75355、通常release SHA256 `6263CBEBE36210EC2CF6A26B12DD069F775AB4795B689CC80819851900982711`、画像PID 11656（開始UTC 2026-09-07T04:14:13.3419098Z）で旧helperの失敗を再現。段階ごとのtree取得では通知とfocus中のEdit buttonが同時に存在し、先頭Fileへの復帰自体も成立していた。失敗した試験の回転はEscape/Ctrl+Zで戻してから再試行した。
- 同じwindow/binaryで型照合したhelperによる回転→再open→Undoを3周通過。menu→回転→File/Close tab→Cancel→Edit/Undoも計4周通過し、最後のmenu→palette→Escape→Enter再openも成立。すべての編集をUndoして通常終了、stderrは空。PNG SHA256 `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`は不変。Save・clipboard・OS設定変更はない。
- 既存のpalette/live tree回帰を画像の3周操作へ拡張。毎passのfocus node存在、再open先頭File、上下階層移動、通知との役割区別、dirty確認/Cancel保持、logo復帰とUndoを確認する。最初の通知assertionはraw AccessKitのlabelだけを検査して失敗し、Textのvalueも照合して通過した。これはhelper/test条件の訂正であり製品不具合の修正前失敗ではない。
- 264 workspace tests・format・Clippyが通過。既存live ignore 3件は未実行。変更はtestと文書のみで、通常binaryを再build/性能再測定したものではない。helper/logはignoredの`target/tmp/h1-menu-repeat*`。前回checkpointのCI 34082273561は確認時in_progress。
- 次はgrid/filmstripの取消・command実行をまたぐfocusと日常操作を監査する。全screen reader・物理入力/他IME・実DPI/device・最終候補保存/性能・配布・外観受入gateは継続する。

### ボタンfocus中の通常shortcutを回復（2026-09-07 13:07 JST）

egui-winitがfocus中のkeyをまとめて消費する前に、文字入力でない通常controlから有効な現在binding/prefixを共通command処理へ渡す。Space・矢印・Home/End・Escapeの修飾なし/Shift付き操作とTabはUI側へ残す。進行中prefixの続きは既存shortcut処理へ渡す。palette/menu/grid/filmstrip/modal/TextEditは除外し、seek/trim/selectionの値keyとsynthetic key除外を維持する。

- a55cd1d通常release `E6B8A24641057BB8146DFD2883E58E9C39C81B0E88641A40EBE996595B6EC0E6`、画像PID 40548（開始UTC 2026-09-07T04:01:00.7113017Z）でmenu Escape後のlogo focusを確認してR。focusは残るがdirtyにならず回転しないことを再現。新しい回帰もボタンfocus時のR所有権assertionで失敗した。
- 修正後通常release `6263CBEBE36210EC2CF6A26B12DD069F775AB4795B689CC80819851900982711`、画像PID 17424（04:05:17.4757756Z）でR→Ctrl+Zが成立。logo・Reading mode button・tab本体の三箇所でfocusを保ったままdirty/cleanへ遷移した。logoのSpaceはmenuを開き、Ctrl+Shift+Pはpaletteへ移る。検索欄へrotateを入力してEscapeしても画像はdirtyにならなかった。
- 同じwindowで四辺の値・Tab/矢印、398×560 crop/Undo、回転後の値範囲、未保存確認中の取得済みSetValue拒否・Cancel保持を再確認。log中のMethodInvocationExceptionはElementNotEnabledExceptionを含む期待された拒否であり、helperのassertionを通過している。全試験編集をUndoし、両windowを通常終了。stderrは空、PNG hash `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`は不変。
- 回帰はR/Undo、現在のK割当と旧Rの非alias、Ctrl+K Space prefix、取消/不一致keyの配送、未割当・UI navigation/activationの除外、五つのoverlay条件、TextEdit非介入、selectionの既存key所有権を確認。264 workspace tests、format・Clippy・両buildが通過。既存live ignore 3件は未実行。a55cd1d CI 34081422762も成功。custom設定のnative再試験や物理入力matrix全体を完了した意味ではない。
- helper/logはignoredの`target/tmp/h1-button-shortcut*`。Save・clipboard・OS設定変更なし。前項の連続menu再open時のcategory focus/展開の不成立は未解決として次に監査する。全screen reader・実DPI/device・最終候補保存/性能・配布・外観受入gateも継続する。

### menu→palette取消時のaccessibility panicを修正（2026-09-07 12:56 JST）

メニューのcommand選択時は、消える項目ではなくlogoへfocusを引き継いでからdispatchする。paletteや未保存確認の取消先は有効なlogoとなり、SelectAllなどcommand固有のfocus先はその後で優先される。加えて固定eguiの完全root treeを配送する直前に、存在しないfocusをrootへ戻し、同じ古いegui focusを解除する。node一覧・有効なfocusを変更せず、accessibility無効時は何もしない。

- 72d09a9通常release `71EA1F8210E693472BE7EC40F7619B475C93E4515D7826534F1C4D57CE80D166`、Welcome PID 4828（開始UTC 2026-09-07T03:43:34.9037487Z）でmenu→View→Show command palette→zoom→Escape。process消失とstderrの`Focused ID ... is not in the node list` panicを確認した。helper末尾のnull focus/空titleは成功証拠ではなく、以後はEscape後も同一processの生存を再確認するようにした。baselineはpanic終了であり通常終了とは記録しない。
- 最初のheadless keyboard列はpaletteを開けず再現証拠から除外。nativeと同じUIA Click列へ合わせると、focusが同じ出力のnode一覧にないassertionで失敗した。修正後の往復回帰は出力補正に頼らず各frameのfocus存在と最後のlogo復帰を確認する。境界検証の別testでは、描画中に消えたIDをfocusした不整合treeを作り、rootへの補正・egui focus解除・node不変、有効focusとaccessibility無効時の非変更を確認した。
- 最終通常release `E6B8A24641057BB8146DFD2883E58E9C39C81B0E88641A40EBE996595B6EC0E6`、Welcome PID 36344（03:49:51.4909381Z）と画像PID 13852（03:51:11.1352716Z）で同じpalette往復後も生存し、logo focusとEnter再openを確認。画像ではmenuからのdirty close→Cancel→logo復帰も一度確認し、試験用回転はmenuのUndoで戻した。
- 追加操作ではlogo focus中のRが編集へ届かなかった。また連続するmenu再openのhelperで、submenuが見えない、category SetFocusを受け付けない、keyboard focus確認が成立しない例を観測。途中状態を再確認し、Undoしてから次を試した。成功した再開試験と失敗した連続試験を区別し、この横断flowは未完了として次に監査する。
- 6 menu testsと境界検証、263 workspace tests、format・Clippy・両buildが通過。既存live ignore 3件は未実行。72d09a9 CI 34080542225と012cdf2 CI 34080064221も成功。最終二windowは通常終了、stderrは空、PNG hash `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`は不変。helper/logはignoredの`target/tmp/h1-menu-palette*`。Save・clipboard・OS設定変更なし。今回の終了修正は、残る操作・全screen reader・実環境・最終候補保存/性能・配布・外観受入gateの代替ではない。

### logo menuのEscape後にkeyboard操作を継続（2026-09-07 12:40 JST）

親menu・submenuをEscapeで閉じ、commandを実行していない時だけlogo buttonへfocusを戻す。次のEnter/Spaceで再openでき、Tabで離れた後や背景clickではfocusを戻さない。command registry・配置・外観・input bindingは変更しない。

- 012cdf2通常release `D263A831F4554302B5E92A13A67BEE66F7D720D7A686F5BFC7CDBD02107BB851`、Welcome PID 21444（開始UTC 2026-09-07T03:35:03.5442734Z）でlogo Focus→Enter→Escape、およびEnter→Right→Escapeの両方でlogo focusがなくなることを再現。新しいapp回帰も同じassertionで失敗した。
- 修正後通常release `71EA1F8210E693472BE7EC40F7619B475C93E4515D7826534F1C4D57CE80D166`、Welcome PID 28972（03:38:42.8942991Z）と画像PID 42732（03:40:24.2505714Z）で同手順が通過。親menu/submenuの取消後にlogoのUIA focusがtrueとなり、Enter/Spaceで再openできた。Welcomeのowned capture `target/tmp/h1-menu-return-focus.png`でfocus枠も確認した。
- 画像windowでは続けてmenuのSelect whole mediaをInvokeし、600×800の全体選択と左辺focusを確認。command実行のfocusをlogoへ戻さず、dirty編集も作らない。両windowとbaselineは通常終了、stderrは空、PNG hash `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`は不変。
- 5 menu testsと261 workspace tests、format・Clippy・両buildが通過。既存live ignore 3件は未実行。回帰はroot/submenu、Enter/Space再open、Tab後のidle focus保持、背景clickとの区別を含む。最初のrelease buildはまだ起動中のbaseline binaryの更新で拒否されたため、その同一windowの通常終了を確認してから再buildした。強制終了・削除・履歴変更は行っていない。
- helper/logはignoredの`target/tmp/h1-menu-return*`。Save・clipboard・OS設定変更なし。012cdf2 CI 34080064221は確認時進行中。これはscreen reader全体、各overlayを連続して開くflow、実入力/DPI/device、最終候補保存/性能・配布・外観受入を完了させるものではない。

### 未保存確認Cancel後のfocus復帰（2026-09-07 12:32 JST）

現在tabの未保存確認を開く時にwidget/tabのidentityを一件保持し、取消後、eguiの前passのmodal制限が消えてから一回だけfocusを戻す。確認中の再要求・復帰描画前の再open・Save As取消で上書きせず、離脱確定やmedia loadでは破棄する。全modalの共通focus stackを追加する変更ではない。

- 9dce606通常release `3CB71A59516DFB2E66ABA302C9489B2C580FBC81CEA6058D3D3400CFDC602986`、画像PID 32316（開始UTC 2026-09-07T03:20:12.5813341Z）でR→Ctrl+A→各辺Focus→Ctrl+W→EscapeまたはCancel Invoke。編集と値は残るが四辺ともfocusが失われた。app回帰でも同じ失敗を再現した。
- 最初の直接request_focusは、前passのmodal制限によって次の描画で消えることを回帰で検出。解除後の描画まで保持するよう修正した。中間通常PID 13544（03:24:51.5622632Z、binary `25EC3CCD1AD04C66B45B34D05654E234D7950D35DEDC4CB9B01DEDD3E15F9B8F`）で四辺の往復が通過。さらにCancel直後、復帰描画前の再確認で保持IDを失う回帰を追加して修正した。
- 追加回帰前の通常release `B25156091D6EA4FC7F57B8F36B6C3311F0F197D7CF20BEC774673511E73D70C5`、画像PID 49880（03:27:05.5321050Z）と動画PID 4532（03:28:02.7561199Z）で四辺の同手順が通過。確認中は背景Slider無効、Cancel後は元の辺を画像1px・動画2pxずつ調整でき、回転編集とmediaを保持した。
- 画像ではExport and continue→実Save AsのEscape→確認Cancelも確認し、右辺focusとdirty編集を保持。file名の入力・保存は実行していない。owned capture `target/tmp/h1-guard-return-focus.png`で右辺focus枠を確認し、落ち着いた後の5秒CPU時間増分は0秒だった。性能gate全体の再測定ではない。
- 回帰はegui EscapeとResolveGuard Cancel、4倍画像のfocus/reveal、確認中の再要求、描画前の再open、非同期picker取消callback、矢印・履歴保持、Discard後の隣tabへ旧focusを持ち込まないことを検証。260 workspace tests、format・Clippy・両buildが通過。既存live ignore 3件は未実行。9dce606 CI 34079199258も成功。
- 隣tab復帰の追加回帰後、必須checkを再実行し、最終通常releaseのhashは`D263A831F4554302B5E92A13A67BEE66F7D720D7A686F5BFC7CDBD02107BB851`となった。このbinaryの画像PID 22044（03:31:25.1240169Z）と動画PID 29036（03:32:15.9901566Z）でも四辺の同手順を再確認し、画像は実Save As取消からの復帰も通過。最終owned capture `target/tmp/h1-guard-return-verified-focus.png`を目視確認し、画像の5秒idle CPU増分も0秒だった。
- 全六windowで試験用回転をUndoして通常終了。画像stderrは空、動画はD3d11va選択診断のみ。PNG hash `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`、動画 `DC645595A1165506BF5C3E685B14D7EA3B0116BBDFE74839E7DA5834CF60DA0C`は不変。helper/logはignoredの`target/tmp/h1-guard-return*`。Save・clipboard・OS設定変更なし。全screen reader、実入力/DPI/device、最終候補保存/性能、配布・外観受入は引き続き未完了。

### パレット取消後の選択操作を継続（2026-09-07 12:18 JST）

選択辺をfocus→Ctrl+Shift+P→検索→Escape→矢印というflowで、取消後に元の辺へfocusを戻す。保持するのは直前のwidget ID一件だけで、command実行・file dropでは破棄する。全modalのfocus stackやscreen reader全体を実装したものではない。

- 88aaa73通常release `0EB87A49E11F0FA089B56AE5E83E7875D8CA781594AF0ED952F9E84A81320B31`、画像PID 20664（開始UTC 2026-09-07T03:10:13.0993746Z）で左辺100pxからpaletteを開き、rotate→Ctrl+A→crop→Escape。値は保持するが四辺ともfocusがなくなった。既存app回帰へ右辺からの往復を追加すると、元の右辺へ戻るassertionで失敗した。
- 修正後通常release `3CB71A59516DFB2E66ABA302C9489B2C580FBC81CEA6058D3D3400CFDC602986`、画像PID 43088（03:13:31.0989354Z）と動画PID 48740（03:15:34.7820676Z）で同じflowを四辺それぞれ確認。取消直後は値と対象を保持し、次の矢印で画像1px・動画2pxだけ変更する。検索欄のCtrl+Aは文字だけへ作用し、media移動やdirty編集を起こさない。paletteからSelect whole mediaを実行した後は左辺へfocusし、次の取消でも古い右/下辺へ戻らない。
- 画像は100%でTab/逆Tab・最小panと手動(-300,+60)保持・同じ辺へのUIA再Focusも再確認。その後のpalette往復とowned capture `target/tmp/h1-palette-return-focus.png`を確認した。動画は1716×878 crop→Undo→1920×1080・cleanを確認。全三windowは通常終了、画像stderrは空、動画はD3d11va選択の診断のみ。これは再生性能の測定ではない。
- 10 selection tests、260 workspace tests、format・Clippy・両buildが通過。既存live ignore 3件は未実行。回帰はegui Escapeとapp側dismissの両入口、palette再open、4倍画像での復帰/reveal、commandによるfocus切替と古い復帰先破棄を含む。88aaa73 CI 34078410677も成功した。
- PNG hash `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`、動画 `DC645595A1165506BF5C3E685B14D7EA3B0116BBDFE74839E7DA5834CF60DA0C`は不変。helper/logはignoredの`target/tmp/h1-palette-return*`。Save・clipboard・OS設定変更なし。次は保存確認Cancelなど残る画面間focusを監査し、全screen reader・実入力/DPI/device・最終候補保存/性能・配布/外観受入gateを継続する。

### 拡大画像の選択handleへfocusを追従（2026-09-07 12:02 JST）

focus移動・明示的なUIA Focus・Select whole media・値変更で、対象handleとfocus枠がviewportへ入る最小panだけを適用する。倍率・選択pixel・履歴は変えず、手動panや通常再描画では自動で戻さない。pointer button保持中・無効なcontrolのrevealは消費して破棄し、後から再開しない。動画にzoom/panを追加する変更ではない。

- 42ae83e通常release `11FD1824A03D313820ED8FDCB6CD40825B2BE94251224579A3A9523B553AB9C2`、PID 48216（開始UTC 2026-09-07T02:53:16.5674727Z）で600×800 PNGをCtrl+Hの100%表示にし、全体選択の左→右→上へTab移動。上辺はfocusありだがscreen Y=116、window上端234で画面外だった。4倍画像の回帰でも左辺bounds x=-327で失敗した。
- 最初の修正はfocus変化と値変更で追従したが、同じ辺をfocusしたまま手動pan→UIA再Focusすると戻らなかった。中間PID 15340（02:58:27.3912286Z、binary `20DA287C99AC798549015C3415930F7DE19FD6781338688320FCA2A5F53FA7B9`）と追加回帰で再現し、eguiが消費する前に明示Focusも確認するよう修正した。
- 最終通常release `0EB87A49E11F0FA089B56AE5E83E7875D8CA781594AF0ED952F9E84A81320B31`、PID 40316（03:00:52.6367912Z）でTab/Shift+Tab、UIA Focus、bottomの値変更、Ctrl+Aを確認。top/bottomの14px boundsはwindow基準Y=43/521となり、title/statusを避けた領域内へ入った。手動右dragの(-300,+60)はrelease後も保持し、同じ左辺へのUIA再Focusだけで戻った。100%とsource/選択の値を保持するowned capture `target/tmp/h1-selection-reveal-complete.png`も目視確認した。
- 同じprocessで既存398×560 crop/Undo、回転後の値範囲、dirty guardのdisabled値要求拒否/Cancel、paletteのCtrl+Aを再確認。落ち着いた後の5秒idle CPU時間増分は0.015625秒であり、0とは記録しない。三つのtrial windowはすべてcleanで通常終了、stderrは空、PNG hash `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`は不変。Save・clipboard・OS設定変更なし。
- 4倍で四辺のbounds・最小移動量・選択/倍率/履歴保持・同じ辺への再Focus・idle/manual pan非追従を確認。reveal要求の一回消費・disabled/button保持による破棄も回帰で確認し、10 selection tests、260 workspace tests、format・Clippy・debug/release buildが通過した。既存live ignore 3件は未実行。helper/logはignoredの`target/tmp/h1-selection-reveal*`。window resize/実DPIやscreen reader全体・最終候補性能/保存・配布/外観受入の代替とはしない。

### 選択範囲のkeyboard／UIA操作（2026-09-07 11:48 JST）

画像・動画のSelect whole media（既定Ctrl+A）をEdit/menu/palette/custom bindingへ加え、既存の四辺へpixel Sliderを公開した。選択作成はdirtyにせず左辺へfocusし、Tab・矢印・Home/Endと数値操作から既存crop/Undoへ接続する。readingでは無効、検索欄のCtrl+Aは文字選択のまま。新しいtoolbarや常設panel、runtime/dependencyは追加していない。

- baseline `7E198DCB66E9B2809E3C7D242A3EFC41A1918250A1E93397395CC674B266F08B`、PID 32144（開始UTC 2026-09-07T02:26:00.5325245Z）には選択の名前付き操作がなく、新規回帰も同じ欠落で失敗した。foreground取得に失敗した二回はkeyを送っておらずCtrl+Aの実測とは扱わない。baselineはcleanで通常終了。
- 最終通常releaseはSHA-256 `11FD1824A03D313820ED8FDCB6CD40825B2BE94251224579A3A9523B553AB9C2`。画像PID 17468（02:47:13.5057273Z）、動画PID 40944（02:48:02.5986783Z）。既存menuのUIA Invokeから全体選択を作り、その後は同一PID/開始時刻とforegroundを照合したkey入力・取得済みRangeValuePatternで操作した。
- 600×800 PNGで四辺を101/499/120/680へ設定し、crop後の再選択が398×560を公開すること、Undo後600×800・cleanへ戻ることを確認。Tabが左辺から右辺へ移り、各矢印は1 pixel、逆転要求は元の値を保持する。回転後は800×600へ追従し、未保存確認中の取得済み値操作はElementNotEnabledException、Cancelで編集保持、Undoで復帰した。選択の値変更中は同じRuntimeIdを維持する。
- 動画は1920×1080、2 pixel step。奇数の数値要求を偶数へ丸め、左辺のRight後104/1820/102/980から1716×878へcropし、Undo後1920×1080・cleanへ戻った。SpaceによるPauseも選択focus中に使える。画像・動画ともSaveせずsourceを保持し、最終両windowは正常終了した。短いD3D11VA診断は長時間性能やCPU-transfer gateの再測定ではない。
- pointerでは全体選択の画像端handle中心の丸めで外側に落ち、dragが始まらない問題も再現した。既存辺のhitだけはsurface内の画像外側から許可し、新規選択は引き続き画像内からに限定する。四辺×画像/動画×4通りのevent配送を回帰に追加し、最終画像windowでも左辺の外側2pxからのdrag→Escape→Ctrl+Aが通過した。surface自体のclipを越える操作やzoomで画面外となる辺のfocus追従までは確認していない。
- 最終画像windowでpaletteへrotateを入力、Ctrl+A→cropへ置換してからEscapeを押しても、元のselection左辺100pxは保持された。focus枠・既存shade/handleの最終owned capture `target/tmp/h1-selection-complete-focus.png`を目視確認した。
- 試作中のinput lock内focus照会はdebug回帰で停止を検出し、照会をlock外へ移して修正した。中間image PID 32096のcrop寸法を一時statusのUIA文字から読む試験は成功証拠から除外し、同じprocessを再確認してから実際のcrop後pixel boundsで検証し直した。描画整理前のPID 33400/24404を含め、中間windowもすべてUndo後に通常終了した。
- 最終format・Clippy・259 tests・debug/release buildは通過、既存live ignore 3件は未実行のまま。PNG SHA-256 `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`、動画 `DC645595A1165506BF5C3E685B14D7EA3B0116BBDFE74839E7DA5834CF60DA0C`は前後同一。helper/log/captureはignoredの`target/tmp/h1-selection*`。clipboard/OS設定変更なし。screen reader全体、実入力/DPI/device、最終候補の性能/保存・配布・owner受入は未完了。

### 32967f9 releaseの30分再生再測定（2026-09-07 11:19 JST）

通常releaseの同一processで4K60 H.264/AACをEOFまで連続再生した。PID 44652、開始UTC 2026-09-07T01:44:14.9795225Z、EOF観測11:14:28 JST（起動後1813.7秒）。adapter `00000000:000146b5`、D3D11VA、960×576、1倍、アプリ内mute、UIA tree取得なし。Seek・pause・再起動・並行したbuildや重い試験はない。事前hash済みsourceなのでcold-storage試験ではない。

- EOF logは107,771 hardware frames＝107,758 presented＋13 dropped、CPU transfer 0。全区間drop率は0.012063%。A/V driftはp95 4.704ms・最大30.109msで30分の40/100ms基準内。アプリのvideo PTSとaudio master時計の差であり、display/speakerの物理遅延ではない。
- 終了後、固定FFprobeの`-select_streams v:0 -read_intervals %600 -count_frames`で先頭600秒を再decodeし35,925 frames、headerの全frame数107,771を確認した。全13 dropsを先頭10分へ割り当てても13/35,925×100＝0.036187%以下で0.1%基準内。正確な区間drop数ではなく保守的上限である。
- 30秒間隔の61標本中、起動5分以降のPlayingは50標本。private memoryは最初223.16 MiB、最後225.42 MiB、範囲223.16～332.19 MiB。912.97秒の332.19 MiBは次の942.99秒に223.99 MiBへ戻り、OSのPeakPagedMemorySize64も332.20 MiBだった。一時増加の原因は未特定で、粗い標本から全peak・GPU allocation・リークの有無を断定しない。EOF標本は177.52 MiB。
- 途中の終了操作のtool出力が失われたため、11:19に同じPID/開始時刻とEnded・clean titleを再確認した。Undoを再送せず、落ち着いたEOF状態で5秒間のCPU時間増分0秒、private 179.48 MiBを測定。CloseMainWindowがtrueを返し、5秒以内のprocess終了を確認した。終了codeは取得できておらず0とは記録しない。再生試験の再実行や強制終了はない。
- raw evidenceはignoredの`target/tmp/h1-30m-gate-32967f9.stderr.log`と`.samples.jsonl`。sourceは`tests/generated/m1/m3-4k60-30m.mp4`、SHA-256 `FEE0E738E7149225A7B4DEA02CDA75AAE6873288CBE5A1077B101829ADFD0C10`。binaryは下記Seekと同じ`7E198DCB66E9B2809E3C7D242A3EFC41A1918250A1E93397395CC674B266F08B`で、終了後の両hashも一致した。format・Clippy・256 testsを再実行し通過（既存live ignore 3件は未実行のまま）。9bfc6f8のCI 34074097655も成功。Save・clipboard・OS設定変更なし。これは現在binary/基準機/単一fixtureの再確認であり、最終候補やscreen reader・実環境・配布のgateを代替しない。

### 32967f9 releaseのSeek再測定（2026-09-07 10:45 JST）

production変更なし。同一1080p H.264/AAC fixture、960×576、1倍、アプリ内muteで各要求のPresent完了を待ち、5秒前進10回/後退10回を5往復した。各modeの100 indicesは重複せず、各processの完了logも200件と一致する。p50/p95は昇順50/95番目。測定中のbuildや重い試験、Seekの再送・process再起動はない。

| 経路・状態 | 完了数 | p50 | p95 | 最大 |
| --- | ---: | ---: | ---: | ---: |
| 通常・Paused | 100/100 | 27.913ms | 33.651ms | 39.654ms |
| 通常・Playing | 100/100 | 84.527ms | 103.382ms | 134.114ms |
| UIA tree取得後・Paused | 100/100 | 41.630ms | 57.750ms | 61.900ms |
| UIA tree取得後・Playing | 100/100 | 78.309ms | 103.416ms | 108.045ms |

- 全4条件でM3のp95≤300msを満たす。通常PID 6328（開始UTC 2026-09-07 01:37:39.0263612Z）、UIA tree取得後PID 15392（01:39:50.5450183Z）。後者はSliderを取得してから通常keyで測定し、固定accesskit_winit 0.32.2のWindows backendがdeactivation handlerを使用しないこともsourceで確認した。常駐screen readerのイベント処理/音声出力負荷を加えた試験ではない。試行順・開始source位置の差があり、二経路の差を純粋なUIA overheadとは断定しない。
- 計測はアプリのSeek受付からPresent成功まで。foreground/PID/開始時刻を各入力前に照合し、Altによる他windowへの入力を使わない。物理keyboard/OS配送/DWM走査表示の時間は含まない。両windowはmuteをUndoしcleanで正常終了。sourceの前後hashは同一、Save/clipboard/OS設定変更なし。raw log/sampleはignoredの`target/tmp/h1-seek-gate-32967f9*`。
- binary SHA-256は`7E198DCB66E9B2809E3C7D242A3EFC41A1918250A1E93397395CC674B266F08B`、sourceは`DC645595A1165506BF5C3E685B14D7EA3B0116BBDFE74839E7DA5834CF60DA0C`。この基準機/codec/単一試行の現在release測定であり、今後変更した最終候補へ流用しない。format、Clippy、256 testsも再確認した。既存live ignore 3件は残る。
- 続く30分4K60試験は同じbinary、source `FEE0E738E7149225A7B4DEA02CDA75AAE6873288CBE5A1077B101829ADFD0C10`で開始した。開始12.5秒のsampleはPlaying、private memory 244.81 MiB。その後のEOFと終了確認は上の11:19記録を参照し、開始時点の未判定を現在の状態として扱わない。

### overlay中の背景playlist操作（2026-09-07 10:34 JST）

- 50b209d CI 34044338058は成功。前回captureの背景hoverを調べ、基準release `294DCD4ED37A2476F600713CEFE60146ADBAE59A388E55E6850F893C8219EAE4`、PID 45748（開始UTC 2026-09-07 01:26:38.7421313Z）でfilmstrip中の背景行がenabled、pointer clickで002、cached UIA Invokeで003へ選曲できることを確認した。
- playlistの既存wheel制限をUi全体の入力可否へ揃え、menu popupも含める。opacityを保持して配置/暗幕を変えない。回帰は背景enabledで失敗し、修正後はfilmstrip/palette/grid/modal/popupの5条件で同じID/位置、disabled、click/Invoke拒否、tooltip不在と解除後の再有効化が通過。途中のtooltip失敗はsynthetic popupが維持されない試験条件が原因だった。fixtureを修正し、不要だったtooltip側の実装変更は取り除いた。
- 最終release `7E198DCB66E9B2809E3C7D242A3EFC41A1918250A1E93397395CC674B266F08B`、PID 42088（01:33:15.0284285Z）では背景click後も001を保持し、cached InvokeはElementNotEnabledException。filmstripの002は選曲でき、Escape後は同じcached playlist参照から003へ移動できた。所有capture `target/tmp/h1-playlist-overlay.png`で背景行のhover枠/tooltipがないことを目視確認した。
- 7 playlist tests、256 workspace tests（app 140/core 36/runtime 76/integration 4）、format、Clippy、両buildが通過。既存live ignore 3件は残る。両windowは正常終了し、既存silent fixtureを使用、Save/clipboard/原本/OS設定変更なし。ログのSoftware path/zero video metricsは性能gateではない。全screen reader/selection、実入力/DPI/device・最終候補/配布gateは継続する。

### filmstripのTab focus追従とEscape（2026-09-07 01:04 JST）

- 前turnの1d64216はpush済み、作業開始時worktreeはclean。CI 34043801020は作業中に成功。基準release `7A9A6585AC63BBBC7284443333811F67E4F398E91535DCDD2C05115D161FDE3C`、PID 43388（開始UTC 2026-09-06 15:56:32.4766748Z）で所有50曲のfilmstrip 001へUIA Focus後、Tabで002を再生してもfocusは001に留まった。
- Tabのguard付きNavigate後に一回の現在項目focus要求を追加。5万項目の回帰でsnapshot待ち・初回Area sizing、先頭から末尾へのfocus、最大9可視項目、再描画時の別focus保持とclearでの取消を確認した。新しい移動commandや全件描画は追加しない。
- 中間release `EE599CEB76BE73B401A1C5386D4781BDF5055F1CC5A1FC09E1493F235D8A2EA3`、PID 20280（16:00:51.2199388Z）で002～014のTab/逆移動focusは一致したが、Escapeはfocusだけを解除してfilmstripを残した。後続UIA treeにも009～017のfilmstrip Buttonが存在する。この入口も修正した。最初の試行はforeground確認で入力前に停止し、同じPID/開始時刻を確認して再試行した。基準試行のEscape後014照会は元から可視範囲外なので、閉じた証拠には含めない。
- 最終release `294DCD4ED37A2476F600713CEFE60146ADBAE59A388E55E6850F893C8219EAE4`、PID 24540（16:02:51.9426810Z）で同じ連続Tab→014、Shift+Tab→013、Escape一回でfilmstrip消失が通過。013をmuteでdirtyにして再open・Tabすると保存確認へ入り、Cancelで013/編集を保持して013のfocusへ戻る。Escape、Undoでclean、正常終了まで確認した。keyboardは所有foregroundへの入力注入であり物理入力ではない。
- 5 filmstrip tests、255 workspace tests（app 139/core 36/runtime 76/integration 4）、format、Clippy、debug/release buildが通過。既存live ignore 3件は残る。全3windowは正常終了、所有capture `target/tmp/h1-filmstrip-focus.png`の013表示/focusを目視確認。既存の無音WAV fixtureを使い、Save/clipboard/原本/OS設定変更なし。Software pathと音声用zero video metricsは性能gateではない。screen reader全体、selection、他のfocus経路、実入力/DPI/device・最終候補/配布gateは継続する。

### playlistの画面外行へのkeyboard移動（2026-09-07 00:54 JST）

- 前turnのab655b8はpush済みで作業開始時のworktreeはclean、CI 34043334821は実行中。基準release `469CEA77BDF72E47B383C28DC5D06D5E7F03532832D5E750F5E2009633DA908F`、PID 42612（開始UTC 2026-09-06 15:47:28.9178265Z）で所有50曲の先頭行へUIA Focus、Endを送っても先頭に留まる。1万曲のheadless回帰も同じ期待で失敗した。
- focus先のpath/IDを現在曲から分離し、上下/Home/End/PageUp/PageDownで必要な行を表示・focusする。Enter/Spaceだけ選曲へ渡す。初案は同frameのEnd→Enterで選曲が抜け、回帰で失敗。移動と実行を受信順に処理し、逆順のSpace→Homeも元の対象を実行するよう修正した。可視Button数は12未満のまま、20行の上下移動、disabled拒否、既存Shell identity・wheel・手動scroll保持を検証した。
- 中間release `830B0C4FD3A60CD59D73EA8D338262BE3A26B95998568E4CC6A52E16A79B4130`、PID 39192（15:50:21.5244777Z）と最終release `7A9A6585AC63BBBC7284443333811F67E4F398E91535DCDD2C05115D161FDE3C`、PID 42060（15:52:54.3185176Z）でEnd→50、Up→49、Enterで049再生、Home→1、PageDown→13を確認。最初のfocus移動だけでは001を保持する。049をmuteでdirtyにして一覧末尾へ移動・Enterするとguard、背景行disabled、Cancelで049/編集保持、Undoでcleanと正常終了まで通過。物理入力ではなく所有foregroundへの入力注入であり、同frame順序はheadlessの証拠である。
- 6 playlist tests、254 workspace tests（app 138/core 36/runtime 76/integration 4）、format、Clippy、debug/release buildが通過。既存live ignore 3件は残る。全3windowは正常終了し、所有capture `target/tmp/h1-playlist-keyboard.png`でfocus行表示を目視確認。fixtureは新規生成した1秒無音WAVの50 copiesで、既存素材/Save/clipboard/OS設定は変更していない。ログはSoftware pathと音声用のzero video metricsであり性能gateではない。screen reader全体、filmstripの画面外操作、selectionと既存実環境/配布gateは未完了。

### trim端点の値操作とfocus保持（2026-09-07 00:44 JST）

- b616c51 CI 34041820031は成功。通常release `46D211EA1B99B5CA458C2B0536357E0251FA4DC98B58DECF586EC85CE6CC0A8B`、PID 44060（開始UTC 2026-09-06 15:28:17.1791577Z）では30秒動画のtimelineに再生位置Sliderだけがあり、開始/終了の名前付きSliderはない。headless回帰もこの欠落で失敗した。
- source秒の開始/終了、0～durationと1秒step、値操作・focus表示を追加。stable widget IDとgesture generationを分離し、編集後も取得済み参照とfocusを維持する。初回focus直後のRightで別gripへfocusが移る回帰を修正。終了拡張→開始変更が同frameに届く場合の描画順逆転も回帰で再現し、数値要求の受信順を保つ処理で修復。交互の5要求に零長拒否を混ぜ、有効な4編集だけをUndoできることも検証した。
- 中間release `2001F1CDF636745E0C1B46C3D277BCEAD6E140E9775F8A6B14E94349CD1CE3D9`、PID 29608（15:30:54.1196932Z）と最終release `469CEA77BDF72E47B383C28DC5D06D5E7F03532832D5E750F5E2009633DA908F`、PID 47240（15:43:24.7131907Z）で、cached RangeValuePatternから開始2.5/終了20秒、開始25秒の拒否・理由表示、Rightで3.5秒とfocus保持、Undo/Redo、同じRuntimeIdを確認。Close windowで背景両端点はdisabled、SetValueはElementNotEnabledException、Cancel後は編集保持。Undoでcleanへ戻し、開始gripの80px dragで2.764830508秒、再Undoと正常終了も通過した。同frameのbatch順序はheadlessで検証し、native試験は順次要求である。
- 9 trim回帰、253 tests（app 137/core 36/runtime 76/integration 4）、format、Clippy、debug/release buildが通過。既存live ignore 3件は残る。所有capture `target/tmp/h1-trim-values.png`で開始gripのfocusと両端点表示を目視確認。全3windowは正常終了し、movie.mp4のSHA-256は`36179A1F70ABC1EF4B4C73B85F4F387B3333080E48F9A76A55ED70EE578DEE1C`のまま。Save/clipboard/OS設定変更なし。動画stderrのD3d11va・Seek latency・AAC末尾警告は空ログや最終性能gateとして扱わない。物理key、screen reader全体、selection・画面外項目と既存実環境/配布gateは未完了。

### 全画面操作部へのkeyboard入口（2026-09-07 00:15 JST）

- 4993a9c CI 34040716761は成功。全画面操作部はpointer下端hoverのみで、Tabだけでは到達できない。通常release `720F9EC306949240271B342B6A597290F1250606063F1EF1049567F36AF1C749`、PID 32984（開始UTC 2026-09-06 14:59:56.8617681Z）でmenuからfullscreenへ入り、foreground確認後Tabを送ってもUIAには案内Textだけが残った。headless回帰もTab後にcontrolsがないことで失敗した。
- eguiへ届いたTab/Shift+TabでExit fullscreenへ入る経路を追加し、操作部のfocusとTab巡回中の表示を維持する。初回Area sizingを跨いでfocus要求を保持し、内容部分のclickでfocus/keyboard表示を解除する。window focus喪失・modal/overlay・content dragも除外する。初案のcontext input lock内でのlayout再要求はdebug回帰でlock失敗となり撤回。最終実装は入力の読取りとfocus変更を分離し、追加poll/timerはない。
- 中間通常release `1062996843B4E3C7397D0AE90AF705F9488F637A36863272EC694478BBEDB2B8`、画像PID 34840（15:08:49.2501521Z）でTab→Exit、続くTabでImage position→Rightで01から02、内容clickで非表示、Shift+Tabで再表示、Enterで通常windowへ戻る流れが通過。所有capture `target/tmp/h1-fullscreen-keyboard.png`で画像領域を縮めず操作部を重ねることを目視確認した。直前のrelease差替えはbaseline window保持中で失敗しており、旧hashのまま起動したPID 32112（15:08:18.5498324Z）は成功証拠から除外し正常終了した。
- 通常windowでのfocus変更を明示的に除外した最終release `46D211EA1B99B5CA458C2B0536357E0251FA4DC98B58DECF586EC85CE6CC0A8B`、動画PID 43224（15:12:35.0784882Z）でも、30秒Pausedからfullscreen/TabでExitへ入り、Tab巡回で再生位置へ、Leftで25秒Paused、Shift+TabでExitへ戻りEnterで通常window復帰/正常終了が通過した。物理keyboardではなく所有foregroundへの入力注入である。
- 新規focus回帰と既存pointer seek/overlay回帰、252 tests（app 136/core 36/runtime 76/integration 4）、format、Clippy、debug/release buildが通過。既存live ignore 3件は残る。全4試験windowは正常終了し、画像stderrは空、動画stderrはD3d11va選択・seek latency・既存AAC末尾警告と短いfixtureの統計（900表示、drop/CPU transfer 0）を記録。Save/clipboard/OS設定変更なし。この短区間を最終候補性能gateの代替にせず、全screen reader・実入力/DPI/device・配布も未完了である。

### フォルダー更新後の一覧操作対象とfilmstrip意味情報（2026-09-06 23:54 JST）

- 9984456 CI 34040095093は成功。playlistのcached row actionがShell順更新後に別曲を選ぶ回帰を再現し、filmstripは名前付きButton欠落で回帰が失敗した。通常release `D4F8D64C46FC573D9CCC3F0CE27505B0111A2D147F13064412BEB02A9BE3F409`、PID 47568（開始UTC 14:47:42.7912434Z）でも、所有01/02/03.wavの02行を保持して01の拡張子を一時的に対象外へ変えると、同じRuntimeIdが03を指した。filmstripには可視3項目の名前付きButtonがなく、現在曲のTextだけがあった。
- playlist/filmstripのwidget IDを移動先pathへ固定し、playlistのToggle扱いを解除。可視filmstripを名前付きButton、両方のFullDescriptionをpathと現在項目の説明にした。filmstripのfocusにも既存hover枠/名前を表示する。順序・行幅・click/middle click・可視範囲限定の描画/previewと既存guardは維持した。
- 最終通常release `720F9EC306949240271B342B6A597290F1250606063F1EF1049567F36AF1C749`、PID 43568（14:51:05.0853466Z）では、同じ更新後にcached 02行が`1. 02.wav`となりRuntimeIdを保持、Invokeで02を開いた。続いてfilmstripのcached 03もフォルダー更新後に保持され、Invokeで03を開いた。native IUIAutomationElement6でfull path/current track/current itemの説明を確認。01へのUIA Focusと所有capture `target/tmp/h1-filmstrip-accessibility-focus.png`で枠/名前を目視確認した。
- 同じprocessでToggle muteを編集として追加し、filmstripの01をInvokeするとUnsaved edits/IsModalへ進み、背景02行のInvokeはElementNotEnabledExceptionとなった。Cancel後は03とdirty編集を保持し、Undo/正常終了した。両試験windowは正常終了し、stderrはSoftware decode選択のみ。所有01の一時renameは毎回復元し、3コピーと既存tone.wavのSHA-256は全て`0E0CD597CC65B8A0C05633352D0F8985D0F9FF02554C166771C72D6BDFF96020`。Save/clipboard/OS設定変更なし。
- 新規2回帰を含むplaylist/filmstrip 9 testsと251 workspace tests（app 135/core 36/runtime 76/integration 4）、format、Clippy、debug/release buildが通過。既存live ignore 3件は残る。native比較とhelper/logはignored target/tmpに限定。画面外項目の全UIA navigation、全screen reader/focus、selection/trimと実環境/配布gateは未完了である。

### タブ操作対象の同一性とclose名（2026-09-06 23:42 JST）

- 99eca60 CI 34039457094は成功。既存tab closeはUIA名が全て「×」であり、並べ替え後の取得済みtab actionも別tabへ渡ることをheadlessで再現した。通常release `C470B847FD6EEA642D4F79A1AA189845FC8AF30F389F23F64776EA91300DBC95`、PID 33880（開始UTC 14:35:59.4577430Z）へ所有01/02/03.pngを開き、02のUIA参照を保持して01を閉じると、同じRuntimeIdのNameが03.pngへ変わった。参照先を再取得して隠さず、同じ参照で比較した。
- TabIdを使う明示child UI IDへactivate/closeをまとめ、close名を`Close tab: filename`、descriptionをfull pathにした。外観・pointerのhit領域・drag・既存CloseTab guardは変更しない。回帰は取得済みactivate/close、並べ替え後のfocus、隣接tab終了、未保存確認とdisabled拒否/Cancel、削除済みtabへの古いaction不実行を検査する。
- 最終通常release `D4F8D64C46FC573D9CCC3F0CE27505B0111A2D147F13064412BEB02A9BE3F409`、PID 40980（14:40:26.6190708Z）で同じ01/02/03手順を実行。01終了後も02の取得済みactivate/close参照とRuntimeIdを保持し、取得済みcloseで02だけを閉じ03を残した。03のR編集→名前付きclose→Unsaved edits/IsModal→Cancelで編集保持、Undo/正常終了も通過。managed clientのHelpTextは空だが、固定adapterはdescriptionをFullDescriptionへ公開しており、独立native IUIAutomationElement6 clientで03の正確なfull pathを確認した。
- 8件のtab関連test、249 tests（app 133/core 36/runtime 76/integration 4）、format、Clippy、debug/release buildが通過。並列testが同じWindows時刻を取得して一時directory作成に衝突したため、test helperだけにprocess IDとatomic連番を追加した。既存live ignore 3件は成功証拠に含めない。両windowは正常終了、stderrは空、3素材のhash不変。所有capture `target/tmp/h1-tab-identity-after.png`を目視確認し、UIA補助script/logもignored target/tmpに保持。Save/clipboard/OS設定変更なし。全screen reader/focus、selection/trimと既存launch gateの完了とは扱わない。

### 再生・画像位置のUIA値操作とfocus（2026-09-06 23:30 JST）

- 3356d0dの通常release、PID 40456（開始UTC 14:02:02.2054712Z）ではcompact bar/timelineともSliderがなかった。既存widgetへ名前、現在値、範囲、stepと値操作を追加し、再生はsource秒、画像はShell snapshotの画像だけを数えた1始まりの位置を使う。範囲外の有限値はclampし、画像は整数へ丸める。既存Seek/未保存guardへ接続し、直接値変更は進行中pointer gestureを取り消す。
- headless回帰は変更前にnamed slider欠落で失敗。値の上下限・NaN/無限・別tree/node・文字列の拒否、連続event順、discard passを跨ぐ一回実行、disabled/modal、Shell順・非画像除外・編集保持、pointer release取消を検査する。focus中のLeft/Rightは5秒または1枚、Home/Endは端点へ移動する。他のshortcut/prefixは既存dispatcherへ渡し、synthetic focus keyは新しい経路で実行しない。
- 中間video PID 3144（14:10:15.8284440Z）はcompact SetValue 30/5.25、timeline 12.5、Home/Endが通過。helperによるAlt付きforeground取得直後の最初のRightは未反映で、同じprocessを実foreground確認し、Altを送らないRightで10.25を確認した。画像試行でも同じ補助入力の初回未反映は成功に数えない。固定egui-winitのfocus-wide key captureにはseek focus限定のshortcut経路を追加した。
- 画像中間PID 43640（14:12:37.0796278Z）の3枚目は誤って既存の16385px上限制約fixtureを選び、期待通りdimension errorとなった。新しい画像不具合とは扱わず、所有する03.pngだけを正常red素材へ変更。PID 47700（14:16:36.8496656Z）ではSetValue、R、移動guard、disabled値拒否、Cancel後の編集保持、Ctrl+Z、Endが通過した。
- 最終通常release SHA-256 `C470B847FD6EEA642D4F79A1AA189845FC8AF30F389F23F64776EA91300DBC95`、video PID 43868（14:23:24.2557298Z）のまとめた試験出力は欠落したため判定から除外。同じ生存processを照合し直し、timeline 7.25→Rightで12.25、SpaceのPlaying/Paused、SetValue 14.5とfocus枠を再確認して正常終了した。画像PID 31432（14:29:21.8801458Z）では1→2、focus中R、SetValue 3によるUnsaved edits/IsModal、値2保持・disabled、追加SetValueのElementNotEnabledException、Cancel/編集保持、Ctrl+Z、Endで3、正常終了が通過。直前の起動PID 35680はFFmpeg DLLのPATH設定を省いた環境で既に終了しており、UI試験には含めない。
- 248 tests（app 132/core 36/runtime 76/integration 4）、format、Clippy、debug/release buildが通過し、既存live ignore 3件は残る。画像stderrは空。動画stderrにはD3d11va選択、seek latencyと既存AAC末尾timestamp警告があり、性能gate計測ではない。capture `target/tmp/h1-uia-timeline-focus.png`と`h1-uia-image-range-focus.png`は所有windowに限定して目視確認した。Save/clipboard/OS設定/原本変更なし。trim grip・selection・全screen reader/focus順や既存実環境/配布gateは未完了。

### Shell STA待機によるUIA反復停止の修正（2026-09-06 22:55 JST）

- 5f13a18 CI 34036970810は成功。素材を読まずdirtyなtabだけを作る一時比較PID 48072（開始UTC 13:45:19.1643086Z）は10往復成功。画像読込みだけを加えた42532（13:46:28.5349412Z）も10往復成功し、folder情報取得だけを加えた45108（13:46:28.5455480Z）は3往復後、4回目のClose window検索で`0x80131505`となった。画像rendererやAccessKitの名前処理ではなく、Shell処理を含む経路へ絞れた。
- Shell STAはsnapshot完了後に通常のCondvarだけで待ち、Windows messageを処理していなかった。所有する非表示windowへのWM_NULLを送る回帰testは変更前に1秒timeoutで失敗。要求/終了用auto-reset eventとmessage-aware waitへ置き換え、mailbox lock外でqueueを処理すると通過した。待機対象からmessageを除く負の比較でも再び失敗し、復元後に通過。既存の最新要求・世代失効・実行中close・実Shell順testも維持する。
- 比較用のapp変更を全除去した通常release `8AA8FF70D9278B354BFC8CA408154E639F781A718CCCAE0218F871BF712495E3`、PID 44824（13:49:55.1564300Z）でR→Close window/Cancelを30往復し、全て通過。前回通常buildの3回目停止とは異なり、最終treeにも12要素が残った。独立Rust MTA clientのconnection/transaction timeoutは各2秒のままである。
- 同じPIDの読込み済み所有PNGだけを既存53-byte不正fixtureへ差し替え、UIAからExport and continueを実行し、未存在の専用出力先をnative Save Asへ入力した。別MTA clientでExport failedの18要素、IsModal=true、配下OKを取得/Invokeし、戻り先Unsaved editsの20要素、IsModal=true、3 buttonを取得/Cancelできた。その後さらに10往復が通過し、失敗前からprocessを替えていない。
- 所有PNGを正常素材から復元し、SHA-256 `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`を確認。新規export出力は作られなかった。5秒idleのCPU増分0ms、private memoryは125.58MiBの単発観測。全4試験windowはUndo後に正常終了、通常stderrは空、clipboard/OS設定/capture変更なし。sourceとhelper/logはignored target/tmpに限定した。
- この修正は再現したSTA待機停止を解決するものであり、全screen reader、実入力/DPI/device、最終候補性能や配布の完了証拠ではない。UI・依存バージョン・Shell並び順は変更せず、周期pollも追加しない。
- 最後に回帰のmessage probeを3回へ強化した最終通常release `1BFC6C4B10802FBCACAA71BD5E7FF5444EE79984B16873D3A1F43450776962C8`、PID 39620（13:56:19.2743538Z）でも10往復/Undo/正常終了を確認し、stderrは空。245 tests（app 129/core 36/runtime 76/integration 4）、format、Clippy、両buildが通過。既存live ignore 3件は実機成功の証拠には含めない。

### UI Automationのnative client・最小provider比較（2026-09-06 22:40 JST）

- 3edc5b8 CI 34033827557は成功。所有PNGのClose window/Cancel反復を継続調査した。独立Rust clientはMTAのCUIAutomation8を使い、connection/transaction timeoutを各2秒に設定する。本体releaseのPID 19604（開始UTC 12:46:30.9088737Z）は2往復後、3回目のCancel検索で`0x80131505`。managed UIA wrapperや短命clientだけの問題ではない。
- 一時コピーの固定accesskit_windows 0.32.1へWM_GETOBJECT・GetPropertyValue・Navigateの入口/出口を記録した。PID 39184（12:57:53.1525908Z）でも同じ失敗が出るが、記録したproperty/navigation呼出しは戻り、本体のtree更新も継続する。追加の親edge観測に明白な循環は見つからない。未計測のCOM呼出しやWindows内部待ちまでは除外できず、「providerは正常」とは判定しない。root COM参照cacheとWM_GETOBJECT対象制限の比較も失敗し、採用しない。
- 本体のUI縮小比較では通常buttonだけが5往復成功、top barだけは4回目、chrome buttonだけと通常のサイズ指定buttonだけは7回目の検索で失敗した。通常buttonの5回成功を長期安定の証拠にしない。tooltip削除、直接の名前設定、glyphの別描画も改善しない。最終比較PID 35976（13:39:20.1422166Z）はサイズ指定buttonと実保存確認だけを残し、frameの描画/presentを省略しても6往復後、7回目のClose window検索で失敗。描画装置は初期化したままである。
- ignored `target/tmp/uia-native-probe/`に独立clientと小さなwinit/egui providerを作成。最初のproviderのCancel無効は試験側のrepaint deadline処理不足で、修正後は通過した。`0x80040200`はUIA_E_ELEMENTNOTENABLEDであり、本体のtimeoutとは区別する（後続の固定Windows定数との照合で旧名称誤記を訂正）。最小providerのサイズ指定button/同等guardはdebugで10往復、release PID 11040（13:28:08.7755953Z）で15往復成功。20ms周期の更新を加えたrelease PID 40340（13:33:48.6561625Z）も20往復成功した。
- 本体binaryへ一時組込みして同じ依存解決で比較すると、PID 34164（13:35:03.1186771Z）は15往復成功。guardのID/Export button構造を合わせた43796（13:35:57.0205806Z）、本体のwindow設定/font/style/mouse hookを加えた36908（13:36:45.1889470Z）、renderer初期化を加えた41556（13:37:34.9077465Z）、Applicationの常駐workerも作成した41788（13:38:21.6232113Z）も各15往復成功。実media読込みや本体のevent/state処理全部を再現した試験ではなく、原因は依然未確定。
- 既存headless 5往復を、nativeで観測したFocus→Clickの順へ合わせ、返されたfocus IDがtree内に存在することも検査する。全一時production変更、依存override、traceを除去し、通常buildへ戻す。依存更新やUI縮小は修正として残さない。次は本体のmedia読込み後の状態・event処理と、この通過providerの差を比較する。
- 復元後の通常release SHA-256は`6BF0D9EC65BA79BC6EC392631D79AAA561622ECF6A5BBD3742E42D8E883AC3D8`。PID 13884（13:42:03.3737141Z）でも2往復後の3回目Cancel検索が同じtimeoutで失敗した。WM_NULLは応答し、Escape/Undo後に正常終了、stderrは空。244 tests（app 129/core 36/runtime 75/integration 4）、format、Clippy、両buildの通過と、native問題が未解決であることを分けて記録する。既存live ignore 3件は実機成功の証拠に含めない。
- 全所有windowは正常終了。本体の試験回転はUndoし、PNGのSHA-256は`5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`で不変。Save、clipboard、OS設定変更、画面captureなし。診断source・binary・ログはtarget/tmpだけに保持し、これらの成功をscreen readerやlaunch gateの完了とは扱わない。

### UI Automationの繰り返し確認での取得停止（2026-09-06 21:39 JST）

- f0f1994 CI 34032823555は成功。通常release `DB4BA189E55AADF5FEA202F5DA356B30330E9A0A69E5EC66F468F5E247F477E0`で再調査した。所有PNGコピーを読み込み後に不正PNGへ置換する試行PID 5352（開始UTC 12:22:04.2182507Z）は、保存確認20要素からexport失敗後0要素となった。新しいMTA照会clientでも同じで、Escapeで失敗通知を閉じても戻らない。STAだけの問題やエラー文だけの問題とは断定できない。
- WelcomeからOpenを経る通常release PID 46068（12:28:02.0047695Z）は、Export failedを18要素のtree内で取得し、IsModal=true、配下OKのInvokeまで成功した。しかし次の保存確認照会は0要素となった。一時ログ付きdebug PID 39168（12:25:16.4413738Z）は同様の流れでOK→保存確認Cancelも通過したが、診断buildの成功を通常buildの解決とは扱わない。
- 通常release PID 47288（12:31:03.9809339Z）では、同一MTA clientとroot参照を保持して保存確認→native Save As→Export failedを観測。失敗通知のIsModalと配下OKを確認/実行した後のFindAllがtimeoutした。短命clientだけに限定された問題とも言えない。
- 最小化した手順は「所有PNGをRでdirty化→UIAのClose window→Cancelを5回繰り返す」。通常release PID 46572（12:33:33.4466587Z）は2回復帰後、3回目のClose window実行後のCancel照会が停止した。modalの意味情報5行だけを外した比較release PID 24452（12:36:20.9419832Z、SHA-256 `BB9BF07999B07B9111410D666398DAE3907BB5B12B51C4417745ABD2F0962D9D`）も同じ位置で停止。比較元の名称追加だけを原因とはできず、FFmpeg・保存・native pickerなしでも再現する。WM_NULL応答とkeyboard取消/Undo/正常終了は保たれたが、これだけではproviderやevent loopの健全性を証明しない。
- 既存headless回帰にもClose window/Cancelの5往復を追加し、各action一回、guard状態、終了しないこと、編集履歴保持を確認した。これは通過するが、Windows provider経路の回帰再現ではない。244 tests・format・Clippy・両build通過。診断ログと比較用コードは全て除去し、production変更なし。test追加後の通常release SHA-256は`6D4405AB108DFDFDEB77BFF13458A2310AEBFD22427CC7E4B2ABBDAAE0C5F451`で、上記native比較binaryとは区別する。
- 全6試験windowを正常終了し、所有コピーを原本と同じSHA-256 `5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`へ復元。新規export出力なし、通常app stderrは空、clipboard/OS設定変更なし。ignored `target/tmp/h1-uia-export-recovery/`と`h1-uia-inspect.ps1`、`h1-uia-persistent.ps1`、`h1-uia-guard-cycle.ps1`、`h1-native-dialog-path.ps1`に再試験用素材/手順を保持する。次はこの5往復を用い、provider取得/イベント配送を独立に計測する。根本原因は未確定で、依存更新や意味情報の削除を修正として採用しない。

### 保存関連modalの名前と階層（2026-09-06 21:23 JST）

- 0690cc7比較releaseのPID 41992（開始UTC 12:01:20.0348354Z）では、保存確認の見出しはTextで親がapp root、Window型の子要素は0だった。3つの既存modalに見出しと同名のAccessKit Dialog/modal情報を付け、内容を同じUIの子にする。通常background exportと既存focus/keyboard方針は変更しない。
- 回帰はDialog不在で失敗し、最初の実装も子buttonが別階層になるため失敗した。固定eguiのaccessibility用`ui.unique_id()`を使う修正後、5表示mode・240～960pxの縮小/復元で名前、modal flag、操作buttonの所属と既存layout確認が通過。244 tests（app 129/core 36/runtime 75/integration 4）、format、Clippy、両buildが通過。既存live ignore 3件は実機経路の証明に含めない。
- 最終通常release SHA-256は`DB4BA189E55AADF5FEA202F5DA356B30330E9A0A69E5EC66F468F5E247F477E0`、比較は`CD5F741B81A3B62639C3D06AA60FA14A62743645AF7B78209320C67DC0DDF4B2`。PID 21248（開始UTC 12:07:08.3207967Z）でUnsaved editsのWindowPattern.IsModal=trueと3 buttonの所属を確認。専用の新規出力先へのexportではExporting before continuingもIsModal=true、その配下のCancel exportをInvokeできた。先行試行の出力記録欠落は成功根拠に使わず、保存確認へ戻った状態を確認して別の新規出力先で再検証した。
- エラー試行PID 35584（開始UTC 12:15:34.1070704Z）は読み込み済みの所有素材コピーだけを不正PNGへ置き換え、専用の新規出力先へexportした。Export failed画面と詳細は所有windowのcaptureで確認したが、UIA照会がtimeoutし、その後同一processへの照会も子要素0だった。Tab後も変わらず、原因と比較buildでの再現は未確認。エラーmodalのnative semantics成功とは扱わず、次にこのtree喪失を切り分ける。
- 比較/最終/エラーの全windowは取消・所有回転のUndo後に正常終了。新規export出力は残らず、エラー用コピーを元の赤PNGへ復元した。原本2枚のhashは不変、app stderrは全て空（意図したFFmpeg失敗は画面内の詳細）。ignoredの`target/tmp/h1-modal-names*`に試験記録を保持し、前面を保証できなかった対象外captureは削除した。clipboard/OS設定は変更せず、screen readerの読上げ順・focus・全custom widget対応は未証明。

### 保存確認中のUI Automation入力保護（2026-09-06 20:57 JST）

- ddb9a3f通常releaseの所有PID 44264（開始UTC 2026-09-06T11:49:51.6434446Z）、600×800の赤PNGでR→Ctrl+Wを実施。保存確認中も背景のmenu/Reading mode/Close windowがUIAでenabledと公開され、menuのInvokeでFile/Edit/Viewが現れた。固定eguiのAccessKit Clickはpointerのmodal遮断とは別に処理されていた。
- 背景rootを無効化し、既存backdrop以外の追加opacity低下を避ける。popupは閉じ、独立overlayは状態を保って表示を保留する。配送済みUiActionも確認/最前面error解除/export取消以外は拒否し、error通知中の古いDiscardで下の保存確認を進めない。
- 修正通常releaseの所有PID 44432（開始UTC 2026-09-06T11:55:10.0740628Z）で同じR→Ctrl+Wを確認。背景3 buttonがdisabledで、確認前のmenu参照によるInvokeはElementNotEnabledとして拒否され、File submenuも現れない。CancelのInvoke後はdirty titleを保持し、背景menuを再有効化した。試験回転をUndoして正常終了。比較windowもCancel/Undo後に正常終了し、両stderrは空。source SHA-256は`5F24C4FFDEA139A9C49BDEAD1873D0E714A6273BACE45DFB567D958AE2ADB72C`のまま、Save/clipboard/OS設定の変更なし。
- native修正binary SHA-256: `549B4D0BA13AD5DF9689A096FBD629958D15BDB1DE7FC8B38CB66E77D83AEEAA`、比較binary: `39642B0314D1BADBAB000CEDC6BCA985AA5C0DE4C48A44CDD2BB7A637039209B`。その後のtest-only追加を含むbuildは`CD5F741B81A3B62639C3D06AA60FA14A62743645AF7B78209320C67DC0DDF4B2`。実機capture/logはignoredの`target/tmp/h1-uia-guard-*`。960×576、現在機のWindows build 26200に限定した入力保護の証拠である。
- 回帰は変更前に配送済み背景commandの拒否で失敗。修正後はmodal初回/継続frameのtree、背景Click、error中の旧Discard、Cancelの一回実行、履歴保持とpalette表示の保留/復帰を検証する。244 tests（app 129/core 36/runtime 75/integration 4）・format・Clippy・両buildが通過。screen readerでのmodal名/読み順/focus、全custom widgetの操作確認は残る。

### Windows UI Automation連携（2026-09-06 20:44 JST）

- c59fa73の通常release（PID 19532、開始UTC 2026-09-06T11:28:08.0491227Z）で所有windowのUI Automation descendantsは0だった。固定egui-winitのaccesskit featureを有効化し、windowの初回表示前にadapterを作成、初期tree要求・action・無効化を既存event loopへ接続した。非Windows用の推移依存もlockされるが、Windows dependency treeにasync-executorはなく、app独自のworker/poll/COM providerは追加しない。
- 先行buildのUIA検査でWelcomeの12要素とOpen File…のInvoke→native dialog→取消、Close windowのInvokeを確認。一方、logo/queryが空白名、検索のSetValueが成功を返しても空欄のまま、palette候補がInvokeではなくTogglePatternとして公開される不足を発見した。検索宛ての文字列SetValueだけを既存egui編集eventへ変換し、名前とcommand semanticsを修正した。標準widget一般の独自実装や描画変更はしていない。
- 最終通常release（PID 37848、開始UTC 2026-09-06T11:43:47.3724958Z）、960×576、Windows build 26200で再確認。UIAから`towavue menu`→View→Show command paletteをInvokeし、Search commandsのValuePatternで`Open`、日本語・アクセント文字・絵文字、空文字が完全一致した。再度Openに設定し、候補のOpen fileをInvokeすると所有native dialogが開く。fileを選ばず取消後、Close windowのInvokeで正常終了した。WelcomeのPlay or pauseがdisabledであることも確認した。
- 最終binary SHA-256: `39642B0314D1BADBAB000CEDC6BCA985AA5C0DE4C48A44CDD2BB7A637039209B`。基準binary: `394617AF866B14D89006F64FF576326C7D0A59F21818CC65077DA6E114628087`。最終のUIA操作後、通知期限を過ぎた5秒idle CPU増分は0ms。全5試験windowは正常終了、stderrは空。最終query captureは960×576で、ignoredの`target/tmp/h1-accessibility-*`へ保存。clipboard・元素材・OS設定は変更せず、screen readerは起動していない。
- 3回帰を追加し、tree生成の要求/無効化/再要求、Welcome commandの一回実行、logo名、検索のUnicode/空/単一行化/誤node/連続SetValueと後続文字eventの順序、command非Toggleを検証。243 tests（app 128/core 36/runtime 75/integration 4）・format・Clippy・両buildが通過、既存live ignore 3件あり。native tree全体の読上げ・focus順・modal隔離、timeline/selection/playlist等のcustom semantics、実DPI/入力matrixは未完了である。

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
