# towavue ロードマップ

各milestoneは前のゲートを満たしてから開始する。新機能の数ではなく、観測可能な正しさを完了条件とする。

## M0 — Foundation（完了）

Git、Rust workspace、設計・運用文書、CIを構築する。再生、UI、Shell連携のruntime実装は行わない。

完了ゲート:

- `concepts/`がGit対象外である。
- workspaceがformat、Clippy、testを警告なしで通る。
- Windows CI定義が同じ検証を行う。
- OS、ライセンス、crate境界、D3D11、FFmpeg、WASAPI、Explorer sortの契約が文書間で一致する。
- verified checkpointが`origin/main`へpushされる。

M0は2026-09-04にWindows CIを含めてゲートを通過した。

## M1 — Software playback vertical slice

単一windowと単一fileに限定し、software video decode、D3D11 upload、event-driven WASAPI Sharedによる音声、play/pause/EOFを完成させる。

fixtureは少なくともMP4/H.264/AAC、MKV/HEVC/AAC、WebM/VP9/Opusを含める。UIの再現、tab、folder navigation、編集は行わない。

M1は2026-09-04に完了した。固定FFmpeg 9.0.1で3種類のfixtureを生成・検査し、software decode integration testを通過した。実機ではD3D11 Flip Discard swap chainへのupload、WASAPI Sharedの音声排出、Spaceによるpause/resume、最終frame/sample排出後のEOF遷移を確認した。現在の次工程はM2だが、まだ着手していない。

## M2 — D3D11VA zero-copy

アプリ作成のD3D11 deviceをFFmpegへ渡し、D3D11VA surfaceからVideo ProcessorとSwapChainまでCPU transferなしで表示する。D3D11VA非対応時のみM1のsoftware pathへfallbackする。

adapter LUID、hardware frame count、CPU transfer countを記録し、hardware pathで`CPU transfer = 0`を証明する。

M2は2026-09-04に完了した。基準adapter `00000000:0001311b`でH.264をD3D11VA decodeし、60 hardware frames、0 CPU transfersでVideo ProcessorからSwapChainまで表示してEOFへ到達した。同adapterでhardware初期化できないHEVCとVP9、およびD3D11VA構成を持たないFFV1はM1 software pathへfallbackし、それぞれ60 CPU transfersでEOFへ到達した。

## M3 — Seek, synchronization, and resilience

audio master clock、video-only clock、frame pacing、generation付きSeek、連続Seek、pause/resume、endpoint変更、device removalを完成させる。

基準機で4K60を10分再生してdrop率0.1%未満、30分でA/V drift p95 40ms以下・最大100ms以下、ローカル1080p H.264の100回Seekでp95 300ms以下を目標ゲートとする。

M3は2026-09-04に完了した。demux、video decode、audio decode、WASAPI outputをbounded queueで分離し、`IAudioClock`を通常のmaster、device clock停止時とvideo-only区間を単調時計で補う構成にした。Seekはpipelineをgeneration単位で破棄・再構築し、古いeventを無視する。default render endpoint変更とD3D11 device removalはtyped eventから現在位置でpipelineを再構築する。基準adapter `00000000:0001311b`の30分4K60 H.264/AAC実時間再生では107,768 framesを表示、3 framesをdropしてdrop率0.0028%、A/V drift p95 4.772ms・最大35.759msだった。ローカル1080p H.264の100回Seekはp95 37.750ms・最大56.991msだった。現在の次工程はM4である。

## M4 — Application shell and navigation（完了）

tab、status bar、command registry、command palette、customizable shortcut、audio folder playlist、filmstrip、Explorer folder sort連携を実装する。

Explorer sortは`docs/ARCHITECTURE.md`の`FolderSnapshot`契約と検証matrixを満たすこと。

