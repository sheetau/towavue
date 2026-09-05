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
