# towavue

towavueは、画像・動画・音声を一つの軽快なWindowsアプリで閲覧・再生し、基本的な非破壊編集まで行うことを目標とするプロジェクトです。メディアを主役にした最小UIと、D3D11を中心とするGPU常駐の表示経路を両立させます。

## 現在の状態

**M7: Advanced presentation and interactionまで完了しています。** M6までの閲覧・再生・非破壊編集基盤に、非同期waveform／thumbnail cache、timeline、メディア別grid menu、window外tab detach、hardware encode優先とfallback、HDR color-space能力判定を追加しました。source fileは直接変更せず、フォルダー内の移動順は同じフォルダーを開いているExplorerの実際のSort By状態を優先し、Explorerが閉じている場合もShell viewが解決した保存状態またはfolder templateを利用します。

- 対応予定OS: Windows 10 22H2以降
- 対応予定アーキテクチャ: x86-64
- Rust: 1.98.0 / Edition 2024 / MSVC ABI
- ライセンス: MIT OR Apache-2.0
- 次の工程: H1 human evaluation and UX stabilization。実際の利用flowを観察し、小さな検証可能な単位でUI/UXと機能の不一致を直す

## 文書

- [ARCHITECTURE.md](docs/ARCHITECTURE.md): 技術選定、境界、データフロー、不変条件
- [ROADMAP.md](docs/ROADMAP.md): 段階的な実装順序と各ゲート
- [DEVELOPMENT.md](docs/DEVELOPMENT.md): 開発版の試用方法、手動確認matrix、変更内容ごとの編集先
- [KNOWN_GAPS.md](docs/KNOWN_GAPS.md): 現時点の制約、UI草案との差、次に検証する順序
- [AGENTS.md](AGENTS.md): 実装者・エージェントが常に守るルール
- [SESSION_LOG.md](SESSION_LOG.md): セッションをまたぐ事実ベースの進捗記録

`concepts/`はUI草案を含むローカル参照資料であり、Gitには含めません。設計上の決定事項は上記の追跡対象文書へ転記します。

## ビルドと実行

fileを指定せず起動するとWelcomeのSTART欄からOpen File / Open Folderを選べます。現在のshortcutも表示し、幅が狭い場合はhoverで確認できます。Explorerからのdropでも開けます。最後のmedia tabを閉じるとWelcomeへ戻ります。recent一覧とsession復元はまだありません。

Visual StudioのDesktop development with C++ workload、Windows SDK、LLVMを導入します。Developer PowerShellで、checksum固定済みのFFmpeg 9.0.1開発ファイルを準備してから実行します。ダウンロード先と生成fixtureはGit対象外です。

```powershell
$ffmpegDir = .\scripts\setup-ffmpeg.ps1
$env:FFMPEG_DIR = $ffmpegDir
$env:LIBCLANG_PATH = Join-Path $env:ProgramFiles 'LLVM\bin'
$env:PATH = "$(Join-Path $ffmpegDir 'bin');$env:PATH"

.\scripts\generate-m1-fixtures.ps1
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run -p towavue-app -- path\to\media.mp4
```

標準shortcutはSpaceでpause/resume（再生終了後は先頭から再開）、左右矢印で前/次の画像へ移動（動画・音声では5秒Seek）、Ctrl+左右で同種media移動、Alt+左右で全種media移動、Fでfilmstrip、Ctrl+Shift+Pでcommand paletteです。画像移動はreading modeでも一枚ずつ進み、未保存編集があれば確認します。設定は初回起動時に`%APPDATA%\towavue\shortcuts.conf`へ生成され、`Ctrl+K Ctrl+S`のようなprefix shortcutも指定できます。画像用キーは`previous_image` / `next_image`で変更でき、既存設定fileへ追記しなくても新しい既定値を利用します。menuまたは同shortcutのReload commandで再読込します。

prefixの続きは1秒以内に入力します。Escape、click、別commandやwindowへのfocus移動で待ちを解除し、途中から戻ったkeyを前のprefixへつなげません。

設定ファイルでは`+` keyを`Plus`（例: `Ctrl+Plus`）と書けます。旧版が生成した`+`や`Ctrl++`も読み込めるため、既存設定の書き直しは不要です。