M4は2026-09-04に完了した。eguiを同一D3D11 back bufferへ統合し、tab、status bar、共有command registry、command palette、prefix対応カスタムshortcut、audio folder playlist、全media filmstripを実装した。`FolderOrderProvider`は専用STAでmatching live Explorer viewを優先し、閉じている場合は非表示`IExplorerBrowser`、失敗時だけWindows自然名前順を用いる。folder変更はoverlapped `ReadDirectoryChangesW`と150 ms debounceでsnapshotを再取得する。fixture付きExplorer integration testでName、Date modified、Date created、Size、Typeの昇順・降順、同値、複数列、およびsort変更後の再取得を確認した。現在の次工程はM5である。

## M5 — Images and reading mode（完了）

静止画、アニメ画像、zoom、selection、crop preview、reading modeを追加する。動画と共有するのはvisual surfaceとpresentation上の概念に限定する。

M5は2026-09-05に完了した。BMP、JPEG、PNG、TIFF、WebPの静止画、GIF・WebP・APNGのanimation、FFmpegによるAVIFを安全なRGBA frameへdecodeし、同じD3D11 device上のegui textureとして表示する。EXIF orientation、deadline駆動animation、cursor anchor zoom、actual/fit、右drag pan、正方形・比率保持・辺resize対応selection、非破壊crop previewを実装した。reading modeは共有`FolderSnapshot`のExplorer Shell順から2～10枚を選び、横・縦配置と表示順反転を行う表示専用modeである。実cropや保存はまだ行わない。現在の次工程はM6である。

## M6 — Non-destructive editing and export（完了）

crop、rotate、flip、trim、volume、rateを非破壊操作として保持し、undo/redo、unsaved indicator、close guard、FFmpeg exportを追加する。

M6は2026-09-05に完了した。tab単位のsaved cursor付きedit historyへcrop、時計回り・反時計回り90度回転、水平・垂直反転、trim端点、volume、rateを保持し、branch対応undo/redoと画像のoperation順previewを実装した。動画crop selectionと動画・音声の編集値もsourceを変更せず保持する。dirty状態はtab/window/statusへ表示し、media移動、tab close、process終了をExport / Discard / Cancel modalで保護する。Save Asと再SaveはmetadataをcopyするFFmpeg software exportを行い、source同一pathは拒否する。画像crop+rotateと音声付き動画trim+rate+volumeの実export・再decodeをfixtureで確認した。現在の次工程はM7である。

## M7 — Advanced presentation and interaction（完了）

HDR、waveform/thumbnail cache、multi-window tab drag、grid menu、hardware encode、詳細アニメーションを追加する。

M7は2026-09-05に完了した。path・size・更新時刻keyと64 MiB上限を持つdisk cache、UI thread外で生成するwaveform / duration / hover thumbnail、click/drag Seek付きtimeline、メディア種別ごとに設定可能な`1234/qwer/asdf/zxcv` grid menu、window外tab dropによる別process window化、Media Foundation hardware encodeの強制要求とsoftware fallbackを実装した。gridは短いopacity transitionだけを使う。PQ/HLG metadataはhardware frameとともに保持し、D3D11 Video Processorが明示的にHDR→SDR変換を保証する場合だけcolor spaceを設定する。基準adapterは同変換を保証しなかったためHDR表示を成功とは扱わず、typed errorを確認した。HDR displayへの10-bit pass-throughとprocess間tab再結合は対象外である。

## H1 — Human evaluation and UX stabilization（進行中）

M0～M7で構築した技術sliceを開発版として人が操作し、日常flowの摩擦、表示と実際の動作の不一致、応答停止、発見性、DPI・入力・error stateの問題を収集して直す。これは草案の未実装項目を一括投入するfeature milestoneではなく、観察できる一つのscenarioを単位にする反復phaseである。

各変更のgate:

- 再現手順、期待結果、対象media・環境が記録されている。
- 変更前後を同じ手順で比較し、実windowで結果を確認する。
- stateやdomain logicの回帰には自動testがある。
- UI差分へ無関係なruntime refactorや次のfeatureを混ぜない。
- format、workspace全targetのClippy、workspace全targetのtestが通る。
- architecture上の決定が変わる場合は実装前に`ARCHITECTURE.md`を更新する。

