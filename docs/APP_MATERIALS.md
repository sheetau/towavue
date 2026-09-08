# 本体のライセンス・Rust・フォント資料

現候補のtowavue.exeに対応するoffline review用kit。MIT／Apache-2.0の原文、Cargo.lock固定の146 normal／build dependenciesと埋込みfontの表示、Rust 1.98.0／MSVCの18原文を一つにまとめる。アプリ本体・Windows font・Microsoft runtimeはコピーしない。

## 生成

```powershell
.\scripts\prepare-rust-notices.ps1
.\scripts\prepare-rust-runtime-notices.ps1 -Scope towavue
.\scripts\prepare-app-materials.ps1 -RustNotices 'target/distribution/RUST-THIRD-PARTY-NOTICES.txt' -RuntimeNotices 'target/distribution/TOWAVUE-RUST-RUNTIME-NOTICES.zip' -Executable 'path/to/candidate/towavue.exe' -OutputDirectory 'target/distribution/app-materials-v1'
.\scripts\test-app-materials.ps1 -RustNotices 'target/distribution/RUST-THIRD-PARTY-NOTICES.txt' -RuntimeNotices 'target/distribution/TOWAVUE-RUST-RUNTIME-NOTICES.zip' -Executable 'path/to/candidate/towavue.exe'
```

Rust依存資料は既存の固定Cargo cache／上流原文を使う。runtime資料は固定rustc／rust-src archivesを使い、不足時の取得は明示した`-Download`だけで行う。app collector自体はnetwork／compiler／installerを使わず、不足・改変入力を修復しない。既存outputは上書きせず、失敗時に完成markerを作らない。

`-Scope towavue`は本体用2 archivesだけを読み、18文書とREADME／INPUTSを持つZIPを生成する。従来の既定出力は履歴比較用の36文書のまま維持し、別のfilenameを使う。旧BtbN向け1.97.1／GNUのarchiveがcacheになくても本体用を生成でき、ZIPの原文とmetadataにも混ぜない。現native rav1e／libdoviのRust資料は別kitである。

## 照合と限界

[固定一覧](app-material-inputs.json)は本体exe、2 notice files、6 repository資料と元runtime inventoryをsize／hashへ固定する。Cargo.lockと依存一覧、toolchain versionとMSVC target、ZIP内18文書の原文hash、選択した2 archiveの由来を照合する。`INPUTS.json`と最後の`EVIDENCE.json`を含む11 files／3,356,087 bytesのkitを[共通catalog](NATIVE_MATERIAL_CATALOG.md)へ結ぶ。

146依存の資料にはepaint_default_fontsのHack／OFL／UFL／emoji-icon-fontの4原文と、Unicode・inline・上流補足も含む。normal／build dependency一覧を実link一覧とは扱わない。OSから読む日本語fallback fontは同梱しない。標準libraryの全SPDX辞書を保持することも、全licenseの適用や完全なstatic入力範囲の証明ではない。RustのMSVC target名から、Visual C++ runtimeの配布許諾を導かない。

2026-09-08、本体kitは別cwd／反復の全hash一致、10入力の欠落・同size改変pairs、6 mapping不整合、元入力／既存output／process環境保持を確認した。Rust依存資料の全146項目・原文・補足・font検査と、runtime資料の旧36文書／本体18文書・2入力の欠落／改変・旧版不要の検査も通過。runtime生成の相対outputがPowerShellのcwdと異なる場所へ出る問題を新試験で検出し、provider基準の絶対path解決へ修正して全再試験した。

本体kitの完成は配布承認ではない。個別に残るnative scope、VC前提installer、最終releaseからのsource／notice提供、候補品質、Setup.exeと対象Windowsの導入／更新／削除、実環境・owner受入は未完了のまま保持する。
