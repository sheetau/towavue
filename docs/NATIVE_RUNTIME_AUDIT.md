# Native runtime package audit

2026-09-08の修正済みnative FFmpeg候補について、実helperのPE importからpackage由来を照合した記録。配布承認・全対応sourceの完成を意味しない。build経緯は[MABS_BUILD.md](MABS_BUILD.md)を参照する。

## 今回固定できた範囲

| 対象 | 実確認 |
|---|---|
| helperとPE依存closure | ffmpeg／ffprobe、7 FFmpeg DLL、85 package DLLの計94 files。name／size／SHA256／import edgeを固定 |
| package由来 | 85 DLLを72 ownerへ逆引きし、固定archive内の実fileと全size／hashが一致 |
| package入力 | 72 archive、計67180054 bytes。既存input manifestのhashを検査してから選択抽出 |
| 元build記録 | 各packageの`.PKGINFO`と`.BUILDINFO`を保持。name／versionを照合し、元recipe SHA256を取得 |
| package収録notice | `mingw64/share/licenses`内の80通常fileを保持し、個別size／hashを固定 |
| 対応PKGBUILD | 72件すべてでbuild記録と原本hashが一致。固定URL・revision・size・hashを別一覧へ保存 |

[native-runtime-package-audit.json](native-runtime-package-audit.json)は実closureとpackage metadataのbaseline、[native-runtime-recipes.json](native-runtime-recipes.json)は対応recipeの入力一覧である。どちらもmachine固有の絶対pathを含めない。Windows KnownDLL／API set／その他host-systemの区別は既存PE監査と同じであり、host-system fileのWindows 10上での存在保証ではない。

## 再検証

```powershell
.\scripts\prepare-native-runtime-audit.ps1 -RuntimeDirectory 'path/to/candidate/bin' `
    -MsysRoot 'path/to/msys64' -OutputDirectory 'path/to/fresh/audit'
.\scripts\test-native-runtime-audit.ps1 -RuntimeDirectory 'path/to/candidate/bin' `
    -MsysRoot 'path/to/msys64'
.\scripts\get-native-runtime-recipes.ps1 -Download
.\scripts\test-native-runtime-recipes.ps1
```

監査は現在の固定FFmpeg major/DLL名を対象とする。既存outputは拒否し、全package archiveのpreflight後に通常memberだけを選択抽出する。監査directoryは310 files（85 DLLの検査用copy、144 metadata、80 notices、INDEX.json）を含む。**このdirectoryをそのまま配布しない。** 元DLLやpackage cacheを変更せず、PATHはprocess内で復元する。署名のcryptographic検証を再実行するscriptではなく、以前検証した署名付きinput manifestのarchive hashを用いる。

testでは任意cwdから再生成し、94 runtime／72 packageの全JSONがbaselineと一致、80 notice hashが一致することを確認した。既存output、空cache、同sizeで改変したpackageを拒否する。さらにDLLのCOFF timestamp byteだけを変え、正常にPE importを解析できても元packageとのhash不一致で拒否する。archiveの失敗はoutputを作らず、DLL不一致では診断用の部分出力を保持するがINDEXを作らない。各失敗で入力・先行INDEX・PATHを保持することを検証した。

## Recipeの特定

MSYS2調査revision `052099e63e69816e35b05f28c852a5209c4dd1e0`で43件が一致し、29件は一致しなかった。packageのbuilddate近傍の履歴から、同じ`.BUILDINFO` hashになる原本を特定した。Crypto++だけ初回のqueryで`+`がspaceとして解釈され候補0件となったため、pathをURL encodeして再照合した。履歴探索の「見つからない」をsource不存在と判断していない。

getterは探索を再実行せず、特定済みの72 URL/hashだけを利用する。`-Download`なしはoffline検査で、欠落cacheを作らない。既存の同size改変recipeも上書きせず、後続downloadを始める前に拒否する。実際に空の検証cacheへ72件すべて取得・照合し、既存cacheのtimestamp保持、任意cwd、欠落・改変拒否のtestも通った。recipeをsourceしたりbuildを実行したりはしない。