保存確認のEscapeは編集を保持してCancelします。Tab／Shift+Tabでbuttonを選び、Enter／Spaceで実行できます。export失敗ではエラー通知だけを閉じ、保留中の保存確認は残ります。背景クリックで保存・破棄・確認解除は行いません。

Cancel exportは保留中の自動終了・移動を止めます。保存の確定前なら既存出力と未保存編集を保持します。取消より先に保存が完了した場合は、その保存済み出力を残します。

graphicsの再作成に失敗した場合はWindows標準のRetry/Cancelを表示します。Cancelは編集を保持し、Alt+F4で終了を要求すると、描画なしでもYes（現在fileをExport）／No（終了時は全未保存編集を破棄）／Cancel（保持）の確認から保存できます。保存失敗もnative通知で案内し、編集は残します。export中はwindow titleへ進捗を表示します。

H1ではtitle/tab barを黒基調の単一barへまとめ、左端logoにmenu、右端にwindow操作、下部に再生操作と省略path・状態情報を配置しました。上部の空白をdragして移動、double-clickで最大化・復元、window端をdragしてresizeできます。長い名前はhoverで全文を確認できます。

tabはbar内でdragして並べ替えられます。挿入線の位置で離すと順序だけを変更し、表示中のmedia・未保存編集は保持します。Escapeまたはbar外・window内へのdropで取り消します。window外へのdropは既存の別windowへ移す操作になり、未保存時は確認します。

日本語の名前はWindowsにある日本語fontを補助fontとして表示します。fontの同梱・downloadは行いません。日本語fontがない環境では欠字が残り、terminalへ診断を出します。paletteはIME変換・確定とcommand実行のEnterを分離します。毎frameのfocus再要求による変換取消を修正し、Windows日本語IMEの候補表示・上下選択・確定・Escape取消を実windowで確認しました。物理keyboard・他IME・混在DPIを含む入力matrixは未完了です。

logo menuはFile / Edit / Viewに分かれています。FileにOpen・Export・tab close・shortcut再読込、EditにUndo・crop・回転・trim・音量/速度、Viewに再生・移動・zoom・reading・各overlayをまとめています。現在のcustom shortcutを右側に表示し、使えない項目は無効表示、縦に収まらない場合はscrollできます。

logoへTabでfocusしてEnter／Spaceでmenuを開けます。menu内は上下またはTab／Shift+Tabで有効項目を移動し、右でsubmenu、左で親へ戻ります。Enter／Spaceで選択、Escapeでmenu全体を閉じます。再open時は先頭項目へ戻り、keyboardで選んだ項目はscroll内に表示します。

F11またはViewのToggle fullscreenで、現在monitorのborderless fullscreenへ切り替えられます。通常のbar・timelineは隠れ、画像・動画・readingを広く表示します。pointerを下端へ移すと、映像のサイズを変えずにstatus／seek barとfullscreen解除buttonが現れます。操作部から離れると隠れますが、Seekのdrag中はreleaseまで保持します。Escapeは開いたoverlayを先に閉じ、次に通常windowへ戻ります。paletteやfilmstrip、保存確認はfullscreenでも利用でき、Tまたはstatus barのtimeline buttonで通常windowへ戻ってtimelineを表示します。復帰時は元の位置・size・最大化状態を保ちます。Enterは割り当てていません。

fullscreenの画像・動画・readingでは、操作が2秒ないとcursorも隠れます。pointer移動・click・wheel・key入力で戻り、button保持中や操作overlay・保存確認・読み込み中は表示を維持します。音声playlistとWelcomeでは隠しません。

timelineを閉じているときはstatus上端の細いbarで動画・音声の位置を変更できます。画像では同じfolderの画像順へ移動します。dragは離した時に一回だけ確定し、再生終了後のSeekは一時停止状態になります。command paletteは検索後に上下keyで候補を選び、Enterで実行、Escapeで閉じられます。

動画のhover thumbnailを取得できない区間では「Thumbnail unavailable」と表示し、同じfileを開いている間はその区間を繰り返し取得しません。再openで再試行できます。thumbnailの失敗だけで再生・Seek・保存を無効にはしません。

動画はbar・timelineを除いた領域へ縦横比を保って表示し、非正方形pixelのsample aspect ratioも反映します。hardware/softwareとも同じ表示矩形を使い、crop selectionも映像に合わせます。

