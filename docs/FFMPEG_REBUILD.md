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

### WSLを使わない候補選定（2026-09-08）

ownerはdebug・保守負担を理由にWSL 2を採用しないと決定した。WSL導入の質問は解決済みで、導入・OS機能変更を行わない。以下は調査結果と推奨順であり、新toolchainの導入や配布binaryの採用完了ではない。

- **第一候補: media-autobuild_suiteのWindows native build。** [固定README](https://github.com/m-ab-s/media-autobuild_suite/blob/02eab87287e2df528f5c48512677684c323cacd0/README.md)はMSYS2/MinGW-w64上でのbuild、shared出力、短い空白なしpath、64-bit buildで18GB以上の空きを案内する。WSL/Dockerは不要だが、別compiler環境の保守は残る。[compile script](https://github.com/m-ab-s/media-autobuild_suite/blob/02eab87287e2df528f5c48512677684c323cacd0/build/media-suite_compile.sh)はChromaprint選択時にfftw packageを除き、MSYS2のchromaprintを使用する。[MSYS2 recipe](https://github.com/msys2/MINGW-packages/blob/052099e63e69816e35b05f28c852a5209c4dd1e0/mingw-w64-chromaprint/PKGBUILD)は1.6.1のstatic/shared双方で`FFT_LIB=kissfft`を指定する。これは今回のbackend問題を避けられるrecipe上の根拠であり、生成binaryや全推移依存の検証を代替しない。
- **BtbN既成lgpl-sharedは条件付き候補。** ローカルbuild環境が不要な利点はあるが、調査時HEADの[Chromaprint recipe](https://github.com/BtbN/FFmpeg-Builds/blob/281eb062dbebd20014f777dd7bb651330443f181/scripts.d/50-chromaprint.sh)にも`FFT_LIB=fftw3`と`-lfftw3`が残る。最新assetを検査したとは扱わず、名称や更新日だけで既存の棄却判断を解除しない。FFmpeg majorを下げる代替もABI変更を伴うため暗黙に採用しない。
- **予備案: 修正したBtbN recipeをCIだけでbuild。** 開発機へWSL/Dockerを入れずにWindows用DLLを生成できる設計だが、Linux CI/imageの保守は残る。下記の旧image復元は直近の必須作業ではなく予備調査へ下げる。新たなworkflow実行・image復元は今回の候補比較では行わない。

[MABS_BUILD.md](MABS_BUILD.md)へ固定revisionのINI値、更新動作、既存機能との差を記録した。次はbootstrap/package集合と不足する依存の準備方法を固定する。FFmpeg commit、MSYS2 package version/配布archive、source、patch、compilerを固定・保存し、suiteの自動更新とsource削除をそのまま配布手順へ持ち込まない。既存MSVC Rustアプリとのリンク、DLL探索、再生/保存/性能の確認後に採用を判断する。機能の削除や本体license変更を前提にしない。

### 隔離CIでの全体再リンク検証

`.github/workflows/ffmpeg-relink-probe.yml`は手動実行専用であり、通常pushごとに重い再buildを始めない。既存の固定image `d1d34e5b...`をdigest指定で取得し、config digestも`f895b2da...`へ一致することを確認する。ChromaprintとFFmpegのcodeload sourceは固定commitとSHA256で検証する。Chromaprint archiveは1582333 bytes、SHA256 `eba1536d49daa17ae3c56904ea004342c42135dfdaabd7e9c5decbdb473d95ca`で、430通常fileが確認済みcacheと一致した。両archiveのpath/typeを検査済みで、唯一のheader symlink aliasは展開しない。

`scripts/probe-ffmpeg-relink.sh`はimage内でChromaprintをGNU targetの静的libraryとして再buildし、新prefixのpkg-configを優先してFFmpeg全体を再リンクする。imageの既存feature flagsを保持し、link traceを有効にして、新libraryの選択とFFTW link入力の不在を検査する。7 DLLとffmpeg/ffprobeの生成を確認するが、Windowsでの実行や性能検証は別gateである。

2026-09-08の初回probe run `34176348521`はsource取得・検証後、固定imageのpullで`manifest unknown`となり停止した。container内buildは未実行である。registryのmanifest GET/HEADは404だが、保存済みmanifest/configのSHA256は元digestと一致し、指定するconfigと17 layerのHEADはsize一致で200だった。固定blob集合から元環境を復元する調査は予備案として保留する。全layerの取得・展開・Docker import成功はまだ確認していない。

コンテナは非root、networkなし、read-only root、capabilityなし、権限昇格なし。source/scriptはread-only、一時workとtmpfsだけを書込み可能にする。Docker socketやGitHub tokenを渡さず、artifact/cache/image/releaseを公開するstepはない。既存の他libraryを再利用するこのprobeだけでは、有効依存graph・対応source/notice全体の確定を代替しない。開発機のWSL導入や本体DLLの差し替えも行わない。

1. 固定FFmpeg sourceと同じ機能を保つcontrolled build環境を用意する。上記patchを適用したChromaprintとFFmpeg全体を再linkし、compiler・全有効依存・source・patch・configure・link設定を取得時点で固定する。rav1eの`cargo update cc`のような非固定更新を新buildへ持ち込まない。既存のLinux build imageの実行、WSL/Dockerの導入やOS変更はまだ行っていない。
2. 新binaryのfeature一覧を旧binaryと比較する。Chromaprintを残し、FFTWをlinkしていないことをbackend設定、実link入力、library symbolとbinaryの検査で確認する。文字列がないだけでは不在証明としない。fingerprintの比較と速度計測も行う。
3. 新binaryに対応する全source/notice/runtime資料を作る。原本recipe archiveは`git -c core.autocrlf=false archive`で生成した`recipe-8267213e26c1031621e6e1210fe3aa4867214f6a-lf.tar`を使い、全265 entryのGit blob一致を再確認する。旧archiveはCRLF変換されていたため対応原本として使わない。
4. towavueを新DLLへ差し替えて、format/Clippy/testに加え、画像・動画・音声の再生/Seek/preview/保存/再open、hardware decode/encodeの既存gateを再検証する。単体Chromaprint試験で代用しない。
5. VC runtimeの条件と同梱一覧を確定した後、選択済みSetup.exeの実装と隔離環境の導入/更新/削除試験へ進む。binary署名・公開・file関連付けは別途扱う。