最初の優先候補は、動画・音声edit値とlive playbackの不一致、同期画像loadと同期exportによるUI停止、timeline/tab/filmstrip/menuの発見性と操作感である。詳細な試用方法と現状差分は`docs/DEVELOPMENT.md`と`docs/KNOWN_GAPS.md`を正とする。PackagingはH1と並行して暗黙に開始せず、FFmpeg binaryとlicense条件を別途決定してから計画する。

2026-09-05、exportの応答停止を改善した。runtimeのbackground job、書き出し時間表示、cancel、成功後だけのtarget置換、exportした履歴位置のsaved判定、dirty guardの成功・失敗・cancel遷移を追加した。生成fixtureと実windowで再生継続、既存target保護、追加編集のdirty保持を確認した。

同日、画像decodeをlatest-only workerへ移し、画像/reading要求のRGBA保持量を512 MiBへ制限した。6000×6000画像の起動時panicをdevice上限の正しい伝達で修正し、上限超過・破損pageのerror表示と相対path起動時のShell順navigationを確認した。H1全体は未完了で、live volume/rate、草案の外観と操作感の再現を引き続き優先する。

続いてvolume/muteをlive playbackへ反映した。WASAPIへ渡す直前のgain、5 ms ramp、undo/redoとpipeline再構築時の保持を追加し、対象processのsession peakで100%・50%・muteを確認した。試験中に再現したmono WAVのdecode失敗も、未指定channel layoutの補完で修正し、mono/stereoの直列・並列decodeと実windowを確認した。rateと草案の外観・操作感は引き続き未完了である。

その後、0.25～4倍のピッチ維持live rateを追加した。速度変更はsource位置を保つgeneration付き再構築とし、音声・映像時計、pause中のframe保持、音声drain後の時計引継ぎを対応させた。tone/clockの自動testと2倍・4倍の実動画trialが通過した。次は草案に沿ったcompact shellと日常操作の改善を進める。H1全体とlaunch auditは未完了である。

compact shellの初回改善として、32px title/tab bar、30px status、logo menu、等分tab、native window操作、名前省略・情報右寄せを実装した。EOF後のPlayも先頭から再開する。実windowで最小幅、移動・resize・最大化・最小化復帰、長い名前の複数tab、dirty close guardを確認した。次は動画をbarに隠れないmedia領域へ収める表示修正と、thin seek bar・日常操作の改善を進める。

動画の表示矩形を同frameのUI layoutから求め、hardware/software共通のaspect-fitとsample aspect ratio、selection位置を対応させた。縦長H.264・非正方形pixelのFFV1で四辺の保持とtimeline追従を確認した。UIのrepaint deadlineも待機へ反映したが、操作後のUI一部欠落の原因確定は残っており、thin seek barより先に描画安定性を調べる。

続くpixel照合で、直近captureに対するUI欠落の目視判定を訂正した。hardware/software各10回のtimeline開閉でもtabとcontrolsは一致した。音声drain後のpauseは実WASAPI testと実windowでFaultedを再現し、正常排出と異常終了を区別して修正した。次はthin seek barとkeyboard中心の日常操作を改善し、launch auditへ進める。

thin seek bar、画像のShell順位置移動、paletteの上下選択・Enter・Escapeを追加した。dragはreleaseで一回だけ確定し、EOFからのSeekはPausedにする。停止中の非frame境界Seekで黒画面になる問題も再現・修正し、最初のframeを保持してから再開できることを確認した。H1は継続中で、次はOpen・folder移動・入力を通した日常flowと残る草案との差を監査する。画像thumbnail、本画面scrub、物理keyboard・IME・DPIのmatrixは今回の完了範囲ではない。

Open Folderの日常flowで、対応mediaがないfolderを選ぶと元のnavigationだけが壊れる問題を修正した。元のsnapshot・tab・編集を保持し、画像移動を続けられることを実windowと回帰testで確認した。同時にShell snapshot待機中の応答停止を確認したため、次は取得待ちをUI threadから外す改善を優先する。

