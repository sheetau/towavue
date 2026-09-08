# Native資料の案内と集約

取得済みの原本を一つのdirectoryから辿るための、offline review用catalog。**完成した配布資料・配布承認・公開済みsource取得先ではない。** アプリやDLL、インストーラーは生成しない。

## 生成と読み方

検証済みkitを`target/distribution`へ用意した上で、未作成の出力先を指定する。入力directoryの内部や親へは出力しない。

```powershell
.\scripts\prepare-native-material-catalog.ps1 -MaterialsDirectory 'target/distribution' -OutputDirectory 'target/native-material-catalog-v1'
.\scripts\test-native-material-catalog.ps1 -MaterialsDirectory 'target/distribution'
```

packageとrecipeの既定cacheは`vendor/msys2/packages-20260908`と`vendor/msys2/runtime-recipes-20260908`。別配置では`-PackageDirectory`と`-RecipeDirectory`を指定する。取得・install・recipe実行はせず、不足・改変cacheを上書き修復しない。

出力の`README.md`が入口で、10 kitの説明書、sourceの読み方と残作業を案内する。`PACKAGES.md`は71 ownerの元build記録・recipe・79 package noticesへ直接リンクする。packageに表示がない場合は欠落を明示し、source資料の確認へ誘導する。packageのlicense labelだけで最終適用条件を分類しない。

| 出力 | 内容 |
|---|---|
| `materials/` | 固定した10 kitを原文のままコピー。各説明書・入力一覧と全source archivesを保持 |
| `packages/` | 71 ownerの`.BUILDINFO`・`.PKGINFO`・元PKGBUILDと、収録されている原文表示 |
| `README.md`、`PACKAGES.md` | 相対pathによる案内。公開download URLを捏造せず、upstream recipe URLとlocal資料を区別 |
| `native-runtime-package-audit.json`、`native-runtime-recipes.json` | 元72-owner baselineとrecipe由来の記録。現在採用済みruntime一覧ではない |
| `FILES.json` | コピーした全fileと案内のsize／SHA256。自分自身とcompletion markerは含めない |
| `CATALOG.json` | 最後にコピーする固定catalog。途中失敗した出力には生成しない |

旧ZVBI packageは`packages/`から除く。限定ZVBIのsource・patch・builder・生成headerは専用kitを使い、古いbaselineの旧DLL記録をその由来へ流用しない。初期のOpenAL／shader／supplement kitや、除外済みBtbN候補の資料も選択しない。

## 照合の範囲

[固定catalog](native-material-catalog.json)は各kitのfile数・byte数と全tree digestを保持する。digestは相対path、size、SHA256をOrdinal順・UTF-8／LFで結合したもので、名前変更・欠落・追加・同size改変を区別する。元kit 2091 files／656052751 bytesを固定し、コピー後も再検査する。package／recipeは既存の固定hashと対応を検査してから、監査済み通常memberだけを展開する。kit内のreparse pointを辿らない。

初回の集約は2389 files／658443498 bytes。これは資料のサイズであり、インストーラーのサイズではない。原本source archivesを保持するが、runtime DLL／exe／static libraryを別fileとしてコピーしない。source-onlyのtoolや他targetの条件を、本体へ一律適用する表示にはしない。

別cwd／反復生成、全file一致と案内のlocalリンク、142 package／recipe入力の欠落・改変pairs、10 kitの名前変更・改変pairs、5 manifest不整合、途中copy失敗時のmarker保護と入力／既存output保持を検証した。原文の`COPYING.LIB`をlibrary binaryと混同しないよう、試験では拡張子に加えて実際の形式を確認する。全2091 kit原本と最終出力の照合、format／全target Clippy／270 testsも通過し、3 live ignoresは未実行。

## まだ結合していないもの

1. 限定ZVBI候補に対応するFFmpeg本体のsource・実build設定と、別buildしたaribb24／LCEVC／librist／uavs3d／vvenc。
2. towavue自身のlicense、Rust依存・font表示、MSVC Rust標準library資料。除外済みBtbN内の別Rust版を現候補へ混ぜない。
3. 個別に残るhistorical／static／header／dataの表示・適用範囲と、必要な原本の不足確認。
4. 許諾されたVisual C++ runtime前提installerと、最終releaseに対応するsource／noticeの提供経路。

資料が集約できても、runtime採用、インストール先を選べるSetup.exe、対象Windowsでの導入・更新・削除、実環境とowner受入のgateは残る。全permissive sourceの再buildを一律に要求する工程へは変更しない。全体の優先順は[NATIVE_MATERIAL_PLAN.md](NATIVE_MATERIAL_PLAN.md)、実行物側のgateは[DISTRIBUTION.md](DISTRIBUTION.md)を参照する。
