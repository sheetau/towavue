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