動画の回転metadataも、90度単位の回転・反転として自動適用します。その向きを基準にcropや手動回転を行い、保存後も同じ向きになります。任意角度や変形を含む非対応のdisplay matrixは無視せず、理由を画面へ表示します。再生失敗の理由は、一時通知が消えた後も別mediaを開くまで残ります。

動画のR/Lによる90度回転、H/Vによる反転、selectionとCtrl+Yによるcropも、現在の再生画面へ操作順に反映します。Undo/Redoとtab復帰でも編集結果を表示し、回転後の縦横比を保ちます。hardware decodeの編集表示も同じGPU内で処理し、CPUへ映像を戻しません。

cropは確定したpixel矩形をpreviewと保存で共有します。画像は1 pixel、動画は偶数pixel単位へ選択を合わせ、確定時に出力寸法を表示します。動画は既定encoderの制約で16×16未満を確定せず、選択を残して案内します。画像の1×1 cropも保存でき、寸法が変わらない全領域cropでは未保存編集を増やしません。

Gはメディア種別ごとの4×4 grid menuを開き、`1234/qwer/asdf/zxcv`またはclickで選択します。配置は`%APPDATA%\towavue\grid.conf`で変更できます。動画・音声ではTでwaveform timelineを表示し、動画上のhover thumbnailとclick/drag Seekを利用できます。previewはUI thread外で生成され、path・size・更新時刻をkeyにした最大64 MiBのcacheを`%LOCALAPPDATA%\towavue\preview-cache`へ保存します。

timelineは上端をdragして高さを変えられます。高さの変更ではSeekや編集は行いません。windowを縮めると映像領域を残す高さへ制限し、狭い幅ではtrim表示の説明部分を省いて端点時刻を優先します。

trimの開始は上側、終了は下側のgripを横dragして調整できます。drag中は候補を表示するだけで、離した時に一回だけ編集し、Undoで戻せます。Escapeやfocus喪失で取り消します。逆転・零長の候補は赤く示し、離しても元の範囲を保持します。I/Oでの指定も引き続き使えます。

gridは小さいwindowでも4列を保ち、長い名前は省略・hoverで全文表示します。clickとkeyはどちらも一回実行して閉じます。

gridのkeyはQWERTYの`1234/qwer/asdf/zxcv`に相当する物理位置です。Shiftで位置は変わらず、Ctrl/Alt/Windows key付きは通常shortcutとして扱います。paletteを開くとgridは閉じます。

画像ではCtrl+wheelまたは+/-でzoom、Ctrl+Hでactual size、Shift+Wでfit、右dragでpanします。左dragでselectionを作り、辺dragでresize、Shift付き作成で正方形、Shift付きresizeで比率を保持します。選択範囲clickまたはCtrl+Shift+Yはcrop previewです。Bでreading mode、Rで縦横切替、Hで表示順反転、Ctrl+[ / Ctrl+]で表示数を2～10枚に変更できます。

画像の100%は画面の実pixel基準です。zoomは現在の表示領域とcrop・回転後の寸法を使うため、小さいwindowやcrop previewからの一段の拡大も現在の見た目を基準にします。

大きい画像もFitでは2%未満まで縮小して全体を収めます。reading modeにも同じ計算を使い、手動zoomの10%未満は小数2桁で表示します。

通常画像表示ではCtrl+Yでcropを履歴へ追加し、R/Lで90度回転、H/Vで反転します。動画・音声ではI/Oでtrimの開始・終了、上下矢印でvolume、Mでmute、`,` / `.` / `/`でrateを変更・resetします。Ctrl+Z / Ctrl+Shift+Zはundo/redo、Ctrl+Shift+SはSave As、Ctrl+Sは直近export先への再Saveです。dirtyなmediaの移動・close・終了時はExport / Discard / Cancelを選択できます。同一source pathへのexportは拒否されます。

