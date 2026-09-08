# 本体とNative資料の案内・集約

取得済みの原本を一つのdirectoryから辿るための、offline review用catalog。**完成した配布資料・配布承認・公開済みsource取得先ではない。** アプリやDLL、インストーラーは生成しない。

現行v11はHelp対応exeへ更新したapp-materials-v2を選ぶ。2875 files／842769539 bytesで、`FILES.json`のSHA256は`8a0fb5b2a8a9b8687f88d3e84a5b3101f2d40e9bb33f0c7f86ddd2dff8517c1f`。12 native kitsと本体の9原本は変えず、本体のidentity記録と集約先だけを更新する。旧v10は履歴として残す。以下の各版の数値はその時点の記録である。

## 生成と読み方

検証済みkitを`target/distribution`へ用意した上で、未作成の出力先を指定する。入力directoryの内部や親へは出力しない。

```powershell
.\scripts\prepare-native-material-catalog.ps1 -MaterialsDirectory 'target/distribution' -OutputDirectory 'target/native-material-catalog-v11'
.\scripts\test-native-material-catalog.ps1 -MaterialsDirectory 'target/distribution'
```

packageとrecipeの既定cacheは`vendor/msys2/packages-20260908`と`vendor/msys2/runtime-recipes-20260908`。別配置では`-PackageDirectory`と`-RecipeDirectory`を指定する。取得・install・recipe実行はせず、不足・改変cacheを上書き修復しない。

出力の`README.md`が入口で、本体とnativeの13 kitの説明書、sourceの読み方と残作業を案内する。`PACKAGES.md`は71 ownerの元build記録・recipe・79 package noticesへ直接リンクする。packageに表示がない場合は欠落を明示し、source資料の確認へ誘導する。packageのlicense labelだけで最終適用条件を分類しない。

| 出力 | 内容 |
|---|---|
| `materials/` | 固定した13 kitを原文のままコピー。各説明書・入力一覧と収集済み原文／source archivesを保持 |
| `packages/` | 71 ownerの`.BUILDINFO`・`.PKGINFO`・元PKGBUILDと、収録されている原文表示 |
| `README.md`、`PACKAGES.md` | 相対pathによる案内。公開download URLを捏造せず、upstream recipe URLとlocal資料を区別 |
| `native-runtime-package-audit.json`、`native-runtime-recipes.json` | 元72-owner baselineとrecipe由来の記録。現在採用済みruntime一覧ではない |
| `FILES.json` | コピーした全fileと案内のsize／SHA256。自分自身とcompletion markerは含めない |
| `CATALOG.json` | 最後にコピーする固定catalog。途中失敗した出力には生成しない |

旧ZVBI packageは`packages/`から除く。限定ZVBIのsource・patch・builder・生成headerは専用kitを使い、古いbaselineの旧DLL記録をその由来へ流用しない。初期のOpenAL／shader／supplement kitや、除外済みBtbN候補の資料も選択しない。

## 照合の範囲

[固定catalog](native-material-catalog.json)は各kitのfile数・byte数と全tree digestを保持する。digestは相対path、size、SHA256をOrdinal順・UTF-8／LFで結合したもので、名前変更・欠落・追加・同size改変を区別する。元kit 2577 files／840260390 bytesを固定し、コピー後も再検査する。package／recipeは既存の固定hashと対応を検査してから、監査済み通常memberだけを展開する。kit内のreparse pointを辿らない。

本体kitを加えたv3の集約は2460 files／724157839 bytes。これは資料のサイズであり、インストーラーのサイズではない。原本source archivesを保持するが、runtime DLL／exe／static libraryを別fileとしてコピーしない。source-onlyのtoolや他targetの条件を、本体へ一律適用する表示にはしない。

libplaceboのfast_float／xxHash／glad／旧Vulkan-Headersを加えたv4は2552 files／737682203 bytes。9-input shader kitへ実packageと元sourceの対応を保持し、生成器・生成code・Khronos data／headerの個別表示を辿れる。元12 kitという構成と71 runtime package ownerは変えず、4つの入力を追加runtime DLLと数えない。`FILES.json`のSHA256は`a0a6bbbba4a2ee2ce28cae9cc5985d06788a8017d5092a2a706bc56e5f935722`。VC前提条件は版・読み取り専用判定を確認済みとして残項目を更新し、実導入／terms UIと最終source提供は未完了のまま区別する。

