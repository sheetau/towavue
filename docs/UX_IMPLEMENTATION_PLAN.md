# 機能・操作・UI改善（2026-09-09 owner goal）

この計画はローカル草案の `concept.txt` と `follow-up-plan.txt` を現在の実装へ照合した、新goalの作業台帳である。草案自体はGitへ含めない。ローンチ準備・署名・公開・インストーラーの再認定は再開しない。

## 優先規則と検証

- single D3D11 device、core/runtime/appの境界、Shell順、非破壊編集と保存先保護、WASAPI Shared、D3D11VA→softwareは維持する。草案のCUDA／Exclusive／wgpu案へ戻さない。
- 以前のH1記録で「この変更には含めない」とした機能は、永久的な却下ではない。follow-upの明示要望を未完事項として扱い、構造を変える際は先にARCHITECTUREへ採用契約を追記する。明確な採用仕様との競合は理由を記録する。
- 既存実装・過去のtest成功だけで草案全体の達成とはしない。下記の各項目に実装箇所、対象操作の自動回帰、同条件の実画面／runtime検証を対応させる。外観は座標・寸法・色と画面の両方で確認する。
- 一度に全機能を混ぜず、依存順に検証可能な変更へ分ける。format／Clippy／全target testsと関連実画面を通してcheckpointをpushする。Windows 10実機は必須化せず未検証を明記する。

## 作業台帳

### 2026-09-12追記の差分台帳

追記で指摘された項目は、以前の主経路実装や自動testの成功で完了にしない。以下もgoalの完了条件へ含める。思案中と明記されたファイル検索paletteは引き続き実装対象外。

| 対象 | 採用する追記・現在の状態 |
| --- | --- |
| U01 native caption | button下の1px隙間・左右padding・非focus色・fullscreen上端の欠けを再現し是正する。未完 |
| U04 配色／tab配置 | hover背景を#2C2C2Cへ変更済み。共通widgetのhover／active／open背景と文字を回帰確認。tab上下3px相当の余白・native内側borderとの整合は未完 |
| U07 dirty表示／履歴 | close iconを未保存indicatorへ切替。選択だけや元と同じ結果をdirtyにしない契約を全操作で再監査する。未完 |
| U04 status／focus | loading等の画面内messageをstatus左へ集約し、重複する寸法／形式を除きframe数は右へ。reading drag値の表示、Escape／tab切替時の赤いoutline除去。未完 |
| M01/G01 menu | button下の固定位置、直接section時は親を出さず中身だけ保持、Image jumpをView子menuへ、logoは背景を変えずgray／white線色・左にも同じgap・Edit方向45度を実装。親非表示／先頭位置・境界角・通常rootの4分類・nested keyboardと全command配置・取消／一回dispatch、3サイズ×3倍率の再openを回帰確認。実画面／全focus・DPI・pixel単位の余白監査は未完 |
| U03/U04 font／resize | seek preview等のfont適用漏れ、右端resize時の左寄せtext／icon振動を調査。性能を優先し修正可能性を判断する。未完 |
| G01/I08 非active wheel | 音声list・画像縦／Shift横scroll・既存音量領域のfocus必須条件を除去。非activeで距離／軸／非選曲／focus不変、volume、取消／所有権を回帰確認。OS実入力・Ctrl＋wheel等の他経路は未完 |
| V03/A02 timeline外観／入力 | #181818の角丸背景、左右上の同幅余白、白50%volume線とRowResize、CTIを内部に収め白1px＋上のplayheadだけdrag可能、選択左右resize／cursor、白20% difference塗り＋左右白1px点線。未完 |
| U09/U10 tooltip／preview | hover終了後に残るtooltip／tab preview不発を根本調査し、非active hoverと元要素への中央揃えも確認。未完 |
| U04 popup shadow | 左右中央・濃さ・ぼかしを調整し、描画の粗さを負荷優先で評価。未完 |
| U12 seek hover | 背景と白進捗の間にhover位置まで白25%相当の進捗を全幅で表示。未完 |
| V04/A02 volume HUD | 変更時のみ少し残る縦barを左中央、余白に応じて上中央横barへ切替可能なら採用。status左の重複messageを除く。音声list外を音量操作対象へ拡張。未完 |
| V02/I02 seek preview | 切替時に下端から昇る動画previewのglitchを解消し、画像も含めhover遅延なし・未生成でもtext表示。未完 |
| A01 playlist | 再生中は白文字のみで背景はhoverだけ、右端に曲の長さを表示。未完 |

画像の選択／pan／zoom追記はI07～I09の既存台帳、100枚原寸・小previewの速度はI03/U10の未達gateを引き継ぐ。今回の可視試験はWelcome画面まで取得したが、activation成功応答後もclick／Ctrl+Oの反映を観測できず、配色や非active実入力の成功証拠にはしない。

2026-09-12 I03 color-preparation checkpoint: 不透明行の判定を最大32画素のAND＋alpha maskへ置換し、混在blockで打切り。透過色の変換はeguiのまま。全alpha値・13種類の幅・block境界前後の単独透過画素で完全一致を確認。100 JPEGの準備中央値は即時6.627／6.645→5.545／5.563ms、最短33msで7.207／7.139→6.280／6.016ms。切替中央値は即時16.746／16.726msでほぼ横ばい、最短33msで7.527／7.254ms（p95 8.419／8.339ms）。同じwarm CPU測定の局所改善であり、blankは両条件100件、previewは96／3・再測定96／5で残る。可視GPU・実入力burst・cold／全形式／資源と全台帳は継続する。

2026-09-12 I03 overlap checkpoint: 移動先が先読みを引き継げるようになった後で、texture準備前への先読み開始を再評価。100 JPEGの即時切替中央値22.598／22.659→16.645／16.592ms、p95 23.432／23.451→17.660／17.739ms。最短33msは7.983／7.987→8.506／8.337msで改善なし（p95 9.272／10.146ms）。現在画像のtexture処理をcontext lockで止め、既存workerが隣画像の原寸とpreviewを準備できることを回帰化。旧開始順で失敗、新開始順で成功。予算・worker・対象順・readingを変えず、同一completion末尾で再submitしない。中間blankは両条件100件、previewは97／6・再測定96／5で残り、可視GPU／burst／cold／全形式／資源と全台帳は継続する。

2026-09-12 I03 100-image/handoff checkpoint: 生成4096×2304 JPEG 100枚・計31.8MiBのRelease計測で、入力処理／completion処理／原寸meshまでの時間を分離。画像切替の空要求二回を除き、移動先要求一回で復号中先読みを引き継ぐ。即時次要求の中央値28.496→22.598／22.659ms、p95 30.669→23.432／23.451ms。最短33ms条件は8.011→7.983／7.987ms。先読み開始を表示準備より前へ動かすだけの案は改善せず撤回した。回帰で世代一回置換・前後／循環の画素保持を確認。空表示／低解像度なしのgateは未達で、可視GPU／burst入力／cold／他形式／資源と全台帳を継続する。

I03再測定: `cargo test -p towavue-app --release hundred_large_images_report_navigation_gaps_and_preparation_cost -- --ignored --nocapture`（既存FFMPEG_DIR設定が必要）。専用一時領域で100 JPEGを生成し、終了時に除去する。filesystemは生成直後でwarm、Shell snapshotは合成順、最初だけ500msの先読み時間を与える。各画像の原寸を待って全100枚を訪れ、最短間隔0／33msを比較する。command直後と通知処理後にも描画データを要求するため、blank／preview件数はその測定上の中間描画であり、実画面のちらつき回数ではない。GPU upload／Present、物理keyや一定周期burstの取りこぼし、IrfanView比較とpeak memoryは別の未完検証。

2026-09-12 I09 selection-zoom checkpoint: crop preview状態と描画切出しを除去。画像の選択内click・Zoom to selection commandは、画像全体と選択枠を保つ通常zoom／panへ移る。100／125／200%・回転画像の入力回mesh／UV保持・bar即時表示・hover／保持cursor・command同等性・選択解除とFit復帰を確認。旧shortcut／grid名はaliasとして読み込み、新名だけを保存する。可視入力・全DPI／全gestureと全台帳は未完。

2026-09-12 I09 outside-click checkpoint: 選択外の表示面／余白へのprimary clickで選択のみ解除。短いreleaseまで待ち、辺／角hit・drag・取消・無効領域と同frame先行gestureの所有権を保つ。回帰で画像内dragの再選択、余白dragの無変更、barによる既存選択保持、100／125／200%の解除とpan／dirty／texture保持を確認。crop preview撤去／通常zoomと全台帳は未完。

2026-09-12 I09 selection-gesture checkpoint: 角を一辺ではなく二辺の操作としてhit testし、Shift時は開始比率と対角を固定する。辺／角resizeとprimary Crosshair、画像の選択内secondary移動のAllScrollを追加。選択移動はpixel寸法を保ち、画像pan／履歴とは別に画像端で止まり取消で復元する。四隅・Shift・保持／同frame完結・上下限と100／125／200%のpan／dirty保持を回帰確認。外部click解除・crop preview撤去／範囲zoom、可視操作と全台帳は未完。