Shell snapshot取得を最新1件の非同期要求へ移し、generationによる失効、同folderの旧snapshot保持、reading更新、待機中のwindow closeを実装・検証した。実Explorer sort matrixも通過した。一方でnative pickerをCancelするだけでも再生遅延が再現したため、次はpicker自体のUI thread待機と復帰時の再生を修正する。H1とlaunch auditは継続中である。

native Open file/folder・Save Asも専用STAへ移し、本体入力のmodal制限とowner寿命を保ったまま描画・再生を継続するようにした。30秒H.264/AACでOpen Folder→Cancelした際のdropは808から0になった。Save AsのCancel時のdirty guard復元、pointerを動かさない再click、書き出し成功後のtab終了も実windowで確認した。次は静止画・pause時のidle負荷と日常入力を監査し、草案との残る表示・操作差を詰める。H1全体は未完了である。

idle監査では再描画eventの自己再予約、静止grid、音声のみの再生に無条件描画を発見し、入力・animation・位置更新deadlineへ限定した。5秒間のCPU時間は静止画/Welcomeで約5.9秒から計測分解能以下、音声再生で約5.9秒から0.47秒へ下がった。必要な動画/GIF更新とmenu操作は実windowで維持を確認した。次はExplorerからのfile dropを含む日常Open操作と、草案のfilmstrip/menuとの差を監査する。launch全体は未完了である。

Explorerからのfile/folder dropを既存のOpenへ接続した。実OLE drag/dropで画像・動画・音声、複数画像、folder、hover取消、dirty編集保持、確認dialog中の拒否を確認した。folderは従来のlatest-only Openで、複数folderのimport機能は追加しない。次はfilmstripのthumbnailとmenuの操作・発見性を、日常flowと草案の両面から改善する。H1は継続中である。

filmstripを中央のthumbnail列へ変更し、画像/動画preview、音声waveform・duration、現在項目の枠と名前を追加した。可視項目だけを最大64件の単一workerで読み込み、古い結果を失効させる。実windowでclick・dirty guard・middle click・Tab前後移動・wheel横scroll・破損previewを確認し、cold cacheの30秒再生も900 frames / 0 dropsで完了した。次はlogo menuの整理・発見性と、日常閲覧に必要な残りの操作を監査する。H1とlaunch全体は未完了である。

logo menuの全command縦列をFile / Edit / Viewへ分類し、関連項目の区切りとcustom shortcutの右揃えを追加した。全commandの一意配置、無効項目、dispatch、最小windowでの末尾到達をtestし、実windowで回転・Undo・dirty closeとpalette起動を確認した。方向gestureは後回しとし、次はfullscreenを含む日常閲覧の表示・入力不足を検証する。launch全体は引き続き未完了である。

F11/View menuからのborderless fullscreenを追加し、persistent bar・timelineを隠してmedia領域を拡大した。Escapeはoverlay/modalを優先し、復帰時に位置・size・最大化を保持する。最大化から直接入る際の旧client領域残りを再現し、解除/復元順序を修正した。画像・reading・hardware/software動画とdirty guardを実windowで確認し、102 testが通過した。次はfullscreenのpointer/操作案内、keyboard・DPIを含む閲覧flowと残るlive-previewの差を監査する。H1とlaunch全体は未完了である。

fullscreenの画像・reading・動画へ2秒idleのcursor非表示を追加した。入力で戻し、button保持・overlay・modal・loading/error中は表示を維持する。右button保持判定と最小化からのstationary pointer復帰で実機上の問題を再現・修正した。静止PNGの追加連続描画はなく、104 testが通過した。次は動画編集のlive previewとkeyboard・DPIを含む残りの閲覧flowを監査する。edge-hover controls、double-clickやlaunch全体の完了とは扱わない。

