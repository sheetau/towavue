# 本体とNative資料の案内・集約

取得済みの原本を一つのdirectoryから辿るための、offline review用catalog。**完成した配布資料・配布承認・公開済みsource取得先ではない。** アプリやDLL、インストーラーは生成しない。

## 生成と読み方

検証済みkitを`target/distribution`へ用意した上で、未作成の出力先を指定する。入力directoryの内部や親へは出力しない。

```powershell
.\scripts\prepare-native-material-catalog.ps1 -MaterialsDirectory 'target/distribution' -OutputDirectory 'target/native-material-catalog-v3'
.\scripts\test-native-material-catalog.ps1 -MaterialsDirectory 'target/distribution'
```

packageとrecipeの既定cacheは`vendor/msys2/packages-20260908`と`vendor/msys2/runtime-recipes-20260908`。別配置では`-PackageDirectory`と`-RecipeDirectory`を指定する。取得・install・recipe実行はせず、不足・改変cacheを上書き修復しない。

出力の`README.md`が入口で、本体とnativeの12 kitの説明書、sourceの読み方と残作業を案内する。`PACKAGES.md`は71 ownerの元build記録・recipe・79 package noticesへ直接リンクする。packageに表示がない場合は欠落を明示し、source資料の確認へ誘導する。packageのlicense labelだけで最終適用条件を分類しない。

| 出力 | 内容 |
|---|---|
| `materials/` | 固定した12 kitを原文のままコピー。各説明書・入力一覧と収集済み原文／source archivesを保持 |
| `packages/` | 71 ownerの`.BUILDINFO`・`.PKGINFO`・元PKGBUILDと、収録されている原文表示 |
| `README.md`、`PACKAGES.md` | 相対pathによる案内。公開download URLを捏造せず、upstream recipe URLとlocal資料を区別 |
| `native-runtime-package-audit.json`、`native-runtime-recipes.json` | 元72-owner baselineとrecipe由来の記録。現在採用済みruntime一覧ではない |
| `FILES.json` | コピーした全fileと案内のsize／SHA256。自分自身とcompletion markerは含めない |
| `CATALOG.json` | 最後にコピーする固定catalog。途中失敗した出力には生成しない |

旧ZVBI packageは`packages/`から除く。限定ZVBIのsource・patch・builder・生成headerは専用kitを使い、古いbaselineの旧DLL記録をその由来へ流用しない。初期のOpenAL／shader／supplement kitや、除外済みBtbN候補の資料も選択しない。

## 照合の範囲

[固定catalog](native-material-catalog.json)は各kitのfile数・byte数と全tree digestを保持する。digestは相対path、size、SHA256をOrdinal順・UTF-8／LFで結合したもので、名前変更・欠落・追加・同size改変を区別する。元kit 2350 files／764630897 bytesを固定し、コピー後も再検査する。package／recipeは既存の固定hashと対応を検査してから、監査済み通常memberだけを展開する。kit内のreparse pointを辿らない。

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

## まだ残るもの

1. 個別に残るhistorical／static／header／dataの表示・適用範囲と、必要な原本の不足確認。
2. 許諾されたVisual C++ runtime前提installerと、最終releaseに対応するsource／noticeの提供経路。

資料が集約できても、runtime採用、インストール先を選べるSetup.exe、対象Windowsでの導入・更新・削除、実環境とowner受入のgateは残る。全permissive sourceの再buildを一律に要求する工程へは変更しない。全体の優先順は[NATIVE_MATERIAL_PLAN.md](NATIVE_MATERIAL_PLAN.md)、実行物側のgateは[DISTRIBUTION.md](DISTRIBUTION.md)を参照する。