2026-09-12 I08 bounded-image-scroll checkpoint: 画像の自由panを画像端までに制限し、収まる軸の中央固定、右dragのGrabbing、縦／Shift横wheelと両軸scrollbarを接続。panとbar offsetは同じ表示位置を共有し、画像texture・dirty履歴を保持する。100／125／200%のFit／両軸／片軸overflow・resize・取消・barクリックがselectionへ漏れないことを確認。選択辺focusも端で止まり、不可視の操作領域をviewportへclipする。実windowのactivationが再試行でも失敗したため可視検証は保留。全入力・外観／UIA／mixed-DPIと全台帳は継続する。

2026-09-12 I07 immediate-zoom checkpoint: follow-up追記のCtrl＋wheel連続ズームを監査。既存の最終倍率testは90frame待っており、入力回では1.3499倍の代わりに1.1003倍しか反映しない状態を再現した。画像だけraw event単位で倍率と基点を処理し、即時mesh反映・逆方向連続入力・同frame複数座標・texture再利用・残量なしを回帰化。Point／Line／Page、phase、修飾key／focus／button／overlayと既存物理倍率・編集後座標も確認。実マウスの可視latency／全DPI・資源と全台帳は未完。

2026-09-12 I03/I04 adjacent-path checkpoint: 通常前後移動で全候補pathを複製せず、元のShell snapshotから選んだ一件だけguardへ渡す。5万件の混在一覧・先頭／中間／末尾・前後／同種filterの120要求で従来の移動先とdirty保持を比較し、Release処理単体は約586→340ms。一枚だけの場合は既存の同一path no-opを維持する。現在位置は線形検索のままであり、復号／描画・可視latencyや全素材の性能改善は認定しない。reading／音声queueと予算は変更せず、全台帳は継続する。

2026-09-11 U07 same-audio-open checkpoint: 現在のcleanな音声tabとpathが一致する外部Openを再loadしない。読込／再生／停止／終端の状態、選択・bar・世代を保持し、Faultedは再試行できる。既存の別source初期化／dirty保護／強制新規を維持。実D3D11／WASAPIの再生・停止・再開でsession世代／位置／view／選択／focusを保持し、背景再生・復旧・closeも確認。全入口／通常window／IME・mixed-DPIと全台帳は未完のまま継続する。

2026-09-11 V03/A02 audition-PCM checkpoint: 選択末尾をtempo入力EOFとせず、同じ編集区間から必要な文脈を読み、出力sample数で停止する。1ms・stretch・gain／Delete跨ぎ・終端近くを含む6範囲×3速度で同じ開始の通常再生と全PCM一致。最終chunkでのconsumer拒否／取消は正常完了へ変換しない。前回観測した選択4倍速の波形差はこの範囲で解消。真の短区間編集の速度処理・初期Seek位相・長い削除区間負荷・全codec／UIと全台帳は未完のまま継続する。

2026-09-11 V03/A02 sample-boundary checkpoint: 再生／保存の編集区間sample数を共通整数計算に変更。17ms等の境界で余分な無音と後続sample欠落を再現し、生成音声の部分gain／Delete全PCM一致を確認。4 sample rates×7速度・非整列／長時間の境界と短区間の選択sample数を回帰化。実WASAPIの無音再生でworker保持・停止／再開・編集clock・復旧も通過。17～84msを4倍速で選択すると、同じ開始から終端を設けない再生とは先頭波形が異なることも観測した。短区間速度処理の音質は未認定で、初期Seek位相・長い削除区間負荷・全codec／UI・全台帳とともに継続する。

2026-09-11 G01/U06 dialog-focus checkpoint: pickerの取消／失敗時に同じtab・media generationの開始元focusを復帰し、別modal／guard・選択成功・対象変更では破棄する。command再実行なしと次のEnter一回だけのdispatchを含む2回帰、全612 tests／Release成功。通常960×576でOpen File／Open FolderのEnter→Escape→focus復帰→Enter再操作を可視確認。前回click不発は再現せず。全入口／実IME／mixed-DPI・全台帳は未完のまま継続する。

2026-09-11 E01 JPEG simple-text checkpoint: Album／Composer／GenreをxmpDMの単純文字値として追加し、計7項目の既存値UI・Keep／Set／Remove・保存へ接続。属性／要素・namespace alias・特殊文字・混在dc値、型違い／重複／修飾の拒否を回帰確認。JPEG関連11 tests、PNG／JPEGのUI／保存4 testsと全610 tests／Release成功。通常960×576で3項目の値表示とAlbum日本語／特殊文字入力・Cancelを確認し、保存の新規実画面試験は行わない。Album artist／Date／Track、他形式・EXIF／IPTC／COM・Extended XMP、全DPI／実IME／品質と全台帳は継続する。

2026-09-11 I03 neighbor-prefetch checkpoint: 通常表示は前後近隣最大9枚を距離順・同距離は直前方向で準備する。原寸10件／256 MiB・batch256 MiB・一workerとreadingの見開き規則は維持する。1～23枚の全開始位置／両方向・重複除外を確認し、実10画像の移動前preview全画素と往復／循環後の原寸一致を回帰化。609 tests／Release成功。通常960×576で生成PNGの逆方向循環・折返しとreadingの二枚単位表示を可視確認。これらはfirst-paint速度差や9枚常駐の保証ではない。cold／全素材／資源・連打時の待機と全台帳を継続する。

2026-09-11 I03/U10 animation-prefetch checkpoint: GIF／APNG／animated WebP／AVIFの先頭1frameだけを同じ先読みworkerで取得し、元寸法付き240×160以内の共有previewへ登録する。全frame列は原寸cacheに入れず、原寸要求の全画素／delayを維持。4形式のsample画素・一frame予算、非通知／非disk生成・重複／待機抑止・取消／変更／失敗の登録拒否を確認。608 tests／Release成功。通常960×576の未訪問800×600・120frame GIFで約205msの観測にLoading中preview、その後原寸animationを確認した。ただし変更前も約227ms観測ではpreviewが出ており、この素材での可視速度差は未実証。移動前の準備はruntime回帰で証明し、cold／全素材／peakと全UX台帳は継続する。

2026-09-11 H1/I06/V05 input-pairing checkpoint: 選択・pan・Alt回転の共有入力を最初のreleaseで区切り、後続gestureの位置・修飾キーを混ぜない。選択は押下時刻・移動履歴でclick／dragを判定し、後続clickによる不発・誤previewを修正。複数gesture・同座標のAlt違い・前frame保持・原点復帰・長押し・press前移動・所有権と再描画を自動回帰確認。動画のGPU preview／取消・Undo／保存再読込も成功。通常960×576の生成PNGで選択→範囲内click preview→Escapeの可視確認。可視APIは右button／Alt保持dragに非対応のため、それらの今回の検証は自動入力に限定する。全native timing・mixed-DPI、同frame全gestureの個別再生は認定せず、全UX台帳を継続する。

2026-09-11 U10 warm-card checkpoint: 可視filmstripで左の未生成PNGが後続の取得済みcardまで待たせる場面を確認し、workerのmemory先行公開→元順のmiss生成へ変更。画像／動画／音声の画素・duration、非decode／非disk lookup、source変更・取消と世代破棄を回帰確認する。既存Releaseでは6000×4000の4形式のfilmstrip／Tab・Shift+Tab・原寸表示、2枚reading、WebPのLoading中preview→原寸を可視確認。fresh processの未訪問PNGは原寸表示まで確認したが、先行previewの瞬間は撮影で区別できず未認定。全DPI／cold／全素材・資源と全台帳は継続する。

同checkpointの最終Releaseでも、通常PNGのLoading中縮小preview→原寸、filmstripの未生成61 MB PNGだけがLoadingの間の後続warm cards、完成後の同位置・同選択とEscape復帰を可視確認。通常PNGのこの標本はWelcomeでdisk thumbnailがmemoryへ入った後なので、原寸要求による直接disk lookupの瞬間とは区別する。601 testsとloader回帰10連続が成功した。

2026-09-11 U04 grayscale checkpoint: overlay・入力欄の既定色とpalette／playlist／filmstripの固有色を共通tokensへ統一。popup 3倍率・palette compact・playlist hover／focus非選択・クリック・dragカードの描画回帰を確認。Computer Use接続復旧後、通常960×576で無音2曲の選択・pause、palette検索／Escapeとlogo menuを可視確認。全DPI・全media状態とfilmstripの今回の可視検証は残る。全goalは未完のまま継続する。

2026-09-11 I03/U10元寸法付きdisk preview checkpoint: direct静止画サムネイルの元寸法をcache PNGへ保持し、memoryが空でも原寸前に再利用する。専用情報の位置／CRC／寸法・1 MiB読取上限を検査し、旧cacheは破棄・寸法推測せずthumbnailとして維持。4形式／8向き・不正情報／取消と原寸前通知・全原寸一致／preview退役を回帰確認。可視UI／cold／全品質／資源と全台帳は未完。

