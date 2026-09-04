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

## M4 — Application shell and navigation

tab、status bar、command registry、command palette、customizable shortcut、audio folder playlist、filmstrip、Explorer folder sort連携を実装する。

Explorer sortは`docs/ARCHITECTURE.md`の`FolderSnapshot`契約と検証matrixを満たすこと。

## M5 — Images and reading mode

静止画、アニメ画像、zoom、selection、crop preview、reading modeを追加する。動画と共有するのはvisual surfaceとpresentation上の概念に限定する。

## M6 — Non-destructive editing and export

crop、rotate、flip、trim、volume、rateを非破壊操作として保持し、undo/redo、unsaved indicator、close guard、FFmpeg exportを追加する。

## M7 — Advanced presentation and interaction

HDR、waveform/thumbnail cache、multi-window tab drag、grid menu、hardware encode、詳細アニメーションを追加する。
