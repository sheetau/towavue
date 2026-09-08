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

15 packageにはこのlicense directory内の通常fileがない。Chromaprint／OpenALは別途sourceから補完済み。残りはGMP、LAME、libass、libssh、libtheora、libvorbis、libvpx、LZ4、opencore-amr、Snappy、TwoLAME、ZeroMQ、zimgである。別directory/source内のnotice有無は個別確認が必要で、directoryが空というだけでlicense表示が不要とは扱わない。

次は固定recipeに記載されたsource、patch、同梱source/dataを照合する。特にgettext、lcms2、LZ4、XZ、ZVBIなどの混合license表示は、OpenALと同様にlibrary／tool／documentationの範囲を分ける。GMP・FreeTypeの選択条件、GCC runtime exceptionもpackage labelだけで処理しない。

この一覧は静的にFFmpegへ組み込んだsource dependencies、header-only code、埋め込みdata、動的LoadLibrary、driverやplugin resourceの完全な一覧ではない。recipe本体が揃っても、参照先archive・patchやMSYS2 build infrastructureは自動的には揃わない。全体build再現、release media/performance、Setup.exeと隔離Windowsでの導入／更新／削除、physical environmentとowner承認のgateを維持する。
