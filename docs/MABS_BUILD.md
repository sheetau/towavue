# Windows native FFmpeg build候補

2026-09-08時点のsource監査。WSLは使わない。まだ実行可能な配布build手順ではなく、MABSを無変更で起動してよいという承認でもない。

## 固定した調査対象

- media-autobuild_suite: `02eab87287e2df528f5c48512677684c323cacd0`。ignoredな`vendor/ffmpeg/media-autobuild_suite`へ取得し、LFのままcheckout、worktree cleanを確認した。batchやshell scriptは実行していない。
- FFmpeg: 既存の`e47273f4d9227152dcbf543cebaf9e2430ddbcc4`を維持する。
- 比較基準: SHA256 `f895b2da6a46618e840f97ae85abf1b946cfb39b692d7642cf89a06fefedbbbc`の既存image config内`FF_CONFIGURE`。81 option、うち61 enable。これは実decoder/filter一覧ではなく、明示build引数の基準である。
- MSYS2 recipe調査: `052099e63e69816e35b05f28c852a5209c4dd1e0`。recipe commitだけでは、pacmanが取得する実packageを固定したことにならない。

主要MABS原本のSHA256:

| File | SHA256 |
|---|---|
| media-autobuild_suite.bat | `428b9a9deb7137606a6275403492427eda38acffa70abeb45e644204260e9ed0` |
| build/media-suite_compile.sh | `b391e4489a738e530261f6388010241968e8cbfa94be323e9b34e6dd54148b4c` |
| build/media-suite_helper.sh | `fb55749ba8cf382070863259e4374b1c360792dabca9bc7fb3db32e98bd815c4` |
| build/media-suite_update.sh | `663b6aff9d16c6c18121087f974fff972a30eb64478a3b9e325569cd7d749a48` |

## 固定bootstrapの取得・検証

[msys2-bootstrap-inputs.json](msys2-bootstrap-inputs.json)は日付release `2026-06-11`のx64 base SFX、上流checksum、署名、package一覧をsize/SHA256で固定する。調査時にGitHubの`releases/latest` APIはnightlyを返したため、このAPIをsetupで解決しない。

```powershell
.\scripts\get-msys2-bootstrap.ps1 -Download
.\scripts\test-msys2-bootstrap.ps1
```

取得先はignoredな`vendor/msys2/bootstrap-20260611`。`-Download`なしはoffline検証だけであり、不足cacheは作らず失敗する。取得ありでも既存の不正cacheを上書きしない。curlの接続20秒・全体180秒上限、download一時fileのhash検証後の移動を使う。このscriptとCIは原本取得/検証だけで、SFX実行、MSYS2初期化やpackage更新を行わない。

