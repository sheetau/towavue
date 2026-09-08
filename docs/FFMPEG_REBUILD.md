# FFmpeg配布buildの再構成

## 既存binaryを配布しない理由

2026-09-08、固定BtbN archiveのChromaprint→FFTW静的リンクを確認した。[根拠一覧](ffmpeg-distribution-rejection.json)にrecipe、当時のlibraryとpkg-config、avformat DLLのhashと識別文字列の位置を記録した。FFmpeg自身のLGPL表示とGPL無効の設定は、推移依存までLGPL構成である証明にならない。

固定[Chromaprint README](https://github.com/acoustid/chromaprint/blob/aed8eba2202dd9d7b3b0a56c77904cc805490d72/README.md#fft-library)も、FFTW backendを使ったbinaryはGPLになると説明する。FFTWの固定COPYRIGHTはGPL 2以降である。これはGPL配布自体が不可能という意味ではなく、towavueで決めたMIT OR Apache-2.0本体とLGPL FFmpeg構成に、このbinaryをそのまま採用しないという判断である。別契約の存在を推測せず、本体のlicense変更や商用license購入も行わない。

既存vendor archive、開発build、検証記録は保持する。ただしinstallerへcopyする候補ではない。codecを暗黙に無効化してこの問題を回避しない。

## 最小の修正案と単体検証

[chromaprint-kissfft.patch](../third-party/ffmpeg/chromaprint-kissfft.patch)は固定recipeの1 fileだけを変更する。

- Chromaprint stageのFFTW依存を除く。
- `FFT_LIB=kissfft`を明示し、同じ固定Chromaprint sourceに含まれるKissFFTを使う。
- `Libs.private`から`-lfftw3`だけを除き、`-lstdc++`を保持する。FFmpegの`--enable-chromaprint`は維持する。

これはFFmpeg全体の修正済みbinaryではない。適用前hashは`18f0d650b2f6442f374dd8b1a2fa000a15ab88ecbf021a0a3f5a32750c0992b6`、適用後は`e66512544f8a950657b1e41defe2b2a966b412fa6bd5e0a5239e73e46d487de9`。固定原本に限定するzero-context patchなので、`git -c core.autocrlf=false apply --unidiff-zero --check`で確認してから同じoptionで適用する。隔離fixtureで実適用と結果hashを確認済み。

同commitのChromaprintを既存MSVCとCMakeで単体buildした。`BUILD_SHARED_LIBS=ON`、`BUILD_TOOLS=OFF`、`BUILD_TESTS=OFF`、`FFT_LIB=kissfft`、`CMAKE_DISABLE_FIND_PACKAGE_FFmpeg=ON`を指定し、内部avresampleは既定のまま。source path/typeを検査し、`.git`とheaderのsymlink aliasを除いて隔離directoryへ展開した。上流の数値変換warningは発生しており、警告なしのproduction buildとは扱わない。

20秒の440 Hz音、linear chirp、seed 1202のwhite noise、無音を、11025 Hz monoと44100 Hz stereoで生成した。各PCMを単体API（new/start/feed/finish/get_raw_fingerprint、algorithm 1）と従来ffmpegのchromaprint muxer（raw形式）へ渡すと、8条件すべてで140個のuint32値が完全一致した。任意の入力での同値性、処理速度、GNU targetの全体buildを証明したものではない。

KissFFTのCOPYINGだけでなく、その参照先`LICENSES/BSD-3-Clause`も対応資料へ含める。既存のbasename-prefixによるnative通知探索では、この参照先本文は検出対象外だった。2 fileのhashは根拠一覧に記録した。新backendの資料を、FFTW版binaryの元の通知であるかのように混ぜない。

## 次の実装・検証ゲート

1. 固定FFmpeg sourceと同じ機能を保つcontrolled build環境を用意する。上記patchを適用したChromaprintとFFmpeg全体を再linkし、compiler・全有効依存・source・patch・configure・link設定を取得時点で固定する。rav1eの`cargo update cc`のような非固定更新を新buildへ持ち込まない。既存のLinux build imageの実行、WSL/Dockerの導入やOS変更はまだ行っていない。
2. 新binaryのfeature一覧を旧binaryと比較する。Chromaprintを残し、FFTWをlinkしていないことをbackend設定、実link入力、library symbolとbinaryの検査で確認する。文字列がないだけでは不在証明としない。fingerprintの比較と速度計測も行う。
3. 新binaryに対応する全source/notice/runtime資料を作る。原本recipe archiveは`git -c core.autocrlf=false archive`で生成した`recipe-8267213e26c1031621e6e1210fe3aa4867214f6a-lf.tar`を使い、全265 entryのGit blob一致を再確認する。旧archiveはCRLF変換されていたため対応原本として使わない。
4. towavueを新DLLへ差し替えて、format/Clippy/testに加え、画像・動画・音声の再生/Seek/preview/保存/再open、hardware decode/encodeの既存gateを再検証する。単体Chromaprint試験で代用しない。
5. VC runtimeの条件と同梱一覧を確定した後、選択済みSetup.exeの実装と隔離環境の導入/更新/削除試験へ進む。binary署名・公開・file関連付けは別途扱う。
