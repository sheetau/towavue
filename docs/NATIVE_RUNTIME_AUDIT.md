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

限定ZVBIを用いた後続候補では、このbaselineの85 package DLLのうちZVBIだけを置き換える。残る84 package DLLは同じhashであり、限定DLLの入力・source資料は下記の専用manifestで区別する。旧package由来の証拠を限定DLLの由来として扱わない。

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

全72 ownerと一覧外のstatic／header等の残作業は[NATIVE_MATERIAL_PLAN.md](NATIVE_MATERIAL_PLAN.md)へ整理した。対応sourceを優先するものとnotice／組込み範囲を先に調べるものを分け、permissive library全件の単体再buildを一律の条件にはしない。分類・材料取得を配布承認には用いない。

15 packageにはこのlicense directory内の通常fileがない。Chromaprint／OpenALは別途sourceから補完済み。残り13件のGMP、LAME、libass、libssh、libtheora、libvorbis、libvpx、LZ4、opencore-amr、Snappy、TwoLAME、ZeroMQ、zimgについても、元recipeが指定するsource archiveとpatch/templateを取得・照合した。directoryが空というだけでlicense表示が不要とは扱わない。

### 13件のsource補完

[native-source-supplements.json](native-source-supplements.json)の初回補完では13 source archiveと20 patch/template、計20909402 bytesを固定した。全33 inputのSHA256が対応PKGBUILDと一致する。source署名の検証ではなく、GMP/libssh recipeでSKIP指定の署名fileは含めない。取得したsourceの全member名とtypeを監査し、zimgの7個だけ存在する内部symlinkは展開しない。現在は下記のXZ／FreeType／gettext-runtime追加を含む16 packagesを同じscriptで扱う。

47件の選択文書（license・authors・NOTICE・patent文書・GMP/LZ4の範囲確認用header/build記述）を原本byteのまま抽出する。全source、全patch/template、13 recipes、参照inventory、READMEと合わせて97 filesとなる。DLL/exeはcopyせず、recipe実行・patch適用・buildも行わない。

```powershell
.\scripts\get-native-runtime-recipes.ps1 -Download
.\scripts\prepare-native-source-supplements.ps1 -OutputDirectory 'path/to/fresh/materials' -Download
.\scripts\test-native-source-supplements.ps1
```

通常はofflineで、`-Download`時だけ欠落source inputを取得する。全既存inputとrecipeを先に検査するため、後方の改変inputも先行download前に拒否する。既存outputとinput directoryへの重なりを拒否し、完成markerのINPUTS.jsonは最後にcopyする。空cacheから全33 URLの実取得・照合が通過した。任意cwdから2回の全97 hash一致、cache timestamp保持、46 input（33 source/patch＋13 recipe）それぞれの欠落・同size改変拒否と入力・先行output保持を検証した。