2026-09-11 U10直接静止画サムネイルcheckpoint: キャッシュと専用高速経路を優先し、それ以外の静止画を原寸RGBA128 MiB内で直接decode／nearest縮小して既存PNG cacheへ保存する。4形式の全sample画素／alpha・budget／取消、disk再利用とanimation fallbackを回帰確認。24MP PNG warm ReleaseはAPI取得約514→141ms。Computer Use更新後もpipe不可のため可視確認は保留。PNG本表示の初回・cold／全品質・資源・previewと原寸の同時重複、全UX台帳は未完。

2026-09-11 U10/I03未訪問サムネイルcheckpoint: filmstrip／tab hover／recent等の未訪問大JPEG・BMPへ既存の高速previewを共用し、原寸読込前にも元寸法付きcacheを供給。既存memory／diskは優先、非対応fallback維持。単独JPEGの先頭seekによるNoFrameも再現・修正。色／alpha／寸法・共有と小4形式の従来経路／diskを回帰確認し、24MP warm Releaseの取得は修正済みCLI約91→14ms（JPEG）／280→1.3ms（BMP）。可視UI全体、cold／全素材／GPU共有／資源と全台帳は未完。

2026-09-11 I03見開き先読みcheckpoint: 通常は隣一枚、readingは隣見開き全体をShell順で一workerが準備する。cache最大10枚／合計256 MiB、batch成功分の合計上限とwarm hitの昇格で先頭優先を保つ。実行中の後続ページも採用し、古いbatch末尾は停止する。10実PNGの画素／原寸Arc／previewと、ページ数・先頭枚数・前後／wrap／縦横／逆順を回帰確認。Computer Use pipe接続不良で可視確認は保留。初回表示・animation／より広い先読み・cold／UI時間／資源を含む全台帳は未完。

2026-09-11 I03先読み引継ぎcheckpoint: 実行中の同一先頭pathの復号を最新要求へ採用し、既存原寸cacheから受け取る。queued／別pathは待たず、取消／close／変更／失敗・予算とlease置換を回帰確認。実PNGで原寸再decode0・全画素一致、24MPの制御したwarm Release要求→結果は約118→48ms。UI時間、先読み枚数／全形式・初回段階表示・cold／資源を含む全台帳は継続する。

2026-09-11 I03 decoder再利用checkpoint: PNG／WebPの静止画判定後に同じdecoderを使い、PNG metadataの二重読取をforeground／prefetchから省く。8色形式×EXIF8向きの全画素・alpha／予算と既存animation／取消を確認。PNGの主な画素復号コストは未解消、FFmpeg直接packet案も遅く不採用。進行中先読みの引継ぎ・他形式初回表示・cold／UI時間／全資源と全台帳を維持する。

2026-09-11 I03 BMP-first checkpoint: 24bit BI_RGB BMPの疎なsample行から先行表示し、元寸法・上下方向・paddingを保つ。24MP warm Releaseの先行取得約0.9ms／原寸約76ms。原寸不変、非対応alpha fallback・上限・取消とJPEG共通mailbox／cache lifecycleを回帰確認。Computer Use接続不可のため可視確認は保留。PNG等・cold／実UI時間・全資源を含む台帳全体を継続する。

2026-09-11 I03 JPEG品質checkpoint: 9種の通常／progressive・gray・直接RGB・CMYK（黒版あり含む）で独立decoderと原寸への代表色比較、寸法・alphaを確認。grayは通常回帰、外部生成matrixは明示opt-inとし、production変更はない。YCCK／ICC・写真全画素・可視latency／他形式の初回表示と全台帳を継続する。

2026-09-11 I03転送checkpoint: rendererの全texture生成でArc<ColorImage>を保持し、余分な画素cloneを省く。partialだけCOWとし、実GPUで再現したRowPitch無視の行消失を修正。7幅のGPU画素・共有元／所有権と既存描画を確認する。24MP転送単体はRelease中央値約31→23ms、UI end-to-end／常駐memory改善を示す値ではない。初回表示・cold・全JPEG品質／資源peakと全台帳は継続する。

2026-09-11 I03 JPEG-first checkpoint: 大きなJPEGの原寸cache missへ同梱FFmpegの縮小復号を追加し、元寸法・EXIFを保つpreviewを一件mailboxで先行通知する。原寸／先読み経路の画質は変えず、非対応時は原寸へ戻す。8向きの寸法・代表色と、原寸前の通知／cache共用／原寸置換・failure／cancel／source変更／closeを回帰化。warm Release標本では約13msでpreview、原寸は約42→55msへ増加する。PNG等の初回preview、cold／UI end-to-end・全JPEG方式／peak、および全台帳は未完のまま維持する。

2026-09-11 I03取消checkpoint: foregroundと静止画prefetchはbuffered file読取／Seek・画素変換の区切りでも古い世代を破棄する。5静止画形式の全画素一致／途中取消と既存animation previewを回帰確認。6000×4000のwarm Release復号で20ms時点取消後の処理終了はPNG約121→25ms、JPEG約44→28ms、BMP約79→21ms。OS read／codec内計算の強制中断や初回静止画previewの実装ではなく、cold／可視UI latencyと下記全台帳は未完のまま維持する。

2026-09-11 U01/U04 caption-geometry checkpoint: native controlsの実bottomに1 physical px区切り線を隣接させ、96 DPIの1px／192 DPIの6px隙間を解消。title barの左右6 logical px外側marginだけを除き、native境界・status余白・tab名10px余白は保持する。最大化で画面外となるtop insetを避け、最大26 logical pxの行へlogo／tabを中央配置する。通常・最大化×96／192 DPIの可視画素、logo clickとnative close guard／Cancel後268,160画素一致／Discardを確認。任意UI倍率・全DPI・Windows 10・全media・物理menu key・drag latencyと残台帳は継続する。

2026-09-10 U10 shared-preview checkpoint: PreviewCacheをhost全体へ共有し、同じkeyの生成／probeを一件に集約。RGBAの64件／16 MiB枠はwindow数で増やさず、duration成功値は別の64件枠とする。取消中のwaiterは生成側を止めず、生成失敗／取消はleaseを解放。別keyは並行処理し、foreground縮小seed・元window閉鎖後の再利用を維持する。競合／取消／失敗再試行・source変更／上限と複数Applicationでの共有を回帰化。実時間の改善幅、cold先行生成／動画sheet／GPU texture共有は未認定。

2026-09-10 U08 launch-routing checkpoint: 同じユーザーSID／session／実行ファイルからの入口を同hostへ集約し、通常どおり新windowを開く。UTF-16絶対path／Welcomeだけをmessage-only HWNDで転送し、startup ackを待つ。mutexはhost寿命と初期競合の調整、転送先のexe／SIDは別に照合。実processで並行転送・拒否／timeoutと再送なし・終了後のmarker解放、非表示HWNDで実IPC・file／folder／Welcome・元state保持とGPU共有を確認。旧独立processの回収は行わない。可視Explorer／foreground・重なり／mixed-DPI・性能と全台帳は継続する。

2026-09-10 U08 merge-input checkpoint: 通常のtab外dropから同host既存windowへの結合を接続。source→screen→targetの座標変換・topmost root照合、実tab strip／Welcomeのgap・挿入線／端scroll、releaseでの再検証と既存stage／live state移送を使う。成功後だけtargetへfocusを渡す。3幅×3密度でgap／clip／stale layout・inactive scroll、非表示HWNDでdrag→indicator→release／dirty animation・Welcome／modal・body・古いreleaseの拒否を確認。非表示試験はOS hit選択だけを注入しており、可視実入力・重なり／mixed-DPIは未認定。別process入口の所有権と全UX台帳・資源／速度を継続する。

2026-09-10 U08 hosted-filmstrip checkpoint: サムネイルの新window要求を同じhost／deviceへ接続し、元tabの履歴・保存先・再生session／時計を変えずに元ファイルを独立して開く。queueの重複や古いtab／媒体／folder・modal／overlayを拒否する。非表示HWNDで画像・無音音声／動画・壊れた画像、同deviceへのhardware frame描画、初期化前後／missing path失敗時のfilmstrip保持と子破棄を確認。起動受付とasync decodeは分離し、受付後の媒体エラーは子windowへ表示する。既存windowへの通常drag結合／indicator、別process入口の所有権、可視window／mixed-DPI／性能は引き続き未完。

2026-09-10 U08 image-transfer checkpoint: 画像をactive／retained状態から同一hostの新windowへ移す。context用textureのstage失敗では元tabを残し、アニメーションframe／deadline／sampling、未保存編集と元画素、読みかけページ／エラー／previewを保持。不足ページ／処理中resampleだけを再開し、decoded予算は保持済みbytesを含めて維持する。原本ファイルなしでの画素共有・Undo、real画像worker、非表示HWNDの通常detach action・focus・反復移送・共有GPU復旧・close guardを確認。filmstrip同host化と既存windowへの通常drag結合／indicator、可視window／mixed-DPI／速度は引き続き未完。

