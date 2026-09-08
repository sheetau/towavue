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
| LV2 / VAAPI | MABSに直接の準備処理は確認できないが、固定MSYS2 treeにLV2/lilv/serd/sord/sratomとlibva recipeは存在する | 必要packageと推移依存を固定・明示追加する方法を検証。VAAPIをtowavueのD3D11VA代替にはしない |
| CUDA LLVM | clang package導入失敗時、helperがoptionを除去する | 構成差を失敗として検出し、静かに機能が減ったbuildを採用しない |
| feature / license調整 | helperが選択license等に応じてoptionを書き換える | 入力fileだけでなく、最終configure・config.h・実binaryの一覧を比較 |

OpenH264 x64圧縮DLLに上流scriptが指定するSHA256は`dab5f2a872777f9a58b69bfa9fbcf20d9f82f2d6ec91383fd70bff49bd34ac9f`。これは取得物を今回検証したという意味ではない。

この調査から、MABSはWSL不要の有力候補だが、Full/LGPL/sharedの選択だけで旧buildと同等になるとは判断しない。初期compilerの導入・動作確認に続き、次はmedia package集合と上記3つの直接準備がない依存（LCEVC/LV2/VAAPI）の準備方法を確定する。その後、隔離buildで7 DLLと両helperを生成し、MSVC Rustとのリンク・Windows実行・既存機能を確認する。配布可否とlaunch完了は別gateである。