動画のcrop・90度回転・反転を現在の再生へ反映した。画像と共有する履歴順UV、回転後のSAR・selection、hardware編集時だけの同device内RGBA合成を追加し、CPU readbackは行わない。hardware/softwareの実window、Undo/Redo、停止中Seekと実exportの画素照合を確認し、106 testが通過した。約1分4K60も途中から回転表示にしてdrop 0で完了した。次はcropのpixel丸め・極小範囲と、残るtrim/live表示の不一致を調べる。keyboard・DPI、配布を含むlaunch全体は未完了である。

cropを整数pixel矩形へ変更し、previewとFFmpeg exportの独立した丸めをなくした。1×1 PNGの保存失敗と隣接色のにじみを再現・修正し、画像1 pixel・動画偶数pixelの選択、動画16×16未満の拒否、寸法案内と全領域no-opを追加した。PNGの奇数寸法・回転後再cropと最小H.264 exportを含む114 test、実windowの保存・選択保持が通過した。次はtrim範囲とlive playbackの不一致、入力・DPIなど残るlaunch gateを監査する。H1全体は継続中である。

trim監査では、同じ位置のI/Oを受理しSave Asで初めて失敗する問題を再現した。端点の入力時検証、暗黙のsource先頭/末尾、no-op、timelineの保存範囲・ミリ秒表示、Undo通知を追加し、119 testと動画/音声の実windowを確認した。これはlive trim完了ではなく入力・確認の段階である。次は範囲外の再選択と両立する範囲内再生、音声sample端点・rate・Seek・EOFを検証する。H1とlaunch全体は継続中である。

続いてtrimをlive playbackへ接続した。半開区間内の映像・音声sampleだけを送り、両streamの終端でworkerを回収する。終端で停止して再Playは開始へ戻り、範囲外Seekはpaused source previewとして端点の再選択を可能にする。123 testとhardware/software動画・0.25倍音声、2倍動画、Undo/Redo・tab復帰を確認した。音声sampleのナノ秒丸めで生じた1 sample差も修正した。次は極小範囲・低精度PTS・export境界を追加監査し、keyboard/IME・DPI・metadata orientationを含む残るlaunch gateへ進む。H1全体は未完了である。

境界監査では、exportが余分なframeを残す秒丸めと、低精度音声PTSの16 samples差を再現・修正した。整数tick/sample端点、source時刻維持とstream指定、空trim出力のpublish拒否を追加した。source offset、frame色、非圧縮sample列、既存保存先保護と実windowのH.264/AAC保存を確認した。ただし途中Seek後の音声sub-tick位相差は残る。次はこの差の影響を定量化し、残るmetadata orientation・keyboard/IME・DPIとlaunch gateを進める。H1全体は未完了である。

途中Seekの音声差は対象fixtureで8 samples（約0.167 ms）と定量化した。続いて回転metadata付き動画が横向きになる問題を実windowで再現し、90度単位の回転・反転を編集より先に適用した。hardware/software、crop保存・再open・Undoと130 testsを確認した。上下反転を含むOpenH264保存失敗と、非対応matrixの理由が消える黒画面も修正した。任意行列は明示errorとし、全container・動的metadataの保証はしない。次はkeyboard/IME・DPI、日常操作と残るlaunch gateを進める。H1全体は継続中である。

日本語filenameが欠字になる問題を実windowで再現し、OS日本語fontを既定fontの後ろへ追加した。paletteのIME確定Enterの誤実行と、上下focus移動による確定文字の取りこぼしもevent回帰testで修正した。132 tests、実windowの日本語tab/statusと通常palette操作、静止時の低CPUを確認した。実IME候補操作、物理keyboard、複数DPI/monitorと残るlaunch gateは未完了で、H1を継続する。

小さいwindowのZoom inが固定960×576基準で飛ぶ問題と、画像100%がUI倍率に追従する問題を修正した。現在viewport・編集後寸法とphysical pixel基準を使い、pointer anchorを維持する。実windowで残った二重拡大は固定UI rendererのadapter補正で解消した。133 testsと8×8画像の実pixel照合、拡大UIのwindow controls、hardware動画を確認した。異なる実DPI間のmonitor移動・実IME候補操作と残るlaunch gateは引き続き未完了である。

