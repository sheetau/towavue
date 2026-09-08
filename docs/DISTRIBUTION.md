# インストーラー配布の準備

2026-09-07のowner指定により、インストール先を選べるSetup.exeを目標とする。これは配布前の技術・資料監査であり、公開可能なpackageや法的適合性の認定ではない。現行のRust構成とFFmpeg動的リンクを維持する。H1の品質確認も継続する。

2026-09-08更新: **下記の固定開発binaryは配布候補から除外する。** Chromaprint経由でGPLのFFTWが静的リンクされていた。recipe、実libraryの未解決symbol、当時のpkg-config、avformat DLL内の識別文字列が一致する。LGPL表示だけでは既定の配布条件を満たさない。[再build計画と単体検証](FFMPEG_REBUILD.md)、[固定した根拠](ffmpeg-distribution-rejection.json)を参照。開発用fileは保持し、本体のlicense変更やcodecの暗黙の削除はしない。

## 固定binaryの確認（2026-09-07 22:18 JST）

- towavue: `a5639e5`由来の通常release、SHA256 `339046281C097BE5BA05D91BDA9D73F8501BD433A0A6AA96EF4C77E6DD87FCA2`。この監査時のtracked HEADは`6a3c820`で、その間は文書変更のみ。
- FFmpeg: BtbNの[autobuild-2026-09-03-13-17](https://github.com/BtbN/FFmpeg-Builds/releases/tag/autobuild-2026-09-03-13-17)、`ffmpeg-n9.0.1-11-ge47273f4d9-win64-lgpl-shared-9.0.zip`。
- archive SHA256: `AD26FCA80435853043BD75A989BE38261FA28B54BB623459B88786177B22FA86`。setup scriptの固定値と再照合した。展開済みbinの全10 fileもZIP内entryのSHA256と一致する。
- FFmpeg source commit: `e47273f4d9227152dcbf543cebaf9e2430ddbcc4`。単なる`n9.0.1`のsource archiveを同一sourceとみなさない。
- build recipe release tagのcommit: `8267213e26c1031621e6e1210fe3aa4867214f6a`。recipeを取得できることだけでは、binary生成時の全dependency source・toolchain/imageの一致を証明しない。

LLVM `llvm-readobj --coff-imports`で本体とFFmpeg bin全fileのPE importを確認した。本体はFFmpeg DLL 6個、補助のffmpeg.exe/ffprobe.exeはさらにavdevice-63.dllを直接importする。現行開発binaryが必要とするfileは以下の7 DLLと2補助exeである。上記の除外判断により、この一覧を配布承認としない。ffplay.exeはtowavueから使用しない。

| File | Bytes | SHA256 |
|---|---:|---|
| avcodec-63.dll | 71069184 | `01FCBB3EA53BE70C31FD95417353EAB9D4F9FBE53FD8A2FDB1999993B2B9E9C5` |
| avdevice-63.dll | 3923968 | `89424D3816D2F8961CD4996CA905130BF9564A16D6FB8377D8B1ED99562730F6` |
| avfilter-12.dll | 30014976 | `BDF552523BCE0927BCBDB8F3D69156A9C15737508BEF14A1431442610BD4EBE1` |
| avformat-63.dll | 22153728 | `93CFE2D26AB9406757E6DD539EFB0C1CADE1C6A4B54F0871C13F1A01AC61C355` |
| avutil-61.dll | 2939392 | `A48DF5106691B9390F892532BF6175C9B4013C945A6A515E835FFD600944B960` |
| swresample-7.dll | 722944 | `2FDCE56C4CFB950D3F9394F0096D666D932A875B0B24D97E8C5AA8CEB448A372` |
| swscale-10.dll | 2479616 | `28316B4F40E2BEE3BB1E56ACED8C2A451438C1814BA3D37EB8097DA8E83E4D37` |
| ffmpeg.exe | 538112 | `F77325FBDF959E14020AFA4B9187691EE322F61E40716B5573B1C4C360DF8702` |
| ffprobe.exe | 227328 | `9A3FA8E399EF8BE898A7D9CB52B2E7FD055A05629A911B136101C49D36E6B0E0` |

このimport検査はdriverやcodecによる実行時の動的load、Windows 10上のAPI可用性、DLLの差替え互換性を証明しない。OSのDLLを開発機からcopyして同梱しない。本体は別途VCRUNTIME140.dllも直接importする。

## FFmpegと第三者資料

実行した`ffmpeg.exe -hide_banner -buildconf`は`--enable-version3 --enable-shared --disable-static`を含み、`--enable-gpl`と`--enable-nonfree`を含まない。`-L`はLGPL version 3 or later、archiveのLICENSE.txtはLGPLv3を表示する。[固定recipeのdefaults-lgpl.sh](https://github.com/BtbN/FFmpeg-Builds/blob/8267213e26c1031621e6e1210fe3aa4867214f6a/variants/defaults-lgpl.sh)とも一致する。配布表示をLGPL 2.1固定にしない。

[FFmpeg公式の配布チェックリスト](https://ffmpeg.org/legal.html)と[このsourceのLGPLv3本文](https://github.com/FFmpeg/FFmpeg/blob/e47273f4d9227152dcbf543cebaf9e2430ddbcc4/COPYING.LGPLv3)を根拠に、対応するsource・build条件・変更・表示を整える。LGPLv3に加えて参照されるGPLv3本文も必要資料として扱い、利用者による互換DLLへの差替えを妨げない。DLLであることだけを配布条件の充足としない。

現時点で不足している資料:

- 配布binaryに対応するFFmpeg source一式、build configuration、変更差分と取得可能な公開先。上記releaseのassetはbinary archive群とchecksums.sha256で、対応source一式のassetはない。GitHub自動生成のSource code archiveはFFmpeg-Buildsのrecipeであり、FFmpeg本体と第三者sourceをまとめたものではない。
- 静的に組み込まれた第三者libraryの正確なrevision・source・patch・license/notice一覧。`--pkg-config-flags=--static`と多数の外部library有効化があり、DLL名一覧だけでは足りない。[packaging recipe](https://github.com/BtbN/FFmpeg-Builds/blob/8267213e26c1031621e6e1210fe3aa4867214f6a/variants/windows-install-shared.sh)はFFmpegのheaders/docs等をcopyするが、第三者source一覧を同梱せず、pkg-configのLibs.private行も除く。
- build時のdependency source内容とtoolchainの確認。下記の追跡でimage digestとrecipeの対応は判明したが、release tagを現在cloneしてbuildするだけで既存binaryを再現できるとはまだ確認していない。
- towavue自身のCargo.lockに固定されたRust依存のlicense/notice一覧。FFmpeg側の調査で代用しない。

特許関連の扱いも著作権licenseとは分ける。固定[OpenH264 recipe](https://github.com/BtbN/FFmpeg-Builds/blob/8267213e26c1031621e6e1210fe3aa4867214f6a/scripts.d/50-openh264.sh)はcommit `98bc7cbbeb7381c94ef8f9a5d158327abbf6b8b9`をsourceから静的buildする。[Cisco公式FAQ](https://www.openh264.org/faq.html)のCisco配布binaryに関する費用負担を、この別buildへ自動適用できるとは扱わない。具体的な配布条件で必要な判断を行い、全codecの特許問題がないとは宣言しない。

上流READMEは日次buildを直近14件だけ保持する方針である。再配布条件を満たしたsource/binaryを自分たちのreleaseで保持する手順が必要で、期限付きの上流URLだけに依存しない。現在のarchiveはローカルvendorに保持しているが、Gitやreleaseへのbinary/source公開はまだ行っていない。

### 対応buildとsourceの取得（2026-09-07 22:31 JST）

[上流run 33754284571](https://github.com/BtbN/FFmpeg-Builds/actions/runs/33754284571)のHEADは上記recipe commitと一致する。image job `100646797043`の生成digestと、FFmpeg job `100652840136`がpullしたdigestは、ともに`sha256:d1d34e5bb498e76cea19ee671301b7864d7e22fd806746324138c0870f253d01`。FFmpeg jobの出力名も固定archiveと一致する。imageのtarget baseは`sha256:033e9c42838fccb3b56984a9075ce87b9ab2491ac9c7f53b5070b8b42596ce16`である。

image jobが参照したsource cacheとrecipe内のrepository/revisionを[ffmpeg-build-inputs.json](ffmpeg-build-inputs.json)へ記録した。78件は実際のbuild-stage入力参照であり、最終DLLに組み込まれた全componentのSBOMではない。ファイル名内のhashはdownload command文字列のhashであって、source archiveの内容hashではない。OpenSSL、Vulkan-Headers、Mbed TLSはtag指定、LAMEはSVN revision 6761であり、取得内容との追加照合が必要である。

ローカルへ保存・確認したもの（すべてGit対象外）:

- recipe: `vendor/ffmpeg/build-recipe-20260903`、固定commitのclean checkout。build/download script自体は実行していない。
- FFmpeg本体: `vendor/ffmpeg/source-e47273f4d9227152dcbf543cebaf9e2430ddbcc4.tar.gz`、17323649 bytes、SHA256 `6491DAE95E3CF3CDBAC02933B55860E782B0C4F0A6BD8F37CEF30FDED259283C`。commit指定のGitHub codeloadから取得し、configureとGPLv3/LGPLv3本文のentryを確認した。第三者sourceはこのarchiveに含めた扱いにしない。
- build record: artifact `9893172856`、`target/tmp/ffmpeg-image-record-20260903/build-record.download`、245291 bytes。SHA256 `95B4D5C136589460A348EADC77510859F14744F8DE0805CAB33AC99685A017D8`はAPIのartifact digestと一致し、上記image manifestを含む。これはimageの全layerそのものではない。APIのURL末尾はzipだが実体はgzipのOCI archiveで、通常のgh展開が拒否した後、raw downloadのhashとtar entryで確認した。

同じrunのsource取得cache artifact `9892918840`（download-cache、2024951640 bytes、API digest `sha256:0fc20317ded7bc2aef7fdac1f085ffb8be435ae143ac78bc46ac3b6ae9d3fd83`）の取得は22:48 JSTに完了した。外側のZIP自体はghが保持しないため、そのAPI digestをローカルで再計算したとは扱わない。取得した`target/tmp/ffmpeg-source-cache-20260903/cache.tar.gz`は2024336209 bytes、SHA256 `B02DC5084BA6717F7FF66961692E87E93C8A7AB7D6967F321192ED19092AD08A`である。

cacheには115個の実archiveと115個のsymlink aliasがある。pathとentry種別を確認し、buildが参照した78個のhash付き通常fileだけを同directoryの`selected/.cache/downloads`へ展開した。aliasや内側source treeは展開していない。実revision・license本文に加え、aom/aribb24のpatchやscript中の変更、submodule・build tool依存の照合は別途必要である。

78個すべての内容SHA256を上記JSONへ追記した。root Git HEADを持つ74件のうち、71件のcommit指定はrecipeと一致し、残るOpenSSL/Vulkan-Headers/Mbed TLSはcacheのFETCH_HEADに指定tag名があり、解決されたHEADも記録した。OpenCLのheaders/loaderとnv-codec-headersの3系列はnested repositoryで、計5 HEADがrecipeと一致する。FFmpeg 9向けrecipeが選ぶnv-codec系列は`ffnvcodec`であり、他2系列も組み込まれたと扱わない。

LAMEのSVN databaseはメモリ内・query-onlyで読み、rootと全449 NODES行のrevision 6761を確認した。AMFはdownload recipeが`.git`とThirdpartyを除去する。23:03 JSTの追加照合で、残る577 fileすべてのGit blob hashが固定commitのtreeと一致し、欠落・追加・不一致はなかった。14 archiveにsubmodule manifestがあり、他のroot HEAD照合だけではsubmoduleやworktree内容の完全性を証明しない。内側entryの読取り以外に、sourceのbuild scriptは実行していない。

### 原本patchとnative通知の抽出（2026-09-07 23:24 JST）

Windows上のrecipe checkoutは`core.autocrlf=true`によりpatchをCRLFへ変換していた。AOMの適用checkはそのcopyで失敗し、固定commitのraw blobでは成功した。上流patchの不整合とは扱わない。2026-09-08の訂正: 初回の`git archive`もこの設定の影響を受け、全265 entryがGit blobと異なっていた。`git -c core.autocrlf=false archive`で別fileへ作り直し、全265 entryの一致と欠落なしを確認した。新archiveは706560 bytes、SHA256 `2d6211a7e4becbb581bf64e2d1a67ff060fb413cd157d479959934ea23f3b2a4`。保存先・旧archiveの除外理由と4 patchの原本hashは[ffmpeg-build-inputs.json](ffmpeg-build-inputs.json)に記録した。既存のraw blobを使ったAOM/ARIBの適用checkとは区別する。

AOM 1 patchとARIB B24の12→13→17の3 patchは、cache内の対象fileを変えない`git apply --check`を通過した。これは適用可能性の確認であり、実際のgit am・autoreconf・buildや完成binaryの再現ではない。ARIBのconfigure.acのversion書換えなど、patch file以外の処理もraw recipeに含めて保持する。

[ffmpeg-notice-inputs.json](ffmpeg-notice-inputs.json)は、78 archiveを内部展開せず走査したnative通知資料の入力一覧である。

- 名前で検出した385 fileのうち、通知本文でないxzの`build-aux/license-check.sh`を除いた384 fileを記録した。nested/test/build資料も含む発見一覧であり、すべてが最終DLLへ組み込まれたという意味ではない。
- named fileがないnv-codec-headersは、FFmpeg 9向けに選ばれた系列の5 headerに通知全文があった。各headerのhashと、先頭commentのoffset・長さ・hashを記録した。このheader向け許諾をNVIDIAのdriverやSDK全体へ拡張した表示にしない。
- 原文は`target/tmp/native-notice-audit-3np__jt8/texts/<SHA256>.txt`へbyte単位で保持した。LCMS AUTHORS、OpenJPEG viewer notice、OpenMPT内LuaSocket notice、rav1e PATENTSの4 fileはUTF-8ではない。Windows-1252での表示を確認したが、元のencodingを断定したり、不正byteを置換して本文を欠落させたりしない。
- reflogとremote refを除いたroot/nested/module HEADは117件あった。submoduleの未取得・build時の追加取得やgitlinkとの照合が残るため、HEAD一覧だけをsource全体の完成証明としない。

追加で確認すべきcompiler/runtime資料も切り分けた。固定ffmpeg.exeはGCC 15.2.0 / crosstool-NG 1.28.0.23_185f348を表示し、base-win64 recipeにはstatic-libgcc/static-libstdc++、binaryのconfigurationには`-lgomp`がある。これらの資料は78 archiveとは別に扱い、以下の追跡結果を記録した。

またrav1e recipeは`cargo update cc`後にstatic libraryをbuildする。元cacheのCargo.lockだけでは更新後の完全な依存集合を証明しない。元image jobのlogにもccのupdate/download/compile行はなく、現在のnetwork解決で当時のversionを推測しない。rav1e側の依存と、下記towavue本体のMSVC向け146 packageは別物であり、前回のRust本文集で代用しない。

### GCC本文と実rav1e libraryの追跡（2026-09-07 23:43 JST）

[ffmpeg-runtime-inputs.json](ffmpeg-runtime-inputs.json)へ追加runtime資料と原本のhashを記録した。

- GCC 15.2.0のrelease commit `5115c7e447fc07457443df874bf57840e8316d5f`から、COPYING3とCOPYING.RUNTIMEを`third-party/gcc-15.2.0`へ保存した。Git blobとSHA256は原本と一致する。libgcc/libgcc2.c、libgomp/libgomp.h、libstdc++のhashtable実装の先頭通知にもGPL 3以降とRuntime Library Exception 3.1の参照を確認した。[例外本文](https://github.com/gcc-mirror/gcc/blob/5115c7e447fc07457443df874bf57840e8316d5f/COPYING.RUNTIME)は対象fileとcompilation process等に条件を置くため、他componentへ一律に適用した表示にはしない。
- 当時のimage digestからmanifestとconfigを取得し、双方のcontent hashを照合した。config digestは`sha256:f895b2da6a46618e840f97ae85abf1b946cfb39b692d7642cf89a06fefedbbbc`。そのmanifestが指定するprefix layer（85360808 bytes、`sha256:bbb9aba0ba171e6f9b085b197c582a52222c024d606a13580bddd3f1b11311f5`）も取得・照合した。containerやlibraryは実行していない。
- layer内のlibrav1e.aは58124336 bytes、SHA256 `BC9A20F69AF4EE4ED2776BDD19354516BCA1DC60CAEF76A4C18C8B244247864A`、549 archive memberを持つ。pkg-configはrav1e 0.8.0を示し、library内のcompiler情報はRust 1.97.1 / commit `8bab26f4f68e0e26f0bb7960be334d5b520ea452`である。towavue側のRust 1.98.0とは区別する。
- library内のCargo source pathで観測した23個のname/versionは、元source lockのpackage/checksumと一致した。対応crate archiveを取得・hash確認し、38件の通知本文をbyte単位で保存した。ただし文字列に現れないpackageがないとは証明しない。元lockは272 packageを持ち、ccは更新前の1.2.26なので、全依存や更新後lockの復元完了とは扱わない。
- libraryのmtime `2026-08-19T15:49:10Z`を含む同targetのbuild候補はrun `32253888986` / job `96120312838`だった。ただしrun-logと直接job-logの双方がHTTP 410を返すため、同じlayerの生成元や更新後ccを確認できていない。timestampだけで同一buildとは断定しない。

取得したimage資料・libraryは`target/tmp/ffmpeg-registry-provenance-20260903`、23 crateと本文は`target/tmp/ffmpeg-rav1e-crates-20260903`に保持している。GCCの2本文以外はGit対象外。次はrav1eの有効依存とRust 1.97.1 runtime/compiler-builtinsの通知、残るnative source/build資料を整える。この追跡結果は同梱するbinaryの承認や完全なSBOMではない。

## Rust依存と組み込みフォント（2026-09-07 22:48 JST）

[rust-license-inputs.json](rust-license-inputs.json)にWindows x86-64向けの依存とライセンス資料の取得元を記録した。`cargo metadata --locked --offline --filter-platform x86_64-pc-windows-msvc`からtowavue-appのnormal/build依存を辿り、dev-only edgeを除いた146 packageが対象である。build用packageも含むため、最終binaryのlinked-runtime SBOMとは呼ばない。

- 全146個のローカルcrate archiveのSHA256がCargo.lockのchecksumと一致した。rootのlicense/notice等264 file、フォント資料4 file、下記13 packageのVCS情報fileを、archive内の生bytesとSHA256で照合した。root名での発見だけでは、source内やnested directoryの追加noticeを網羅したことにはならない。
- root資料がない13 packageのうち、AccessKit、egui、clipboard-win、profilingの12 packageはcrateのVCS情報が示す固定commitから11個の原本文書を取得した。ローカルの原本はGit blob SHA1と一致し、SHA256も上記JSONへ記録した。取得した本文は`target/tmp/rust-license-sources`配下に保持しており、まだ配布用bundleには組み込んでいない。
- AccessKitのrootにはMIT/Apache本文だけでなく`LICENSE.chromium`もある。Cargoのlicense expressionだけを見て追加のChromium BSD noticeを捨てない。
- epaint_default_fontsはHack、Noto Emoji、Ubuntu Light、emoji-icon-fontを埋め込む。`fonts/Hack-Regular.txt`にはMITに加えてDejaVu/Bitstream Veraの記載があり、別途OFL、Ubuntu Font Licence、emoji-icon-fontのMIT本文も保持する。egui本体のMIT/Apacheだけでフォントを扱わない。
- ffmpeg-sys-next 9.0.0はCargo.tomlでSPDX識別子WTFPLを宣言するが、crate rootと固定上流treeにlicense/notice fileはなかった。23:03 JSTの補完では、[SPDXが同識別子へ対応付ける標準本文](https://spdx.org/licenses/WTFPL.html)を固定commitから`third-party/licenses/WTFPL.txt`へ保存し、原本のGit blob/SHA256と照合した。本文集には宣言を根拠とする標準本文であることを明記し、crate由来の文書とは表示しない。[公式FAQ](https://www.wtfpl.net/faq/)の説明どおり、本文中のSam Hocevarのcopyrightはlicense文書の著者であり、crateの著者という表示にはしない。

### Rust本文集の生成・検証（2026-09-07 23:05 JST）

root以外も探索し、regex-syntaxのUnicode table noticeとtracing-coreのspin実装のMIT noticeを追加した。tiffの`tests/COPYRIGHT`は同梱しない試験画像だけのcreditなので除外する。選択式licenseは元のOR表示と各本文を保持し、ANDへ変更した扱いにはしない。ソース中の個別noticeの最終確認と、native FFmpeg/VC runtime資料は別途残る。

通常の依存取得・build後に、次の手順で本文集を再生成できる。

```powershell
.\scripts\setup-rust-notices.ps1
.\scripts\prepare-rust-notices.ps1
.\scripts\test-rust-notices.ps1
```

setupは固定URLから不足する上流11 fileだけを取得し、取得前のcacheも取得後もSHA256を確認する。generator自体はofflineで、Cargo.lock・実際のWindows normal/build依存集合・全crate archive・上流本文・標準本文を検証してから、`target/distribution/RUST-THIRD-PARTY-NOTICES.txt`を書き出す。元crateのtar entryを直接読み、ローカルに展開したsourceの変更を混ぜない。別cwdからもrepositoryの固定toolchainを使い、入力エラーでは既存出力を変更しない。

146 section、UTF-8/BOMなし/LF、必須追加notice、二回生成のbyte一致、上流本文の欠落・改変の拒否と既存出力保持を専用testで確認した。空cacheからの11 file取得と、再実行時の更新なしも別の隔離directoryで通過した。本文集は1542330 bytes、SHA256 `9849D7B4A28EDD77C3A816A073CC09C30E4BC5201305FC3DCACF564E7F112DF1`。CIにも生成検証を追加する。このRust本文集だけでinstaller全体の再配布条件を満たしたとは扱わない。

### Rust標準ライブラリ・補助runtimeの資料（2026-09-08）

Cargo packageの本文集とは別に、[rust-runtime-inputs.json](rust-runtime-inputs.json)へ次の公式配布物を固定した。release manifestのURL/hash、archiveのURL/size/hash、選択した原本36 fileのpath/size/hashを記録している。

| 使用箇所 | Rust | 対象 | compiler commit |
|---|---|---|---|
| towavue | 1.98.0 | x86_64-pc-windows-msvc | `88d9e12ae178fab0fb5cc050a94da85685d449ea` |
| 固定FFmpeg内のrav1e | 1.97.1 | x86_64-pc-windows-gnu | `8bab26f4f68e0e26f0bb7960be334d5b520ea452` |

公式rustc archive内のversion/git-commit-hashは、開発機のrustcと取得済みlibrav1e.aに観測したcompiler情報へそれぞれ一致する。1.98.0の`COPYRIGHT-library.html`は、インストール済みの原本ともbyte一致した。これはcompilerの識別情報と文書の照合であり、rav1eの全build依存や完成libraryの再現証明ではない。

各versionについて、rustc componentから標準ライブラリ向け`COPYRIGHT-library.html`、rootのCOPYRIGHT/MIT/Apache本文と公式licenses directoryを保存する。HTMLは他platformやbuild依存も含む上流の報告書であり、Windows binaryのlinked SBOMとして扱わない。licenses directoryは本文辞書として全体を保持し、そこにGPL等があることを全licenseがtowavueへ適用されるという表示にしない。

両HTMLにはcompiler-builtinsの明示的な記載がなかったため、対応する公式rust-src componentの`compiler-builtins/LICENSE.txt`と`compiler-builtins/libm/LICENSE.txt`も別fileで加える。compiler-builtinsのAND条件やLLVM exception、libm側の追加copyrightを保持し、標準ライブラリ全般のMIT OR Apache-2.0で置き換えない。個別source中の通知や他のnative runtimeの適用確認は別途残る。

```powershell
.\scripts\prepare-rust-runtime-notices.ps1 -Download
.\scripts\test-rust-runtime-notices.ps1
```

`-Download`は不足する固定4 archiveだけを`target/tmp/rust-runtime-materials`へ取得し、改変cacheは上書き修復せず拒否する。switchなしはoffline。全archiveと原本36 fileのsize/hashを検証してから、`target/distribution/RUST-RUNTIME-NOTICES.zip`を生成する。tarのstdoutだけを読み、上流のpathをfilesystemへ展開せず、compiler/libraryを実行・インストールしない。ZIPには原本文書と説明・入力一覧だけを入れる。本文の改行・bytesは変更しない。

検証では原本36件のbyte一致、二回生成の一致、別cwd起動、入力欠落/同size改変の拒否、正常な既存出力と不正cacheの保持を確認する。CIにも同じ取得・検証を追加した。この資料を含めても、native FFmpegの残るsource/noticeとVC runtime、installer導入試験は完了していない。

初回CIのrun 34174369000は資料step中にjobの時間上限へ到達した。元logにはarchive単位の進捗がなく、どの取得・読取りで待ったかは特定できない。生成scriptは取得/検証/読取りのstageを表示し、curl.exeで接続20秒・転送全体180秒の上限を設けた。checksum検証と不正cache/既存出力の保護は維持する。無制限retryやjob時間の引延ばしで成功扱いにしない。

続くrun 34175731040では4 archiveの取得・hash検証が約3秒で完了し、最初のrustc `.xz`読取りで約10分待ってjob上限へ到達した。原因を通信と断定せず、固定release manifestが指定する同versionのgzip archiveへ切り替えた。全36文書は従来と同じsize/hashで、入力一覧だけが変わる。読取りstdoutを非同期で排出し、60秒でtarを停止して失敗を報告する。tarのversionも記録する。ローカルの生成・回帰試験は通過したが、Windows Server CIでの解消確認と、旧tar内部の停止原因の確定は別である。

修正後のrun `34177092293`は全step成功。Windows Server 2022上の資料stepも約42秒で取得から回帰試験まで完了した。tar表示はlibarchive 3.8.4で、ローカルと違ってliblzmaを列挙しない。gzip経路の成功は確認できたが、旧XZ読取りの内部停止原因やtimeout分岐を実証したとは扱わない。

## Visual C++ runtime

開発機のVisual Studio Community 2026配下で、x64 Redistributableを読み取り確認した。file versionは`14.51.36247.0`、18731856 bytes、SHA256は`843068991DAAA1F73AD9F6239BCE4D0F6A07A51F18C37EA2A867E9BECA71295C`。AuthenticodeはValid、署名者はMicrosoft Corporationである。実行・copy・インストールはしていない。

[Microsoftの配布手順](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170)はRedistributable packageによる導入を推奨する。[Visual Studio 2026の再配布一覧](https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution)と該当editionのlicense条件を確認し、許諾された変更なしのfileだけを候補にする。インストール済みであることや署名が有効なことだけでは利用者のlicense条件充足を証明しない。

installer設計前に、配布buildと互換なruntimeの固定version、導入済みversionの判定、必要時の前提installer実行・終了code・再起動要求を決める。towavueのアンインストールで共有VC runtimeを削除しない。app-local配置やstatic CRTへ暗黙に変更してこの確認を迂回しない。

## 次のゲート

1. [FFMPEG_REBUILD.md](FFMPEG_REBUILD.md)に従い、機能を保つ代替buildの有効dependency/source/revision/patchとlink入力を固定する。既存binaryの除外は確定しており、期限切れの旧log追跡だけを繰り返して配布承認へ進めない。既存の確認済み原本資料は再buildの入力・比較基準として活用する。
2. 取得したlicense/notice、Rust依存、VC runtimeを含む配布資料を照合する。この文書は最終license bundleではない。
3. 資料が揃ってから、ROADMAPのSetup.exe実装・隔離環境での導入/更新/削除検証へ進む。開発用FFMPEG_DIR/PATH不要、別作業directoryでの起動、preview/export、tab detachと既存H1 gateを保持する。

既存helper探索の独立修正は[DEVELOPMENT.md](DEVELOPMENT.md)で局所検証した。同梱helperを優先し、不足時にPATHの別版へ切り替えない。通常debugの無環境変数・別cwd・日本語pathでpreview／保存／tab detachが動くことと、上記の配布採用・installer／対象Windows gateは区別する。