画像と動画はfileごとのtab、音声は同じfolderのplaylist tabとして開きます。filmstripはShell snapshotの全対応mediaをExplorer順で表示し、middle clickで明示的に新規tabを作れます。H1では中央のthumbnail列へ変更し、画像・動画preview、音声waveformとdurationを表示します。現在項目を中央へ寄せ、wheelで横scroll、Tab / Shift+Tabで移動できます。previewは可視項目だけを単一workerで読み込みます。フォルダー変更は`ReadDirectoryChangesW`で検知してdebounce後にsnapshotを更新します。M1のcodec fixtureはMP4/H.264/AAC、MKV/HEVC/AAC、WebM/VP9/Opusです。再生終了時のdiagnosticにはadapter LUID、hardware frame数、CPU transfer数、表示・drop frame数、Seek latency、A/V driftを記録します。

Shellのフォルダー情報はH1で非同期取得に変更しました。取得中はstatusにOpening folder / Loading orderを表示し、切替後の古い結果は適用しません。native Open file/folder・Save Asも専用threadで表示し、選択中の描画・再生を継続します。本体への入力は従来どおりmodal制限され、選択画面を閉じると復帰します。

Explorerから画像・動画・音声file、またはfolderをwindowへdropして開けます。複数fileにも対応し、画像・動画は新規tab、音声は同folderのplaylistへ入ります。folderはShell順の先頭mediaを開く方式です。未保存編集は元tabに保持し、確認dialogの最中はdropを受け付けません。sourceの移動やcopyは行いません。

tabをwindow外へdragしてdropすると同じmediaを別processのwindowへ移し、dirtyなtabには既存のExport / Discard / Cancel guardを適用します。動画exportはCtrl+Shift+EでMedia Foundation hardware encode優先を切り替えられ、利用不能ならsoftwareへfallbackし、実際の経路をstatusへ表示します。PQ/HLG sourceはD3D11 Video Processorの色空間変換能力を確認してからSDRへtone mapし、adapterが変換を保証しない場合は不正な色で表示せず明示的なerrorにします。

H1ではSaveをbackground化しました。書き出し中も再生・tab切替・追加編集ができ、進捗windowからcancelできます。成功時だけ出力先を置換し、失敗・cancelでは既存fileと編集を保持します。書き出し開始後に追加した編集は未保存のまま残ります。

volume・mute・rateは現在の再生にも反映します。rateはピッチを維持した0.25～4倍で、変更時は現在位置から再開します。undo/redo・Seek・tab復帰でも編集値を使い、他アプリやWindowsのmaster volumeは変更しません。timelineとSeekの時刻は元メディア基準です。

画像とreading pageのdecodeもbackground化し、切替後の古い結果は表示しません。保持するRGBA frame列は1要求合計512 MiBまでです（decoderの作業領域やGPUを含むprocess全体の上限ではありません）。上限超過・破損画像・GPUの寸法上限は画面へerrorを表示します。

静止画・一時停止・静止したmenuでは不要な連続描画を止め、入力・読み込み完了・animationの期限に応じて更新します。音声再生の位置表示も期限付きで更新し、音声出力のpollから無条件に再描画しません。

再生の映像・音声は独立した入力と上限付きqueueで供給します。音声の出力待ちが映像の初期表示や停止中Seekを塞がず、短い映像と長い音声の組合せでも映像を表示できます。hardware成立前は音声decodeを開始せず、fallbackによる音声の二重再生を避けます。

I/Oでtrim端点を指定するとtimelineが開き、再生・保存するsource範囲をミリ秒付きで表示します。未指定の開始/終了は先頭/末尾です。逆転・零長・範囲外はその場で拒否し、同じ端点の再指定は履歴を増やしません。Playは範囲内を再生して終端で停止し、再Playで範囲の開始へ戻ります。範囲外へSeekすると一時停止のsource previewになり、I/Oで端点を選び直せます。その状態のPlayも範囲の開始へ戻ります。Undo/Redo・rate変更・tab復帰でも現在のtrimを使います。

trim保存は開始以上・終了未満のframe／sampleを選びます。短すぎて動画frameや音声sampleが残らない場合は保存を失敗にし、既存の出力先を保護します。動画の最後のframe長や圧縮音声のpaddingにより、出力containerのdurationは指定区間と同一とは限りません。低精度PTSの途中Seek後に残る音声sample位相差は監査中です。

## ライセンス

本リポジトリのコードは、利用者の選択により[MIT License](LICENSE-MIT)または[Apache License 2.0](LICENSE-APACHE)の下で利用できます。将来同梱するFFmpeg DLLとその他の第三者コンポーネントには、それぞれのライセンスが適用されます。