大きい画像のFitに残っていた2%下限を除いた。16,384px高の画像で隠れていた両端が、480×300と縦横reading modeでも表示される。Fitからの縮小も2%へ跳ねず、小さい倍率を小数表示する。135 testsと実windowの1.08%表示を確認した。H1とlaunch全体は継続中である。

最小windowでgridの両側が切れる問題を修正した。表示領域内の4×4寸法、名前とpathの省略・tooltipを使い、clickもkeyと同じく一回実行して閉じる。136 tests、実windowの列表示・拡大UI・Zoom in clickを確認した。次は入力/focusを含む残りの操作とlaunch gateを継続監査する。

grid上のCtrl+Sが画像回転になる問題を実windowで再現・修正した。physical keyによる位置対応、Ctrl/Alt/Superの除外、palette・modal優先とpalette起動時のgrid closeを追加した。138 testsと保存dialogのCancel、Shift付きcell実行・Undo、palette検索を確認した。実keyboard layout/IMEと残るlaunch gateは引き続き未完了である。

prefix入力の期限切れ案内とfocusをまたぐ継続を修正した。別操作・focus喪失・Escapeで待ちを解除し、prefix自身の案内だけを消す。139 tests、通常のCtrl+K Ctrl+S、402ms以内のfocus往復後にCtrl+Sが独立した保存操作になる実window試験が通過した。H1と残るlaunch gateの監査を継続する。

保存確認がEscapeで閉じない問題を実windowで再現し、編集を保持するCancelへ対応させた。export失敗では最前面の通知だけを閉じる。背景clickの非解除と、tab・dirty・fullscreen保持を含む140 testsが通過した。H1は継続中で、残るmodal操作・layoutとlaunch全体の安定性/性能を監査する。

window縮小・UI拡大で保存確認の左端と操作buttonが切れる問題を修正した。確認・export通知の幅制限、file名の省略/tooltip、button折り返しとエラー詳細の高さ制限を追加した。141 testsと480×300の拡大UIでCancel clickを確認した。次は長時間再生を含むlaunch全体の検証を継続する。物理IME・混在DPIと配布方針は未確定である。

dbf13b4のrelease再検証は30分4K60のEOFまで到達したが、最大A/V driftが281.340msで100msゲートを超えた。p95 4.725ms、全区間drop率0.015774%、CPU transfer 0でも、長時間ゲート全体は未達とする。次は外れ値の発生位置とpacingを調べて再検証し、計測境界を修正した100回Seek試験へ進む。H1は継続中。

UI遅延後に古いframeをpromotionする問題を300msの試験用遅延で再現した。描画直前にも既存late discardを行う修正で、同条件の最大driftは283.758msから27.039msへ改善し、frame/drop総数も整合する。142 testsと通常buildのpaused Seek確認が通った。遅延/traceコード除去後の30分再試験は107,750 presented＋21 dropped、CPU transfer 0、drift p95 4.803ms・最大37.416msで同期ゲートを満たした。全区間drop率0.019486%、先頭10分へ全dropsを割り当てた上限でも0.058455%で、0.1%未満となる。元の長時間runの遅延原因そのものは未特定。次はSeek計測境界の修正と100回再測定を行う。H1全体は継続中。

Seek計測を同期再構築の前から最初の映像Present成功までへ修正した。旧M3の数値は再構築後からVideoReady通知までの部分計測であり、新しい値とは直接比較しない。通常release・ローカル1080p H.264/AACの中断なし100回で、再生中p95 100.004ms（最大105.746ms）、停止中p95 43.291ms（最大74.149ms）となり、300msゲートを満たした。各要求の完了後に次を送る5秒前後移動で、物理入力/DWM走査表示までの遅延や連打の保証ではない。144 testsと実windowの停止中映像保持・正常終了を確認した。次は残る入力・device recovery・日常flowのlaunch監査を進める。物理IME・混在DPIと配布方針を含むH1全体は未完了。