2026-09-10 U08 live-transfer checkpoint: 音声／動画を同一hostの新windowへsession／未保存編集ごと移す入口と、不変originによる移送後の通知配送を接続。非表示の所有HWNDでactive／非active動画の停止frame／位置・focus、無音音声の再生／停止とrepeat／shuffle、元window削除後の通知、初期化前後の失敗時の元tab保持を確認。画像のcontext間移送・filmstrip同host化・通常のwindow間drop／結合indicator・物理入力／mixed-DPIは未完。新window作成成功後だけsourceを外し、最後のtab移動後はWelcomeを残す契約を採用する。

2026-09-10 U08 filmstrip追加checkpoint: サムネイルからの外dragをpath-onlyの新window要求へ接続し、既存previewを共用。元tab・dirty edit・保存先・再生を変更せず、起動失敗ならfilmstripを残す。preview無効化を含む11取消条件／batch input／pointer gap・世代／所属確認と実GPU描画／取消を検証する。子windowの実起動・通常window入力は未認定。既存tabの結合・状態移送についてownerへ非blocking確認中であり、その回答と採用契約を次工程へ反映する。全台帳を継続する。

2026-09-10 U08 drag追加checkpoint: tab本体をpointerへ追従させ、隣接tabを即時に投影移動。端の横scroll、release時だけの確定、batch press/move/release・CursorLeft後の外drop、11取消条件とfresh press復帰を接続する。3幅×3密度と実GPU復旧前後の往復／media上取消・履歴／transport保持を確認。次はwindow間結合・状態移送とfilmstrip入口。既存path-only detachをその代替の完成とはせず、他の全台帳を継続する。

2026-09-10 U07 surface追加checkpoint: 非active時に最後のD3D11VA frameを同一deviceの1枚へ独立化し、array全体の保持を解除する。H.264小画像／1080pの24→1枚、test-only readback一致、5往復・PTS／metadata・異device失敗時保持／再copy抑制を確認する。P010の実decode試験はこの環境のVP9 Profile 2初期化失敗で明示skip。復帰decoder／Seek待ちと全VRAM予算は別の残件とし、U08を含む全台帳を継続する。

2026-09-10 U07 focus追加checkpoint: media側22役割のfocusをtab別に保持し、一時widget IDの使い回しに依存せず復帰させる。読み込んだ画像／実動画・音声tabの往復、背景EOF／GPU復旧、未読込待ち・新入力優先、全画面barの初回sizingとfallback、source再読込／close時の解除を確認。次は非active動画resource／復帰遅延を照合する。通常window／物理入力／混在DPI／全UIAを含む未完台帳は維持する。

2026-09-10 U09追加checkpoint: tab名／closeのShift+F10・Menu key・UIA入口、非active対象、Escape／guard取消後のoriginとclose後の現在tab／Welcome focusを接続。右clickのanchor、reorder後の連続矢印とdisabled skipを確認し、共通menuの二重focus移動を修正する。実GPU復旧前後10点で6入口／Escape・CPU転送0。次はU07のtab別focus／復帰契約を照合し、全台帳の未完項目は維持する。

2026-09-10 M01追加checkpoint: logoの3方向を既存File／Edit／View submenuへ接続し、8px閾値／release確定・shaft移動／非選択矢印の半透明・取消と通常menuを共存させる。press所有、複数／疎な入力、閉じたparent stateの再open、guard／Undo・keyboard左右／Escapeを検証。native GPU復旧前後10点でも描画し履歴／transport不変・CPU転送0。次はU09のkeyboard context menu入口へ進み、E01残項目／U05／M01通常windowを含む全台帳を継続する。

2026-09-10 U05追加checkpoint: 草案の保存進捗をtoolbar下境界へ接続。source／編集snapshotとworker時刻による推定、normalizeの二pass、画像／未知長の有界indeterminate、取消停止／terminal解除、非操作UIAを追加。hoverで太さ・入力状態を変えず、軽い画像移動では表示しない。既存取消・guard・全画面詳細を維持。E01の残る形式／項目や他の台帳を落とさず、次にM01の明示方向menu操作を照合する。

2026-09-10 E01 JPEG UI checkpoint: 共通metadata dialogへJPEG4項目・非同期の言語別値／作者順表示を接続し、PNGと共通のSave／再Save／全Keep復元・Remove／出力形式失敗時のtarget保護／guard／source lifecycleを検証する。runtimeの形式別項目・XML validationを共有し、読取待ち／失敗・隠れた非対応項目／不正XML値はApply不可。既定JPEG保存もKeepの全値を保持する。他6項目・EXIF／IPTC／COM整合・Extended XMP・他画像形式／通常window／全codec品質を含む全台帳は継続する。

2026-09-10 15:18 E01／U02追加checkpoint: metadata設定UIと非同期既存値表示をSave／再Save／AudioOnlyへ接続し、source/tab単位保持・取消／stale／guard、全10項目・IMEを検証。全app描画がmodal内popupも毎frame閉じる不具合を再現し、metadata・画像／動画resizeの選択を修正。実GPU上のmetadata Apply／復旧と4filter選択も確認。画像metadata・通常window／mixed-DPI／全codecと全台帳の未完事項を継続する。

2026-09-10 14:54 E01追加checkpoint: 個別metadataの10文字項目をtyped export optionsへ接続。非破壊・値の事後照合・未対応形式／表記変形時の既存target保持、通常動画／音声・AudioOnlyとnormalize／timeline共存を確認。設定UI／source別保持と画像metadataは次工程で、全台帳の未完事項を継続する。

2026-09-10 14:40 E01追加checkpoint: File「Audio export options」のnormalize／channel選択を通常Save・再Save・Export as・AudioOnlyへ接続。Apply／Cancel／Escape、focus／compact・stale source／tab／generation、未保存guardと実PCMを確認。設定はtab内の現在source限りで、別曲・再読込・closeに引き継がない。実GPU復旧前後の設定UI操作も確認。次は個別metadata書換へ進み、通常window／全codec品質と全台帳の未完事項を継続する。

2026-09-10 14:15 E01追加checkpoint: peak −1 dBFS二pass normalize／Mono・Stereo変換をtyped runtime optionsへ実装。編集後PCM・静音／微小音／過大float／6chKeepとsource・target保護、動画画素不変、解析／encodeのphase／cancelを確認。設定UIと保存option保持は次工程で、通常操作からはまだ有効にできない。metadata書換と全台帳を継続する。

2026-09-10 13:58 E01追加checkpoint: 動画からの音声のみ別名保存をFile menu／custom binding／native7形式と既存非同期workerへ接続。best audio、時間／局所編集、再open・lossless PCM・独立timeline sample列とsource／既存target／動画Save状態・離脱guard保持を確認。normalization／channel変換・metadata書換と全台帳の未完事項を継続する。

2026-09-10 13:34 V04追加checkpoint: 音声`,／.`を前後10msの微小Seekとして実装し、View／custom bindingへ単位を明示。停止・累積・先頭／実EOF・編集時間軸／rate・範囲外移動と履歴／source保持を実WASAPIで確認。動画は実フレーム、音声は時間単位でありPCM sample／圧縮frame境界移動ではない。次はE01の書き出しオプションへ進み、全台帳の未完事項を維持する。

2026-09-10 13:23 V05追加checkpoint: 動画resizeのCtrl+R／Edit menu／比率・寸法・4filter／GPU preview／Apply・Cancelを接続。共通snapshot／budget、古いtoken・不正値、identity・focus／overlay、Undo/Redo・保存再読込と実D3D11VA CPU転送0を確認。次はV04音声のframe相当操作。通常window・全素材／持続性能など全台帳の未完事項を維持する。

2026-09-10 13:04 V05追加checkpoint: GPUの4方式resample／符号付き中間／係数cache／512 MiB合算予算を接続。縮小の色間引きを保存側で明示無効化し、WARPの寸法・pattern・合成と実GPUの1080p／4Kのreference比較を通過。次は動画resize操作UI。全素材品質／性能・通常windowの認定を完了扱いにせず、全台帳の残件を保持する。

2026-09-10 12:40 V05追加checkpoint: 動画resize／resampleのcore値と4方式のsoftware保存基盤を接続。OpenH264の実拒否から最小16pxへ揃え、偶数寸法・SAR1、4方式×7条件と合成順序、時刻・音声PCM／原本・既存target保護を確認。GPU4方式／中間精度・予算／操作UIは未接続のまま次へ進む。全台帳の残件は維持する。

2026-09-10 12:27 V05追加checkpoint: 動画の表示zoom／panを共有viewとcommandへ接続。1×／1.25×／2×のSAR・physical寸法、cursor基点／UV clip、取消・context・保持とzoom前後の保存全画素を確認。実D3D11VAでもCPU転送0。resize／resampleと通常window／全素材品質・性能、全台帳残件は継続する。