SFXは52898952 bytes、SHA256 `c105946e64e08f099ac0e4647461ce762b95333ad211777666476a9a41451d65`。ローカルで取得・hash照合し、隔離したGnuPG keyringで署名を検証した。VALIDSIGのprimary fingerprintは[公式installer文書](https://www.msys2.org/docs/installer/)の`0EBF782C5D53F7E5FB02A66746BD761F7A49B0EC`と一致し、署名subkeyは`E0AA0F031DBD80FFBA57B06D5A62D0CAB6264964`。個人keyringやtrust設定は変更していない。取得script内でGnuPG署名検証を自動実行した扱いにはしない。

ローカルでは、archive全16581 entryが`msys64/`以下で、通常file/directoryだけであることを確認した。Windows tarは一覧を読めるが、BOING.WAVの7z filterを扱えず展開が失敗した。`vendor/msys2/base-20260611`は不完全な診断用展開であり使用しない。検証済みSFXの抽出だけを別の新規`vendor/msys2/base-sfx-20260611`へ実行し成功した。local package databaseの全90件の名前/versionは公式一覧と一致する。この長いpathは調査・外部installer用とし、実build用の短いASCII pathは下記の別環境に用意した。MABS自体はまだ実行していない。

これはbase環境だけの固定である。追加のcompiler/FFmpeg依存packageや更新後の実効graph、全対応sourceの固定が済んだという意味ではない。

## 初期ビルドツールのpackage固定

[msys2-toolchain-inputs.json](msys2-toolchain-inputs.json)は2026-09-08に取得したmingw64/msys databaseから、初期MABS環境の235 package（圧縮合計251642436 bytes）を固定する。bootstrapに含まれる90 packageすべてとMABS初期toolの計114 rootを、空の隔離local databaseに対してpacman 6.1.0/libalpm 14.0.0で解決した。特定のinstalled状態に依存した差分一覧ではなく、基礎環境を含む取得集合である。

- mingw64 database SHA256: `58dada89a98d5edcd8b3d959d62202ffff132a1027f6923dabc19c0a220bd5fe`
- msys database SHA256: `195d16e76dffa1b4322ac8e7eb0d68e2c989b14ed8120a1a0ff28a9cb5c9cd46`
- 両databaseの署名者fingerprint: `5F944B027F7FE2091985AA2EFA11531AA0AA7F57`
- 新GNU候補: GCC `16.2.0-3`、CMake `4.4.3-2`、Ninja `1.13.2-1`、NASM `3.02-1`。従来BtbNのGCC 15.2と同一toolchainではなく、Rust本体のMSVC環境は変えない。

署名検証には署名済みbootstrapに含まれるMSYS2公開鍵と5つのmaster key一覧を使用した。専用keyring内だけでその5鍵を明示trust anchorとして設定し、database signerのfull trustを確認した。個人keyringや既存環境の署名policyは変更せず、隔離pacman設定も`SigLevel = Required`を維持する。長いpathでagent初期化が失敗するため、ここではsecret key作成や通常のpacman-key初期化を行っていない。pacmanは旧pubring形式がないというwarningを出すが、署名検証付きのprint-only解決は成功した。これはそのまま本番の鍵初期化手順ではない。

全235 recordのname/version/size/SHA256/filenameを元database entryと別途照合し、すべてのrootが選択またはprovideされることを確認した。依存version制約の解決は独自実装でなくpacmanに任せる。91 entryには埋込みPGPSIGがなく、直接取得するdetached signatureを検証する。署名欄の不在だけでpackageを未署名と断定したり、署名検証を無効化して導入したりしない。

```powershell
.\scripts\get-msys2-toolchain.ps1 -Download
.\scripts\test-msys2-toolchain.ps1
```

getterは`vendor/msys2/packages-20260908`へ固定URLからarchiveとdetached signatureを取得し、size/hashを照合する。switchなしはoffline、改変cacheは取得ありでも保持して拒否する。packageの展開・導入、hook、live repository解決は行わない。通常pushのCIへ235 packageの取得を追加せず、native build準備として明示実行する。この集合にはFFmpegの全外部libraryやLCEVC追加build、対応source一式はまだ含まれない。

ローカルで全235 archiveを取得し、size/hashと内部`.PKGINFO`のname/versionが固定recordに一致した。detached signatureも全235件を取得し、同じ隔離keyringでVALIDSIG/full trustとrevoked一覧との非該当を確認した。全署名のprimary fingerprintは上記database signerと一致する。[署名入力一覧](msys2-toolchain-signatures.json)にbytes/hash/fingerprintを残す。getterはこの検証済み署名fileのbyte固定を行うもので、GnuPGを実行し直すscriptではない。回帰試験は全470 cache fileの再利用、別cwd、offline欠落、同sizeで改変したarchiveと署名の拒否・保持を確認する。

## 短い専用環境への導入試験

2026-09-08、user profile以下の新規・短いASCII専用directoryへ検証済みSFXを展開した。Rust/MSVCや既存FFmpeg DLL、OS feature、global PATHは変更していない。通常login profileは実行せず、新環境内で`pacman-key --init`と`--populate msys2`だけを実行した。local master secret keyはその環境内だけに保持する。これは前節の公開鍵だけの監査keyringと異なる通常の初期化であり、上流pacman-keyがbundled keyのimportに用いる`--allow-weak-key-signatures`処理を含む。packageの`SigLevel`と`LocalFileSigLevel`は`Required`を維持する。

全235 archiveのinstall scriptlet/hook関連9 fileを確認した。変更先はroot内のXML catalog、証明書bundle、shell一覧、文書索引、専用keyringであり、Perl module確認も含む。Windows証明書storeを変更する処理ではない。通常の`-Syu` core更新には他のMSYS processを終了する処理があるため、別bootstrapのpacmanから明示`--root`/`--dbpath`/`--gpgdir`を指定し、repositoryを含まない設定とローカル`-U --needed`で導入した。57件は同版でskip、178件を導入・更新した。

このcross-root試験は無警告の自動setupとして採用しない。stdin target指定はterminal再open errorを出し、終了codeは1だったが、ALPM logにはtransaction completedが残り全235件の実導入を確認できた。またkey scriptletのprocess substitutionが`/dev/fd`で失敗し、Perl module testもfailした。新環境自身のnon-login shellでkey populateとPerl testを再実行し成功。XML catalogと両証明書bundleのpost_installも再実行し、文書索引を再生成した（indexを持たない文書・画像のwarningあり）。短いfilename引数をcache cwdから渡すnative `-Up`再検証は235件、exit 0であり、長い絶対pathのstdin渡しを再現手順にしない。導入scriptの自動化は未完了である。

```powershell
.\scripts\test-msys2-environment.ps1 -MsysRoot 'path/to/dedicated/msys64'
```

この検証scriptはlive repositoryを参照せず、全235 package名/versionの完全一致、`pacman -Dk`、`-Qk`による欠落なし、GCC/CMake/Ninja/NASMの起動、C DLLとC++ executableのcompile/link/実行を確認する。PATHは処理内だけに限定し復元する。ローカルで成功し、未更新bootstrapの拒否と別cwdでも検証した。これは初期toolchainの動作確認であり、package fileの全byte照合、MSVC ABI、全FFmpeg依存、最終配布buildの証明ではない。

## LCEVCのnative追加build

2026-09-08、既存recipeと同じLCEVCdec `a254bd474649e5dcd8182689ac414420bfe8d8c3`を上記GNU環境でbuildした。[既存source入力一覧](ffmpeg-build-inputs.json)のcache原本は4621388 bytes、SHA256 `3bc741c5076ee7c279a553181cac684e58cd245e27bb82d538f17a7a0807cb85`。全866 archive entryのpath/typeを検査し、新しい短いsource directoryへ展開した。全676 tracked fileをGit blobとraw byteで比較し、差分は下記の1 fileだけと確認した。原本recipeの末尾にある削除commandは実行しない。

上流のstatic用pkg-config設定では`-lstdc++ -lm`がLCEVC archiveより前に出る。C compilerと`pkg-config --static --cflags --libs lcevc_dec`で公開APIをリンクすると、C++ runtimeの未解決symbolで失敗した。[lcevc-static-link-order.patch](../third-party/ffmpeg/lcevc-static-link-order.patch)は`cmake/modules/CMakeInstall.cmake`の1行だけを変更し、その依存を末尾へ移す。原本SHA256 `d0ca6316fda02d425a18f63e23fe29190d25c4daaafc65de7083c3c45aad9161`、修正後`b93e16fa93bc816c59cf7babb5108ee7b7efdf7c5492a3ad15357790949c3354`。隔離fixtureで原本照合・patch実適用・結果hashを確認した。

検証済みGit sourceへ`git -c core.autocrlf=false apply --unidiff-zero --check`で確認してから同patchを適用する。以下のscriptはsource取得・patch適用やpackage更新をせず、固定HEADと変更file/hashを確認し、新規outputだけへbuildする。

```powershell
.\scripts\build-lcevc-native.ps1 -MsysRoot 'path/to/msys64' `
    -SourceDirectory 'path/to/patched/LCEVCdec' -BuildDirectory 'path/to/fresh/build'
```

Release/static、CPU pipeline有効、Vulkan・sample・test・trace・metricsは無効で、codecを削るminimum-size設定は使わない。既存recipeの`VN_SDK_PIPELINE_LEGACY`はこのsourceに存在しないため渡さず、`PC_LIBS_PRIVATE`も上流が再設定するため外から重複指定しない。compiler、Ninja、Git、Pythonを専用環境の実体へ明示する。初回探索ではPATHを限定してもWindows側Python 3.14.2が選ばれたため、再検証では固定packageの3.14.7を指定した。tag由来versionは取得できず、上流のproject version 4.2.0を使う。build日時埋込みは無効のままである。

新規directoryへの全97 build stepと別cwdからの再buildが成功し、8 static libraryを生成した。C callerの`LCEVC_CreateDecoder`→`LCEVC_InitializeDecoder`→`LCEVC_DestroyDecoder`も、修正後のpkg-config出力だけでリンク・実行できた。既存output拒否とPATH復元を確認済み。これはenhancement bitstreamのdecodeやFFmpeg全体のリンク・実再生の代替ではない。試験exeは`libstdc++-6.dll`と`libwinpthread-1.dll`もimportするため、最終FFmpegのruntime結合方法・同梱資料は別途確定する。

staged prefixにはCOPYINGとLICENSE.mdを原本byteのまま保持した。SHA256はそれぞれ`3afa5369b4fb44e18280b6e0e275971f78bc6eaf5f53553f41f2483fd8b1267e`と`14358b0ecf6e7036c211c10f0f25563c94483dee7b8c7c954e09e10f3771d0af`。[上流の追加情報](https://github.com/v-novaltd/LCEVCdec/blob/a254bd474649e5dcd8182689ac414420bfe8d8c3/README.md#notice)はBSD-3-Clause-Clearと特許ライセンスを含まない旨の保持を求める。build成功だけで配布条件を解決済みとせず、対応source・変更差分とともに配布gateへ引き継ぐ。

## LV2 / VAAPI packageの追加

2026-09-08、同じ署名済みmingw64/msys databaseと空のlocal databaseから、初期114 rootに`mingw-w64-x86_64-{lv2,lilv,libva}`を加えてpacmanで解決した。初期235 packageのhash/versionはすべて同じで、追加14件、圧縮合計3700292 bytesとなった。[msys2-media-inputs.json](msys2-media-inputs.json)にarchiveとdetached signatureのsize/hash、署名者を固定する。内訳はLV2、lilv、serd、sord、sratom、zix、libvaと、lilvのpackage依存に含まれるlibsndfile、libogg、FLAC、LAME、libvorbis、mpg123、Opusである。

全14件のarchive hash、`.PKGINFO`のname/versionとpathを検証し、専用環境のkeyringでdetached signatureのVALIDSIG/full trustを確認した。primary fingerprintは全件`5F944B027F7FE2091985AA2EFA11531AA0AA7F57`。追加package自身のinstall scriptlet/hookはなかった。getterは従来と同じbyte検証であり、毎回のGPG実行やpackage導入を含めない。

```powershell
.\scripts\get-msys2-toolchain.ps1 -IncludeMediaDependencies -Download
.\scripts\test-msys2-toolchain.ps1 -IncludeMediaDependencies
.\scripts\test-msys2-environment.ps1 -MsysRoot 'path/to/msys64' -IncludeMediaDependencies
```

ローカルでは新環境自身のpacmanにrepositoryなし・Required署名設定と14個の短いarchive filenameを渡し、`-U --needed`で正常導入した。初期base packageを更新する処理ではない。全249件のname/version一致、database整合性、file欠落なしを確認した。上記switchなしの取得testは従来の235件を引き続き検証する。media追加後の環境全体検証にはswitchが必要であり、余分なinstalled packageを無条件に許容する変更ではない。

環境testへLV2/lilv/VAAPIのC compile/link/runtime試験を追加した。pkg-config探索を専用prefixへ限定し、lilvのworld/URI作成と解放、`vaErrorStr`の呼出しを確認する。plugin scan/load、VA display作成、hardware初期化は行わない。lilv 0.26.4、LV2 1.18.10、libva package 2.24.1-1のheader/libraryで成功し、libvaのpkg-config/API versionは1.24.0（headerとも一致）だった。任意cwd、環境変数の復元、498 cache fileの再利用と既存の欠落・改変拒否testも通過した。

これでMABSに直接の準備処理がなかったLCEVC/LV2/VAAPIの個別準備は実証できたが、FFmpeg全体のconfigure/link・有効feature比較、実codec/filter動作、全対応source/noticeとruntime配布条件は未完了である。towavueへVAAPI再生経路やplugin機能を追加した意味ではなく、既存FFmpeg構成を維持するbuild入力として扱う。

## 全feature用package集合と最初のconfigure試験

2026-09-08、[ffmpeg-native-features.json](ffmpeg-native-features.json)へ元image configの全81 option（61 enable）を移した。`test-ffmpeg-native-features.ps1`は原本SHA256照合と全optionの完全一致、enableごとの準備経路と入力固定を検証する。原本がない環境では原本比較のskipを明示し、自己一致だけを原本照合成功とはしない。現時点の対応は52 package feature、5 source feature、4 built-in featureである。

同じdatabase snapshotから当初のpackage候補を解決し、既存249件のhash/versionを変えず72件、154812396 compressed bytesを追加取得した。全archiveのhashと`.PKGINFO`、全detached signatureのVALIDSIG/full trustを検証し、[media入力一覧](msys2-media-inputs.json)を計86件へ拡張した。`-IncludeMediaDependencies`のこの時点の取得・環境test対象はbase込み321件であり、前節の14件だけを追加するmodeではない。通常CIへこの大きなpackage取得は追加しない。

追加の導入処理5 fileはXML catalog、GIO module cache、GSettings schema、fontconfig cacheの更新だった。XDG cache/config/dataを専用build rootへ向け、repositoryなし・Required署名のlocal `-U --needed`で導入し正常終了した。FreeType/HarfbuzzとTIFF/WebPの依存cycle warningはあったが、導入後の全321件のname/version、database整合性・file存在を確認した。642 cache fileの再利用・欠落/改変拒否、既存native C/C++とLV2/VAAPI smokeも通過した。package依存には補助tool用も含まれるため、この集合全体をinstallerへcopyする方針ではない。

Chromaprint packageのstatic archiveには`fft_lib_kissfft.cpp.obj`、`kiss_fft.c.obj`、`kiss_fftr.c.obj`があり、未解決FFTW symbolはなかった。これは前のrecipe根拠を補う実入力の確認であり、最終FFmpeg linkとfingerprint比較を代替しない。OpenH264もMSYS2 package候補を取得したが、MABSのCisco配布DLLと同じもの・同じ条件と仮定せず、最終link入力と対応材料の確認を残す。

固定FFmpeg source archive（SHA256 `6491dae95e3cf3cdbac02933b55860e782b0c4f0a6bd8f37cef30fded259283c`）の全10548 entryを検査して専用rootへ展開した。元の81 optionすべてにnative mingw32/x86_64、pkg-config static指定と専用prefixだけを加え、non-login Bashから実configureを開始した。最初の停止理由は**aribb24 package 1.0.3-7が、GPL無効時の`aribb24 > 1.0.3`検査を満たさないこと**だった。configureはexit 1であり、全体build成功とはしない。後続依存の検査も完了していない。

このpackageは配布候補として採用せず、環境内の診断入力として記録する。feature対応を、既存recipeの固定aribb24 source `5e9be272f96e00f15a2f3c5f8ba7e124862aec38`へ変更した。元cacheは109104 bytes、SHA256 `a39f0c4cd4b28cbaecaa8a65d93667525875ffedffba7a6f9f30eeebd542ceda`。次にその版を別prefixへbuildし、pkg-configの実選択を確認してconfigureを再試行する。`--enable-gpl`の追加や`--disable-libaribb24`で回避しない。LCEVC以外のlibrist（mbedTLSを含む）、uavs3d、vvencのnative source buildもまだ必要である。

## aribb24のnative source build

2026-09-08、上記の固定sourceを119 archive entryのpath/type検査後に展開し、元recipeの`12.patch`、`13.patch`、`17.patch`を順に適用した。作業用recipe checkoutのCRLF patchでは最初の適用検査に失敗したため、SHA256 `2d6211a7e4becbb581bf64e2d1a67ff060fb413cd157d479959934ea23f3b2a4`のraw LF recipe archiveから取り直し、[入力一覧](ffmpeg-build-inputs.json)の各patch size/hashを照合した。原本のpatchを`git -c core.autocrlf=false apply --check`で検査してから実適用する。

固定sourceのCOPYINGはLGPLv3本文、public headerはLGPL 2.1-or-laterを記載する。元recipeのversion変更を[aribb24-version.patch](../third-party/ffmpeg/aribb24-version.patch)として保存し、`configure.ac`の表示を1.0.3から1.0.4へ変更した。これは既存GPL packageのversionだけを変えて採用する処理ではない。このpatchには`git apply --unidiff-zero --check`と実適用を使う。隔離fixtureで原本への実適用と結果hashを検証し、全24 tracked fileのraw Git blob比較で、差分がこのversion変更と元の3 patchに由来する6 fileだけであることも確認した。

```powershell
.\scripts\build-aribb24-native.ps1 -MsysRoot 'path/to/msys64' `
    -SourceDirectory 'path/to/patched/aribb24' -BuildDirectory 'path/to/fresh/build'
```

このscriptは取得・patch適用・package更新をせず、固定HEADと6 fileのhashを検査する。専用non-login Bashで`ACLOCAL_PATH=/mingw64/share/aclocal`を指定して`autoreconf -fi`、native x86-64 static build、別prefixへのinstallを行う。検索pathなしの最初のAutotools試行は`PKG_CHECK_MODULES`展開に失敗したが、指定後は成功した。上流のobsolete macro、Windows permission macro再定義、`strncpy`警告とlibtoolのstatic-only警告は残っており、警告なしのbuildとは扱わない。ARIB自体は明示的にstaticを指定している。

手動buildに加え、scriptによる2つの新規outputへのbuild・公開C APIのリンクと実行が成功した。`pkg-config`が新prefixを選び、FFmpegと同じ`aribb24 > 1.0.3`条件を満たすことを検査する。小さな入力`0e 41`（LS1/A）は期待するUTF-8 U+FF21へdecodeできた。任意cwd、環境変数復元、既存output拒否も確認済み。試験exeはWindows DLL以外に`libpng16-16.dll`をimportするため、これは依存すべてのstatic linkを意味しない。staged COPYINGとREADME.mdはsource原本のbyteを保持し、SHA256はそれぞれ`da7eabb7bafdf7d3ae5e9f223aa5bdc1eece45ac569dc21b3b037520b4464768`と`a9d5a0c8c8824d792cc57198f251c723b7ce69183efbc3a80de6384e0e60002c`である。

元81 optionをすべて維持し、ARIB新prefix→LCEVC prefix→MSYS2の順で全FFmpeg configureを再実行した。ARIB検査は通過し、次の停止理由は`librist >= 0.2.7 not found using pkg-config`となった。configure全体は未成功であり、librist/mbedTLS、uavs3d、vvencの準備を続ける。今回の1文字試験は実字幕stream・描画・FFmpeg全体・配布条件の検証を代替しない。

## libristのnative buildと実際の暗号依存

2026-09-08、元recipeの固定mbedTLS v4.2.0とlibrist source archiveをsize/hash検査して展開した。mbedTLSは5138 entryで、147のsymlinkはすべてarchive内部の通常file/directoryを指していた。MSYS2 tarで展開する際は`/c/...`表記を使い、Windows driveのcolonをremote host指定と解釈させない。libristは550の通常entryであり、展開後の全485 tracked blobがHEAD `4f45ef8f78983892d52ccd52d9f675435b23738f`とraw byteで一致した。

mbedTLS HEADは`ece41aa84d7879d7e55c59e955a5884b541f7f3b`、同梱submoduleはframework `dde0c4a0e448a0552f18817dcea633bb851fd288`、TF-PSA-Crypto `73c5da561c8e5253db7b1fb440eda86fde8d8024`、mldsa-native `5772b4f4a0105694b1203abb582273f78fa951b7`だった。native Release/static、program/test無効、`GEN_FILES=ON`でconfigureは通ったが、Pythonのjsonschema不足でcode generationが失敗した。

同じ署名済みdatabase snapshotへPython jsonschema/Jinjaの2 rootを加え、既存321 packageを変更せず8件、1508303 compressed bytesを追加した。各archiveのhash・`.PKGINFO`、detached signatureのVALIDSIG/full trustを照合し、追加scriptlet/hookがないことを確認した。全署名のprimaryは`5F944B027F7FE2091985AA2EFA11531AA0AA7F57`。repositoryなし・Required署名のlocal `-U --needed`で導入し、全329 packageのname/version・database整合性・欠落なしを検証した。現在の[media入力](msys2-media-inputs.json)は94件、`-IncludeMediaDependencies`はbase込み329件である。658 cache fileの再利用・欠落/改変拒否・native C/C++/LV2/VAAPI検証も成功した。追加8件はmbedTLS 4.2診断build用で、installer入力ではない。

追加後にmbedTLS 4.2の全library build/installは成功した。しかしlibristはその外部版のCMake target/public `mbedtls/aes.h`を利用できず、`builtin_mbedtls=false`を指定しても、既定fallbackによって自分のsourceに含まれるmbedTLS 3.6.6を使用した。cJSONも同梱版を使用した。**mbedTLS 4.2とリンクできたとは扱わない。** 最初のC callerもリンク自体は通ったが、`librist_version()`をpackage版番号と誤認した試験で失敗した。このAPIが返すのはVCS識別子であり、Windowsのfilemode判定による`-dirty`もcommand-local Git設定で解消した。

```powershell
.\scripts\build-librist-native.ps1 -MsysRoot 'path/to/msys64' `
    -SourceDirectory 'path/to/verified/librist' -BuildDirectory 'path/to/fresh/build'
```

候補手順は固定librist source同梱のmbedTLS 3.6.6とcJSONを**明示的に選択**し、その他のbuiltin fallbackとwrap downloadを無効にする。外部mbedTLS 4.2 prefixを探索に入れず、元recipeの`Requires: mbedcrypto`追記も流用しない。liblz4は固定MSYS2 packageを使う。source変更は不要で、`librist.a`には実際にmbedTLSのAES/MPI定義が含まれる。2つの新規outputへnative static buildし、pkg-config prefix/version検査、公開C APIによるMAIN profile受信contextの生成・破棄、任意cwd・環境復元・既存output拒否が成功した。peerやstreamは開始していない。上流Mesonのminimum-version警告とGCCのUDP変数警告は残る。

試験exeはWindows DLL以外に`liblz4.dll`と`libwinpthread-1.dll`をimportする。libristのCOPYING、`contrib/mbedtls/LICENSE`、cJSON source内noticeを保持する必要があり、最終runtime graph・対応source/notice収集・暗号化通信試験は未完了である。外部mbedTLS 4.2の診断成果物は候補link入力から除外する。

全81 FFmpeg optionの再configureはlibrist検査を通過し、次の`uavs3d >= 1.1.41 not found using pkg-config`でexit 1となった。残るuavs3d/vvencのnative buildと全体統合を続ける。アプリ本体へネットワーク機能を追加したものではなく、配布・launch完了を意味しない。

## uavs3d / VVenCと全configureの通過

2026-09-08、固定uavs3d `0e20d2c291853f196c68922a264bcd8471d75b68`とVVenC `0f2e874451d6b194615e5dfefdc96796a7da00f4`をnative buildした。[入力一覧](ffmpeg-build-inputs.json)のsize/hashを照合し、uavs3dの180 entry、VVenCの880 entryが通常file/directoryであることを確認して展開した。全112／790 tracked blobをraw比較し、uavs3dの下記header以外は原本と一致した。

uavs3dの初回configureはPowerShellで未引用の`3.5`が分割されて失敗した。また上流`version.sh`は出力先引数を受け取ってもGitの作業場所を変えず、呼出元towavueのcommitから誤った版情報を生成した。version付きCMake引数を文字列で渡し、configure/build/installをuavs3d source cwdから行うことで、正しい`1.2.89`と固定SHAへ復帰した。生成された`version.h`と`uavs3d.pc`はsource内のbuild出力であり、同じsourceから同時buildしない。

公開headerの`__cdecl`再定義により、C callerの`-Werror`検証が失敗した。[uavs3d-cdecl-guard.patch](../third-party/ffmpeg/uavs3d-cdecl-guard.patch)は既存定義がある場合の再定義だけを防ぐ。`git apply --unidiff-zero --check`から実適用し、隔離fixtureで結果SHA256 `c4193da1a00d43cb41eb9e7f61c5782545e8f76ece72477edd52310f30cb6192`を検証した。修正後のnative buildと厳格なC caller compile/linkが成功した。

```powershell
.\scripts\build-video-codec-native.ps1 -Codec uavs3d -MsysRoot 'path/to/msys64' `
    -SourceDirectory 'path/to/patched/uavs3d' -BuildDirectory 'path/to/fresh/avs3-build'
.\scripts\build-video-codec-native.ps1 -Codec vvenc -MsysRoot 'path/to/msys64' `
    -SourceDirectory 'path/to/verified/vvenc' -BuildDirectory 'path/to/fresh/vvc-build'
```

scriptは固定HEAD・許可した差分だけを検証し、取得やpatch適用をしない。uavs3dは元recipeと同じ10-bit/static、VVenCはRelease/static/library-only、SIMD有効、LTO無効、元と同じC++ runtime link指定を維持する。VVenCの初回手動buildは上流既定のsource内出力とccacheを使用したが、scriptでは出力を新規build root内に閉じ、ccacheを無効にした。Gitのautocrlf/filemodeもprocess内だけで設定して偽のdirty判定を避け、環境を復元する。CMake互換性警告とVVenCのlibrary-only時の上流test無効警告は残る。

両codecとも2つの新規outputへbuildし、任意cwd、環境復元、既存output拒否、選択prefixとFFmpeg版条件を確認した。uavs3dはdecoderの生成・破棄と10-bit build表示までで、AVS3 bitstreamは未decodeである。VVenC `1.15.0-dev`は公開C APIから128×128の1 frameをencodeし、flush完了・1 access unit／291 bytesを確認した。これは独立decoderによるVVC復号・画質・速度の検証ではない。uavs3d試験exeはWindows DLLのみ、VVenC試験exeは加えて`libstdc++-6.dll`と`libwinpthread-1.dll`をimportする。原本COPYING／LICENSE.txt／AUTHORS.mdと対応sourceは最終配布材料へ引き継ぐ。

全81 FFmpeg optionによるconfigureがexit 0で完了し、生成config.hでも全61 enableに対応するCONFIG/HAVE macroが1であることを確認した。configureはLGPLv3-or-laterと表示するが、これは最終link入力や配布条件の承認ではない。**`spirv-headers not found, swscale SPIR-V backend unavailable`警告が残る**ため、auto-detected機能まで同等とは扱わない。

初回の手動codec prefixを使った全FFmpegコンパイルもexit 0で完了し、7 versioned DLL／import libraryとffmpeg・ffprobe・ffplayを生成した。build内DLL directoryと専用MSYS2 binをprocess PATHへ指定すると、ffmpeg／ffprobeが9.0.1として起動し、合成3 framesを処理できた。さらにFFmpegからlibvvencで128×128／10-bitの1 frameをencodeし、ffprobeの復号frame数1・VVC／yuv420p10leと、FFmpeg側のVVC decoderによる出力完了を確認した。codec単体のC試験より一段進んだ統合証拠だが、アプリのMSVCリンク・隔離runtime・実再生／性能・配布材料の検証はまだである。現行アプリのFFmpegは置き換えていない。SPIR-V入力の補完、修正済みcodec prefixへの統一、実binaryの機能比較と依存closureの確定を続ける。

## SPIR-V補完、リンク順の修正とruntime隔離試験

2026-09-08、同じ署名済みsnapshotから`mingw-w64-x86_64-spirv-headers 2~1.4.357.0-1`だけを追加した。193255 bytes、SHA256 `afe7a50e11fc56d8c59666dcf195d40fb439c11e38f96a2fa1acfe62fc24e073`。archive／`.PKGINFO`／detached signatureのfull trust、追加hookなしを確認してlocal `-U --needed`で導入した。既存329 packageは変更せず、現在の[media入力](msys2-media-inputs.json)は95件、全体330件である。660 cache fileの検証、環境整合性、native C/C++/LV2/VAAPI試験も成功した。再configureではSPIR-V header macroが有効になり、不足警告は解消した。

**前節の最初のnative FFmpeg生成物と、ヘッダーだけ補完した次の生成物は配布候補から除外する。** 実PE importを辿ると、前者は新しいaribb24 prefixをpkg-configで選んでも、旧MSYS2 `libaribb24-0.dll`へリンクしていた。全体の`-L`列では他library由来のMSYS2 pathが先行していたためである。pkg-configの版検査とFFmpegの自己申告licenseだけでは、この選択違いを検出できなかった。成果物は診断用に保持し、現行アプリのDLLは変更していない。

修正はFFmpeg sourceの変更ではなく、全81 optionを維持したうえで`--extra-cflags`と`--extra-ldflags`へ5つの専用prefixを先行指定することとした。順序はaribb24、LCEVC、librist、uavs3d、VVenCで、前者へ各`-I<prefix>/include`、後者へ各`-L<prefix>/lib`を渡す。全prefixは検証済みscriptの出力を使用し、同じ順序のpkg-config pathに続けてMSYS2 prefixを置く。これらは各1つの文字列引数としてconfigureへ渡す。生成CFLAGS/LDFLAGSで実際の順序を確認し、新規buildでconfigure／makeがexit 0となった。

新avcodec DLLには`arib_instance_new`と`arib_decode_buffer`の定義が存在し、`libaribb24-0.dll`のimportはなくなった。全runtime graphにも旧ARIB DLLは含まれない。全61 enable macroとSPIR-V headerが有効、GPL/nonfree flagは無効である。これは他の全リンク入力や配布条件の承認を意味しない。

```powershell
.\scripts\get-native-runtime-dependencies.ps1 -EntryPoints 'path/to/ffmpeg.exe','path/to/ffprobe.exe' `
    -SearchDirectories 'path/to/ffmpeg/dlls','path/to/msys64/mingw64/bin' `
    -ObjdumpExecutable 'path/to/msys64/mingw64/bin/objdump.exe'
.\scripts\test-native-runtime-dependencies.ps1 -MsysRoot 'path/to/msys64'
```

監査scriptはPE importを再帰的に読み、file size/hashと依存edgeを返すだけで、copyや実行はしない。実buildでは7 library directoryを検索対象へ渡す。Windows KnownDLLs、API set、その他のhost-system fileを区別し、通常DLLは指定directory内の一意な候補を要求する。System32に存在するだけでWindows標準と扱わない。実際にOpenCL.dllはSystem32にもあるKhronos loaderであり、隔離試験では指定MSYS2版を同梱した。KnownDLL名のshadow、通常DLLの重複・欠落、任意cwdとhashを既存C/C++ fixtureで検証した。動的LoadLibraryやdriver/plugin探索、host-system fileの対象OSでの存在は別検証である。

修正前graphは95 files／143833446 bytes、新graphは94 files／143954529 bytesだった。新graphは2 helper、7 FFmpeg DLL、72 package由来の85 DLLで、`ffplay`はこの集約対象に含めない。新規隔離directoryへ94 filesをcopyして全hashを照合し、別の空cwd・`PATH=Windows System32のみ`で合成3 framesの処理と、既存VVC fixtureの復号frame数1を確認した。開発toolchain pathは不要だったが、これは現在のhost上の試験であり、clean Windows 10／installer試験ではない。単なるimport graphは全対応source/notice一覧の代わりにはならない。

旧開発FFmpegと新隔離helperの実一覧を比較し、decoder 537、encoder 228、filter 531、demuxer 364、muxer 184、protocol 44、hardware API名9がすべて一致した。各集合で追加・欠落とも0である。hardware API名の列挙は実device動作を意味しない。次はこの候補の再現可能な構成手順、Chromaprint fingerprint／backend、全材料、MSVCアプリ統合・実再生／性能を検証し、Setup.exeへ進める。

## MSVCアプリ統合とChromaprint比較

2026-09-08、上記のリンク優先順位を修正したbuildから、新規prefixへ`make install`がexit 0で完了した。標準installはMSVC用`.lib`を`bin`へ置くが、固定`ffmpeg-sys-next 9.0.0`の`FFMPEG_DIR`経路は`lib`を探索する。このため7個のimport libraryを同prefixの`lib`へcopyし、元fileとSHA256一致を確認した。sourceやimport libraryの再生成・本体の依存変更は不要だった。

installはFFmpeg DLLをstripするため、前節のbuild内fileのhashをそのまま使わず、install後のffmpeg／ffprobeから再びPE依存graphを取得した。94 files／137343426 bytesで、旧ARIB DLLを含まない。外部85 package DLLだけを新prefixの`bin`へcopyし、各hashを確認した。開発用prefixにはheaders、import libraries、ffplayやexamplesもあり、このdirectory全体を配布対象とする決定ではない。

新prefixの`FFMPEG_DIR`と`bin` PATH、既存LLVMをprocess内で指定し、別の`CARGO_TARGET_DIR`からMSVCアプリをcompile/linkした。format、workspace全targetのClippy、268 tests（app 152／core 36／runtime 76／integration 4）が成功した。3件のlive testは未実行。hardware-preferred exportを`--nocapture`で再実行すると、Media Foundation H.264 hardware encoderを利用できずhardware assertionは明示skipだった。software fallbackと出力のdecode成功をhardware encode成功とは扱わない。

検証用debugアプリのSHA256は`10197c121d4b45b10b3d7b6bedf939b37c03275511e90ffc06b3ed937d226d1a`。別cwd、PATHを新prefixのbinとSystem32だけに限定してH.264/AACの2秒fixtureを開いた。実processの6個のFFmpeg module pathがすべて新prefixを指すことを照合し、通常close後のdiagnosticはD3D11VA、hardware frames 60、CPU transfers 0、presented 60、dropped 0だった。A/V drift p95は3.401 ms、最大3.427 msだが、短いdebug試験の観測値でありperformance gateの代わりにはならない。AACのtimestamp警告は残る。画面の目視・音声の聴取・長時間再生・release版の比較はこの試験に含めない。元の開発DLLとrelease exeは変更していない。

```powershell
.\scripts\test-ffmpeg-chromaprint.ps1 -ReferenceExecutable 'path/to/reference/bin/ffmpeg.exe' `
    -CandidateExecutable 'path/to/candidate/bin/ffmpeg.exe'
```

比較scriptは旧helperで20秒の440 Hz音、200 Hzからのlinear chirp、seed 1202のwhite noise、無音をPCM化し、その同一byte列を両helperへ渡す。11025 Hz mono／44100 Hz stereoの8条件で、algorithm 1のraw fingerprint各140 wordsが完全一致した。PATHはSystem32だけにし、各helperに隣接するDLLを使う。任意cwdでの再実行、PATH復元、同一exe指定の拒否も確認した。fixtureは毎回新規のignored directoryへ保持する。以前の単体API試験をこの結果で置き換えず、native FFmpeg経由の追加証拠とする。全入力での同値性、backendの実link由来、速度、対応source/noticeの完全性は別gateである。

次は正しい全体build／staging手順の再現、packageの実link入力・対応source/noticeの確定とrelease版の媒体／性能検証を進める。helperの隣接探索はまだ実装しておらず、今回のアプリ試験も`FFMPEG_DIR`を指定している。Setup.exeや設定不要のinstall動作が確認できたとは扱わない。

## Native Chromaprintの対応資料とpackage横断確認

2026-09-08、実avformatのimportに`libchromaprint.dll`と9個のChromaprint APIを確認した。candidate DLLは109083 bytes／SHA256 `c4b8563883605a106c09b060040b27d265be66ce4ca85287448fee62a0fd6163`で、固定した署名済みMSYS2 packageの展開物と一致する。DLLの通常importはGCC/C++ runtimeとWindows libraryで、FFTW DLLはない。ただしimport一覧だけで静的backend不在を証明するものではない。

package内`.BUILDINFO`の`pkgbuild_sha256sum`は`434eb5b783c1ce5f83352a7bf69853d53ba751d12f63a4417be8b0dd3ca39bd7`で、[固定PKGBUILD](https://raw.githubusercontent.com/msys2/MINGW-packages/052099e63e69816e35b05f28c852a5209c4dd1e0/mingw-w64-chromaprint/PKGBUILD)の実bytesと一致した。recipeはstatic/sharedともKissFFTを明示する。指定されたChromaprint 1.6.1 release sourceは1579624 bytes／SHA256 `3368805af0ee47b9df74df10b5001a44569e01df2844dab520031720dde9ad23`で、取得物と一致した。上流packageのGCCは16.1.0-5であり、今回FFmpegをbuildした16.2.0-3と同じとは記録しない。元の全installed一覧を`.BUILDINFO`ごと保持する。これは署名済みpackageからrecipe/sourceへ辿る証拠であり、同一DLLを再生成した証拠ではない。

source全481 entryを検査した。最初の通常file限定検査は内部header symlinkで停止し、展開しなかった。唯一の`src/include/chromaprint.h -> ../chromaprint.h`が実在する内部headerを指すことを確認し、そのaliasを除いて診断用に展開した。資料作成scriptはこの展開treeを信用せず、hash固定archiveから必要な通常fileだけを選択抽出する。

```powershell
.\scripts\prepare-chromaprint-materials.ps1 `
    -PackageArchive 'path/to/mingw-w64-x86_64-chromaprint-1.6.1-1-any.pkg.tar.zst' `
    -Recipe 'path/to/PKGBUILD' -SourceArchive 'path/to/chromaprint-1.6.1.tar.gz' `
    -RuntimeDll 'path/to/candidate/libchromaprint.dll' `
    -LgplLicense 'path/to/ffmpeg/COPYING.LGPLv2.1' -OutputDirectory 'path/to/fresh/materials'
```

同じ5入力を`test-chromaprint-materials.ps1`へ渡すと回帰検証できる。[入力一覧](native-chromaprint-inputs.json)とmedia package一覧を照合し、入力全部を検査してから新規outputだけに書く。source archive原本、recipe、packageの2 metadata、6 source notice file、LGPL 2.1全文、入力一覧と説明の13 filesを作る。実出力は1641422 bytesで、binaryは含まない。ChromaprintのLICENSE.mdはLGPL本文へのlinkのみなので、固定FFmpeg source内の全文も保持する。KissFFTのCOPYINGだけでなく参照先BSD本文、内蔵resamplerの個別著作権表示を含めた。

最初の任意cwd検査で、PowerShell locationとprocess cwdが異なる場合の相対output解決ミスを検出した。providerのpath解決に修正し、誤配置した自作fixtureはignored診断directoryへ移動した。修正後、2つの新規outputの全file hash一致、既存output拒否、5入力それぞれの欠落と同size改変の拒否、拒否時の入力・出力保持が成功した。取得・再build・公開はscriptに含めない。全FFmpeg／GCC runtimeの対応資料をこれだけで充足した扱いにはしない。

同じruntime graphの85 package DLLを72 ownerへ逆引きし、全ownerの固定archive hash、`.PKGINFO`とfile一覧も読んだ。15 packageには`share/licenses`配下の通常fileがない。別の場所やsource内にnoticeがある可能性は残るため、全体収集ではChromaprint以外も個別照合する。複数licenseを持つpackageはlibrary／tool／documentationの範囲を区別する。

**次にOpenALの範囲を優先確認する。** avdeviceは`libopenal-1.dll`をimportし、実package 1.25.2-1と[固定recipe](https://raw.githubusercontent.com/msys2/MINGW-packages/052099e63e69816e35b05f28c852a5209c4dd1e0/mingw-w64-openal/PKGBUILD)のmetadataはGPL-2.0-or-laterだが、[同版上流COPYING](https://raw.githubusercontent.com/kcat/openal-soft/1.25.2/COPYING)はGNU Library GPL v2である。この差を誤記ともGPL DLLの確定とも推測しない。対応recipe hash、source、4 patches、実libraryに入るsourceと通知を照合してから採否を判断する。現候補をinstallerへ採用・公開する承認はまだ行わない。

## OpenALのlibrary/tool分離と埋め込みHRTF

2026-09-08、前節のOpenAL package表示とlibrary本文の差をsource levelで確認した。candidate `libopenal-1.dll`は2728444 bytes／SHA256 `cb97d2f5a9797ff64db90d0691e4ff9eb2a088026351c9bad7aa26d229a354ce`で、固定packageの展開物と一致する。実avdeviceはこのDLLをimportする。packageには`makemhr.exe`と`openal-info.exe`も入るが、94-file runtime graphには含まれない。OpenALをtowavueのWASAPI出力の代替にした変更ではない。

当初取得したMSYS2調査revisionのPKGBUILDは、package内`.BUILDINFO`のhashと一致しなかった。履歴から[3da1e4e9の原本](https://raw.githubusercontent.com/msys2/MINGW-packages/3da1e4e9f763ce05ebf2c0e244dcf3e66f44bb66/mingw-w64-openal/PKGBUILD)を取得すると、3353 bytes／SHA256 `18af836318e047249589a2df8af950a792f5512bc21923b9470f5902abf6414a`で一致した。差は後の`mingw32`削除だけだが、対応資料にはhashが一致する古い原本を採用する。source 1407972 bytes／SHA256 `fb27e5839aa11f0e5b9d33756965291fad5d6909ab928ea1f796f4a1a6877894`と4 patchも、そのrecipeのchecksumと一致した。

sourceの518 entryはすべて通常file/directoryで、全461 fileを展開後に原本と比較した。最初の`git apply --check`は古いfilename patchのcontext不一致で停止したが、実recipeと同じ`patch -Np1`ではfuzz 1／offset 373で適用できた。結果が変わったのはCMakeLists.txt、cmake/FindMySOFA.cmake、openal.pc.inの3 fileだけである。結果hashはそれぞれ`8130c55255d9757237e0daffeb3fa97d8d134f08c9f8b48e4a82ff89fe240b87`、`76c7e252be3da5050bccba9b5c5fe4e0cd93e4f3ee0f12104cef566ea4cc7956`、`3024b239da0558a0ea46c51f86b26a1f5a6d8761b0ca6e6eeb12ff209b6e60aa`。原本patchを保持し、ライセンス表示・library/tool構成の変更はない。

[library source](https://raw.githubusercontent.com/kcat/openal-soft/1.25.2/al/buffer.cpp)はGNU Library GPL v2以降を明示する。一方、GPLの[SOFA support](https://raw.githubusercontent.com/kcat/openal-soft/1.25.2/utils/sofa-support.cpp)とmakemhr sourceはCMakeのutility targetへ分離され、OpenAL library targetのsource/link入力ではない。DLLの通常importにもlibmysofaはない。したがってpackage全体のGPL metadataをDLL本体のGPL-only判定へ直結させず、DLLのLGPL表示と、含めないGPL utilityの範囲を区別する。packageの原metadataは書き換えない。完全な実build traceやbit同一の再buildをこのsource監査だけで証明した扱いにはしない。

さらに、sourceの`hrtf/Default HRTF.mhr`全159841 bytes／SHA256 `0b2f09f4d9167dec4e977e7e1a8c78df17e87da06fb0871b9896fadaa473a440`が、実DLLのbyte offset 1775392にそのまま埋め込まれていることを確認した。別data fileを配布しなくても、この入力は消えない。OpenAL docs/hrtf.txtが指す[MIT KEMAR提供元](https://sound.media.mit.edu/resources/KEMAR.html)は、研究・商用利用で著者表示を求めている。Bill Gardner／Keith Martinと1994 MIT Media Laboratoryの著作権表示、由来URLを資料READMEへ追加した。これは一般の「MIT license」を選んだという意味ではない。

```powershell
.\scripts\prepare-native-package-materials.ps1 -Component openal `
    -PackageArchive 'path/to/mingw-w64-x86_64-openal-1.25.2-1-any.pkg.tar.zst' `
    -Recipe 'path/to/exact/PKGBUILD' -SourceArchive 'path/to/openal-1.25.2.tar.gz' `
    -RuntimeDll 'path/to/candidate/libopenal-1.dll' -PatchDirectory 'path/to/four/patches' `
    -GplLicense 'path/to/ffmpeg/COPYING.GPLv2' -OutputDirectory 'path/to/fresh/materials'
```

同じ入力を`test-native-package-materials.ps1 -Component openal`へ渡すと検証できる。Chromaprintで導入した処理をこの2 componentで共用し、従来のChromaprint commandはwrapperで維持した。[OpenAL入力一覧](native-openal-inputs.json)は原本・実DLL・patch・noticeを固定する。最終OpenAL資料は18 files／1550136 bytesで、source archive、recipe、4 patches、2 package metadata、7 source notice、GPL全文、入力一覧・説明を含む。COPYINGのLibrary GPL本文、PFFFT・fmt・GSL、library著作権表示とHRTF由来を保持する。原本archiveに残すGPL utility sourceのためGPL本文も補うが、utility binaryは含めない。

初期17-file資料はHRTF表示補完前の診断用として保持し、`native-openal-materials-v2`を今回の検証結果とする。OpenALの5通常入力と4 patchesそれぞれの欠落・同size改変拒否、任意cwdでの全file一致、既存出力保護が通った。Chromaprintの5入力試験も従来commandから成功した。生成scriptは取得・patch適用・build・binary copy・公開をしない。残る混合license packageや表示不足packageのsource対応、全体build再現・runtime/performance／Setup.exeのgateは未完了である。

## 設定値の対応

[固定batch](https://github.com/m-ab-s/media-autobuild_suite/blob/02eab87287e2df528f5c48512677684c323cacd0/media-autobuild_suite.bat)で確認したINI値。全optionを網羅したINIではないため、この表だけを貼り付けて無人実行しない。未指定値は再質問・INI再生成の対象になる。

| 意図 | INI値 | 注意 |
|---|---|---|
| x64のみ | `arch=3` | 32-bit環境を作らない |
| LGPLv3 | `license2=4` | batch内部では`lgplv3`。数値3はGPLであり誤用しない |
| FFmpeg shared | `ffmpegB2=3` | `ffmpegChoice`ではない。内部は`shared` |
| 明示option file | `ffmpegChoice=1` | `build/ffmpeg_options.txt`を用意し、Full既定値を流用しない |
| GCC | `CC=2` | Rust本体のMSVC toolchainは変更しない |
| source保持 | `deleteSource=2` | 取得archive・Git・patch・実効lockも別途保存 |
| suite更新scriptを作らない | `updateSuite=2` | MSYS2 package更新を止める設定ではない |
| ログ保持 | `logging=1` | 成功時のconfigure前後とlink入力も保持する |
| UPX packなし | `pack=2` | 初回の検証では圧縮変換を加えない |
| FFmpeg source固定 | `ffmpegPath=https://github.com/FFmpeg/FFmpeg.git#commit=e47273f4d9227152dcbf543cebaf9e2430ddbcc4` | helperのGit ref処理を使う。実HEAD照合を別途必須にする |

初期setupは固定されていない`nightly-x86_64/...latest.sfx.exe`を取得・実行する。さらに`updated.log`がない場合はpackage更新が走る。`updateSuite=2`や大きな`pkgUpdateTime`だけで再現性を確保した扱いにしない。短い空白なしbuild pathを使い、通常のcmd環境から起動する設計であり、MSVC Developer Shellからの起動は上流が拒否する。

## 既存構成との差と実行前の作業

[固定compile script](https://github.com/m-ab-s/media-autobuild_suite/blob/02eab87287e2df528f5c48512677684c323cacd0/build/media-suite_compile.sh)とhelperを確認した。単純な文字列検索はbrace展開を見落とすため、未一致を非対応と断定しない。例えばAMRは`libopencore-amr{wb,nb}`で処理される。

| 項目 | 確認した差 | 必要な対応 |
|---|---|---|
| Chromaprint | MSYS2 packageを使い、FFTW packageを除く。固定MSYS2 recipeはKissFFT | 実packageのhash/sourceとリンク入力を照合し、既存fingerprint試験を再実行 |
| OpenH264 | source静的libraryから、Cisco 2.6.0の`libopenh264-7.dll`へ変わる | API/import libraryとの整合性、追加DLL探索、配布条件を確認。未承認のままinstallerへ入れない |
| OpenAPV | 既定option一覧にはないが、`enabled liboapv`のbuild処理は存在する | 明示optionと`SOURCE_REPO_OPENAPV`の固定を維持 |
| LCEVC decoder | MABS build scriptと調査したMSYS2 package treeに該当項目を確認できない | 固定sourceのnative static buildと公開API試験は上記の1行修正で成功。FFmpegへの統合・実decodeを検証し、黙って無効化しない |
| LV2 / VAAPI | MABSに直接の準備処理は確認できない | 上記14 packageの固定・導入とC link/runtime試験は成功。FFmpegへの統合を検証し、VAAPIをtowavueのD3D11VA代替にはしない |
| CUDA LLVM | clang package導入失敗時、helperがoptionを除去する | 構成差を失敗として検出し、静かに機能が減ったbuildを採用しない |
| feature / license調整 | helperが選択license等に応じてoptionを書き換える | 入力fileだけでなく、最終configure・config.h・実binaryの一覧を比較 |

OpenH264 x64圧縮DLLに上流scriptが指定するSHA256は`dab5f2a872777f9a58b69bfa9fbcf20d9f82f2d6ec91383fd70bff49bd34ac9f`。これは取得物を今回検証したという意味ではない。

この調査から、MABSはWSL不要の有力候補だが、Full/LGPL/sharedの選択だけで旧buildと同等になるとは判断しない。compilerとLCEVC/LV2/VAAPIの個別準備に続き、次は残るmedia依存を固定して既存の全feature optionへ統合する。隔離buildで7 DLLと両helperを生成し、MSVC Rustとのリンク・Windows実行・既存機能を確認する。配布可否とlaunch完了は別gateである。
