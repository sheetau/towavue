# Native FFmpeg build and developer staging

WSLを使わず、固定MSYS2環境と5つの検証済みsource prefixからFFmpegをbuildする手順。Setup.exe作成や配布承認ではない。構成選定と過去の失敗は[MABS_BUILD.md](MABS_BUILD.md)、対応資料の範囲は[NATIVE_RUNTIME_AUDIT.md](NATIVE_RUNTIME_AUDIT.md)を参照する。

## 入力と実行

必要なものは次のとおり。scriptはpackage導入・更新・source取得・patch適用を行わない。

- 固定snapshotのMSYS2 330 packages。toolchain/mediaの両input一覧と名前・版が一致すること。
- 固定FFmpeg commit `e47273f4d9227152dcbf543cebaf9e2430ddbcc4`のsource archive、17323649 bytes、SHA256 `6491dae95e3cf3cdbac02933b55860e782b0c4f0a6bd8f37cef30fded259283c`。既定は既存の`vendor/ffmpeg/source-<commit>.tar.gz`。
- 既存の`build-aribb24-native`、`build-lcevc-native`、`build-librist-native`、`build-video-codec-native`から作った5 prefix。各source revisionと許可patchの検証はこれらの前段scriptの責務である。FFmpeg builderはprefix内の実入力hashを記録するが、そのhashだけで前段の正しいbuildを証明しない。
- 新しい短いASCII／空白なし出力path。既存directoryは拒否し、失敗した出力も消去・再開しない。相対pathはPowerShellの現在locationを基準に解決する。

```powershell
.\scripts\build-ffmpeg-native.ps1 -MsysRoot 'C:/tvbuild/msys64' `
    -BuildDirectory 'C:/tvbuild/ffmpeg-new' `
    -Aribb24Prefix 'C:/tvbuild/aribb24/prefix' `
    -LcevcPrefix 'C:/tvbuild/lcevc/prefix' `
    -LibristPrefix 'C:/tvbuild/librist/prefix' `
    -Uavs3dPrefix 'C:/tvbuild/uavs3d/prefix' `
    -VvencPrefix 'C:/tvbuild/vvenc/prefix'

.\scripts\test-ffmpeg-native-build.ps1 -MsysRoot 'C:/tvbuild/msys64' `
    -BuildDirectory 'C:/tvbuild/ffmpeg-new' `
    -ReferenceExecutable 'path/to/reference/bin/ffmpeg.exe'
```

上記pathは説明用。前段prefixのpkg-configには生成時の絶対pathが含まれる。pkgconfがprefixを自動補正する場合もあるが、全fieldやlibrary/headerの整合性を保証するものではないため、移動だけで正しいと判断せず、必要な場所で前段を再生成する。

## 手順が検査すること

source archiveの全10548 entryは通常file／directoryのみで、単一root内にあり、symlink／traversalなしと監査した。builderはその固定size/hashを確認してから、新しいsource treeへ展開する。

pkg-config、CFLAGS、LDFLAGSの順序をaribb24 → LCEVC → librist → uavs3d → VVenC → MSYS2にする。全81 optionを維持し、生成configの61 enable、SPIR-V header、GPL/nonfree無効を確認する。CFLAGS/LDFLAGSでは先頭空白を許容し、5 prefixの順序は許容差なく確認する。後方のMSYS2 libraryを誤選択した過去のARIB問題を再発させないため、compile後のavcodecにも2個のARIB定義を要求する。

non-login Bashの8-job buildと`make install`を別logへ保存する。process内の探索・compiler flag・Bash起動file設定を一時的に外し、成功／失敗時とも復元する。OS全体のPATHやtoolchain設定は変更しない。

標準install後、7個のMSVC import libraryを`bin`から`lib`へ同一byteでcopyする。ffmpeg／ffprobeのPE graphを取得し、監査済み候補の94 file名とimport edgeが一致すること、85 package DLLのsize/hashが一致することを要求する。その85 DLLだけをprefix/binへcopyし、隣接fileだけでgraphを再取得する。