2026-09-10 11:04 I06追加checkpoint: 動画自由回転のcore値とsoftware exportを追加。SARを保つsquare-pixel化、RGB8の維持、偶数黒canvas、既存crop／quarter turn／flip／再回転との順序と保存前geometry照合を回帰確認。表示matrix付き素材のtrim／rate／audioも比較した。動画のGPU表示／操作UIは未接続で、下表の動画自由回転と全残件は未完のまま維持する。

2026-09-10 11:22 I06追加checkpoint: 同一device内の順序付きGPU rasterと再生入口を追加。寸法別texture再利用／512 MiB payload予算、WARP上の画素・黒canvas・SAR／全8 orientation・合成順序を検証。動画のUI／適用前予算確認／編集後geometry・selection接続、HDR・通常window／性能は引き続き未完。下表を含むUX全体のgoalをこの基盤へ縮小しない。

2026-09-10 11:45 I06追加checkpoint: 動画の角度command／modal／slider／実映像GPU previewを接続。context・custom key・非編集取消・geometry／budget事前検証、crop／再回転／Undo/Redo・保存再読込とsoftware／D3D11VAを回帰確認。動画Alt-drag、通常window外観／全素材品質／性能と台帳の全未完項目を継続する。

2026-09-10 12:12 I06追加checkpoint: 動画Alt保持左dragを角度UIと共通のGPU preview／検証／確定へ接続。全canvas Fit、1×／2×、12取消条件、release順序／所有権、Undo／0度／保存再読込と実D3D11VA CPU転送0を確認。次はV05動画zoom／resizeの契約照合。通常window入力・外観／全素材品質／性能を含む全残件は維持する。

「未完」は新goalの残件。「要照合」は採用済み契約・実装・実画面の追加確認が必要であり、完了扱いではない。

