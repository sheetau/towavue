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

### 混合licenseの追加読み取り（2026-09-08、未完了）

再生成releaseの30分再生中には、小さなrecipe／文書／設定fileの読み取りだけを行った。以下は元packageと対応するrecipe、および上流のtag／commitの範囲確認であり、対応source archiveの取得・全byte照合・再buildや最終binaryの組込み範囲の証明ではない。

**Little CMS 2.19.1:** 対応recipe `c13ff2ab5c718d63da878bb180c2cc0b5ce40dec`は`-Ddefault_library=both -Dfastfloat=true`を指定する。tagのcommit `21c582a594fe5279f90c0b93437c398f93bf62b0`では、[本体library](https://github.com/mm2/Little-CMS/blob/21c582a594fe5279f90c0b93437c398f93bf62b0/src/meson.build)と[fast_float library](https://github.com/mm2/Little-CMS/blob/21c582a594fe5279f90c0b93437c398f93bf62b0/plugins/fast_float/src/meson.build)は別targetだが、後者をpkg-configの追加libraryへ入れる。本機の`lcms2.pc`（SHA256 `37a9c51d841218ad064be5cc1eb307120229ddc7c10d7411a702bdecd3aa6011`）も`-llcms2 -llcms2_fast_float`を含み、再生成FFmpegの`EXTRALIBS-avfilter`／`EXTRALIBS-avcodec`にもその引数がある。監査済みPE graphにfast_float DLLがないことだけで、静的コードも不在と断定しない。実際の選択archive／symbolとの照合を次に行う。GPLプラグインを本体MITと同一扱いせず、逆に未使用link引数だけで組込み済みとも扱わない。

**ZVBI 0.2.45:** annotated tag `45138a87f86b683f9c3611793752ac08795d836f`はcommit `d3a5ee9f2b047bf16cd1ee5ccf6ec05ee75409d0`を指す。元recipeのVCS checksumは、MSYS2 makepkg実装では`git -c core.abbrev=no archive --format tar <tag>`のSHA256であり、任意のcodeload圧縮archiveのhashではない。まだそのVCS archiveの一致を確認した扱いにはしない。

上流[NEWSの0.2.28記録](https://github.com/zapping-vbi/zvbi/blob/d3a5ee9f2b047bf16cd1ee5ccf6ec05ee75409d0/NEWS)はlibraryのLGPL v2以降への移行を説明する。一方、現在の[COPYING.md](https://github.com/zapping-vbi/zvbi/blob/d3a5ee9f2b047bf16cd1ee5ccf6ec05ee75409d0/COPYING.md)と`src/pdc.c`／`src/packet-830.c`のheaderは当該fileをGPL v2とし、[src/Makefile.am](https://github.com/zapping-vbi/zvbi/blob/d3a5ee9f2b047bf16cd1ee5ccf6ec05ee75409d0/src/Makefile.am)は両方を`libzvbi_la_SOURCES`へ含める。package内のCOPYINGも同じ個別条件を記す。唯一のMSYS2 patchは`-no-undefined`を追加するだけで、この相違を解消しない。GPLの`exp-vtx.c`はsourceに残るが本文が`#if 0`で無効化されており、Makefileへの列挙だけで同じ扱いにしない。FFmpegのconfigureはZVBIが0.2.28以降かを調べるが、個別licenseを検証するものではない。したがって、この不一致は配布判断前の未解決事項として保持し、旧NEWSだけで個別表記を上書きしたり、機能をstubへ置き換えたりしない。次に実DLLの対応symbolとsourceの履歴／適用範囲を確認する。

現repositoryの履歴は2022年のimportで始まるため、READMEが案内する公式`vbi-archive`も確認した。両C fileは[2009-02-16の追加commit](https://github.com/zapping-vbi/vbi-archive/commit/5346888e899da55f73eba73097738021d8cbf8cf)からGPL表記を持ち、直後の[Makefile変更](https://github.com/zapping-vbi/vbi-archive/commit/64392045f9f8ac736a1f365217ec776c457f1df6)でlibrary対象へ追加されている。2008年のLGPL化告知より後の追加であり、告知だけでこれらのfileも許諾されたと推測しない。FFmpegの直接呼出一覧にPDC APIがないことも、配布するZVBI DLL自体からその実装が消える根拠にはならない。

再生終了後、実際のstaged `libzvbi-0.dll`（450046 bytes、SHA256 `fa1b722aa22739a9b147f7c918a981c51fc48ebd1269b192e412b6f346f816cc`）が元package監査と一致することを再確認した。PE exportには`vbi_decode_teletext_8301_cni`／`local_time`、`vbi_decode_teletext_8302_cni`／`pdc`、`vbi_pil_is_valid_date`／`vbi_pil_to_time`等があり、forwarderではなくDLL内のExport RVAを持つ。例えば`vbi_pil_is_valid_date`はRVA `0x29620`、`vbi_pil_to_time`は`0x29ab0`。個別GPL表記の対象を単なる同梱toolと分類してこの候補をLGPL構成として承認することはできない。対応sourceのchecksum結合を完成させ、許諾の明確化または機能を保つ代替構成を検討する。外部への問い合わせ、本体license変更、PDCのstub化やcodec無効化はまだ行っていない。

Little CMSについては、同じ環境のfast_float静的archiveが定義する14個の公開text symbolを取得し、再生成buildのstrip前avcodec／avfilterの全symbol table（65,335／27,148 records）と照合して一致0件、fast_float import markerも0件だった。これは未使用link引数という解釈と整合するが、全推移DLLの静的入力監査やlink traceの代わりにはしない。

**XZ／FreeType／GCC runtime:** XZ 5.8.3の[固定COPYING](https://github.com/tukaani-project/xz/blob/4b73f2ec19a99ef465282fbce633e8deb33691b3/COPYING)はliblzmaの0BSDと、CLI用getoptのLGPL／補助scriptのGPLを区別する。FreeType 2.14.3の[LICENSE.TXT](https://github.com/freetype/freetype/blob/0a0221a1347e2f1e07c395263540026e9a0aa7c7/LICENSE.TXT)はFTL／GPLの選択に加え、BDF／PCF／hash、gzip、HarfBuzz由来fileの別条件を列挙する。FTL側を選ぶ場合も同本文と製品文書のクレジットが必要であり、FTLだけのcopyで全sourceの表示が揃うとは扱わない。GCC 16.2.0-3の元recipeとpackage内READMEはlibgcc／libstdc++／libgomp／libatomicのGCC Runtime Library ExceptionとlibquadmathのLGPLを区別する。現graphで使う3 DLLは前者だが、[例外本文](https://gcc.gnu.org/onlinedocs/libstdc++/manual/license.html)の適用対象／Eligible Compilation Process、対応source、その他の静的・header入力は別に確認する。package全体のGPL labelをそのまま本体アプリのlicenseへ移さない。

この一覧は静的にFFmpegへ組み込んだsource dependencies、header-only code、埋め込みdata、動的LoadLibrary、driverやplugin resourceの完全な一覧ではない。recipe本体が揃っても、参照先archive・patchやMSYS2 build infrastructureは自動的には揃わない。全体build再現、release media/performance、Setup.exeと隔離Windowsでの導入／更新／削除、physical environmentとowner承認のgateを維持する。
