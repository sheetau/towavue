# Native FFmpegの対応資料

限定ZVBIを使う候補FFmpegと、個別にnative buildしたaribb24／LCEVC／librist／uavs3d／VVenCの資料。WSLを使わず、実行物の差替えやinstaller作成は行わない。

## 生成と照合

[固定入力一覧](native-ffmpeg-material-inputs.json)にある6 archivesと6 patchesを一つの入力directoryへ置く。5つのGit tarは固定commitから、記録されたLF指定・prefixで生成したもので、元recipeのchecksumや署名検証を主張しない。ARIBの上流patchも元のLF版を使う。

```powershell
.\scripts\prepare-native-ffmpeg-materials.ps1 -SourceDirectory 'path/to/source-inputs' -FfmpegBuildDirectory 'path/to/verified-ffmpeg-build' -OutputDirectory 'target/distribution/native-ffmpeg-materials-v1'
.\scripts\test-native-ffmpeg-materials.ps1 -SourceDirectory 'path/to/source-inputs' -FfmpegBuildDirectory 'path/to/verified-ffmpeg-build'
```

入力を取得・修復したり、compiler／installerを実行したりしない。実buildの2記録と19 repository資料もsize／hashで照合し、記録された130 prefix filesと候補runtime 94 filesが現物に一致することを確認する。出力は未作成かつ入力と重ならないdirectoryに限定する。

6 archivesの通常file・directoryだけを隔離directoryへ展開し、必要な原文を保存した後に6 patchesを再適用する。計12,509 regular filesのpath／size／SHA256 treeが、実buildの元sourceと照合済みの固定値に一致することを検査する。元source・patch・build出力は変更しない。FFmpegは10,422、ARIBは24、LCEVCは676、libristは485、uavs3dは112、VVenCは790 files。これは生成物やsystem headersまで含む完全な入力閉包の証明ではない。

## 出力と範囲

60 files／62,342,607 bytesのkitに、変更していない全source archives、全patches、24 original notices／関連source、native buildersとfixtures／固定入力、説明書を保持する。`BUILD.json`は実際の87 configure flags・330 installed packages・130 prefix inputsを、`RUNTIME.json`は94実行fileのhashとimport名・種別を記録する。machine固有pathだけを明示的placeholderへ変換し、元記録のhashは`INPUTS.json`へ残す。最後の`EVIDENCE.json`が検査完了marker。DLL／exe／static library自体はコピーしない。

ARIBのLGPLv3本文と個別headerのLGPL-2.1-or-later、LCEVCのCOPYINGとClear BSD、librist内のMbedTLS 3.6.6／cJSON、VVenCのVTM／SIMDe／JSON原文を省略しない。旧recipe調査のMbedTLS 4.2を、このlibristの実入力と混同しない。全sourceを単一licenseへ分類せず、patent clearanceも主張しない。

詳しいnative再build入口とpatch順序は[同梱説明書](../third-party/FFMPEG-NATIVE-MATERIALS-README.txt)を参照する。5依存builderは本物の固定Git HEADを必要とし、一部version生成もGit依存であるため、tarだけからのoffline再buildを実証済みとは扱わない。限定ZVBIは別kitが必要。再build結果のbit一致や新たな品質試験の成功を資料生成から推定しない。

このkitは[Native catalog](NATIVE_MATERIAL_CATALOG.md)の11組目に追加する。towavueのRust／font／MSVC表示、残る個別static／header／data範囲、最終releaseからのsource取得、VC runtime、Setup.exeと対象Windows／実環境／owner受入は別の未完了事項。

2026-09-08の試験では、別cwd／反復生成の全file一致、33入力の欠落・同size改変pairs、5 manifest不整合、元入力・既存output・process環境の保持を確認した。最終60 filesも試験出力と一致する。初回の検査は`https://`をWindows drive pathと誤認したため境界判定を修正し、全試験を再実行した。これは資料内の実machine path残留ではなかった。
