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

標準shortcutはSpaceでpause/resume、左右矢印で5秒Seek、Ctrl+左右で同種media移動、Alt+左右で全種media移動、Fでfilmstrip、Ctrl+Shift+Pでcommand paletteです。設定は初回起動時に`%APPDATA%\towavue\shortcuts.conf`へ生成され、`Ctrl+K Ctrl+S`のようなprefix shortcutも指定できます。menuまたは同shortcutのReload commandで再読込します。

Gはメディア種別ごとの4×4 grid menuを開き、`1234/qwer/asdf/zxcv`またはclickで選択します。配置は`%APPDATA%\towavue\grid.conf`で変更できます。動画・音声ではTでwaveform timelineを表示し、動画上のhover thumbnailとclick/drag Seekを利用できます。previewはUI thread外で生成され、path・size・更新時刻をkeyにした最大64 MiBのcacheを`%LOCALAPPDATA%\towavue\preview-cache`へ保存します。

画像ではCtrl+wheelまたは+/-でzoom、Ctrl+Hでactual size、Shift+Wでfit、右dragでpanします。左dragでselectionを作り、辺dragでresize、Shift付き作成で正方形、Shift付きresizeで比率を保持します。選択範囲clickまたはCtrl+Shift+Yはcrop previewです。Bでreading mode、Rで縦横切替、Hで表示順反転、Ctrl+[ / Ctrl+]で表示数を2～10枚に変更できます。

通常画像表示ではCtrl+Yでcropを履歴へ追加し、R/Lで90度回転、H/Vで反転します。動画・音声ではI/Oでtrimの開始・終了、上下矢印でvolume、Mでmute、`,` / `.` / `/`でrateを変更・resetします。Ctrl+Z / Ctrl+Shift+Zはundo/redo、Ctrl+Shift+SはSave As、Ctrl+Sは直近export先への再Saveです。dirtyなmediaの移動・close・終了時はExport / Discard / Cancelを選択できます。同一source pathへのexportは拒否されます。

画像と動画はfileごとのtab、音声は同じfolderのplaylist tabとして開きます。filmstripはShell snapshotの全対応mediaをExplorer順で表示し、middle clickで明示的に新規tabを作れます。フォルダー変更は`ReadDirectoryChangesW`で検知してdebounce後にsnapshotを更新します。M1のcodec fixtureはMP4/H.264/AAC、MKV/HEVC/AAC、WebM/VP9/Opusです。再生終了時のdiagnosticにはadapter LUID、hardware frame数、CPU transfer数、表示・drop frame数、Seek latency、A/V driftを記録します。

tabをwindow外へdragしてdropすると同じmediaを別processのwindowへ移し、dirtyなtabには既存のExport / Discard / Cancel guardを適用します。動画exportはCtrl+Shift+EでMedia Foundation hardware encode優先を切り替えられ、利用不能ならsoftwareへfallbackし、実際の経路をstatusへ表示します。PQ/HLG sourceはD3D11 Video Processorの色空間変換能力を確認してからSDRへtone mapし、adapterが変換を保証しない場合は不正な色で表示せず明示的なerrorにします。

H1ではSaveをbackground化しました。書き出し中も再生・tab切替・追加編集ができ、進捗windowからcancelできます。成功時だけ出力先を置換し、失敗・cancelでは既存fileと編集を保持します。書き出し開始後に追加した編集は未保存のまま残ります。

volumeとmuteは現在の再生にも反映します。undo/redo・Seek・tab復帰でも編集値を使い、他アプリやWindowsのmaster volumeは変更しません。rateは現段階ではexport用で、再生速度は変更しません。

画像とreading pageのdecodeもbackground化し、切替後の古い結果は表示しません。保持するRGBA frame列は1要求合計512 MiBまでです（decoderの作業領域やGPUを含むprocess全体の上限ではありません）。上限超過・破損画像・GPUの寸法上限は画面へerrorを表示します。

## ライセンス

本リポジトリのコードは、利用者の選択により[MIT License](LICENSE-MIT)または[Apache License 2.0](LICENSE-APACHE)の下で利用できます。将来同梱するFFmpeg DLLとその他の第三者コンポーネントには、それぞれのライセンスが適用されます。