| ID | 要求・到達状態 | 初期証拠／残件 |
|---|---|---|
| U01 | ネイティブ角丸・境界・caption controls、重くないwindow drag | 主実装をcheckpoint化。DWM caption＋同一deviceの入力透過child surface。角丸、標準button hit、drag／double-click／最小化／最大化／復元／fullscreen、PNG・短いhardware動画、UIA操作とguard、pointer resize、画像と再生／一時停止動画の復旧を確認。標準system menuのpopupも確認。96／192 DPIの全画面移動で旧寸法が残る問題と復元時の二重拡大を修正。可視3台・両方向2周のbounds／動画画素と最大化復帰／guard mouseを確認。非activeのnative背景だけが灰色になる不一致を公開caption-color属性で修正し、96／192 DPIの可視黒背景・bounds保持・赤いclose hover／native終了を確認。native glyphのinactive色は保持。左右外側paddingとbar下端の隙間を除き、通常・最大化×96／192 DPIで区切り隣接と行配置を確認。全DPI比率・物理キーのmenu操作・drag遅延の定量比較は継続。Snap候補は基準機の設定で無効なため表示未検証 |
| U02 | modalのnative利用をコード量・操作性で判断 | 採用範囲をARCHITECTUREへ記録。exportなしの通常未保存確認をTask Dialogへ移し、既定Cancel・明示名・STA所有・v6 manifest・二重表示防止・失敗時編集保持を接続。実96／192とSDK UIA Cancel／Discard／Save、保存画素・別window独立操作を確認。168文字の日本語／&名で通常／fullscreen×96／192の表示と辺focus復帰・1pxキー調整、title CloseのCancelを確認。blocked UIでfocus解放後のCancel／worker失敗も回帰へ追加。live export／scroll長文error／編集form／tooltipは状態同期・preview・実装量の理由でeguiを維持、file picker／故障通知はnative継続。metadataと画像／動画resizeのpopup保持は既存回帰あり。全素材・最大長・全focus・倍率・UIAクライアントの確認は継続 |
| U03 | Codicon、Figtree＋日本語UI font、数字の等幅 | 主対応済み。monapadのFigtree／Monaco Codiconを同梱、既存tnum字形を再生成可能な派生fontへ固定。Yu Gothic UI Regularのfaceを優先し、glyph・等幅・UI配置と実日本語画面を確認。今後追加する操作のiconとnative caption後の最終照合は継続 |
| U04 | grayscale配色、barの2境界、logo／tabの中央揃え・左寄せ・一定padding | 一部対応。基本バー・共通widget状態色・clear色、logo／tab中央と左10px余白、timeline上へ移る2境界を実装・検証。overlay／入力欄の既定色、palette・playlist・filmstrip固有色を共通色へ修正し、色・compact配置・hover／focus操作を回帰確認。全media状態での最終照合、font/icon変更後の配置確認は残る |
| U05 | 重い保存等の進捗はtoolbar下境界、軽い画像移動で点滅させない | 主経路実装。保存jobのsnapshotへtrim／区間編集／rateを反映し、workerの出力時刻から推定。normalize二pass、静止画／未知長はindeterminate、取消停止／完了・取消・失敗で境界復帰。既存取消／guard維持、UIA進捗、100／125／200%×狭幅の1物理px・hover不変、画像読込非表示、tab切替後の実保存と実GPU復旧前後を確認。通常windowの見え方／物理入力／mixed-DPIの最終照合は継続 |
| U06 | Welcome tab常在、Open file/folderと最近開いたfile | 主要実装済み。空選択をWelcome identityへ置換し、初回Open／最後のcloseを往復。直近40件のpath-only履歴をworkerで永続化し、複数window更新をlock下でmerge、破損fileは保持して警告する。可視cardだけ既存filmstrip worker／低解像度cacheを共用し、thumbnail／waveform・duration・左揃えfile名から開く。Windows 11でPNG／MP4／WAVの履歴、順序更新、正常終了後の再起動復元、UIA OpenとWelcome復帰、狭幅gridを確認。picker取消後は同じ対象・別modalなしなら元のfocusへ戻し、Open File／Open FolderのEnter再操作を確認。session復元や未保存backupは追加しない。全screen reader／物理入力matrixはG01で継続 |
| U07 | tabごとの全表示／再生状態保持、背景音声・複数動画、非activeの表示負荷抑制 | 一部実装。画像の画素／view／読書状態に加え、通常UIで動画／音声session・clock・view・bar開閉・取得済みduration/waveformを保持。複数sessionの背景音声・既知／未知終端・停止位置・fault隔離・close・全sessionの同一device復旧を検証。既知終端の非active動画はdecode停止、未知ならclock同期の有界処理。音声は復帰時に再Openせず、映像も開いた入力を再利用する。保持した最終frameを復帰直後に再描画し、現在位置の新frameへ置換する。非activeのhardware frameは同一deviceの1枚へ独立化し、NV12の24枚pool参照解除を確認。P010実decodeは明示skip。decoder再構築／Seek／置換待機と全VRAM予算は残る。playlist／表示中filmstripのscrollとtimeline高さに加え、media controlのrole／項目path別focusを保持。22役割・画像再読込なし復帰と実GPU／WASAPI切替・復旧を確認。通常window／物理入力／混在DPI／全UIA、全resource予算、device／endpoint失敗の全組合せは未完 |
| U08 | tab dragの連続性、window分離／結合、filmstripから分離、drop indicator | 一部実装。同SID／session／exeの起動をpath-only IPC＋startup ackで同hostへ集約。通常window間drop／gap挿入線・inactive端scroll／Welcome結合、filmstrip独立Open、画像／音声／動画tabのlive state／未保存編集移送を接続。実process転送／timeout／owner終了、非表示HWNDで起動／state／GPU共有、OS hitを注入したdirty画像結合を確認。画像frame／画素Arc・不足ページ／preview／resample・予算、再生／repeat／shuffle・通知・focus／復旧を維持。可視二windowで空白drop拒否を再現・修正し、末尾挿入線／未保存回転／foreground／media拒否／Welcome戻し／native空白dragを確認。Welcome復帰後の黒画面という先の記録は画像の誤読であり、旧3枚と診断なしReleaseの新しい3往復で映像48,140画素の完全一致を確認し撤回した。外dropのrelease位置を無視する欠落を修正し、先頭slotのgrab offsetを保持して表示前に配置。可視の先頭／2番目動画tab、未保存回転／foreground／画素一致、所有する別process windowによる遮蔽時の誤結合拒否と解除後の結合を確認。filmstripも浮遊カードのrelease位置へ独立Openし、元の編集をコピーしない。可視の生成動画／PNG／無音WAVで配置／foreground／元state保持、編集済み元動画と未編集子の画素一致を確認。release点のmonitor作業領域へ位置補正し、移動先DPIでgrabを再計算。3台／96・192 DPIで端・中央、可視の右端操作部／動画移送・filmstrip右下を確認。custom captionのDPI余白再加算を修正し、nativeの3台2周／可視3周で通常windowのサイズと編集済み映像を保持。全画面中のmonitor移動と終了時の二重拡大も修正し、96／192 DPIの3台・両方向2周でbounds／復元／映像を保持。最大化からの切替と別DPIからの最大化復帰／guard mouseを確認。全DPI比率／Explorer・全media／codec・移送速度は未完。旧独立processのlive stateを回収する仕様ではない |
| U09 | tab context menu、閉じる操作群、path copy／開く、reopen closed | 主要実装済み。tab ID固定のright-click menu、close／other／left／right／all、path copy／Explorer選択、Ctrl+Shift+Tによる直近32件のpath-only再表示。menu／palette／custom shortcutで共有commandを使い、dirty tabごとのSave／Discard／Cancelとexport中の保護を確認。Windows 11の実windowで非active対象・clipboard一致・右側／他／全tab close・Cancel／Discard／Welcomeから再表示・Explorer選択を確認。tab名／close buttonのShift+F10・Menu key・UIA入口とorigin復帰、guard取消・close後の現在tab／Welcome復帰を追加。reorder後の連続矢印／disabled skip、3幅×3密度、実GPU復旧前後10点の6入口を確認。96／192 DPIの通常windowでTab focus後のMenu key／Shift+F10、disabled skipとEscape後の再操作を確認。他比率・全体UIA監査は残件。sidebar／pin／preview-tab／追加button／未保存backupは要求外 |
| U10 | tab hover／filmstrip／recent／seekの低解像度preview共用と速い表示 | 一部実装。同hostの全window／workerで64件／16 MiBのRGBAを共有し、同keyの生成を集約。duration成功値も別の64件枠で共有・probe集約する。原寸読込後の240×160以内のpreviewは元window閉鎖後も再利用。GIF／APNG／animated WebP／AVIFは原寸の先頭frameから初回読込中にも供給する（別decodeなし）。先行変更で既読PNG＋GIFのfilmstrip初回観測301.356→84.384ms、warm再表示は約65～85msで明確な差なし。今回の共有変更の実時間は未測定。tab hoverの画像／音声はfilmstrip、動画はseekの16コマsheetをCPU／diskで共用し、非active動画は最後の観測位置。recent可視gridも同じcacheへ接続。隣の静止画先読みからも共有previewを供給し、縮小eviction後は保持中原寸から再供給する。動画sheetの現在位置先読み／hover優先と単枚fallback、可視windowの停止中hover／Seek／tab表示を確認。animationの先頭preview先読みも接続。可視速度差、GPU texture共用と全素材性能は未完 |
| U11 | 選択線は反転色1pxのみ、不要なgrip／shadow／暗幕なし | 主経路実装。画像／動画／時間選択を1物理pxの反転枠へ統一し、暗幕・gripを除去。125%の角重複、geometry／app回帰とWARP／実GPU readbackを確認。通常windowの生成PNG／SAR動画でも96／192 DPIの枠全画素の反転と内外塗りなしを確認。画像の辺resize／範囲内clickも通過。時間端点／音量／長さに残っていたfocus四角を除き、画像と同様に対象と値をstatusへ表示。192 DPIの4対象focus前後245,760画素一致と96 DPIでのキー調整を確認。音声通常96／192とfullscreen192の反転枠・4対象focusを追加確認。画像fullscreenの辺focusでstatusが隠れる欠陥を修正し、実UIA96／192の4辺・media全画素一致・キー調整と100／125／200%回帰を確認。hit領域・keyboard／UIA・数値ラベルは保持。全比率・素材・全focus／UIA監査は継続 |
| U12 | compact seekのhoverつまみを両端内に収め、非hoverは全幅1px | 対応済み。つまみ半径を除いた移動区間を描画・hover・releaseで共有。100／125／200%の自動描画・座標回帰と通常releaseの画像両端表示を確認。実OSの混在DPIは未検証 |
| I01 | Fitの余分な8px余白を除き、Cover表示command／shortcutを追加 | 対応済み。core/image.rsのCover、共有CoverWindow commandとShift+C、main.rsのviewport。3新規回帰と通常releaseの960×576画素比較によりFit全幅／Cover全領域・bar非侵入を確認。下記実績参照 |
| I02 | 読書modeは隙間なし連結、重複しない見開き送り、先頭枚数offset、枚数／offset drag＋shortcut | 主要対応済み。8837892の固定ページ分割にcursor固定dragを追加。上下／左右の主軸を固定し、release確定・Escape／focus／resize／離脱／overlay取消で解放・復元する。active/edit対象保持、drag直後の左右送り、Windows 11の通常／fullscreenとUIA Toggle、1px固定範囲から元のdesktop範囲へ戻ることを確認。follow-up追記のdirty開始禁止／読書中の編集・Undo/Redo禁止／同輪郭filled iconも追加し、実windowでdisabled状態・別dirty tabへの通常表示復帰・読書解除後のRedo再開を確認。100／125／200%の相対移動は自動回帰、実OS混在DPIは最終matrixで継続 |
| I03 | 高速な画像移動、decode／表示分離、取消、先読み・段階表示の適切な採用 | 一部実装。latest-only foregroundと独立した一worker先読み、10枚／256 MiB共有decode cache。通常は前後近隣最大9枚（距離優先・同距離は直前方向）、readingは隣見開き全体を合計予算内で逐次準備し、実行中の同一ページは先頭以外もforegroundへ採用する。先読み結果と既存原寸hitから元寸法付き縮小previewも共有し、追加decode／表示通知なし。6000×6000 PNGのtitle完了中央値220.113→31.840ms、31連打後のtargetを確認。見開きの逐次公開に加え、共有memoryに元寸法付きpreviewがあれば独立workerで取得して通常／readingの原寸読込中に表示する。大GIF再訪は約36ms標本でpreview（原寸Loading）を確認、baselineは約214ms標本まで黒。原寸のzoom／crop／rotate座標と同じ描画、旧結果／読込済みページへの上書き拒否を検証。初回GIF／APNG／animated WebP／AVIFは原寸decoderの先頭frameを縮小し、世代付き一件mailboxで全frame完了前に通知する。取消／source変更／close、原寸成功／失敗のpreview退役と実codec画素一致を確認。大きなJPEGは同梱FFmpegの1/8復号を用い、EXIF／元寸法を保つ初回previewを原寸前に通知する。同key生成中は重複も待機もせず原寸へ進む。生成標本ではpreview約13ms／原寸約55ms（先行なし約42ms）。animationは先頭frameだけを既存workerで先読みして縮小cacheへ供給し、原寸cacheには登録しない。その他静止画のfirst-decode／cold-storage、可視速度差／連打全般の待機低減は未完。近似previewと原寸を混同しない |
| I04 | 画像の移動keyと端点・複数枚jump、reading時の役割 | 主実装済み。左右／Home／EndにPageUp／PageDown／Backspace／Space／A／D、Ctrl+数字で1～10枚、Shiftで逆向き、Ctrl+Space／Backspaceで5枚を追加。Shell順の画像数で数え、jumpは端で停止、通常reading移動は見開き単位。採用済みCtrl+左右＝同種一枚は維持。旧設定の限定移行、customキー／prefix・記号・text focus保護、dirty Cancel、実画像の非同期読込・画素／tab／見開き／端点no-reloadを回帰確認。通常window／keyboard layout／IMEの最終確認は残る |
| I05 | 画像／選択範囲のclipboard copy、resize/resample、interpolation | 主経路実装済み。Ctrl+Cの編集後frame／selection／透過RGBA、Ctrl+Rの寸法／比率／4補間／非同期処理／Undo/Redo／保存PNG一致を検証。View／paletteに表示専用Smooth／Nearestも追加し、画像・animation・読書・共有cache・復旧dataとcopy／履歴不変を回帰。固定rendererのtexture options無視をsource-only修正版で解消し、WARPの混在sampler／partial更新画素試験をCIに追加。実windowでもnearest領域246015画素が原色のみ、smoothの245692画素は中間色、copyは元16×16と一致。resize固有の実GPU復旧・混在DPIの追加監査は未完 |
| I06 | preset aspect selection、自由回転、readingの回転／反転alias | readingのR／L配置切替・H／V順反転を接続済み。比率preset7種をEdit menu／palette／Ctrl+Kの後に1～7へ追加し、編集後寸法・SAR・context・履歴・PNG保存画素と実動画cropを回帰確認。画像自由回転のcore値／非同期raster／export、menu／Ctrl+Shift+R／角度dialog／slider／近似配置previewに加えAlt保持左dragを接続。0.1度、外接寸法、alpha補間、PNG一致、animation・Undo/Redo・tab／stale拒否、取消／0度非編集、focus復帰、custom binding衝突、320×300 scroll、drag所有権・release順序・scale・有界描画を回帰確認した。動画のcore／SAR／software保存・単一device rasterに角度UI・適用前budget／編集後geometry・selectionを接続し、実動画／D3D11VAで検証。動画Alt保持dragも同じGPU preview／確定へ接続し、1×／2×、取消12条件、入力所有権・release順序・Undo／0度・保存再読込を回帰確認。HDR／全素材画質、通常window入力／appearance・性能は未完 |
| I07 | 画像Ctrl＋wheelの連続拡大縮小を平滑化待ちなしで即時反映 | 入力経路修正済み。各raw eventの単位・量・修飾key・pointer座標で処理し、同frame meshへ全倍率を反映する。30／120Hz相当の入力、90frameの残量なし、逆方向／同frame複数座標、texture再生成なしと所有権を回帰確認。実マウスからの可視latency・全DPI／連続負荷は未検証 |
| I08 | 画像端までのpan・中央固定、Grabbing、scrollbar／wheel／Shift横scroll | 主経路実装。表示panを既存barと共用し、はみ出す軸だけ移動可能。100／125／200%の両軸／片軸・Fit／resize・drag取消・bar／wheel・編集／texture保持と選択辺focusを回帰確認。可視操作はComputer Use activationエラーで保留。全入力／bar外観／UIA／mixed-DPIは未完 |
| I09 | 選択角の二辺resize／比率保持、方向cursor、範囲内右drag移動、外部click解除、crop preview撤去／範囲zoom | 実装済み部分の回帰確認を継続。四隅・Shift比率／対角固定・方向cursor、画像内のpixel寸法固定移動、外部click解除・取消／barでの選択保持を確認。crop previewは通常の範囲zoomへ置換し、回転後・100／125／200%の入力回mesh／barと編集／texture保持、旧設定引継ぎを確認。可視操作・全gesture／DPI／動画corner品質は未検証 |
| V01 | seek上dragでtimelineを開く、専用buttonを除く、timeline中thumbnailなし | 実装・検証済み。click閾値後の初動が上優勢なら展開のみ、横／下が先ならrelease時Seek。T／View／paletteは動画専用、fullscreenから展開時は通常windowへ戻る。専用buttonとtimelineのhover thumbnail生成／表示を除去。通常releaseで20秒を保持する上drag、横→上でもSeek維持を確認。V03の時間選択編集は別の未完事項 |
| V02 | 区間低解像度previewの先行生成・即時hover、drag中も同じpreview表示 | 一部実装。短編20区間／長編約5秒間隔、16コマ4×4 sheetを現在位置から先読みし、hoverで別sheetを優先。Seekは2枚LRUで同textureのUVだけを切替、tabも共有memory／diskを参照する。準備中は単枚fallback。実H.264の16コマ生成約1.119秒／memory取得約0.56ms、同textureの16位置で追加uploadなしを確認。可視960×576の生成MPEG-4で停止中hoverの非Seek、前半／後半・クリックSeek・tab表示を確認。本画面scrub、長GOP／全codec・mixed-DPI／初期応答分布／peak負荷は未完 |
| V03 | timelineの時間選択・範囲再生・内外削除・連結、部分音量／速度、rubber-band | 部分実装。区間model／export／再生／appの時間軸・Undo/Redo・波形再配置は接続済み。旧trim gripを横dragの時間選択へ置換、CTI drag／clickはSeek。Delete／Ctrl+Y／Ctrl+A／I／O／UIA端点、tab保持・取消・stale拒否、通常releaseの0.5～1.5秒選択→Delete→Undo→Keepと選択履歴export再decodeを確認。部分音量線の縦drag／Alt+横drag stretchと数値UIA・keyboardを追加し、release確定／取消・混在gain・区間速度制限・選択保持を回帰確認。通常releaseでも部分mute→50%→2秒から2.5秒へstretch→Undoを確認。Shift+Spaceの範囲再生／Space停止再開／Escape解除、元の編集軸・履歴保持、背景末尾停止とaudio repeat／auto-next抑止も実装し、通常releaseで1.5秒停止→通常2秒終端復帰まで確認。difference枠はU11で共通実装しWARP／実GPU pixelを確認。通常window・全focus/style監査は残る。長い削除区間のdecode負荷・初期Seek位相・継ぎ目の音質、最終UI/export一致も残る |
| V04 | 動画の閲覧／編集contextでshortcut競合を解消し誤編集を防ぐ | 部分実装。visual選択・crop・回転・flipはtimeline表示中だけ有効。共通command判定とpointer／UIA／辺focusを揃え、閉じる／全画面で途中操作を取消し確定選択・編集結果を保持。視聴・保存・Undo/Redoは維持する。compact seekを毎frame取消す初期不具合を同一frameの回帰で修正。既存の動画export画素照合も維持。J/K/Lを既存Seek／再生commandの追加bindingへ接続し、動画編集中は主bindingのL回転を優先する。custom主binding／prefixの保護、旧設定の限定移行、複数bindingの保存・再読込、hidden window/sessionのSeek／K停止再開を回帰確認。動画comma/periodを実PTS探索へ接続、速度はCtrl+comma/period、旧標準だけ移行。32操作の順序・世代／tab／modal取消・表示frame基準・一時停止・Delete/stretch区間往復をhidden-window回帰で確認。DTS索引のB picture欠落を再現し、video Seekのkey PTS確認で修正。全参照PTS／画素、VFR／B picture／TS／第2streamを照合。長押し2倍速を動画視聴面／共通再生buttonへ接続。400ms静止保持、短いclick維持、元rate/pause復帰、履歴／編集plan／選択再生範囲の保持をhidden-windowと無音WASAPIで確認。focus／Seek／tab／EOF取消、描画frame不在時のreleaseとnative短click経路も回帰対象。音声frame相当操作は10ms微小Seekとしてcomma／period・Viewへ接続し、停止・累積・実EOF／編集時間軸・rate・範囲外移動と非編集を確認。通常window・長GOP／全形式の微小Seek精度・遅延は未完 |
| V05 | 動画のzoom・resizeと既存crop／rotate／flip／fullscreen | timeline内のCtrl＋wheel／+・-／100%／Fit／Coverと右drag panを接続。SAR／physical倍率・cursor基点、viewportとUVのclip、modal／overlay／取消、timeline／fullscreen／tab保持と保存画素不変を回帰確認。実D3D11VAの復旧前後もCPU転送0。resize／resampleのcore値・4filterのsoftware保存・SAR1／合成順序・source照合、GPU4方式／符号付き中間／係数再利用／512 MiB予算と代表画素・速度を確認。Ctrl+R／Edit menuの寸法・比率・4filter／preview／Apply・Cancel UI、identity／snapshot／古いtoken拒否、focus／overlay・Undo/Redo・保存再読込と実D3D11VAも接続。通常window／混在DPI・全素材品質／持続性能は未完、単一device・preview/exportの対応を維持する |
| A01 | 音声の自動次曲、repeat all／one／off、shuffleとbuttons | 主要経路実装。tab別のShell順auto-next、repeat off／all／one、shuffle一巡、前後操作、status buttons／View／palette／音声Ctrl+R。曲末にShell順を非同期再取得し、初回取得前のtab切替にも対応。実WASAPIでactive／背景の次曲・loop・dirty guard・古い通知拒否・失敗隔離を検証。通常releaseでbuttons／shortcut／自然EOFの次曲を確認。modeのrestart永続化、gapless、手動選曲の独立した履歴stackは提供しない |
| A02 | 音声timeline常時、動画共通の選択編集・音量／速度操作 | 音声timelineはfullscreenでも常設、Tでは閉じずcompact seek／専用buttonなし。V03共通の時間選択・Delete／Keep・UIA端点を接続。共有rubber-band音量／Alt+drag stretchと数値操作も接続。共有範囲再生も接続し、repeat off/all/oneと背景停止を実WASAPIで確認。最終操作監査は引き続き未完 |
| E01 | metadata書換、音声抽出、normalize、stereo／mono export | 部分実装。音声のみ出力、normalize／Mono・Stereo設定とSave連携済み。動画／音声の10文字metadata設定UI・非同期既存値・source別保持・再probe／既存target保護を検証。PNG→PNGの10項目とJPEG→JPEGのXMP7項目もFile／custom command・既存値UI・Save／再Save／guard／source lifecycleへ接続済みで全Keep／設定未使用でも保持。JPEGは言語Alt／作者SeqとxmpDM Album／Composer／Genreの単純文字値・Keep／Set／Remove、非XMP bytes・EXIF／画素不変、有界parse・取消・target保護を確認。JPEGのAlbum artist／Date／Track、他形式／EXIF・IPTC・COM整合／Extended XMP、通常window／全codec品質は未完 |
| M01 | logoの三方向menu gestureと最小限の状態表示 | 主経路実装。8 logical px・右上File／右下Edit／左下View、releaseでsubmenuだけ開く。shaftの80ms移動と非選択矢印の半透明、取消／所有・source/tab／graphics世代、普通のclick／keyboard／UIA、guard／一回Undoを検証。3幅×100／125／200%の再openと形状、実GPU復旧前後10点の3方向／Escape・履歴／transport不変・CPU転送0を確認。2026-09-11の通常960×576 windowで生成PNGの3方向、View左右キー、Escapeのlogo focus復帰と通常rootを可視確認。不発を追跡し、release後退出の一括取消とbatched press／moveの遅延drag owner移動を回帰再現・修正。最終Releaseで以前不発だったView→Escape→Edit→Escape→File、通常clickが成功。最大化時のcapture境界エラーは未解消で、全境界条件／最大化／混在DPI・全focus/style監査は継続 |
| G01 | menu／palette／custom prefix／media別grid／dirty guard／Shell順 | 実装あり。追加commandの全入口と重なり・keyboard／IME／UIA・Undo／保存を変更ごとに再検証する |

