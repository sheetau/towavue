# インストーラー配布の準備

2026-09-07のowner指定により、インストール先を選べるSetup.exeを目標とする。これは配布前の技術・資料監査であり、公開可能なpackageや法的適合性の認定ではない。現行のRust構成とFFmpeg動的リンクを維持する。H1の品質確認も継続する。

## 固定binaryの確認（2026-09-07 22:18 JST）

- towavue: `a5639e5`由来の通常release、SHA256 `339046281C097BE5BA05D91BDA9D73F8501BD433A0A6AA96EF4C77E6DD87FCA2`。この監査時のtracked HEADは`6a3c820`で、その間は文書変更のみ。
- FFmpeg: BtbNの[autobuild-2026-09-03-13-17](https://github.com/BtbN/FFmpeg-Builds/releases/tag/autobuild-2026-09-03-13-17)、`ffmpeg-n9.0.1-11-ge47273f4d9-win64-lgpl-shared-9.0.zip`。
- archive SHA256: `AD26FCA80435853043BD75A989BE38261FA28B54BB623459B88786177B22FA86`。setup scriptの固定値と再照合した。展開済みbinの全10 fileもZIP内entryのSHA256と一致する。
- FFmpeg source commit: `e47273f4d9227152dcbf543cebaf9e2430ddbcc4`。単なる`n9.0.1`のsource archiveを同一sourceとみなさない。
- build recipe release tagのcommit: `8267213e26c1031621e6e1210fe3aa4867214f6a`。recipeを取得できることだけでは、binary生成時の全dependency source・toolchain/imageの一致を証明しない。

LLVM `llvm-readobj --coff-imports`で本体とFFmpeg bin全fileのPE importを確認した。本体はFFmpeg DLL 6個、補助のffmpeg.exe/ffprobe.exeはさらにavdevice-63.dllを直接importする。したがって現行binaryの配布候補は以下の7 DLLと2補助exeである。ffplay.exeはtowavueから使用せず、同梱候補から除く。

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

LAMEのSVN databaseはメモリ内・query-onlyで読み、rootと全449 NODES行のrevision 6761を確認した。AMFはdownload recipeが`.git`とThirdpartyを除去するため、HEAD欠落を取得失敗とは扱わないが、固定commitとのtree比較は未完了である。14 archiveにsubmodule manifestがあり、root HEAD照合だけではsubmoduleやworktree内容の完全性を証明しない。内側entryの読取り以外に、sourceのbuild scriptは実行していない。

## Rust依存と組み込みフォント（2026-09-07 22:48 JST）

[rust-license-inputs.json](rust-license-inputs.json)にWindows x86-64向けの依存とライセンス資料の取得元を記録した。`cargo metadata --locked --offline --filter-platform x86_64-pc-windows-msvc`からtowavue-appのnormal/build依存を辿り、dev-only edgeを除いた146 packageが対象である。build用packageも含むため、最終binaryのlinked-runtime SBOMとは呼ばない。

- 全146個のローカルcrate archiveのSHA256がCargo.lockのchecksumと一致した。rootのlicense/notice等264 file、フォント資料4 file、下記13 packageのVCS情報fileを、archive内の生bytesとSHA256で照合した。root名での発見だけでは、source内やnested directoryの追加noticeを網羅したことにはならない。
- root資料がない13 packageのうち、AccessKit、egui、clipboard-win、profilingの12 packageはcrateのVCS情報が示す固定commitから11個の原本文書を取得した。ローカルの原本はGit blob SHA1と一致し、SHA256も上記JSONへ記録した。取得した本文は`target/tmp/rust-license-sources`配下に保持しており、まだ配布用bundleには組み込んでいない。
- AccessKitのrootにはMIT/Apache本文だけでなく`LICENSE.chromium`もある。Cargoのlicense expressionだけを見て追加のChromium BSD noticeを捨てない。
- epaint_default_fontsはHack、Noto Emoji、Ubuntu Light、emoji-icon-fontを埋め込む。`fonts/Hack-Regular.txt`にはMITに加えてDejaVu/Bitstream Veraの記載があり、別途OFL、Ubuntu Font Licence、emoji-icon-fontのMIT本文も保持する。egui本体のMIT/Apacheだけでフォントを扱わない。
- ffmpeg-sys-next 9.0.0はWTFPLを宣言しているが、crate rootと固定上流treeにlicense/notice fileを発見できていない。他のffmpeg crateの本文を同crate由来と偽らず、宣言と適用する本文の根拠を補う必要がある。

この一覧は資料の所在と照合記録であり、完成した第三者notice集や配布条件充足の宣言ではない。選択式licenseと追加noticeを整理し、source内の適用範囲も確認してから同梱本文を作る。

## Visual C++ runtime

開発機のVisual Studio Community 2026配下で、x64 Redistributableを読み取り確認した。file versionは`14.51.36247.0`、18731856 bytes、SHA256は`843068991DAAA1F73AD9F6239BCE4D0F6A07A51F18C37EA2A867E9BECA71295C`。AuthenticodeはValid、署名者はMicrosoft Corporationである。実行・copy・インストールはしていない。

[Microsoftの配布手順](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170)はRedistributable packageによる導入を推奨する。[Visual Studio 2026の再配布一覧](https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution)と該当editionのlicense条件を確認し、許諾された変更なしのfileだけを候補にする。インストール済みであることや署名が有効なことだけでは利用者のlicense条件充足を証明しない。

installer設計前に、配布buildと互換なruntimeの固定version、導入済みversionの判定、必要時の前提installer実行・終了code・再起動要求を決める。towavueのアンインストールで共有VC runtimeを削除しない。app-local配置やstatic CRTへ暗黙に変更してこの確認を迂回しない。

## 次のゲート

1. 固定recipeとbuild記録から第三者source/revision/patch一覧を確定し、不足する資料を取得する。既存binaryとの対応が確認できなければ、根拠のないsourceを添付せず、同一機能を維持する再現可能buildの計画を別途立てる。
2. 取得したlicense/notice、Rust依存、VC runtimeを含む配布資料を照合する。この文書は最終license bundleではない。
3. 資料が揃ってから、ROADMAPのSetup.exe実装・隔離環境での導入/更新/削除検証へ進む。開発用FFMPEG_DIR/PATH不要、別作業directoryでの起動、preview/export、tab detachと既存H1 gateを保持する。
