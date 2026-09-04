# towavue

towavueは、画像・動画・音声を一つの軽快なWindowsアプリで閲覧・再生し、基本的な非破壊編集まで行うことを目標とするプロジェクトです。メディアを主役にした最小UIと、D3D11を中心とするGPU常駐の表示経路を両立させます。

## 現在の状態

**M4: Application shell and navigationまで完了しています。** M3までの再生基盤に、tab、status bar、command menu／palette、カスタムshortcut、audio folder playlist、filmstripを追加しました。フォルダー内の移動順は、同じフォルダーを開いているExplorerの実際のSort By状態を優先し、Explorerが閉じている場合もShell viewが解決した保存状態またはfolder templateを利用します。Shell取得失敗時だけWindows自然名前順へ縮退します。画像の表示と編集はまだ実装していません。

- 対応予定OS: Windows 10 22H2以降
- 対応予定アーキテクチャ: x86-64
- Rust: 1.98.0 / Edition 2024 / MSVC ABI
- ライセンス: MIT OR Apache-2.0
- 次の工程: M5 images and reading mode

## 文書

- [ARCHITECTURE.md](docs/ARCHITECTURE.md): 技術選定、境界、データフロー、不変条件
- [ROADMAP.md](docs/ROADMAP.md): 段階的な実装順序と各ゲート
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

画像と動画はfileごとのtab、音声は同じfolderのplaylist tabとして開きます。filmstripはShell snapshotの全対応mediaをExplorer順で表示し、middle clickで明示的に新規tabを作れます。フォルダー変更は`ReadDirectoryChangesW`で検知してdebounce後にsnapshotを更新します。M1のcodec fixtureはMP4/H.264/AAC、MKV/HEVC/AAC、WebM/VP9/Opusです。再生終了時のdiagnosticにはadapter LUID、hardware frame数、CPU transfer数、表示・drop frame数、Seek latency、A/V driftを記録します。

## ライセンス

本リポジトリのコードは、利用者の選択により[MIT License](LICENSE-MIT)または[Apache License 2.0](LICENSE-APACHE)の下で利用できます。将来同梱するFFmpeg DLLとその他の第三者コンポーネントには、それぞれのライセンスが適用されます。