## 実装順

ownerの最新の明示指示により、16:48のcheckpoint後の待機指定は失効し、既存goalの以下の実装順を再開する。台帳の全残件を維持する。

1. I01の画像viewportを実装・検証し、U04／U12など画面の寸法・操作境界を整える。native caption（U01）、font/icon（U03）は独立して設計・導入する。
2. I02～I05、U06／U09／U10の閲覧flowを実装し、連続操作を測定する。
3. U07のtab/session所有を確立してU08とA01へ進む。複数sessionのdevice・音声・終了契約を実証する。
4. V03の編集modelと時間軸を確立し、V01／V04／A02／E01へ接続する。他の未完項目も台帳から落とさず、操作・外観・性能の最終照合まで進める。

この順序は小さな項目だけでgoalを完了するための縮小ではない。各行の未完／要照合が残る間はgoal全体を完了扱いにしない。

## U12検証実績（2026-09-09 16:48 JST）

- compact seekのactive表示では半径4 logical pxを両端へ確保し、pointer候補・確定値も同じ移動区間へ対応させる。極小幅では半径を縮め、1px未満の移動区間も端点へ正しく対応させる。非activeは全幅1 physical pxを維持し、timelineの座標変換は変更しない。
- 新規2 testsで端点・中央・範囲外・極小幅、100／125／200%密度のhover描画とidle全幅を確認。既存のUIA値変更・release取消を含め、fmt／Clippy／全283 tests通過。実機依存3 testsはignoredのまま。
- 通常release e41427a3、生成PNG二枚、960×576のWindows 11実windowでUIA画像位置1／2を確認。つまみの明るい画素は先頭x=0..7、末尾x=952..959、両方y=542..549でwindow内。正常終了0、生成原本のhashは不変。最初のforeground拒否では入力せず、同じ所有windowのslider focus後に確認した。
- 証拠はignored `target/tmp/image-viewport-20260909/seek-handoff/`。実画面は100%・画像位置での確認であり、動画の実pointer操作やOS混在DPIを新たに実証したものではない。配布成果物の更新・実インストールは行わない。

