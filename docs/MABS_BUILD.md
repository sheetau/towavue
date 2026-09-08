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

ローカルでは、archive全16581 entryが`msys64/`以下で、通常file/directoryだけであることを確認した。Windows tarは一覧を読めるが、BOING.WAVの7z filterを扱えず展開が失敗した。`vendor/msys2/base-20260611`は不完全な診断用展開であり使用しない。検証済みSFXの抽出だけを別の新規`vendor/msys2/base-sfx-20260611`へ実行し成功した。local package databaseの全90件の名前/versionは公式一覧と一致する。login shell、post-install、pacman、MABSはまだ実行していない。この長いpathは調査用であり、実buildには短いASCII pathを別途用意する。

これはbase環境だけの固定である。追加のcompiler/FFmpeg依存packageや更新後の実効graph、全対応sourceの固定が済んだという意味ではない。

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
| LCEVC decoder | MABS build scriptと調査したMSYS2 package treeに該当項目を確認できない | 既存の固定source/recipeを使うWindows向け追加buildを検証。黙って無効化しない |
| LV2 / VAAPI | MABSに直接の準備処理は確認できないが、固定MSYS2 treeにLV2/lilv/serd/sord/sratomとlibva recipeは存在する | 必要packageと推移依存を固定・明示追加する方法を検証。VAAPIをtowavueのD3D11VA代替にはしない |
| CUDA LLVM | clang package導入失敗時、helperがoptionを除去する | 構成差を失敗として検出し、静かに機能が減ったbuildを採用しない |
| feature / license調整 | helperが選択license等に応じてoptionを書き換える | 入力fileだけでなく、最終configure・config.h・実binaryの一覧を比較 |

OpenH264 x64圧縮DLLに上流scriptが指定するSHA256は`dab5f2a872777f9a58b69bfa9fbcf20d9f82f2d6ec91383fd70bff49bd34ac9f`。これは取得物を今回検証したという意味ではない。

この調査から、MABSはWSL不要の有力候補だが、Full/LGPL/sharedの選択だけで旧buildと同等になるとは判断しない。次は固定MSYS2 bootstrap/package集合と上記3つの直接準備がない依存（LCEVC/LV2/VAAPI）の準備方法を確定する。その後、隔離buildで7 DLLと両helperを生成し、MSVC Rustとのリンク・Windows実行・既存機能を確認する。配布可否とlaunch完了は別gateである。