| 出力 | 意味 |
|---|---|
| `source/`、`build/` | 固定sourceの新規展開と別build tree |
| `prefix/` | MSVCアプリ検証向けの開発prefix。ffplay／headers／examplesも含み、全体を配布する決定ではない |
| `configure.log`、`build.log`、`install.log` | 各実行のstdout/stderr。configure内部の詳細はbuild/ffbuild/config.log |
| `build-inputs.json` | source hash、実configure引数、installed packages、5 prefixの全file hash。ローカル絶対pathを含みGitへ入れない |
| `runtime.json` | 配置後の実PE graphとfile hash |
| `COMPLETE.json` | 全build／配置検査を通った時だけ作るmarker。配布承認はfalse |

testは完成markerだけを信用せず、全runtime graph／hash、prefix入力の保持、import library copyを再検査する。referenceとの7種類の機能集合比較、System32のみのPATH・FFMPEG_DIRなし・別cwdで3 frameのencode/decodeと8条件のChromaprint比較を行う。既存output・欠落／同size改変source・誤ったpkg-config prefixの拒否と、入力／環境の保持も確認する。

## 実行結果と残る範囲

2026-09-08の実行では、新しいsource/build/prefixへのconfigure・build・installと94-file stagingが完了した。実出力は137393602 bytesで、以前の候補とfile名／import edgeと85 package DLLのbyteは一致するが、再生成FFmpegを含む全byte一致は主張しない。config.hの842定義の差は配置pathとconfigure文字列の3定義だけで、boolean値の差は0だった。CC_IDENT、CFLAGS、CXXFLAGS、LDFLAGS、ASFLAGS、EXTRALIBSの実生成値も以前の成功buildと一致した。

初回は生成CFLAGSの先頭空白を検査が拒否したためcompile前に停止した。検査を修正し、別の新規出力先・任意cwdで再実行した。呼出元へPATH、pkg-config、BASH_ENV、CFLAGSを復元することも成功時に確認した。

testで当初作った誤prefix fixtureは、pkgconfの自動補正によって選択検査を通り、意図しない試験用configureが始まった。その試験のBash processだけを停止し、部分出力を保持した。fixtureはpackage descriptorを欠落させ、旧MSYS2 prefixへfallbackしたことを拒否する形へ修正した。再実行では全検査が成功し、試験用build processの残存はない。

機能集合はdecoder 537／encoder 228／filter 531／demuxer 364／muxer 184／protocol 44／hardware API名9でreferenceと完全一致、3 frameの補助exe encode/decodeと8条件の音声指紋各140 wordsも一致した。新規Rust target directoryから、この再生成prefixをFFMPEG_DIRへ指定したMSVCのformat／Clippy／268 testsも成功した。3件のlive ignoreは未実行で、旧開発DLL・release exeは変更していない。hardware優先exportの個別再実行はMedia Foundation H.264 encoderを利用できず、hardware assertionを明示skipした。fallbackと出力decodeの成功であり、hardware encode成功とは扱わない。

この手順は既存の正しい構成を新規build／stagingで再現するものであり、全dependencyをsourceから再buildしたり、異なるpath／日時のPEがbit単位で同じと保証したりするものではない。installed packageの名前／版チェックも、その全header／static archiveのbyte検証や署名検証の代わりにはしない。

続く通常releaseの代表保存／再open、4条件各100回のSeek、30分4K60再生は[DEVELOPMENT.md](DEVELOPMENT.md)の条件で通過した。ただし[追加source／実DLL監査](NATIVE_RUNTIME_AUDIT.md)でZVBIの個別GPL表記に対応する関数が確認されたため、この候補をLGPL配布構成として承認しない。性能合格やCOMPLETE markerは、この条件を解決するものではない。

各DLL／静的・header-only・埋め込みdataの対応材料と許諾範囲、最終採用binaryの媒体・長時間・性能確認、helperの隣接探索とSetup.exe、隔離した対象Windowsでの導入／更新／削除を引き続き必要とする。列挙されるhardware APIの名前は、実hardware成功を示さない。