v4でも下記の全catalog回帰が通過し、最終出力と試験出力の2552 filesは全hash一致。追加shader入力の試験は41欠落／改変pairs、8 mapping不整合と途中失敗時のmarker保護を確認した。全targetの270 tests・format・Clippyも通過し、未実行のlive環境試験を合格とは数えない。

PCRE2／libxml2の個別原文を加えたv5は2584 files／744514967 bytes、`FILES.json`のSHA256は`93182f1b75a0572e428e179fd39464da6e237ac5aa4038f228b986768319b71e`。34-owner supplementの元source・patchとSLJIT／dict／list／html5lib表示を保持する。[35 owner原文レビュー](NATIVE_NOTICE_REVIEW.md)はpackage labelとの差と、残るUnicode／内蔵code等の確認を区別する。資料の増加はruntimeの増加や配布承認ではない。

v5の全catalog回帰も通過し、最終2584 filesは試験outputと全hash一致。34-owner supplementでは123入力の欠落／改変pairsと33件のpackage表示不整合を拒否し、既存425 filesを保持した。format／全target Clippy／270 testsも通過したが、live ignoresと最終候補の環境試験は別に残る。

回帰試験は別cwd／反復生成、全file一致と案内のlocalリンク、142 package／recipe入力の欠落・改変pairs、12 kitの名前変更・改変pairs、5 manifest不整合、途中copy失敗時のmarker保護と入力／既存output保持を検査する。原文の`COPYING.LIB`をlibrary binaryと混同しないよう、試験では拡張子に加えて実際の形式を確認する。

[本体資料](APP_MATERIALS.md)はMIT／Apache-2.0原文、146 Rust依存／font表示と、Rust 1.98.0／MSVCだけの18文書ZIPを加える。旧BtbN向けRust 1.97.1／GNUの文書を同梱せず、現native Rustの別kitとも区別する。RustのMSVC targetとMicrosoftのVC redistributableの条件は同一ではない。

2026-09-08、v3の全回帰試験が通過し、最終2460 filesも試験出力と全hash一致。format／全target Clippy／270 testsも通過し、3 live ignoresは未実行。本体exeと94 runtime hashesは変更していない。

[FFmpeg対応資料](NATIVE_FFMPEG_MATERIALS.md)は、限定候補のFFmpeg本体と5 source prefixesの元source・全patch・builder・原文表示、実build／runtime記録を加える。全12,509 original source filesのpatch後照合は、残る生成物・歴史的static入力の完全性や、tarだけからのoffline再buildまで証明するものではない。

fontconfig／HarfBuzz／libunibreakの補完と現行Unicode noticeを加えたv6は2648 files／767083882 bytes、`FILES.json`のSHA256は`f1357c73ef3029af346994b2515c79c5780d44f2d7ba099ca53fbed082ecc250`。37-owner supplement v13は523 files／119960748 bytesで、前v12の457 files（README／INPUTS以外）は不変。新たな原本archive・recipe inputsは8件／20953323 bytes、選択文書は52件。版の混在と別条件は[font data review](NATIVE_FONT_DATA_AUDIT.md)へ記録し、外部dataの最終提供・個別embedded-code／NOTICE・配布gateは完了扱いにしない。

v6の全catalog回帰が通過し、最終2648 filesは試験outputと全hash一致。supplementでは134入力の欠落／改変pairs、42 package-notice不整合、2 Unicode原本の欠落／改変と一覧除去、3 VCS不整合を検証した。format／全target Clippy／270 testsも通過したが、3 live ignoresは未実行で新たなhardware経路の証明ではない。

OpenSSL／OpenCL／libvaの作者と別条件を加えたv7は2722 files／824237414 bytes、`FILES.json`のSHA256は`748d455ab62456ab11e418c9ae7f37d4beec1629a8d719d78e57fed132746358`。40-owner supplement v14は597 files／177095543 bytes、前v13の521 files（README／INPUTS以外）は不変。3 source archivesと6 patch/source inputs、62選択原本を追加し、根拠と限界は[platform notice review](NATIVE_PLATFORM_NOTICE_AUDIT.md)へ記録する。表示の補完であり、候補runtimeの変更や外部driverの配布承認ではない。