graphics recovery開始時のAccess deniedを実windowで再現し、旧worker・swap chainの解放順序を修正した。交換後のfont/画像再送とpreview失効も追加し、再生動画・編集済み停止動画・回転画像で復旧と編集保持を確認した。試験用呼び出し除去後の146 tests・通常release再生も通過した。前checkpointのCIは新規Seek testsの設定共有で失敗しており、既存のprocess/config分離方式を適用した。次はこのCI修正の確認と、renderer再作成不能時の未保存編集の救済・通知経路を検証する。実driver reset、endpoint切替、IME/DPIとH1全体は未完了。

renderer再作成不能時のnative確認を追加し、Retry、Cancelでの編集保持、描画なしのExport失敗・再保存・正常終了を実windowで確認した。停止動画は待機後も元の位置・1.25倍・停止を保ち、復旧後に残ったFaulted titleも修正した。保存PNGは回転後の64画素が一致し、148 tests・通常release buildが通った。前checkpointのCIも成功。次は複数dirty tabとexport中の故障/取消、実driver/endpoint経路を継続監査する。H1全体は未完了。

native復旧確認中に保存が完了すると保存済みtabを再確認する問題を再現し、確認終了後に残るdirty tabを選び直すよう修正した。実windowで2枚とも保存してから終了し、出力画素も一致した。追加試験で、生成設定の`zoom_in = +`が次回起動時に読めない決定的な不具合も判明した。`Plus`による保存と旧形式の読込を対応し、隔離設定の初回起動・再起動・旧設定Reloadを確認した。以前のCI原因を「競合」とした説明は未証明で、この生成/読込不一致を訂正根拠とする。151 tests・Clippy・releaseが通過。次はexport取消を含む残る組合せとdevice/endpointの監査を続ける。H1全体は未完了。

export完了と取消が重なると保留中の終了が実行される問題を回帰testで再現し、保存成功の記録と自動離脱を分離した。置換前の取消は既存出力・未保存編集を保持し、置換後は保存出力を残して自動離脱だけを止める。実windowの描画なし取消を両時点で確認し、完了PNGの64画素も一致した。試験用故障コード除去後の152 tests・Clippy・release buildが通過し、前checkpointのCIも成功。次は実device/endpoint経路と残る入力・日常flowの監査を続ける。物理IME・混在DPI・配布を含むH1全体は未完了。

音声device無効化のAPIエラーが一般Faultへ落ちる抜けを修正し、HRESULTで既存endpoint復旧へ分類した。実clientへの一回限りのエラー注入で再生/停止・rate・mute・回転を保持し、停止映像149,350画素が一致した。注入除去後の153 tests・Clippy・releaseと、無音の実WASAPI drain/rate 2 testsが通過した。OS設定や物理deviceは変更していないため、実切替matrixの完了とはしない。次はtrim範囲外・非稼働sessionの復旧とD3D11 removal経路を監査する。H1全体は継続中。

trim開始前の停止previewでは、復旧Seekが終了済み音声workerへPauseを送りFaultedになることを実windowで再現した。Pausedを新pipelineの作成条件へ渡す変更により、復旧前後311,736画素・位置・編集を保持し、Playでtrim開始へ戻れた。終端後preview、通常releaseのEOF→seek bar→Paused→再Playと153 tests・Clippy・buildも通過した。再現/修正比較は制御されたnative試験であり、物理device切替の証拠ではない。次は非稼働sessionの通知とD3D11 removal経路を監査する。H1全体は未完了。

停止sessionの切断callback登録と、音声結果公開後のgeneration付きAudioReadyを追加した。実WASAPIのpollなし完了待ちは修正前に失敗・修正後に成功し、folder pollを外した停止windowでも入力なしで復旧した。153 tests・実WASAPI 2 tests・Clippy・buildが通過。通常releaseの停止中closeは4回56～84msだが、制御試験の一度の5秒超過は未特定として残す。D3D11の公式強制TDR試験は他アプリへ影響するため未実行で、D3D12専用RemoveDeviceを代用しない。次は終了時の残る観測とlaunch全体の未達項目を整理する。H1全体は継続中。
