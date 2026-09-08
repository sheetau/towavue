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

notice／範囲優先48件は上記24 owner以外の全件。package noticeが存在するだけで完了にしない。この群のopencore-amrはAMR subsetのsource一覧と3GPP由来header、Snappyはtest dataとcore targetの分離を確認し、継承NOTICEとsource archive内のdata表示を保持した。次に優先するのはrav1e／libdoviの内蔵Rust依存、shaderc／SPIR-V／Vulkan等の取り込みsourceと生成dataである。OpenH264を含むcodecの特許・商標判断は著作権license確認と別に残す。

## Owner一覧の外にある入力

- 現候補は元85 package DLLのうちZVBIだけが限定build。84 package DLLの71 ownerと、限定ZVBIの由来を区別する。元packageの72 owner一覧は追跡用のbaselineである。
- FFmpeg本体、aribb24／LCEVC／librist／uavs3d／vvencの5 source prefixes、限定ZVBI、compiler／MinGW headers・static runtimeは別の入力である。PE graphだけでこれらが全て列挙されるとは扱わない。
- towavueのCargo.lockとRust runtimeの資料を、FFmpeg依存内のRust codeへ代用しない。Visual C++ runtimeの頒布条件とWindows system DLLの扱いも別に維持する。

必要な原本材料と実binaryへの対応を確定した後、公開するsource／noticeセットと取得案内を一つにまとめる。新候補のrelease media／性能、開発環境なしのhelper探索、Setup.exe、対象Windowsの導入／更新／削除、実入力・device・owner外観受入は引き続き未完了。資料監査だけでH1やlaunchの完了条件を置き換えない。
