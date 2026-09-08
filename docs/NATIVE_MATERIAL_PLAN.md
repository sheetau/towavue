# Native配布資料の残作業

2026-09-08時点の作業順。配布承認・完成SBOM・法的適合性の認定ではない。対象は[NATIVE_RUNTIME_AUDIT.md](NATIVE_RUNTIME_AUDIT.md)の固定native候補であり、除外済みBtbN binaryの材料を流用して完了扱いにしない。

## 必要条件と調査手段を分ける

同梱するcodeに適用される条件、原本の著作権・license・NOTICE、対応sourceを提供する必要と方法、変更内容、差替え可能性を確認する。**全てのpermissive libraryの全source収集・単体再buildを、一律の法的義務やlaunch gateにはしない。** ただしpackage labelだけでpermissiveと確定せず、取り込みcode・static/header/dataを含む範囲確認が必要な場合はsourceやbuild記録を調べる。資料の生成成功も適合性の証明ではない。

[FFmpeg公式checklist](https://ffmpeg.org/legal.html)は実binaryに対応するsource・build説明・変更と、外部LGPL libraryも含む確認を求める。[実FFmpegのLGPLv3本文](https://github.com/FFmpeg/FFmpeg/blob/e47273f4d9227152dcbf543cebaf9e2430ddbcc4/COPYING.LGPLv3)のheader／combined-work条件と、library自体の頒布条件を区別する。DLLならsource提供が一切不要という判断はしない。元libraryごとのversionや選択条件は別途確定する。

[MPL 2.0の上流説明](https://www.mozilla.org/en-US/MPL/2.0/FAQ/#q8-i-want-to-distribute-outside-my-organization-executable-programs-or-libraries-that-i-have-compiled-from-someone-elses-unchanged-mpl-licensed-source-code-either-standalone-or-part-of-a-larger-work-what-do-i-have-to-do)は対象sourceの取得先案内を求め、libraryだけを再頒布する場合の確認にも言及する。[GCC runtime exception 3.1](https://github.com/gcc-mirror/gcc/blob/releases/gcc-16.2.0/COPYING.RUNTIME)は適用file・Independent Modules・Eligible Compilation Processに条件を置く。別途配るruntime DLLの扱いを、アプリのtarget codeに対する追加許可と混同しない。

## 72 ownerの優先群

[固定package監査](native-runtime-package-audit.json)の72 ownerを重複なく分けた調査queue。名前から`mingw-w64-x86_64-`を省略する。原本を未確認のものも含むため、分類だけで適用条件の確定・採用可否を表明しない。

| 優先群 | 件数 | 次に必要な証拠 |
|---|---:|---|
| LGPL系libraryの対応sourceを優先 | 17 | 原本のversion／選択条件、配布libraryに対応するsource・差分・build条件と提供経路 |
| MPL系 | 2 | 対象source・変更、取得先案内とlibrary再頒布条件 |
| GCC runtime例外 | 1 | 実3 DLLと例外対象fileの対応、compilation条件、DLL自身の資料 |
| library／tool等で条件が異なるもの | 4 | 実際の組込み範囲と選択条件、追加表示 |
| その他のnotice／範囲を先に確認 | 48 | 原本本文・NOTICE・patent/data表示・取り込みcode。source義務の有無はその結果から判断 |

LGPL優先17件: chromaprint、fribidi、gettext-runtime、glib2、gmp、graphite2、lame、libbluray、libgme、libiconv、libplacebo、libsoxr、libssh、mpg123、openal、twolame、zvbi。

17件ともsource archive・patch・選択表示を取得済み（zvbiは限定buildの別資料）。**取得済みは配布条件の完了ではない。** libsoxrはrecipeがchecksumをSKIPするVCS入力のため、指定commitから生成したtarを独自にhash固定し、通常file全137件をGit blobと照合した。PFFFTの別条件も保持する。Graphite2の原本にはSIL由来部分の複数license選択があり、この優先群はLGPL-only認定ではない。FriBidiの4 Unicode inputsは固定upstreamと一致し、そのUnicode License V3原本を追加した。libblurayの内蔵libudfreadはstatic選択とLGPL原文、native BD-JのJNIは複数license選択を確認・保持した。libplacebo等の残るstatic/header/generated入力や最終取得案内を続ける。

MPL群はsrtとzeromqで、両方のsource／patchを取得済み。ZeroMQの逆向きpatchと、SRTのUniversity of Illinois継承表示を保持し、取得案内と個別範囲の確認を続ける。GCC群のgcc-libsも元16.2.0 source・17 patches／gdbinit・package表示を別資料に取得し、実graphのlibgcc／libstdc++／libgompの3 DLLへhash対応させた。例外条件と全compiler入力の確定は別に残る。未同梱のlibquadmathを同じ扱いで加えない。次は残るstatic/header/dataの範囲・表示を先に確認する。

混合scope群はfreetype、lcms2、lz4、xzで、原本材料は4件とも取得済み。lcms2は追加の隔離avcodec／avfilter再linkでfast_floatのimport archiveから選択member 0、static実装archiveの選択なしを確認し、両DLLの.text／.rdataが現候補と完全一致した。これで当該link引数の疑問は狭められたが、全runtimeのstatic/header/dataや最終配布条件の完了にはしない。残る48件の表示・取り込みscopeと公開資料のまとめを進める。

notice／範囲優先48件は上記24 owner以外の全件。package noticeが存在するだけで完了にしない。この群のopencore-amrはAMR subsetのsource一覧と3GPP由来header、Snappyはtest dataとcore targetの分離を確認し、継承NOTICEとsource archive内のdata表示を保持した。rav1e／libdoviは元sourceと内蔵Rust依存の157 crate archives・選択文書を取得し、元lock checksumへ対応させた。Rust 1.87.0／1.97.0の元MSYS2 package表示と標準library sourceも取得・署名検証したが、両版のlibrary著作権一覧は外部依存欄が空だったため、その一覧だけで完了としない。1.87 source lockの全42 cratesと省略されたcompiler-builtins／libm原本を補い、別kitに保持した。今回のMSVC-host metadataは過去のGNU-host unit graphではなく、他targetの材料も含む。次にGNUの実build範囲・個別表示と、shaderc／SPIR-V／Vulkan等の取り込みsource・生成dataを確認する。OpenH264を含むcodecの特許・商標判断は著作権license確認と別に残す。

続く[GNU-host選択確認](native-rust-gnu-selection.json)では、元MSYS2 Rust／Cargoをinstallせずに隔離実行した。元lockのmetadata到達集合は129／28件のまま、library限定unit graphは121／28件（target library unitあり65／23件、hostのみ56／5件）だった。C API crate typesへの変更でも選択集合は不変。compileやbuild scriptは実行せず、既存157件の材料は保持する。これは元build hostの違いを狭める観測であり、歴史的cargo-c／link範囲や生成codeの表示を確定するものではない。次は個別表示・標準library／static／header／生成dataとshaderc／SPIR-V／Vulkanの範囲を進める。

shaderc／SPIRV-Cross／Vulkan Loaderの3 source setsも元recipeのhashへ対応させ、32-owner supplementへ追加した。Vulkan Loader内のcJSON MITとWindows direntのHPND原文・追加著作権、SPIRV-Crossの複数選択と生成header表示を保持する。shadercはMinGWで外部shader librariesとGCC runtimeをstaticに取り込む設定なので、PEの2 system importsだけでは範囲が閉じない。次の固定入力は元buildのglslang 16.3.0-1、SPIRV-Tools 3~1.4.357.0-1、SPIRV-Headers 2~1.4.357.0-1、GCC 16.1.0-5と、LoaderのVulkan-Headers 1~1.4.357.0-1。既に収集した別版runtime DLL用資料をそのまま代用しない。

続く[4 shader static／header inputs](native-shader-inputs.json)では、上記glslang／SPIRV-Tools／SPIRV-Headers／Vulkan-Headersの元package署名・recipeとsource／patch hashを照合した。生成Bison parserの例外、NVIDIA・Khronos・jsoncpp・Paul McGuire由来の原文とscopeを保持する。追加runtime ownerには数えない。glslang自体は旧SPIRV-Tools 3~1.4.350.1-1でbuildされており、そのheader／static scopeとGCC 16.1.0-5は別の残作業である。全compilerを無条件に再buildするのではなく、実際の取り込み範囲と必要な表示を先に狭める。

旧SPIRV-Tools 350.1も元package署名と`.BUILDINFO`一致recipe・source hashを確認した。glslangのwrapperが読む公開headerと、そのpackage内4 headerすべての元sourceとの一致を保存する。これらのincludeは標準C/C++ headerと相互参照で、旧SPIRV-Headers grammar dataは直接includeしない。5-input kitへ原本とpackage表示を追加した。これは旧Toolsの実装を最終shadercへ二重にリンクした証明ではなく、そのbuild-time listから旧toolchain全体を無条件に追加することもしない。GCC 16.1.0-5の実static／standard-header表示と、残る個別表示・source取得案内を続ける。

GCC／GCC-libs 16.1.0-5も両元packageの署名、同一recipe hash、元GNU sourceと14 patch／build inputsを確認し、[専用kit](native-gcc-static-inputs.json)へ保持した。GNU source署名自体は未検証である。10標準headerのinstalled／source byte一致と生成target headerを区別し、GCC例外に加えてHP／SGI／Boost由来表示、libbacktrace／PSTLの原文を保持する。67 files／103717496 bytesの生成・異常入力試験は通過したが、archiveの285／4／199 member一覧は実link選択の証明ではない。残る実static／MinGW／CRT／intrinsicの範囲を狭め、必要なnoticeとsource取得案内をまとめる。GCC資料の取得だけで配布承認や全toolchainの再build義務を導かない。

## Owner一覧の外にある入力

- 現候補は元85 package DLLのうちZVBIだけが限定build。84 package DLLの71 ownerと、限定ZVBIの由来を区別する。元packageの72 owner一覧は追跡用のbaselineである。
- FFmpeg本体、aribb24／LCEVC／librist／uavs3d／vvencの5 source prefixes、限定ZVBI、compiler／MinGW headers・static runtimeは別の入力である。PE graphだけでこれらが全て列挙されるとは扱わない。
- towavueのCargo.lockとRust runtimeの資料を、FFmpeg依存内のRust codeへ代用しない。Visual C++ runtimeの頒布条件とWindows system DLLの扱いも別に維持する。

必要な原本材料と実binaryへの対応を確定した後、公開するsource／noticeセットと取得案内を一つにまとめる。helper探索のbundle優先・別版混在防止は既に実装し、環境変数なしのdebug実windowでpreview／保存／再open／detachを確認した。新候補のrelease media／性能、Setup.exe、対象Windowsの導入／更新／削除、実入力・device・owner外観受入は引き続き未完了。資料監査だけでH1やlaunchの完了条件を置き換えない。