## U04基本バーの検証実績（2026-09-09 17:05 JST）

- 文字由来のmenu寸法と、18pxの仮の高さへ縮む横scroll内のrowを固定26px基準へ変更。title 32px／status 30pxの内容を上下中央へ揃え、tab label左10px・close領域24pxを保持する。既存Button／MenuButtonと明示tab identityを使い、独自の入力providerやUI frameworkを追加しない。
- 新規実UI回帰は320／480／960 logical px × 100／125／200% × timeline有無で、UIA node boundsと実text shape、full-width境界2本を照合。先行実装ではtabがy=7..33へずれることを検出し、scroll内rowの中央配置まで直した。UIA nodeの座標はroot transform前のlogical値であり、試験側の二重DPI除算も修正。
- 共通styleの色を変更する際、枠の太さまで変えるとpalette行の左右揃えが崩れることを既存回帰が検出した。元のstroke幅を維持して色だけ更新し、palette・menu・tab reorder／auto-scroll／focus／UIA回帰を含めM0全284 testsとClippy／formatを通過。実機依存3 testsはignoredとして区別。
- 通常release 476aa84cのWindows 11実windowで、logo／tab label／close／window controlsのscreen中心yはすべて68（window内16）。背景RGB=(0,0,0)、active tab／title下境界=(24,24,24)、hover=(76,76,76)をPNG画素で確認。960×576・400×576、menu呼出し、palette検索／Escapeを確認した。
- 生成H.264動画のTでtimelineを表示／解除。title境界はy=31、timeline上境界はy=450で#181818、旧status境界のy=545..547は黒。解除後は元のseek位置へ戻る。timeline hover thumbnailとtrim gripはまだ既存のままで、V01／V03の完了とはしない。
- 両trialは正常終了0、生成PNG／MP4のhashは不変。証拠はignored `target/tmp/image-viewport-20260909/chrome-after/` と `chrome-timeline/`。実OS混在DPI・native captionは未検証であり、残件を完了扱いにしない。

## U03 font／iconの検証実績（2026-09-09 17:24 JST）

- monapadのFigtree Regular 1.000とMonaco 0.55.1のCodiconを元のhashで固定。Figtreeの既存tnum glyphへ数字cmapだけを変更し、fontTools 4.59.2の再生成／byte一致とadvance 623を検証。元file・派生file・ライセンス・変更説明をassetsへ保持し、通常build時のdownloadやfont処理を不要にした。
- WindowsのYuGothM.ttcはface 0がYu Gothic Medium、face 1がYu Gothic UI Regular。後者を明示して選択する。日本語不足・Codicon未登録を隠さず、OS fontなしでも同梱font／iconを使える構成を回帰で確認。Hiragino Sans等の非標準fontを追加取得しない。
- 固定eguiのhas_glyphはreplacementと同じfaceに属する実在glyphもfalseにするため、family順変更後の日本語testは実glyphのatlas領域が空でなくreplacementと異なることへ照合した。元ttcのcmapにも対象13文字が存在する。11／12／14／20pxで数字幅と時刻文字列幅の一致、使用する10 iconの収録を確認。
- appのheadless UI testも同じ同梱fontへ切り替えた。新しい行高で240×150の確認見出しが1pxはみ出すケースを検出し、200px未満の確認画面の縦間隔を2pxへ詰めてbutton操作まで再検証。fullscreenの消えるclose glyphの期待値もCodiconへ更新。M0全285 tests通過、Clippy／format通過、実機依存3 testsはignored。
- 通常release 442f37daで生成「日本語画像-0123456789.png」を表示。960×576／400×576、日本語名、Codicon、menu／palette、R→close確認→Escape Cancel→Undoを確認し、原本hash不変・正常終了0。中心yは従来のwindow内16を保持。証拠はignored `target/tmp/image-viewport-20260909/font-after/`。native caption・背景tab session等は未完のまま継続する。

## I01検証実績（2026-09-09 16:35 JST）

- 通常画像はmedia領域を8px縮めずFit。Coverは二軸比率の最大値、pan中央、resize追従。Shift+CとView／paletteの共通commandを使用し、custom prefixへ変更後は旧keyを残さない。reading／動画／音声／Welcomeでは無効。
- 回帰: coreの縦横・極小／大画像・resize・手動zoom、shortcutのcontextと設定往復、実UI shapeのFit／Cover×通常／fullscreen×100／125／200% DPI×縦横resize×回転×crop previewを確認。選択／編集列を保持し、clipとmesh寸法を独立した比率計算へ照合した。
- 旧8px viewportを固定値にしていた選択辺reveal testは失敗を確認し、新viewportの0..960／32..546と最小pan 330へ期待値を更新した。焦点辺が完全に見えること・手動pan保持の試験は維持。
- 同じ生成32×16 PNG、同じ960×576 windowの通常release比較: 青色領域の両端を含む座標は、旧Fit `(8,53)..(951,524)`、新Fit `(0,49)..(959,528)`、Cover `(0,32)..(959,545)`。Fitの縦letterboxは元画像の2:1比率による正しい余白であり、Coverのみそれも覆う。status／toolbarへは侵入しない。
- paletteのCover候補とShift+C表示、選択実行、正常終了0を確認。生成media原本は不変。実画面の100%以外は自動layout試験であり、OSの混在DPIを実証したものではない。
- 診断を除いた最終通常release bfb6ce0bでもFit／Shift+C／palette検索・Enter／Shift+Wと終了0を再確認。checkpointのFit／Cover／palette画面は前の通常buildの対応PNGと全画素一致。最終format／Clippy／281 tests通過、実機依存3 testsはignoredとして区別する。
- 途中の「画像がtoolbarを覆う」という目視判定は誤り。問題と判断した保存PNGそのものの上32行を走査すると青色侵入は全て0画素、tab背景(28,28,28)／bar背景(8,8,8)も保持されていた。診断で追加したGPU前の出力ログは除去し、rendererの変更は残さない。バグ修正済みという記録にはしない。
- 証拠はignored `target/tmp/image-viewport-20260909/` のbefore-capture／final／診断trial。生成物・実行ログ・UI操作helperはGitへ含めない。残る全体外観／性能／機能は台帳の別項目として継続する。