v7も全catalog回帰を通過し、最終2722 filesは試験outputと全hash一致。supplementの146入力の欠落／改変pairsと51 package-notice不整合、追加Unicode原本／VCS／link除外／既存output保護も通過した。format／全target Clippy／270 testsは成功し、3 live ignoresは未実行のまま区別する。

libjxl／libopenmptの別表示を加えたv8は2761 files／828134838 bytes、`FILES.json`のSHA256は`682a8c41ad65b6385e68f6614f1a89bbb1ad9eec8982ea85ccaac6050a8d9ee7`。42-owner supplement v15は636 files／180983078 bytesで、前v14の595 files（README／INPUTS以外）は不変。2 source archives、32選択fileとxorshift／Vector Class／TinyFFTの3原文表示を追加した。[内蔵code確認](NATIVE_CODEC_EMBEDDED_AUDIT.md)に使用箇所と参照版の限界を記録する。最終出力は試験生成物と全hash一致し、候補exe／94 runtime filesも不変。

v8も全catalog回帰を通過し、supplementの150入力の欠落／改変pairs、追加5原文の欠落／改変・一覧除去、両ownerを含むpackage表示不整合、VCS／link除外／既存output保護を確認した。新規原文のfixtureコピー漏れは試験側で修正して再実行した。format／全target Clippy／270 testsも通過したが、3 live ignoresは未実行で、配布候補の新しいhardware試験とは数えない。

libpng／libwebpとOpenCL Headersを加えたv9は2844 files／835133390 bytes、`FILES.json`のSHA256は`b2cac19054e2910ea234bbe1bb806d2f976b29bc090f7962f98b68e32b808f96`。44-owner supplement v16は675 files／186988193 bytes、10-input static/API-header kit v4は225 files／37697930 bytes。前版の634／179 files（各README／INPUTS以外）は不変で、最終3出力はそれぞれ試験生成物と全hash一致した。

sourceの156入力欠落／改変pairs、headerの46 pairs・9 mapping不整合、catalogの全回帰が通過した。M0のformat／全target Clippy／270 testsも成功し、3 live ignoresは未実行。候補exeと94 runtime hashesは不変で、[画像library／API headerの原文・根拠](NATIVE_IMAGE_HEADER_NOTICE_AUDIT.md)を辿れる。次は外部dataも含むsource／noticeの最終提供を組み立てる。

外部Unicode dataとPCRE2生成scriptの[補助kit](NATIVE_DATA_MATERIALS.md)を加えたv10は2875 files／842769539 bytes、`FILES.json`のSHA256は`202d8a61a2de59685ba727d11c3094f75e36222a1a3a9744361d61b68fbaaab7`。新kitは31 files／7628780 bytesで、22 data filesと3 scripts、元表示・使用版の記録を含む。既存12 kitの中身は変更せず、13個目として統合する。これはローカル資料への収録であり、公開downloadや最終実行物への対応付けの完了ではない。

v10の全catalog回帰と、data kitの欠落／改変・source対応・link／overlap・途中失敗marker保護が通過した。最終31／2875 filesは各試験出力と全hash一致し、既存12 kitも不変。M0のformat／全target Clippy／270 testsは成功、3 live ignoresは未実行で新しいhardware確認とは数えない。

## まだ残るもの

名指ししたnotice／dataの原本収録は完了し、[候補との対応付け](CANDIDATE_MATERIALS.md)へ進んだ。過去のkitに残るhistorical／other-target／再生成環境の限界は、その検証範囲の記録として維持する。全libraryを再監査する未処理一覧へ読み替えない。

アプリ内のHelp／palette入口は[後続UI build](DEVELOPMENT.md)で実装した。残るのは、最終exeと資料の再対応付け、installerでの案内配置とHTML実表示、最終releaseに対応するsource／noticeの同時提供、許諾されたVisual C++ runtime前提installerと導入検証である。

資料が集約できても、runtime採用、インストール先を選べるSetup.exe、対象Windowsでの導入・更新・削除、実環境とowner受入のgateは残る。全permissive sourceの再buildを一律に要求する工程へは変更しない。全体の優先順は[NATIVE_MATERIAL_PLAN.md](NATIVE_MATERIAL_PLAN.md)、実行物側のgateは[DISTRIBUTION.md](DISTRIBUTION.md)を参照する。