## 残る材料と判断

15 packageにはこのlicense directory内の通常fileがない。Chromaprint／OpenALは別途sourceから補完済み。残り13件のGMP、LAME、libass、libssh、libtheora、libvorbis、libvpx、LZ4、opencore-amr、Snappy、TwoLAME、ZeroMQ、zimgについても、元recipeが指定するsource archiveとpatch/templateを取得・照合した。directoryが空というだけでlicense表示が不要とは扱わない。

### 13件のsource補完

[native-source-supplements.json](native-source-supplements.json)に13 source archiveと20 patch/template、計20909402 bytesを固定した。全33 inputのSHA256が対応PKGBUILDと一致する。source署名の検証ではなく、GMP/libssh recipeでSKIP指定の署名fileは含めない。取得したsourceの全member名とtypeを監査し、zimgの7個だけ存在する内部symlinkは展開しない。

47件の選択文書（license・authors・NOTICE・patent文書・GMP/LZ4の範囲確認用header/build記述）を原本byteのまま抽出する。全source、全patch/template、13 recipes、参照inventory、READMEと合わせて97 filesとなる。DLL/exeはcopyせず、recipe実行・patch適用・buildも行わない。

```powershell
.\scripts\get-native-runtime-recipes.ps1 -Download
.\scripts\prepare-native-source-supplements.ps1 -OutputDirectory 'path/to/fresh/materials' -Download
.\scripts\test-native-source-supplements.ps1
```

通常はofflineで、`-Download`時だけ欠落source inputを取得する。全既存inputとrecipeを先に検査するため、後方の改変inputも先行download前に拒否する。既存outputとinput directoryへの重なりを拒否し、完成markerのINPUTS.jsonは最後にcopyする。空cacheから全33 URLの実取得・照合が通過した。任意cwdから2回の全97 hash一致、cache timestamp保持、46 input（33 source/patch＋13 recipe）それぞれの欠落・同size改変拒否と入力・先行output保持を検証した。

GMPの`gmp-h.in`はLGPL v3以降またはGPL v2以降の選択を明示するため、libraryはLGPL側を予定し、LGPL/GPL v3両本文と元の選択条件を保持する。[上流説明](https://gmplib.org/manual/Copying)とも一致する。LZ4 1.10.0の[原本LICENSE](https://github.com/lz4/lz4/blob/v1.10.0/LICENSE)はBSDのlibとGPLのその他を区別し、lib/Makefileは同directoryのC sourceからlibraryを作る。package全体のGPL表示をDLLへそのまま割り当てない。これらはsource/recipeの範囲確認であり、bit再現buildや最終配布承認ではない。

libvpxの第三者文書・PATENTS、Theoraの技術声明、opencore-amrの追加NOTICEも保持する。SnappyのCOPYINGにはbenchmark dataの別条件があり、recipeがtest/benchmarkを無効にしても、丸ごとのsource archive配布条件は別途確認が必要である。ZeroMQのpatchはrecipeで逆向きに適用し、Theoraにはexport list編集もあるため、patch一覧だけをbuild手順の代わりにしない。範囲と未完了事項は同梱[README](../third-party/NATIVE-SOURCE-SUPPLEMENTS-README.txt)へ記録した。

次は選択文書では拾い切れない個別header・同梱source/dataの範囲と、残るpackageのsource/patchを照合する。特にgettext、lcms2、XZ、ZVBIなどの混合license表示、FreeTypeの選択条件、GCC runtime exceptionもpackage labelだけで処理しない。

この一覧は静的にFFmpegへ組み込んだsource dependencies、header-only code、埋め込みdata、動的LoadLibrary、driverやplugin resourceの完全な一覧ではない。recipe本体が揃っても、参照先archive・patchやMSYS2 build infrastructureは自動的には揃わない。全体build再現、release media/performance、Setup.exeと隔離Windowsでの導入／更新／削除、physical environmentとowner承認のgateを維持する。