GMPの`gmp-h.in`はLGPL v3以降またはGPL v2以降の選択を明示するため、libraryはLGPL側を予定し、LGPL/GPL v3両本文と元の選択条件を保持する。[上流説明](https://gmplib.org/manual/Copying)とも一致する。LZ4 1.10.0の[原本LICENSE](https://github.com/lz4/lz4/blob/v1.10.0/LICENSE)はBSDのlibとGPLのその他を区別し、lib/Makefileは同directoryのC sourceからlibraryを作る。package全体のGPL表示をDLLへそのまま割り当てない。これらはsource/recipeの範囲確認であり、bit再現buildや最終配布承認ではない。

libvpxの第三者文書・PATENTS、Theoraの技術声明、opencore-amrの追加NOTICEも保持する。SnappyのCOPYINGにはbenchmark dataの別条件があり、recipeがtest/benchmarkを無効にしても、丸ごとのsource archive配布条件は別途確認が必要である。ZeroMQのpatchはrecipeで逆向きに適用し、Theoraにはexport list編集もあるため、patch一覧だけをbuild手順の代わりにしない。範囲と未完了事項は同梱[README](../third-party/NATIVE-SOURCE-SUPPLEMENTS-README.txt)へ記録した。

次は選択文書では拾い切れない個別header・同梱source/dataの範囲と、残るpackageのsource/patchを照合する。gettext、lcms2、GCC runtime exceptionなどもpackage labelだけで処理しない。XZ／FreeTypeとZVBIについての後続証拠は以下へ記録する。

### XZ／FreeTypeのsource・二次表示の補完

元packageの`.BUILDINFO`と一致するrecipeから、XZ 5.8.3 archive（1548064 bytes／`fff1ffcf...`）、FreeType 2.14.3 archive（2670220 bytes／`36bc4f1c...`）、FreeTypeの2 patches（624／652 bytes）を取得した。4 inputsすべての全SHA256がrecipeと一致し、上記manifestへ固定した。XZの698 entriesは649通常files＋49 directories、FreeTypeの937 entriesは855通常files＋82 directoriesで、link／危険なmember名はない。source署名は未検証で、SKIP指定の署名fileは含めない。

XZの11文書・scope資料とFreeTypeの17文書・source headersを追加した。元packageに収録されていたXZの5 COPYING、FreeTypeのFTL／GPL本文はすべてsource原本と同じbytes／hashであり、collectorでもこの7件を照合する。FreeTypeは[LICENSE.TXT](https://github.com/freetype/freetype/blob/0a0221a1347e2f1e07c395263540026e9a0aa7c7/LICENSE.TXT)が列挙するBDF／PCF／hash、gzip、HarfBuzz由来4 filesの原本表示も保持する。HarfBuzzのscript-listは他3 filesとcopyright年が異なる。FTLを予定する候補用のFreeType Teamへのcreditと、元patchがgxvalid／otvalid・subpixel renderingを有効にする説明を資料READMEへ追加した。原本表示やpatch本文は書き換えない。

両FreeType patchは元recipeと同じ`-Nbp1`のdry-runを通過した。modules.cfgはoffset +6、ftoption.hはoffset -3／fuzz 2を報告するため、完全context一致や実build再現とは記述しない。対象は意図したmodule行とsubpixel defineであり、sourceは不変。Mesonは見つかればsystem zlibを選び、現在の`libfreetype-6.dll`も`zlib1.dll`等をimportする。DLL hash `bbb3e639...`、`liblzma-5.dll`の`dc9edb9b...`は監査baselineと再一致した。これは全static／header入力の証明ではなく、XZ付属GPL toolsの同梱やFreeType内蔵zlibの使用を推測する根拠にも用いない。

この時点の補完資料は15 packages、37 source／patch inputs（25128962 bytes）、75選択文書を含む131 files／26323591 bytes。新4 URLの実取得と照合、任意cwdからの2回生成、52 inputの欠落・同size改変、元package表示の欠落／変更／重複6 cases、cache timestamp・先行output保持を検証した。旧資料のREADME／INPUTS以外の95 filesは同じhashで、全131 filesがtest出力と一致する。範囲別の表示が揃うことと、全依存の対応source／build入力・最終候補性能・installer gateの完了は区別する。

### Gettext runtimeの対応source補完

元recipeと一致する[gettext 1.0全archive](https://ftp.gnu.org/pub/gnu/gettext/gettext-1.0.tar.lz)は10261665 bytes／SHA256 `d6342cbe1411a2fe7d139bfed80c2d63b1babc92acfedc72501cc105184f61ee`。元MSYS2の6 patchesも含めた7 inputs、計10265586 bytesを取得し、全hashが`.BUILDINFO`対応recipeのchecksumと一致した。9038通常files＋308 directoriesで、link／危険なmember名はない。署名の暗号検証は行っていない。

原本`gettext-runtime/COPYING`はlibintl・libasprintfとheadersをLGPL、付属programs／documentationをGPLと区別する。公開header原型`intl/libgnuintl.in.h`はLGPL 2.1以降の選択を明記する。候補にあるのは`libintl-8.dll`（298731 bytes／`0537c3dd...`）で、監査baselineのhashと再一致し、libiconv／Windows DLLをimportする。他のgettext programsやlibasprintfを同梱対象へ追加しない。6 patches中1件はこの公開headerのWindows printf属性を変更し、残りはprograms／tests／libasprintfを対象とする。全patchとrecipeは保持するが、今回は適用・Autogen・再buildを実行していない。

GPL／LGPL本文、作者、範囲説明、header原型、libraryのbuild記述と代表実装の14 filesを原本byteで追加した。packageの4 noticesはsource原本と一致する。intlとlibasprintfのLGPL本文は同hashの別fileであり、`package_notice`による収録先対応を明記して照合する。正しい二重収録を拒否せず、曖昧・誤った対応や重複指定は拒否する。元archive全体をLGPL-onlyとは表示しない。

現補完資料は16 owners、44 inputs／35394548 bytes、89選択文書、計153 files／36958095 bytes。新7 URLの実取得・照合、任意cwdと再生成の全hash一致、60 inputの欠落／同size改変、notice不一致9 cases、既存output・cache保持を通過した。先行131-file資料のREADME／INPUTS以外の129 filesは不変で、最終153 filesは最新test出力と一致する。実buildのgenerated／gnulib／static入力と公開するsource取得案内はまだ完了していない。

### Library-source優先8件の追加資料

libiconv 1.19、FriBidi 1.0.16、Game Music Emu 0.6.5、mpg123 1.33.7、libbluray 1.5.0、Graphite2 1.3.15、GLib 2.88.3、libplacebo 7.360.1の元archiveと16 patches／templates／hooks／scriptを取得した。24 inputs／29315063 bytesの全hashが`.BUILDINFO`対応recipeと一致する。各ownerの現候補DLLも監査baselineと一致する。上流署名検証、recipe実行、patch適用や各libraryの再buildは行っていない。

archiveのmember名とtypeを確認し、GLibのCOPYINGとgmodule/COPYINGだけが内部link（他7 archiveは通常file／directoryのみ）。選択文書はlinkではなく通常fileの原本から61件追加し、元packageの12 noticesをbyte単位で対応付けた。libiconvの同内容COPYING.LIBは各package pathを明記する。GLibの通常target本文は保持するがlink自体は展開せず、archive内の重複原文も改変しない。

確認した範囲と限界は同梱READMEにも記録した。libiconvはlibrary／headerとprogram／docsを区別する。Game Music Emuは[元CMake](https://github.com/libgme/game-music-emu/blob/0.6.5/CMakeLists.txt)でLGPLのNuked実装が既定、対応recipeに上書きはないが、これはDLLの全compiler入力を証明するものではない。別のGPL MAME実装も全archiveには残す。Graphite2の[LICENSE](https://github.com/silnrsi/graphite/blob/1.3.15/LICENSE)の複数選択を第三者codeやtest fontへ一律に適用しない。FriBidiのUnicode 16 data、libblurayのlibudfread／native BD-J、libplaceboのxxhash／fast_float／Vulkan headers／glad生成物は追加scope確認が必要である。libplaceboのfast_floatとLittle CMSの同名pluginは別物として扱う。

最新補完資料`native-source-supplements-v5`は24 owners、68 inputs／64709611 bytes、150選択文書、計246 files／67179202 bytes。元153-file資料のREADME／INPUTS以外の151 filesはhash不変。新24 URLの取得・照合、任意cwd／再生成の全hash、92 inputの欠落／同size改変、表示不一致12 cases、GLib link非展開と既存cache／output保持を確認した。最初のGLib異常系fixtureは同名COPYINGを複数選んだため失敗し、対象package pathを明示して全試験を再実行した。収集の完了を配布承認や全対応sourceの完了にはしない。[残作業](NATIVE_MATERIAL_PLAN.md)ではlibsoxrのVCS source対応、SRT、GCC、static/header/data表示と最終候補／installerのgateを維持する。

### libsoxrのVCS sourceとSRT資料

libsoxr 0.1.3-5の元recipeは[上流Git repository](https://sourceforge.net/p/soxr/code/)のcommit `945b592b70470e29f917f4de89b4281fbbd540c0`を指定し、source checksumはSKIPする。上流からbare repositoryを取得し、`core.autocrlf=false`と固定prefixでtarを生成した。675840 bytes／SHA256 `c7b08bf2c943c9e3dc51021790529f51be2cdfd40923b815f9135edcbbf9f46c`は今回のarchive pinであり、元recipeのarchive checksumではない。137通常filesのGit blob IDsは全一致し、10 directoriesと内部helper link 1件を含む。submoduleはない。最初の全展開はWindowsでlink作成が失敗したため診断用に保持し、fresh directoryで通常fileのみを選択して照合した。配布用資料でもlinkは展開しない。

元の2 patchesと別入力LICENSE-PFFFTはrecipe checksumと一致する。LICENCEはLGPL 2.1以降とPFFFTの別表示を明記する。候補のlibsoxr.dllは監査hash不変で、元recipeはPFFFT／x64 OpenMPを有効、AVFFTを無効にする。PFFFTのNCAR／UCAR／Pommier、Oouraのfft4g、作者・LGPL原文とbuild記述を保持した。GPLのlsr-testsをsource archiveに残すことは、候補DLLへの組込みを示すものではない。

SRT 1.5.7の[元archive](https://github.com/Haivision/srt/archive/v1.5.7/srt-1.5.7.tar.gz)は1794204 bytes／`017cd1e4...`、唯一のWindows互換header patchもrecipeと一致する。334通常files／33 directories／内部helper link 1件。元LICENSEのMPL 2.0本文だけでなく、srtcore/core.hのUniversity of Illinois継承表示、HaiCrypt headerとbuild記述を保持する。対応recipeはOpenSSLを選び、現候補DLLのhashもbaseline不変。patch適用・独立再build・全file表示の完了やsource署名検証は主張しない。

既存collectorに、recipeの正確なVCS URL／commitとローカルarchive pinの照合、別recipe入力のpackage notice対応を追加した。VCS tarは記録したcommandで事前準備し、Download指定でもHTTP扱いで取得しない。最新`native-source-supplements-v6`は26 owners、74 inputs／67185008 bytes、165選択文書、269 files／70006200 bytesで、前資料のREADME／INPUTS以外の244 filesはhash不変。取得済みのlibrary-source優先群は17件、MPL群は2件となるが、GCC／LCMS／static／header／dataと公開source取得案内、最終候補性能・installer・対象Windowsのgateは未完了である。

### GCC runtimeの元source／patch／表示資料

元packageの`.BUILDINFO`が指定するGCC 16.2.0-3 recipeを確認し、[GCC 16.2.0全source](https://ftp.gnu.org/gnu/gcc/gcc-16.2.0/gcc-16.2.0.tar.xz)を取得した。107200820 bytes／SHA256 `e6738e29597f733270731aa90600f37ffdc045079dfc27ec7e8192cc81085c3e`。17 patchesとgdbinitを含む19 inputs／107250798 bytesの全hashが元recipeと一致する。archiveは158985通常files／6094 directories、link／危険なmember名なし。`.sig`の暗号検証やGCC bootstrapは行っていない。

現候補の`libgcc_s_seh-1.dll`は150998 bytes／`b37c1770...`、`libgomp-1.dll`は329299 bytes／`acf25eee...`、`libstdc++-6.dll`は2661299 bytes／`887c21db...`で、全て元package監査hashと一致した。共有のpackage資料generatorを複数DLLへ対応させ、GCCだけtoolchain inventoryの同一packageへ結び付ける。資料にはruntime binaryも元binary package archiveもcopyせず、package原本の4 noticesと`.PKGINFO`／`.BUILDINFO`を選択抽出する。libatomic／libquadmath等は同梱候補へ追加しない。

libgccの算術／SEH、libgomp、libstdc++例外／allocationの元fileはGPLv3以降とGCC Runtime Library Exception 3.1を明記する。COPYING3／COPYING.RUNTIMEとpackage生成READMEを原文のまま保持し、libbacktraceのBSD系表示、PSTLのApache／LLVM-exceptionとlibrary／documentation条件の区別も記録した。[例外本文](https://github.com/gcc-mirror/gcc/blob/releases/gcc-16.2.0/COPYING.RUNTIME)の対象file・Independent Module・Eligible Compilation Process条件を、別頒布するDLL自体の対応sourceや全archiveのlicenseへ一律に置き換えない。recipeのPOSIX threads、profiled bootstrap、shared/static、libgomp、libstdc++ backtrace設定と元patchを保持したが、全compiler入力・全file表示・各packageのcompilation条件の完了を主張しない。

固定manifestは[native-gcc-libs-inputs.json](native-gcc-libs-inputs.json)、収集条件と限界は[GCC-LIBS-MATERIALS-README.txt](../third-party/GCC-LIBS-MATERIALS-README.txt)。`prepare-native-package-materials.ps1 -Component gcc-libs`へ元PackageArchive／Recipe／SourceArchive／PatchDirectoryを渡し、RuntimeDllには上記3 DLLをその順序の配列で指定する。新しいOutputDirectoryを使う。既存3 componentの単一DLL指定はそのまま利用できる。

最終`gcc-runtime-materials-v1`は47 files／107713444 bytes。19 source選択files、6 package原本files、全source archive／18追加inputs／recipe／説明とmanifestを保持する。別cwdからの再生成と全hash一致、24 inputsの欠落／同size改変、3 DLL集合の不足・余分な重複・順序取り違えを拒否し、先行outputを保持した。Chromaprint／OpenAL／元ZVBIの既存資料試験も通過。GCC材料を集めたことだけで最終runtime品質・Setup.exe・対象Windowsやowner受入のgateは完了しない。

### Little CMSのリンク入力確認を更新

Little CMS 2.19.1の[元source archive](https://github.com/mm2/Little-CMS/releases/download/lcms2.19.1/lcms2-2.19.1.tar.gz)を取得し、5728743 bytes／SHA256 `bfc54f7bab59fbc921012014a8032e4cba4abd46db47d46b76416a8c0b2815c8`が`.BUILDINFO`対応recipeと一致した。340通常files／102 directories、link／危険なmember名はない。coreのMIT原文とGPL fast_float原文・header、build記述、作者・iccjpeg utility表示を区別して保持する。元recipeはcore／pluginを別targetでbuildし、[上流meson設定](https://github.com/mm2/Little-CMS/blob/lcms2.19.1/meson.build)はpkg-configへpluginも追加するため、利用しないconsumerにも`-llcms2_fast_float`が現れる。

現FFmpegの`CONFIG_LCMS2`は既に0で、LCMSはlibjxl_cms／libplacebo経由で使われる。今回flagを変更して無効化したのではない。元`.def`／version scriptと既存1326／567 objectsを使い、avcodec／avfilterのg++ link行だけをfresh directoryへ再実行した。出力DLL／import libraryだけを隔離先へ変更し、GNU ldのmap／cross-reference／二重traceを追加する。makeのdry-runが生成する`.objs`はlocal build記録として保持し、configure・再compile・installは行わない。

mapはfast_floatの**import library** `liblcms2_fast_float.dll.a`を1／2回LOADするが、選択memberは両方0件で、実装のstatic archive `liblcms2_fast_float.a`は選ばない。他のarchive memberはmapに記録されるため、member一覧自体が空という観測ではない。再linkした両DLLの`.text`／`.rdata`は、strip済み現候補DLLから読み取った同領域とbyte単位で一致する。avcodecは21344768／4641640 bytes、avfilterは5429248／1524864 bytes。現候補の全DLL hashも維持される。これで当該2 DLLのlink引数がfast_float実装を取り込むという懸念は解消したが、他componentの全static/header/data監査やDLL全体のbit-identical rebuildとは区別する。

直接LCMSをimportするのは監査graph上のlibjxl_cmsとlibplacebo。追加取得したlibjxl 0.12.0 sourceは1698757 bytes／`03e9be69...`で元recipeと一致し、選択したlib subtreeにplugin登録・fast_float header参照はない。libplaceboの元src treeも同様で、両者はLCMS context生成へplugin引数をNULLで渡す。Little CMS本体のMeson source一覧にもfast_float実装はなく、pluginは別libraryとしてcoreへ依存する。これはrecipe／sourceとPE importの対応証拠であり、全packageの独立再buildを完了したという意味ではない。GPL pluginを製品へ追加したり、元archiveのGPL表示をMITへ変更したりはしない。

入力・map／trace・候補DLL・一致sectionのhashは[native-lcms-link-audit.json](native-lcms-link-audit.json)へ固定した。machine pathsを含む大きなmap／traceと再link binaryはlocal監査directoryだけに保持する。LCMS原本は既存source supplementsに11文書とarchiveを追加し、packageの2 noticesも原本bytesで照合する。公開source取得案内、残る表示／static/header/dataと最終candidate品質・Setup.exeの確認は継続する。

最終`native-source-supplements-v8`は27 owners、75 inputs／72913751 bytes、176選択文書、282 files／75936143 bytes。別cwd・再生成の全hash一致、102 inputsの欠落／同size改変、18 notice不整合、3 VCS対応不整合と既存output保護の試験が通過した。候補runtimeの94 files／138510464 bytesも全hashが既存記録と一致する。

### 継承NOTICE・Unicode data・内蔵UDF/JNIの範囲確認

取得済みのOpenCORE AMR／Snappy／FriBidi／libblurayについて、元archiveのhash・member数・通常file／directoryだけであることを再検証してfresh directoryへ展開した。今回新しいlibraryやcodecをbuild／無効化していない。

- OpenCORE AMR 0.1.6: `Makefile.am`の対象はAMR-NB／AMR-WBと小さなOSCL wrapperで、包括的な`opencore/NOTICE`にあるMPEG video／AAC／Windows Mediaの実装を一括して含む構成ではない。codec treeの元headerにはApache 2.0と、3GPP TS 26.073／26.173由来部分をその条件で扱う許可を得た旨がある。NOTICE全体を削らず、代表header、wrapper、build一覧と別の`patent_disclaimer.txt`も保持する。著作権条件の確認を特許権の許諾へ置き換えない。
- Snappy 1.2.2: core targetは4 C++ filesで、`snappy_test_data.cc`を含むtest-support targetとは分離される。対応recipeはtests／benchmarksをOFFにし、全3 patchもこの対象分離を変更しない。testdataのCC-BY等をcore DLLの条件と混同せず、元source archiveにはそのdataが残るためCOPYINGの個別表示もそのまま保持する。test mediaをruntime payloadへ加えない。
- FriBidi 1.0.16: generatorが使うUnicodeData／ArabicShaping／BidiBrackets／BidiMirroringの4 inputsは、[固定Unicode upstream tree](https://github.com/unicode-org/unicodetools/tree/addf0c992050b10a0bfe8647f90fcc8fdbfe71fc/unicodetools/data/ucd/dev)と全byteが一致した。そのtreeの[Unicode License V3原本](https://github.com/unicode-org/unicodetools/blob/addf0c992050b10a0bfe8647f90fcc8fdbfe71fc/LICENSE)を2033 bytes／SHA256 `fe5c62b543e287981db198f2acfa0ca732d12591a1024536dc8fd85dacd77104`、Git blob `d7e7973c2fd6f2586a8999a69dc21e39af26be0f`で固定し、LGPL本文とは別に同梱資料へ追加した。候補DLLもUnicode 16.0.0を表示する。upstreamのReadMeは未展開templateでFriBidiのrelease ReadMeとは異なるため、その一致は主張しない。4 dataの個別hash・元pathもmanifestに残す。
- libbluray 1.5.0: defaultの`embed_udfread=true`は同archiveのlibudfread 1.2.0をstatic targetにする。元build inventoryに外部libudfreadはなく、候補DLLにはそのUDF診断が存在しUDF DLL importはない。全4実装fileのLGPL 2.1-or-later原文とtarget／optionを保持する。`bdj_jar=disabled`でもnative BD-Jはcore source一覧に残り、recipe既定の空`jdk_home`は内蔵JNI headerを使う。`jni.h`／Windows `jni_md.h`のMPL／GPL／LGPL選択式原文を維持し、LGPL選択が可能なこととLGPL-onlyへの書換えを区別する。JAR／JVM／ASM binaryをstageしたという意味ではない。

上記はhash対応したrecipe／sourceと候補binaryの証拠であり、独立した全DLL再buildや全static/header入力の完了証明ではない。元sourceへの選択fileを24件追加し、Unicode原本1件はrecipe外の`additional_notices`として明示する。collectorはそのtracked原本も出力前にhash検証し、Downloadでも上書きしない。

最終`native-source-supplements-v9`は27 owners、従来どおり75 archive／build inputs、200選択文書と追加Unicode原本、307 files／76355951 bytes。前v8の280 files（README／INPUTS以外）は不変。別cwd／再生成、102欠落・改変pairs、追加noticeの欠落・改変・一覧除去、18 package-notice／3 VCS不整合と既存output保護の試験が通過した。候補runtime全94 filesもhash不変。残るnative／Rust内蔵依存の表示と公開取得案内、最終candidate品質・Setup.exe／対象Windows／owner受入は継続する。

### rav1e／libdoviの元sourceとRust依存資料

元packageの`.BUILDINFO`へhash対応したrecipeから、rav1e 0.8.1とlibdovi 3.4.0のsource archiveを取得した。順に3040879 bytes／SHA256 `06d1523955fb6ed9cf9992eace772121067cca7e8926988a1ee16492febbe01e`、494710 bytes／`8eac4d1c3134f53e8eb216db6450307a737425844113e480d1e9713c142a9fa2`で、recipeの値と一致する。330／242 archive entriesは通常file・directoryのみで、link／危険pathはない。source署名の検証やDLLの再buildは行っていない。

既存source collectorへ2 ownersと68選択文書を追加した。rav1eはLICENSE／PATENTS、全53 x86 asmとISC条件のx86inc、lock／build設定等を含む62文書。libdoviは外側CLIではなく`dolby_vision/`のlibrary manifest／lock等6文書を使う。原文byteを保持し、異なる文字encodingのPATENTSも再encodingしない。最終`native-source-supplements-v10`は29 owners、77 archive／build inputs／76449340 bytes、268選択文書とUnicode原本、379 files／85370086 bytes。前v9の305 files（README／INPUTS以外）は不変。

[native-rust-dependencies.json](native-rust-dependencies.json)は、元lockのchecksumを使ってrav1e 129件／libdovi 28件、計157 crate archives／16022087 bytesを固定する。Cargo 1.98.0の`metadata --locked --filter-platform x86_64-pc-windows-gnu`を隔離cacheで実行し、rav1eはdefault＋capi、libdoviはall-features、normal／build edgeを辿った。build scriptは実行せず元lockも不変。候補DLLにある12／11のprintable crate pathsは全てこの一覧と一致する。ただし解析hostはMSVCで元build hostはGNU、metadataのtest feature統合やCLI／build-only依存も含むため、**歴史的unit graphや実際にlinkされたcodeの完全一覧ではない。** 文字列がないことを非包含の証拠にしない。

元build記録はrav1eがRust 1.87.0／cargo-c 0.10.13、libdoviがRust 1.97.0／cargo-c 0.10.24。libdoviのrecipeはfetchに`--locked`がなく、その後のbuildが`--frozen`である。今回のlock・観測version一致だけでは過去のfetchがlockを変更しなかったとは証明できない。元GNU-hostの依存選択と各Rust版の標準library／compiler表示は残作業で、towavue自身や除外済みBtbNの資料で代用しない。

crate内の選択文書604件はCargo metadataやVCS情報も含み、604 licenseを意味しない。nestedなcrc-catalog／av-metrics／Unicode、build-sourceのlibgit2／libz表示も保持する。crateが省略したprofiling／profiling-procmacrosの原文2件はVCS情報の同commitから取得。av-metricsの原文は元manifestと全13 source blobsが一致するupstream commitへ対応させた。simd_helpersにはcrate／該当upstreamとも独立license fileがないため、MIT宣言と元author記録を保持し、SPDX標準本文を「作者発行のnoticeではない」と明示して別添する。placeholderのcopyright年・権利者を創作しない。取得元とhashはmanifestに記録した。個別source／生成dataの範囲確認は継続する。

offline collectorはruntime監査・source補完manifest・元lockの対応と全入力hashを出力前に照合し、失敗時に完成markerを残さない。通常実行は次のとおり。

```powershell
.\scripts\prepare-native-rust-materials.ps1 `
  -SourceSupplementDirectory target/distribution/native-source-supplements-v10 `
  -CrateCacheDirectory vendor/msys2/native-rust-crates-20260908 `
  -OutputDirectory target/distribution/native-rust-materials-v1
```

出力先はfresh directoryが必要。最終v1は769 files／18200522 bytesで、157 archives・604選択文書・2 locks・外部原文4件・README／INPUTSのみを含み、DLL／exeはcopyしない。別cwd／再生成の全hash一致、165入力の欠落／同size改変、6対応不整合と入力／既存output保護を検証した。source補完側も106入力pairs、Unicodeの欠落／改変／一覧除去、18 package-notice／3 VCS不整合等の回帰試験を通過した。両最終kitは試験出力と全hash一致し、現候補runtime全94 filesも不変。配布承認・compiler/runtime全範囲・Setup.exe完成の証明ではない。

### Native Rust標準libraryのpackage原本と不足表示

rav1e／libdoviのbuild記録にあるMSYS2 Rust 1.87.0-2／1.97.0-1について、元`rust`と対応する`rust-src`の計4 packagesを取得した。全署名を既存MSYS2 keyringのkey `5F944B027F7FE2091985AA2EFA11531AA0AA7F57`で検証し、各`.BUILDINFO`のrecipe hashは[1.87.0-2の固定recipe](https://github.com/msys2/MINGW-packages/blob/301c64f3dd42962143bc7679e088b00c7ed2636f/mingw-w64-rust/PKGBUILD)／[1.97.0-1の固定recipe](https://github.com/msys2/MINGW-packages/blob/a2b4aa07f355fd36922c3318f7584c8d77146899/mingw-w64-rust/PKGBUILD)と一致した。全11 patch／bootstrap inputsもrecipe hashへ対応する。compilerをインストール・実行しておらず、公式Rust同版のbinaryとの同一性は主張しない。

**両packageの`COPYRIGHT-library.html`は同一hashで、out-of-tree dependencies欄が空だった。** SHA256は`592ea218c47b2b50c8d8c53bf44512856fc93942e6686cca55e64311363681c0`。この原本を修正せず保持するが、依存表示の完了証拠にはしない。別の`COPYRIGHT.html`はcompiler scope、`licenses/`は複数用途の本文dictionaryであり、その全licenseをWindows DLLの条件と扱わない。

対応する標準library source packagesと原本表示・lock／manifestを別資料へ保持した。1.87のlibrary lockにある全42 registry crates／11616278 bytesも取得し、全checksumが一致する。これは他target・test／build依存を含むsource-lock材料であり、全42件のWindowsへのlinkを意味しない。1.97のsource packageにはvendor dependenciesとcompiler-builtins／libm sourceが含まれる。`rust-src`はcompiler全sourceではなく、compilerを作った元release archiveはrecipe内のhash／URLを記録するに留め、今回のkitには含めない。

compiler_builtins 0.1.152のcrateには独立license fileがないため、VCS情報のcommit `52d96c47681ef504a8ad7398efffe53214898aab`の原本と、そのlibm submodule `69219c491ee9f05761d2068fd6d4c7c0de6faa3a`の原本を補った。crate内libmの全171 math Rust filesは当該submodule treeのGit blobsと一致する。複合license式と個別math headerをroot labelで置換しない。他target向けFortanix SGX／r-efi／r-efi-allocも独立license fileがなく、元source／metadataは保持するが追加表示の解決済みとはしない。元GNU-hostのunit選択、個別inline／生成dataと非RustのCRT／static入力も別に残る。

[native-rust-runtime-inputs.json](native-rust-runtime-inputs.json)に原本hash・取得先・対応関係と限界を固定した。新collectorは65入力を出力前に照合し、packageのbuild record／recipe／既存native Rustのbuild版／1.87 source lockへの対応も検証する。Download、Cargo実行、compiler binary packageのoutputへのcopyはしない。

```powershell
.\scripts\prepare-native-rust-runtime-materials.ps1 `
  -CacheDirectory path/to/native-rust-runtime-cache `
  -OutputDirectory target/distribution/native-rust-runtime-materials-v1
```

cache内の相対pathと取得URLはmanifestの`inputs`を使う。最終v1は545 files／22961564 bytesで、484選択文書・2 source packages・42 crates・13 recipe／patch／config files・外部原本2件・README／INPUTSを含む。全archiveは選択前に通常file／directoryと安全pathを確認した。別cwd／再生成の全hash一致、65入力の欠落／同size改変、7対応不整合、展開後の文書hash不一致時に完成markerを残さないこと、元入力と既存outputの保護を試験した。M0の268 tests・format／Clippyも通過し、live ignores 3件と候補runtime 94 filesのhashは不変。これは資料収集の完了単位であり、全適用範囲の確定や配布承認ではない。

### GNU hostでのRust依存選択（2026-09-08、compileなし）

上記の資料収集後、元MSYS2 Rust／Cargo 1.87.0-2と1.97.0-1を隔離して実行した。両方の`-vV`でGNU hostを確認し、既存MSYS2へのinstallやpackage更新は行っていない。各Rust packageの`.BUILDINFO`にある補助package計29件の元archive／署名を取得し、全署名を既存keyringで検証した。PE importの不足だったhttp-parserも同じ記録の版で補い、版別の隣接DLL＋System32だけで起動する。これらのcompiler実行用DLLをtowavueの配布候補へ追加しない。

[native-rust-gnu-selection.json](native-rust-gnu-selection.json)にtool／source／lock／runtimeの対応hash、取得先、command、結果を記録した。元source lockと既存の隔離Cargo cacheを使い、`--frozen`でmetadataと[unit graph](https://doc.rust-lang.org/cargo/reference/unstable.html#unit-graph)を取得する。`RUSTC_BOOTSTRAP=1`はgraph取得の子processだけに設定し、製品のtoolchainや設定を変更しない。build script・compile・linkは実行されず、指定target directoryも生成されなかった。

| 元library | GNU metadataのnormal/build到達crate | library graphのcrate | target library unitあり | host unitのみ |
|---|---:|---:|---:|---:|
| rav1e 0.8.1 | 129 | 121 | 65 | 56 |
| libdovi 3.4.0 | 28 | 28 | 23 | 5 |

metadataの選択集合は従来一覧と一致する。library限定graphは164／47 unitsで、rav1eの8 crates（interpolate_name、ppv-lite86、rand、rand_chacha、rand_core、serde、serde_derive、zerocopy）が選ばれない。全選択crateは既存157件の資料に含まれ、実DLLで観測した12／11 crate pathsも全てtarget library側にある。rav1eのgit2／libgit2-sys／libz-sys等はこのgraphではhost側だけだが、proc macro・build scriptの生成code／dataやassemblyを含む可能性まで消えるわけではない。**target側件数はlink後の残存code一覧ではなく、host側の表示を一律に削る根拠でもない。** 既存の157件の材料は削除しない。

確認したcargo-c [0.10.13](https://github.com/lu-zero/cargo-c/blob/v0.10.13/src/build.rs)／[0.10.24](https://github.com/lu-zero/cargo-c/blob/v0.10.24/src/build.rs)は、libraryのみを選択し、capi featureと明示targetを加え、rootの名前／crate typesを変更する一方、`unit_graph`をfalseへ戻す。そのためcargo-c自体にgraph引数を渡さず、通常Cargoで代表条件を確認した。[`cargo rustc --crate-type staticlib,cdylib`](https://doc.rust-lang.org/cargo/commands/cargo-rustc.html)でもgraphを取得し、差分はrootのkind／typesとrav1eのdoctest属性だけだった。libdoviにはroot用`-Cpanic=abort`も渡したが、graphはcodegen条件全体を表すものではない。

両種graphとmetadataの反復hashは一致し、通常graph／metadataは別cwdからの再実行も一致した。元source／lock、compiler入力、実runtimeのhash、選択集合とC API型による差分を照合した。最終候補の94 filesも不変で、M0は270 tests・format／Clippyが通過し、live ignores 3件は未実行。cargo-c内部のCargo版・追加flag、過去のlibdovi unlocked fetch、標準libraryや生成／static／headerの範囲はこの観測だけで確定しない。歴史的buildの完全再現や配布承認とは区別し、次は残る個別表示とshaderc／SPIR-V／Vulkan等を調べる。

### 混合licenseの追加読み取り（2026-09-08、未完了）

再生成releaseの30分再生中には、小さなrecipe／文書／設定fileの読み取りだけを行った。以下は元packageと対応するrecipe、および上流のtag／commitの範囲確認であり、対応source archiveの取得・全byte照合・再buildや最終binaryの組込み範囲の証明ではない。

**Little CMS 2.19.1:** 対応recipe `c13ff2ab5c718d63da878bb180c2cc0b5ce40dec`は`-Ddefault_library=both -Dfastfloat=true`を指定する。tagのcommit `21c582a594fe5279f90c0b93437c398f93bf62b0`では、[本体library](https://github.com/mm2/Little-CMS/blob/21c582a594fe5279f90c0b93437c398f93bf62b0/src/meson.build)と[fast_float library](https://github.com/mm2/Little-CMS/blob/21c582a594fe5279f90c0b93437c398f93bf62b0/plugins/fast_float/src/meson.build)は別targetだが、後者をpkg-configの追加libraryへ入れる。本機の`lcms2.pc`（SHA256 `37a9c51d841218ad064be5cc1eb307120229ddc7c10d7411a702bdecd3aa6011`）も`-llcms2 -llcms2_fast_float`を含み、再生成FFmpegの`EXTRALIBS-avfilter`／`EXTRALIBS-avcodec`にもその引数がある。監査済みPE graphにfast_float DLLがないことだけで、静的コードも不在と断定しない。実際の選択archive／symbolとの照合を次に行う。GPLプラグインを本体MITと同一扱いせず、逆に未使用link引数だけで組込み済みとも扱わない。

**ZVBI 0.2.45:** annotated tag `45138a87f86b683f9c3611793752ac08795d836f`はcommit `d3a5ee9f2b047bf16cd1ee5ccf6ec05ee75409d0`を指す。元recipeのVCS checksumは、MSYS2 makepkg実装では`git -c core.abbrev=no archive --format tar <tag>`のSHA256であり、任意のcodeload圧縮archiveのhashではない。続く取得では、archive時にも`core.autocrlf=false`を指定して4239360 bytes／SHA256 `3dc234d716d1c51d53ae9b6b027775b3a6c9ced974bc1b246d749b44cca7d964`を再現し、元recipeのchecksumと一致した。Git for Windowsのsystem設定を継承した最初のCRLF archiveは4382720 bytesとなり拒否した。pinやOSのGit設定は変更していない。全235 entriesは224通常file＋11 directoryで、linkや危険なpathはなかった。localでtag署名の暗号検証をしたとは主張しない。

上流[NEWSの0.2.28記録](https://github.com/zapping-vbi/zvbi/blob/d3a5ee9f2b047bf16cd1ee5ccf6ec05ee75409d0/NEWS)はlibraryのLGPL v2以降への移行を説明する。一方、現在の[COPYING.md](https://github.com/zapping-vbi/zvbi/blob/d3a5ee9f2b047bf16cd1ee5ccf6ec05ee75409d0/COPYING.md)と`src/pdc.c`／`src/packet-830.c`のheaderは当該fileをGPL v2とし、[src/Makefile.am](https://github.com/zapping-vbi/zvbi/blob/d3a5ee9f2b047bf16cd1ee5ccf6ec05ee75409d0/src/Makefile.am)は両方を`libzvbi_la_SOURCES`へ含める。package内のCOPYINGも同じ個別条件を記す。唯一のMSYS2 patchは`-no-undefined`を追加するだけで、この相違を解消しない。GPLの`exp-vtx.c`はsourceに残るが本文が`#if 0`で無効化されており、Makefileへの列挙だけで同じ扱いにしない。FFmpegのconfigureはZVBIが0.2.28以降かを調べるが、個別licenseを検証するものではない。したがって、この不一致は配布判断前の未解決事項として保持し、旧NEWSだけで個別表記を上書きしたり、機能をstubへ置き換えたりしない。次に実DLLの対応symbolとsourceの履歴／適用範囲を確認する。

現repositoryの履歴は2022年のimportで始まるため、READMEが案内する公式`vbi-archive`も確認した。両C fileは[2009-02-16の追加commit](https://github.com/zapping-vbi/vbi-archive/commit/5346888e899da55f73eba73097738021d8cbf8cf)からGPL表記を持ち、直後の[Makefile変更](https://github.com/zapping-vbi/vbi-archive/commit/64392045f9f8ac736a1f365217ec776c457f1df6)でlibrary対象へ追加されている。2008年のLGPL化告知より後の追加であり、告知だけでこれらのfileも許諾されたと推測しない。FFmpegの直接呼出一覧にPDC APIがないことも、配布するZVBI DLL自体からその実装が消える根拠にはならない。

再生終了後、実際のstaged `libzvbi-0.dll`（450046 bytes、SHA256 `fa1b722aa22739a9b147f7c918a981c51fc48ebd1269b192e412b6f346f816cc`）が元package監査と一致することを再確認した。PE exportには`vbi_decode_teletext_8301_cni`／`local_time`、`vbi_decode_teletext_8302_cni`／`pdc`、`vbi_pil_is_valid_date`／`vbi_pil_to_time`等があり、forwarderではなくDLL内のExport RVAを持つ。例えば`vbi_pil_is_valid_date`はRVA `0x29620`、`vbi_pil_to_time`は`0x29ab0`。個別GPL表記の対象を単なる同梱toolと分類してこの候補をLGPL構成として承認することはできない。対応sourceのchecksum結合を完成させ、許諾の明確化または機能を保つ代替構成を検討する。外部への問い合わせ、本体license変更、PDCのstub化やcodec無効化はまだ行っていない。

ZVBIの元package／recipe／DLL／VCS archive／371-byte patchと選択原本10 filesは[native-zvbi-inputs.json](native-zvbi-inputs.json)へ固定した。既存のmaterial generatorへ`-Component zvbi`を渡し、`-PatchDirectory`に元patchを置く。生成物は全source、recipe、元patch、metadata、原本notice／scope資料、inventoryと説明の17 files／4498226 bytesで、DLL/exeは含めない。任意cwdでの再生成hash一致、全5入力の欠落・同size改変拒否、既存output保護を検証した。Chromaprint／OpenALの同generator回帰も通過した。これは未承認候補の監査材料であり、そのままinstallerへ同梱するものではない。

### ZVBI限定buildの実験（2026-09-08）

ownerは、FFmpegの字幕デコードを維持し、未使用の番組制御・放送時刻APIを含めない構成の検証を許可した。FFmpegは`VBI_EVENT_TTX_PAGE`だけを登録するが、元の共通headerにはGPL表記の`pdc.h`が入り、生成済み`libzvbi.h`にもその内容が転記されている。実装2 filesをlinkから外すだけでは不十分である。

[実験patch](../third-party/patches/zvbi-no-program-id.patch)は固定0.2.45の7 filesだけを変更する。`pdc.*`／`packet-830.*`と既に本文が無効な`exp-vtx.c`をlibrary sourceから除き、LGPL表記の呼出側で対応経路をcompile対象外にする。PDC型のinclude／event member／private decoder memberを除き、未提供の2イベントを要求した登録は既存handlerを変更する前に失敗する。`-1`による全イベント要求も拒否するため、汎用ZVBIの全API互換品ではない。字幕のpage、文字、描画実装は変更せず、成功を返すstubやlicense headerの書換えは追加しない。

公共headerは元の生成規則から再生成する。通常無効なこの規則の`io.h`は実sourceにないため`inout.h`へ直し、別build directoryからもversion入力を参照できるようにする。header生成だけをmaintainer mode外へ移し、network-tableのnetwork取得やGPLのhammgen実行は有効にしない。元source archiveのGPL対象4 filesとCOPYINGは原本のまま保持する。patchは元MSYS2の`-no-undefined`変更も含み、元patchとの二重適用はしない。

```powershell
.\scripts\build-zvbi-native-experiment.ps1 -MsysRoot 'path/to/msys64' `
    -SourceArchive 'path/to/zvbi-0.2.45-git.tar' -BuildDirectory 'path/to/fresh/build'
.\scripts\test-zvbi-native-experiment.ps1 -MsysRoot 'path/to/msys64' `
    -FfmpegPrefix 'path/to/reproduced/ffmpeg/prefix' -ZvbiPrefix 'path/to/fresh/build/prefix'
```

builderはASCII・spaceなしのnative path、固定source／patch hash、未作成outputを要求する。既存native buildと同じ固定330-package MSYS2環境を用い、`test-msys2-environment.ps1 -IncludeMediaDependencies`でname／version・database・C/C++/LV2/VAAPI smokeを再確認した。process環境を限定して元autogen／configureを使い、libraryとpkg-configだけをbuild/installする。合成試験は別directoryに比較用exeと限定ZVBI DLLを置き、元FFmpeg prefixとSystem32だけのPATHを使う。開発用DLLや本体を差し替えず、実際にloadしたZVBI pathも各caller内で照合する。

最初の手作業buildとfreshなscript buildが通過した。後者のDLL SHA256は`aaa79ce2955abf6cc8ed94d36158ccc5501e03b5571a91f906e9104596ee1acb`、再生成headerは`8e2a467f9ac02022a147470c868553eb563636b56b67b8f16da4aec8a8a14acc`。原本224 filesとの全hash比較は意図した7 filesだけが異なる。実compiler依存44 recordsに対象GPL header／C fileやLinux向けGPL headerはなく、preprocess後の公開headerにもPDC宣言はない。PE exportは367→348で、差は固定した19個の番組制御・時刻関連だけ、追加0。外部DLLはiconv、intl、png、winpthreadとWindows DLLである。依存DLL内部まで含む最終license監査の証明ではない。

さらに任意cwdから2回目のfresh script buildも完走し、PATH／pkg-config／CFLAGSの復元を確認した。そのDLL hashは`3b18e1282a06b413e75e907d7521aceb0570b93002880df9a15e582902d7766b`であり、異なるpath／時刻のstrip前binaryがbit再現したとは主張しない。生成header hashと全字幕比較結果は一致する。CRLF archive、同size改変source、既存buildの拒否も確認し、入力と先行DLLを保持して拒否先outputを作らなかった。生成headerの元package版との差は7行追加／181行削除で、除外対象、明示time include、生成注記／入力名だけである。

合成字幕は3形式（bitmap／text／ASS）×4条件（通常文字、色・倍高、national subset、mosaic）で、各3回、計36個の空でない字幕更新を要求する。字幕page filter、8/30 format 1／2の混在、字幕文字列の期待値、壊れたdata-unit長の拒否も確認する。元DLL、限定DLL＋旧header caller、限定DLL＋再生成header callerの結果1423076 bytesは完全一致し、SHA256は`c8cc9da776b4ebd34f67bfcf9126983a362ab35d90813a4345674ad3b56eb161`。比較にはbitmapの全pixel／palette、文字列、位置／時間metadataと使った公開構造体のsize／offsetを含む。単なるdecoder名の列挙ではないが、実放送の全page／error-correction／DRCS等の網羅ではない。

初回のテスト素材は同page headerだけでpage終端を作らず、空出力の時点で失敗した。交互pageで終端を作り、字幕boxを含む素材へ直してから元DLLの出力を確認した。ASSのhard-space表現も通常textとは区別して期待値を検証する。build中に判明した上記header生成の不整合、PowerShellのstderr扱い、patch末尾context欠落は修正後にfresh buildで再検証した。失敗出力を合格材料へ混ぜない。

**この段階ではまだ採用しない。** 上記は既存FFmpeg DLLのZVBI境界の試験であり、新headerを入力にしたFFmpeg全体の再buildではなかった。続く実放送素材と全体buildの結果は下記へ記録する。source・patch・noticeの結合と最終候補のmedia／性能確認は維持する。以前の30分soakは旧ZVBIを含む候補の結果のままで、新DLLの性能合格として流用しない。

### 実放送TSによる追加比較

FFmpegの公開sampleをローカル検証用に取得した。sourceと抽出字幕／bitmapはGitにも配布物にも含めない。download元と全byteのSHA256を記録し、比較前後で入力不変を確認する。

| sample | 入力bytes／SHA256 | probe／字幕decode |
|---|---|---|
| [ticket4165/dvbteletext.ts](https://samples.ffmpeg.org/ffmpeg-bugs/trac/ticket4165/dvbteletext.ts) | 10485760／`0a59da7544cb72ab2249c5c96a5188c19307030be57d11c214a5625ca6e82423` | 17.960056秒、stream 3／PID 0xc9、言語metadata swe,fin。各形式442 packets、各10 nonempty rectangles |
| [ticket2086/teletextsubtitles.ts](https://samples.ffmpeg.org/ffmpeg-bugs/trac/ticket2086/teletextsubtitles.ts) | 101043044／`e005a8b9be00b9229c337ab7e028a28c87011e30f4c61d8b1cf076d21725383f` | 55.456133秒、stream 3／PID 0x6a、言語metadata ita,ita,eng。各1361 packets、bitmap／text／ASSで86／28／86 nonempty rectangles |

test wrapperの`-RecordedInput 'path/to/sample.ts'`で、従来の合成12条件に加えて全Teletext packetsとEOF drainを実行する。fixtureはTeletext streamがちょうど1本あることを要求し、`txt_page=*`でpageを収集する。3形式それぞれに非空出力を要求し、packet数・decoder error・pixel／palette・文字列・位置／時刻を同じcomparison fileへ保存する。ZVBIだけでなくavcodec／avformat／avutilの実load pathもC caller内で確認する。

既存FFmpeg＋元ZVBI、同FFmpeg＋限定DLL＋旧header caller、同FFmpeg＋限定DLL＋新header callerの3条件で、両sampleの全出力が一致した。各decoder error数は0。4165は合成分込み2609741 bytes／SHA256 `dc327785c60d8da44bfc3fcfe3ed0f6af21f15e1621ac259bbe69d196cb26399`、2086は11732615 bytes／`153a9b6527fe7944fc28d26cacd9f893c037cd29cde2dc733cb95b7b4b4261c9`。途中開始の映像header警告や終端付近のTS/PES破損警告は残り、decoder error 0を無警告の意味にはしない。3形式の出力数の差も元DLLと同じであり、text形式をbitmapと同一件数だと扱わない。

別の[stream認識sample](https://samples.ffmpeg.org/ffmpeg-bugs/trac/ticket4221/teletext-streams-misrecognized.ts)は7569408 bytes／SHA256 `e4f4f02659c8b0e0d50691e937a9ff0068836d4379246c22f37abd0bb8de52d1`。現在のFFmpegはTeletext streamを認識せず、未知streamとDVB subtitleを報告した。fixtureは明示failureで停止することを確認し、この素材をTeletext decode成功例には数えない。demux認識問題の修正や全言語／DRCS／error-correctionの網羅は今回の比較からは証明されない。

続いて`build-ffmpeg-native.ps1 -ZvbiPrefix ...`による新規FFmpeg全体buildと94-file stagingが完了した。実compiler依存は限定prefixの`libzvbi.h`を参照し、旧MSYS2 headerを含まない。`-CandidateFfmpegPrefix 'path/to/new/ffmpeg/prefix'`を比較wrapperへ追加し、avcodec／avformat／avutilも新しいDLLであることを確認した上で、上記2本を再比較した。合成条件、録画のpacket／非空出力／error数、全出力hashは旧FFmpegと完全一致した。55秒素材の比較は任意cwdからも成功し、PATHと入力は保持された。新FFmpegの構成・配置の詳細は[NATIVE_FFMPEG_BUILD.md](NATIVE_FFMPEG_BUILD.md)を参照する。これは字幕境界の追加証拠であり、全体配布承認や最終候補の長時間性能検証ではない。

### 限定ZVBI候補に対応するsource資料

[native-zvbi-scoped-inputs.json](native-zvbi-scoped-inputs.json)は、実際に新FFmpegへ渡した限定ZVBIの6 inputs（DLL、再生成header、static／import library、libtool metadata、pkg-config）と、固定builder・patch・source／toolchain一覧を結び付ける。collectorは元archiveへ固定patchを再適用し、元sourceの224 filesすべてを実build sourceとsize／hash照合する。変更は意図した7 filesだけであり、未コンパイルの原本も照合対象に含む。さらに6 inputsをFFmpegのbuild記録と照合し、staging先のZVBI DLLも検査する。観測した入力間の一致であって、暗号学的なbuild証明ではない。

```powershell
.\scripts\prepare-zvbi-scoped-materials.ps1 -SourceArchive 'path/to/zvbi-source.tar' `
    -ZvbiBuildDirectory 'path/to/verified/zvbi/build' `
    -FfmpegBuildDirectory 'path/to/verified/ffmpeg/build' -OutputDirectory 'path/to/fresh/materials'
.\scripts\test-zvbi-scoped-materials.ps1 -SourceArchive 'path/to/zvbi-source.tar' `
    -ZvbiBuildDirectory 'path/to/verified/zvbi/build' -FfmpegBuildDirectory 'path/to/verified/ffmpeg/build'
```

最終資料は13 files／4623779 bytes。無変更の全source archive、元COPYING／NEWS／README、固定patch、builderと入力一覧、実生成header、説明書、最後に生成するEVIDENCE.jsonを含む。DLL／exe／static library／録画／字幕出力は含めない。任意cwdとコピー入力からの全hash一致、22 inputsの欠落・同size改変とFFmpeg記録の6不一致条件、計50拒否case、既存output・元入力・PATH／cwd保持を確認した。初回の異常系試験では元metadataの欠落が通常の読込例外となったため、固定repository inputsの存在・hash検査を読込前へ移し、全caseを再実行した。

同梱builderを資料directoryから参照し、別cwd／fresh native build directoryで再buildした。44 compiler依存と公開header検査を通過し、header hashは`8e2a467f...`のまま、DLLは`8a32b48449dc1bc254a2c87a3077a3c55b97b4b166733034148cb81b6e8abae5`となった。19 exportsだけの除外と、合成12条件＋ticket4165の3形式の全出力一致（`dc327785...`）を新FFmpeg／実load path確認付きで再検証した。最終資料とこの再buildに用いた資料の差は説明書の文章だけで、build inputsは全hash一致。bit-identical rebuildや未試験のstream互換性は主張しない。

これは限定ZVBIのsource／patch／header資料の確認であり、元archive全fileがLGPL-onlyという判断ではない。元license noticesを保持し、MSYS2 packages／bootstrap手順は同梱せず固定workflow revisionへ結び付ける。Autotools生成物、system headers、compilerや推移依存のsource／static／data資料の全体確認、新候補のrelease性能、Setup.exeと対象Windowsのgateは未完了である。旧DLLの監査資料も履歴として分離保持し、製品への差替え・配布承認は行わない。

### その他の混合license確認の続き

Little CMSについては、同じ環境のfast_float静的archiveが定義する14個の公開text symbolを取得し、再生成buildのstrip前avcodec／avfilterの全symbol table（65,335／27,148 records）と照合して一致0件、fast_float import markerも0件だった。これは未使用link引数という解釈と整合するが、全推移DLLの静的入力監査やlink traceの代わりにはしない。

**XZ／FreeType／GCC runtime:** XZ 5.8.3の[固定COPYING](https://github.com/tukaani-project/xz/blob/4b73f2ec19a99ef465282fbce633e8deb33691b3/COPYING)はliblzmaの0BSDと、CLI用getoptのLGPL／補助scriptのGPLを区別する。FreeType 2.14.3の[LICENSE.TXT](https://github.com/freetype/freetype/blob/0a0221a1347e2f1e07c395263540026e9a0aa7c7/LICENSE.TXT)はFTL／GPLの選択に加え、BDF／PCF／hash、gzip、HarfBuzz由来fileの別条件を列挙する。FTL側を選ぶ場合も同本文と製品文書のクレジットが必要であり、FTLだけのcopyで全sourceの表示が揃うとは扱わない。GCC 16.2.0-3の元recipeとpackage内READMEはlibgcc／libstdc++／libgomp／libatomicのGCC Runtime Library ExceptionとlibquadmathのLGPLを区別する。現graphで使う3 DLLは前者だが、[例外本文](https://gcc.gnu.org/onlinedocs/libstdc++/manual/license.html)の適用対象／Eligible Compilation Process、対応source、その他の静的・header入力は別に確認する。package全体のGPL labelをそのまま本体アプリのlicenseへ移さない。

この一覧は静的にFFmpegへ組み込んだsource dependencies、header-only code、埋め込みdata、動的LoadLibrary、driverやplugin resourceの完全な一覧ではない。recipe本体が揃っても、参照先archive・patchやMSYS2 build infrastructureは自動的には揃わない。全体build再現、release media/performance、Setup.exeと隔離Windowsでの導入／更新／削除、physical environmentとowner承認のgateを維持する。
