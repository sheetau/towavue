# towavue ロードマップ

各milestoneは前のゲートを満たしてから開始する。新機能の数ではなく、観測可能な正しさを完了条件とする。

## 現在の優先順位と完了条件（2026-09-09 16:01 owner指定）

2026-09-12 U09/U10 help-lifetime checkpoint: 通常説明の22箇所を共通HoverHelpへ接続。eguiが前frameの元rectだけでtooltipを保持する経路をclip変更で再現し、clipped bounds／layer／hitと非操作型の寿命へ修正。100／125／200%、enabled／disabled、delay、離脱／次要素／overlay、元ボタンclick、大きな自分のtooltipとの重なりを回帰確認。media previewの即時表示契約は変更しない。実OS入力・全残留経路と全UX gateは未完。

2026-09-12 U09/U10/V02/I02 preview checkpoint: previewを通常tooltipから共通の非操作Areaへ分離し、delay／fade待ちを除去、初期／変更sizeは同一frameで再配置。tabは下中央、seekはhover上中央。clipped bounds／layerを使い非active hoverとTooltip層越しのpreview対象要求を許可し、menu／別overlay／disabledは抑止。動画は画像slotを先に確保しcaption高さを固定、時刻fontをProportionalへ揃える。60秒のtooltip設定・旧tooltip・3倍率／active状態、未生成→横長→縦長→失敗の即時描画と比率／caption位置、既存seek drag／見開き／dirty tabを回帰確認。fadbda3のCIで検出されたvendor notice／sourceの更新漏れhashも整合し147 package noticesを検証。一般tooltip寿命・実画面／OS入力と全UX gateは未完。

2026-09-12 V03/A02 timeline-rendering checkpoint: #181818・radius3背景と左右／上8px・下0のmargin、内側3pxの描画領域を導入。音声／動画×3幅×3倍率で背景／波形／選択位置と二境界を確認し、初期96px・tab別resize／小窓時上限の回帰も維持。時間選択だけ白20% difference塗りと左右1物理px点線へ変更し、文字／control線の下へ描く。反転blendをalpha対応し、WARP／hardwareの0／20／100%・重なり／clip／alpha／通常描画復帰の全画素を確認。従来の画像枠は変更しない。実window／入力・全DPI監査とpreview等の全UX gateは未完。

2026-09-12 V03/A02/U12 timeline-input checkpoint: CTIを領域内の白1物理px＋三角markerとし、seek dragの開始をmarkerへ限定。線からの通常範囲選択、左右端resize／cursor／offset保持／交差clampと音量線の白50%／ResizeRowを実装。batched／別frameの一回commit、取消、既存gain／stretch／keyboard／UIAを回帰確認。seekbarは背景と再生済み白の間にhover位置まで白25%進捗を追加し、3倍率の順序・範囲・非commit・disabled／既存端点を確認。実入力・実画面は今回未確認。timeline背景／余白・difference選択塗り／点線、preview等の全UX gateは未完。

2026-09-12 U04 status checkpoint: media表示面のloading／resampling／画像・読書・再生errorをstatus左へ集約。読書drag値を優先し確定後4秒、画像成功時の情報flashを除きframe数を右へ常設。fullscreen上中央の通知は下端barへ統合しfocus取得なし・既存抑止を維持。2幅×3倍率×通常／fullscreenの一回描画・path復帰、部分ページ／複数error、読書取消・履歴と既存focusを回帰確認。Release通常画像のpath／右情報を可視確認。Computer Useの対象とのintegrity差とactivation失敗により実入力は未確認。音量HUD・caption／tab／timeline／preview等と全UX gateは未完。

2026-09-12 U04/G01 diagnostic-outline checkpoint: Debug版eguiのrect単位ID交替警告を画像tab／focus fixtureで再現し、可視診断だけを抑制。menu Escapeとtab間focus復帰の回帰で赤枠なし・元のfocus／非dispatchを確認。通常focusと実ID重複診断は維持し、Release動作は変更しない。status message集約・実画面と全UX gateは未完。

2026-09-12 U07 dirty checkpoint: tabのasteriskをclose領域のCodicon丸印へ置換し、hover／focusでは×へ戻す。保存済みsnapshotと連続した直角回転／反転の同値性でdirtyを比較し、4回回転・二重反転等ではUndo履歴を残してcleanに戻る。5,461通りの向き・分岐／保存中編集・raster境界、代表RGBA一致と3倍率のindicator／close guardを確認。全編集の同値性・実ウィンドウ／全UI監査は未完。

2026-09-12 M01/G01 menu checkpoint: 方向dragのsection中身だけをbutton下へ固定表示し、親の通常clickとcommand描画を共用。Edit方向を45度領域へ、Image jumpをView子menuへ移動。logo背景不変・gray／white線色・左余白を反映し、位置／親非表示／keyboard／取消／再openを回帰確認。初回layout時の無効な子menuへのfocus移動も修正。実画面・native caption／tab寸法と全UX gateは未完。

2026-09-12追記の未反映項目をUX_IMPLEMENTATION_PLANの追記台帳へ追加。hover背景#2C2C2Cと、音声リスト／画像scroll／既存音量領域の非アクティブwheelを修正し、入力回帰とM0を確認した。可視入力の反映は未確認。caption／tab／menu／status／timeline／preview／音量HUD／音声list等の新たな追記も完了条件に含め、従来checkpointで代替しない。

2026-09-12 I03 color-preparation checkpoint: 表示用RGBAの不透明判定を行内32画素単位にまとめ、混在行のegui標準丸めと短絡を維持。全alpha・block境界の一致を確認。100 JPEGの準備中央値は約1ms短縮し、最短33ms切替は約8.3～8.5→7.3～7.5ms。即時切替は約16.7msでほぼ横ばい。中間blank／previewとGPU可視表示・cold／全形式／資源を含む全UX gateは未完。

2026-09-12 I03 overlap checkpoint: 先読み引継ぎ修正を前提に、通常画像の復号成功受取からtexture準備前へ次画像の先読み開始を移動。同一completion末尾の再submitを防ぎ、予算・worker・readingは維持。UI contextのtexture準備を止めても先読みが進む回帰を確認。100 JPEGの即時切替中央値は約22.6→16.6ms、p95は約23.4→17.7ms。最短33ms条件は約8.0→8.3～8.5msと改善せず、blank／previewなし・GPU／実入力／全形式／資源と全UX台帳は未完。

2026-09-12 I03 100-image/handoff checkpoint: 4096×2304 JPEG 100枚のRelease計測を追加し、切替の二重空要求が進行中先読みを取消す経路を修正。要求世代を一回で置換し、原寸ready直後に次へ進むCPU描画条件の中央値は約28.5→22.6ms、p95は約30.7→23.4ms。最短33ms条件は約8msで大差なし。全100枚のpath／寸法／原寸mesh到達は確認したが、途中描画には空表示・低解像度が残る。可視GPU表示・一定周期入力の飛越し・cold／他形式／資源と全UX台帳は未完。

2026-09-12 I09 selection-zoom checkpoint: crop preview状態／選択UV切出しを撤去し、選択内clickと旧command入口を通常Custom倍率／有界panへ置換。選択枠・編集・textureを保ち、100／125／200%と回転後の同frame mesh／bar・cursor・非toggle command・解除後倍率保持・Fit復帰を回帰確認。旧設定名は読込aliasで引き継ぐ。可視入力・全DPI／gestureと全UX台帳は継続する。

2026-09-12 I09 outside-click checkpoint: 選択枠から離れた表示面／余白の短いprimary clickで選択を解除する。辺・角優先、画像内の新規drag、余白dragの無変更、取消・無効領域・最初のgesture所有権を維持する。100／125／200%描画経路でbar操作は既存選択を保ち、外側clickだけが選択を解除してpan／dirty／textureを保つことを確認。crop preview撤去／通常zoomと全UX台帳は継続する。

2026-09-12 I09 selection-gesture checkpoint: 角の二辺resize・Shiftで開始比率／対角固定・方向cursorと、画像範囲内の右dragによる選択移動を追加。移動はpixel幅／高さを保ち、画像端でclamp、共通取消で復元する。四隅×Shift有無・保持／同frame完結・上下限・非正方pixel範囲の移動と、100／125／200%の描画経路でpan／dirty保持を確認。可視操作、動画cornerの全品質、外部click解除・crop preview撤去／範囲zoomと全UX台帳は継続する。

2026-09-12 I08 bounded-image-scroll checkpoint: follow-up追記の画像pan制限・右drag Grabbing・縦wheel／Shift横wheel・overflow軸のfloating scrollbarを接続。表示panを一つの位置として共用し、zoom／preview／resize／編集後も画像端でclampする。100／125／200%のFit固定・両軸／片軸overflow・drag取消・wheel／bar・texture／dirty保持と既存の選択辺focusを確認。端の不可視操作領域は画像外panではなくviewportへのclipで保つ。Computer Useはwindow再選択後もactivationエラーで可視確認を保留。実入力・外観／全DPI／UIAと全UX台帳は継続する。

2026-09-12 I07 immediate-zoom checkpoint: 画像Ctrl＋wheelが平滑化により複数frameへ分割される状態を再現し、raw event量を同じ描画回で全反映する。単位／倍率設定を保ち、各event座標を基点に順序どおり適用し、画像textureは再生成しない。30／120Hz相当の入力回倍率と90frameの残量なし、方向反転／同frame複数入力のmesh、単位・phase・修飾key・入力所有権を確認。通常scroll／動画は変更しない。実Ctrl＋wheelの可視latency／全DPI・連続負荷と全UX台帳は継続する。

2026-09-12 I03/I04 adjacent-path checkpoint: 通常前後移動の全path Vec生成を除き、現在位置から方向順に走査して一件だけguardへ複製する。5万件の混在Shell順・前後／循環／同種filter・dirty保護を既存の選択規則と比較し、Releaseの120要求は約586→340ms。現在位置の線形検索、復号・描画とreading／音声同種queueは変えない。一枚だけの再loadは既存guardで防止済みであり、新規修正とはしない。可視latency・cold／全素材／資源と全UX台帳は継続する。

2026-09-11 U07 same-audio-open checkpoint: 現在のcleanな音声sourceを外部再Openするとsession／位置／選択／focusを初期化する問題を再現・修正。同じ表示tab・pathでFaulted以外なら再loadせず、別曲置換・dirty／export保護・強制新規・失敗時の再試行を維持する。状態matrixと既存source置換回帰、実D3D11／WASAPIで再生／停止中の再Open・世代／位置／選択／focus保持、背景再生／復旧を確認。通常windowの全入口・全UIA／IME／mixed-DPI・全UX台帳は継続する。

2026-09-11 V03/A02 audition-PCM checkpoint: 選択終端でatempo入力まで切り詰め、通常再生と波形が変わる問題を再現・修正。最後の実編集区間内で必要な後続入力を許し、出力を選択sample数で止めた時点でconsumerを正常終了する。6範囲×3速度の全PCM／長さ／PTS・chunk上限、最終chunkの拒否／取消を確認。選択自体で音を変えない契約であり、実際の短い編集区間のtempo品質・初期Seek位相・削除区間負荷・全UI／UX台帳は継続する。

2026-09-11 V03/A02 sample-boundary checkpoint: 17ms等の整列境界が浮動小数点誤差で1sample切り上がり、再生／保存の継ぎ目に無音挿入・後続sample欠落が起きる問題を再現・修正。master速度を正確な固定単位へ変換し、共通i128計算で出力sample数を求める。生成音声のgain／Delete後の全PCM、4 sample rates×7速度の整数境界、短い選択の出力数と既存stretchを確認。短区間4倍速の選択有無による先頭波形差を新たに観測し、音質残件として保持する。初期Seekの位相・長い削除区間の負荷・全UX台帳は継続する。

2026-09-11 G01/U06 dialog-focus checkpoint: native pickerの取消で元のkeyboard focusが失われる問題を再現・修正。開始時のfocus・tab・media generationを保持し、取消／失敗時に同じ対象で別modalが続かない場合だけ復帰する。選択成功・世代／tab変更・guard・孤立完了では破棄し、勝手にcommandを再実行しない。2回帰と全612 tests／Release成功。通常960×576のWelcomeでOpen File／Open FolderのEnter→Escape→元button復帰→Enter再操作を確認。前回の単発click不発は再現せず、全入口／IME／mixed-DPI・全UX台帳は継続する。

2026-09-11 E01 JPEG simple-text checkpoint: 標準XMPのAlbum／Composer／Genreを既存4項目へ追加。namespaceに基づく単純文字値の読取・Keep／Set／Remove・JPEG保存とUIを接続する。属性／要素形式・特殊文字・混在言語／作者保持、重複／配列／修飾値の拒否、非XMP bytes／画素一致と保存先保護を確認。全610 tests／Release成功。通常960×576で3項目の既存値・Albumへの日本語／特殊文字入力・Cancel復帰を確認。実IME composition／全DPI、残る3項目とEXIF／IPTC／COM・他形式／全品質、全UX台帳は未完のまま継続する。

2026-09-11 I03 neighbor-prefetch checkpoint: 通常表示の先読みを移動方向の隣一枚から前後近隣最大9枚へ広げる。距離優先／同距離は直前方向、Shell順／循環／重複除外を維持し、readingの見開き全体とworker／cache予算は変更しない。1～23枚の全開始位置・両方向と実10画像の移動前preview／往復原寸画素を回帰確認。全609 tests／Release成功。通常960×576で生成1920×1080 PNGの01→10→09→10→01、readingの01＋02→03＋04と通常復帰を可視確認。可視速度差／cold／全資源は未測定。7f4ddbcのCI34592804403成功。全UX台帳は未完のまま継続する。

2026-09-11 I03/U10 animation-prefetch checkpoint: 隣のGIF／APNG／animated WebP／AVIFの先頭だけを既存workerで先読みし、原寸cacheとは別の有界previewへ供給する。4形式の画素・元寸法・原寸全frame／delay維持、一frame予算・取消／世代・非通知・生成集約を回帰確認。全608 tests／Release成功。通常960×576の生成GIFでLoading中のpreview→原寸を可視確認したが、変更前も最初の約200ms観測ではpreviewがあり、可視速度差は未認定。1fefec9のCI34591591601成功。cold／全素材／peak・UI全般と全UX台帳の残件は継続する。

2026-09-11 H1/I06/V05 input-pairing checkpoint: 同frameの後続gestureによって先の選択・pan・Alt回転の終点／click判定／修飾キーが変わる不具合を再現・修正。最初のreleaseで区切り、選択には押下時刻・移動履歴を保持する。前frame保持・原点復帰・長押し・同座標Alt違い・所有権を回帰確認。全606 tests成功、動画GPU preview／Undo／export再読込と通常960×576生成PNGの選択→click preview→Escapeを確認。右button／Alt保持dragの今回の確認は自動入力で、全native timing／mixed-DPIは残る。643606aのCI34590038494成功。全UX goalは未完のまま継続する。

2026-09-11 M01 input-order checkpoint: 可視不発を追跡し、release後のPointerGoneによる確定取消と、batched press／moveの遅延hit-testによるdrag owner移動をそれぞれ回帰で再現・修正。退出は入力順で処理し、logoの所有pressを既存egui APIへ固定する。短いclick・foreign owner取消・3方向・実画像操作面とtabを確認。全603 tests／Release成功、最終通常960×576の生成PNGでView→Escape→Edit→Escape→Fileと通常clickが通る。3a019b7のCI34588315920成功。最大化／混在DPI・全focus/style、全UX台帳の残件は継続する。

2026-09-11 U10 warm-card checkpoint: filmstripの先頭missが後続memory hitまで待たせる現象を可視確認し、既存workerのmemory先行公開→miss逐次生成へ修正。source stamp・duration・世代／取消・64件上限を維持し、表示順・worker数・cache予算は変更しない。旧順序の回帰失敗→成功、全体601 tests／Release成功。新Releaseの実960×576で61 MB PNGだけがLoadingの間に後続warm cardsが表示され、その後PNGも同位置に完成することを確認。旧Releaseでも4静止画形式のfilmstrip／原寸、reading WebPの先行表示→原寸を確認した。eb00c43のCI34586129718成功。cold／全DPI・素材・資源と全UX台帳は継続する。

2026-09-11 U04 grayscale checkpoint: 共通styleのoverlay／入力欄の既定色、palette・playlist・filmstripの固有色を既存tokensへ統一。popup 1／1.25／2倍率、palette最大600px／compact／検索、playlistの通常・選択・hover・focusとクリック、card／drag texture維持を回帰確認。Computer Use接続が復旧し、通常960×576の実画面で無音2曲の行選択・pause、palette検索とEscape、logo menuを確認。全体599 tests／Release成功、f3a9c90のCI34584717553成功。全DPI・全media・filmstripの今回の可視状態と全UX台帳は継続する。

2026-09-11 I03/U10 persisted-thumbnail checkpoint: direct静止画cache PNGに補正済み元寸法を保持し、fresh memoryでも原寸前のpreviewへ再利用する。専用20-byte chunk／CRC／寸法とRGBA上限、1 MiBの読取上限・取消／source stampを検査し、古い寸法なしcacheはthumbnailとして維持。4形式・EXIF8向き・破損matrixと、原寸停止中のdisk preview通知→原寸画素一致／退役を確認。c05c528のCI34582851963は成功。可視UI／cold／全品質・資源と全UX台帳は継続する。

2026-09-11 U10 direct-static-thumbnail checkpoint: memory／disk・JPEG/BMP専用previewの後に、原寸RGBA128 MiB以内の静止画を既存画像decoderで直接縮小する。原寸copy／新workerはなく、既存PNG disk cacheと同key生成leaseを維持。4形式の全sample RGBA・budget／取消／disk、GIFと不正入力fallbackを回帰確認。24MP PNG warm Releaseの最終API取得は約514→141ms。可視操作は更新版Computer Useでもpipe接続不可で保留。c5dccf3のCI34582156898は成功。全UX台帳、PNG本表示の初回／cold／全品質／資源は継続する。

2026-09-11 U10/I03 static-filmstrip checkpoint: 未訪問の大きなJPEG／BMPに既存高速previewを共用し、元寸法付き共有memoryへ供給。memory／disk hitを優先し、fast非対応は従来経路を保つ。JPEG単独画像のNoFrameを再現し、画像先頭の不要な-ss 0を除去。小JPEG／BMP／PNG／WebP fallback・disk再利用、大画像の色／alpha／寸法・本表示共有／取消／source変更を回帰確認。24MP warm Releaseの修正済みCLI対比はJPEG約91→14ms／BMP約280→1.3ms。可視UI／cold／全品質・資源と全UX台帳は継続する。

2026-09-11 I03 reading-prefetch checkpoint: 隣見開き全体をShell順で逐次先読みし、256 MiBを維持してcache件数を8→10へ拡張。warm hitを含むbatch合計予算で先頭優先を保ち、後続ページの実行中decodeも移動先に含まれれば採用する。10実PNGの全画素／原寸Arc再利用・全preview、容量／重複／失敗・途中採用／空取消、2～10枚のapp選択matrixを回帰確認。9b1601aのCI34580771257は成功。可視確認はComputer Use pipe接続不良で保留し、速度改善の測定値は追加しない。全UX台帳、初回表示／cold／UI latency／資源を継続する。

2026-09-11 I03 prefetch-handoff checkpoint: 新要求の先頭pathと同じ実行中先読みだけを引き継ぎ、原寸の重複decodeを除く。別画像／空要求／closeは待機解除、未開始jobは非採用、失敗／source変更は通常decode、残予算不足はTooLarge。実PNGの途中からの継続・全画素一致とworker制御matrixを確認。24MP warm Releaseの同一境界比較は要求→結果約118→48ms、可視UIや全形式の保証ではない。62131c4のCI34579596111は成功。全UX台帳、PNG初回表示・先読み範囲／cold／UI latency／資源と未確認BMP表示を継続する。

2026-09-11 I03 decoder-reuse checkpoint: PNG／WebPのanimation判定instanceを静止画にも再利用し、PNG metadataの二重読取をforeground／prefetchから除去。16 MiB textの回帰と8色形式×EXIF8向き×両経路の画素・予算、既存animation／取消を確認。FFmpeg直接PNG packetも測定では遅く不採用。普通のPNG画素復号／初回段階表示・進行中先読みの再利用・UI latencyと全UX台帳は継続する。先行3632806のCI34578656019は成功。

2026-09-11 I03 BMP-first checkpoint: 24bit非圧縮BMPのsample行だけ読む小さな先行表示を追加。向き／padding／全sample色と原寸不変、破損・上限・取消・alpha fallback、共有cacheと原寸前通知／退役を確認。24MP warm Releaseでpreview約0.9ms／原寸約76ms。PNG／BMPの既存FFmpeg通常経路への置換は測定で遅いため見送る。Computer Use接続不可により可視確認は未実施。PNG等の初回表示・cold／UI latency・資源と全UX台帳を継続する。

2026-09-11 I03 JPEG-quality checkpoint: 通常／progressive・gray・直接RGB・CMYK（黒版ありを含む）の生成9種類を独立decoderの代表色と原寸へ比較し、寸法・不透明alphaも確認。外部生成不要のgray回帰と任意の再生成／色比較テストを追加し、productionは変えない。YCCK／ICC／写真全画素・可視latencyの認定ではない。次は他静止画形式の初回表示と実際の画像移動待ちを進め、全UX台帳を維持する。

2026-09-11 I03 texture-upload checkpoint: managed textureがeguiのimmutable画素を共有し、GPU転送前の全Vec cloneを除去。6000×4000の転送単体Release中央値30.540→22.692ms、一時コピー96MBを削減する。partial updateのpacked-row仮定による実GPUの下行消失も再現し、RowPitchに沿う転送へ修正。7幅×WARP／実GPUの画素、COW／所有権・free、不正入力と既存sampling／inversionを検証する。UI全体・資源peak、他静止画形式の初回表示を含む全UX台帳は継続する。

2026-09-11 I03 JPEG-first checkpoint: 大きなJPEGの初回cache missでは同梱FFmpegの1/8復号から小さなpreviewを先に通知し、既存の共有cache／世代mailbox／通常・reading描画を使う。EXIF8向き・元寸法・色／alpha・予算と原寸前通知、成功／失敗／取消／source変更／closeを確認。生成6000×4000のwarm Releaseでpreview約13ms、原寸約55ms（先行なし約42ms）という負荷の交換を記録する。ee9af9fのCI34528272592は成功。他静止画形式の初回preview、cold／UI latency・全JPEG品質／peakと全UX台帳を継続する。

2026-09-11 I03 cancellation checkpoint: foreground／静止画prefetchの読取・Seekと変換境界へ世代取消を接続。PNG／JPEG／BMP／TIFF／WebPで画素一致と途中取消を確認し、拡張子fallbackを維持して既存animated AVIF回帰も通す。生成6000×4000、Release各7回、開始20ms取消の処理終了中央値はPNG120.899→24.735ms、JPEG44.221→28.305ms、BMP78.538→21.159ms。warm file-cacheの復号単体であり、初回段階表示／cold／可視UIの速度保証ではない。a590fe9のCI34526543071は成功。初回静止画previewを含む全UX台帳を引き続き実施し、配布準備は再開しない。

2026-09-11 U02/U08 verification checkpoint: native guardの168文字日本語／`&`名、通常／fullscreen×96／192 DPIの折返し・末尾・全ボタン・取消後の辺focusと1pxキー調整を確認。Cancel／worker失敗後のfocus復帰を既存app回帰へ追加した。全体チェック中、幅1080のportrait作業領域でfilmstrip／tab分離の旧テストが未補正座標を期待して失敗したため、既存のwork-area補正を独立計算する厳密なoracleへ修正。アプリ挙動・許容誤差は変更しない。8551c42のCI34525054544は成功。次はI03の通常静止画first-decodeと表示待ちを測定し、段階表示・取消・先読みの残件へ進む。全UX台帳を継続する。

2026-09-11 U02 native-guard checkpoint: 同windowでexportしていない通常の未保存確認をWindows Task Dialogへ移し、既定Cancel・明示Export／Discard・Exit時の全編集破棄を表示。STA所有・manifest v6・二重表示防止・失敗時Cancelを接続した。file pickerはnative、live export／長文error／編集form／tooltipは操作性と追加実装量を根拠に現行方式を採用。実96／192 DPI、SDK UIA InvokeのCancel／Discard／Save、保存PNG57,600画素、別window独立操作と全チェックを確認。旧.NET UIAのPane判定はnative SDKのButton／Invokeと区別する。a557b5aのCI34523120355は成功。次はnative guardのUnicode長名／fullscreen／focus復帰を確認し、全UIA・IME・style・media品質／性能等の全UX台帳を継続する。

2026-09-11 U11 audio/fullscreen checkpoint: 音声の通常96／192 DPIとfullscreen192 DPIで1px反転枠を確認し、4数値対象のfocus前後でtimeline全画素が一致。画像fullscreenではUIAで辺へfocusしてもstatusが隠れる欠陥を可視と回帰で再現し、既存barの保持条件へselection focusを追加した。実UIA96／192 DPIの4辺で説明・focus保持・media全画素一致、Downで下辺130→131 source pixelsを確認。100／125／200%回帰と全体チェック通過。958ec7fのCI34521224695は成功。全UX goalは未完了で、次はU02のnative modal選択判断を継続する。全比率・素材・UIA・IME・性能・資源・metadata等の台帳を縮小しない。

2026-09-11 U11/U09 visible-selection checkpoint: 生成PNG／SAR動画の選択枠が96／192 DPIで正確な1px反転色、追加の内外塗りなしと確認。画像の辺resize／範囲内click、tabのMenu key／Shift+F10とdisabled skip／Escape復帰も通常windowで確認した。時間端点・音量・長さに残っていたfocus四角は可視と旧コードで失敗する回帰で再現し、statusの対象名・値へ置換。192 DPIの4対象focus前後でtimeline 245,760画素一致、96 DPIへ戻した開始端の0.1秒調整を確認。776988fのCI34519379513は成功。音声／fullscreen／全比率・UIA／native system menuと全UX台帳は継続する。

2026-09-11 U01/U04 caption-geometry checkpoint: native controlsのclient-relative bottomへtitle barを合わせ、1 physical px区切り線までの96 DPI／1px・192 DPI／6pxの隙間と左右6 logical px外側marginを除く。最大化の画面外insetを避け、logo／tab行を中央配置する。可視の通常・最大化×96／192 DPIでnative hover下端と区切りが隣接し、logo menu・close guardのCancel／Discardを確認。Cancel後のmedia領域268,160画素が一致する。6ed328fのCI34517162248は成功。全DPI比率／Windows 10／native menuの物理キー／drag遅延・選択UIと全UX台帳は継続する。

2026-09-11 U01/U04 inactive-caption checkpoint: 非active時にnative controlsの背景だけが灰色になる草案指摘をRGB(43,43,43)で再現。DWMWA_CAPTION_COLORの黒指定だけでclientと揃え、native glyphのinactive色／hover／hitと旧OS fallbackを維持する。可視96／192 DPIの通常・192 DPI最大化でRGB(0,0,0)、bounds不変、赤いclose hover／native click終了と全体チェックが通過。74f492fのCI34515762417は成功。Alt+Spaceは既存の別process global bindingに取られたためnative menuの実キー確認とはせず、追加入力は行わない。次は実boundsに基づく左右padding／bar高さ・下端間隔を詰め、全UX台帳を継続する。

2026-09-11 U01/U08 fullscreen-monitor checkpoint: Win+Shift+矢印の全画面移動で旧client寸法が残り、2台にまたがる欠陥を可視再現。DPI通知中の同期位置提案だけを移動先monitor矩形へ補正し、winitの追跡を維持する。全画面終了の二重拡大も確認し、NativeCaptionへ切替と論理client寸法の復元を一体化。3台／96・192 DPIを両方向に2周してbounds一致、通常復元と映像48,140画素一致、最大化からの切替／別DPI／最大化復帰、guard mouse操作を確認。非表示nativeのproposal／flag／scope解除・実monitor／復元と全体チェックも通過。e31fe06／2d4efb6のCI34512924647／34513773723は成功。元window配置への復帰規則は変更しない。次は通常captionの余白・操作部／native menu・selectionのmixed-DPI外観と操作を監査し、全media／UIA・IME／latency・資源を含む全UX台帳を継続する。

2026-09-11 U07/U08 verification-clock checkpoint: native focus probeのRawInput時刻省略が仮想frame時間を実時計より先へ進めていた。旧コードで約8msの先行を直接検出し、hosted probeは通常描画と同じegui-winit時計へ統一、adapterを持たないrenderer-only fixtureは仮想時計を維持する。音声移送検証でもPause送信直後を停止完了と扱う競合を約13ms差で確認。既存runtime試験と同じ待機後に150msの停止継続を追加検査し、移送前後の完全一致は維持する。両修正後native hostは20回連続、全体チェックも通過。製品コード／UIの待機時間は変更しない。全体の無競合性・実操作のPause latencyの認定ではない。次は最大化／fullscreen状態でのmixed-DPI移動と通常caption操作を監査し、全UX台帳を継続する。

2026-09-11 U01/U08 DPI-size checkpoint: custom captionで除去した標準枠がwinitのDPI resizeで再加算され、往復ごとにサイズが増える問題を修正。winit通知と位置を維持し、通常windowのclient寸法だけを旧／新DPIと実枠差で補正する。旧native回帰の1946×1223での失敗→修正後1920×1152、3台2周の端配置／サイズ保持を確認。可視3周で640×480↔1280×960、編集済み48,140画素一致、96／192 DPI各々で最大化／fullscreenから復帰。0acaaa8のCI34511626154は成功。最終全体チェックは通過したが、途中で既知のnative harness時刻逆行assertが再発したため、次に再現性を監査する。全DPI比率／最大化・fullscreen中のmonitor移動／全media／UIA・IME／style／latency・資源と全UX台帳は継続する。

2026-09-11 U08 monitor-placement checkpoint: release点からmonitorの作業領域を選び、hidden配置後の実DPIで論理grabを再計算して位置をclampする。通常の手動移動とサイズ規則は変更しない。右端793pxのはみ出しと旧native回帰の失敗→修正後成功を確認。3台／96・192 DPIで端・中央、負座標／taskbar／過大windowの回帰、可視右端の閉じる領域・mixed-DPI移送・filmstrip右下と映像画素保持が通過。546f3dbのCI34509593269は成功。別件としてDPI往復で960×576→1946×1223→989×651へ増える既存サイズ問題を確認したため、次はcustom captionとwinitのDPI resizeを修正する。全UX台帳・全media／DPI比率／UIA・IME／style／latency・資源は継続する。

2026-09-11 U08 filmstrip-position checkpoint: filmstrip外dropもrelease位置を無視していたため、浮遊カード左上を要求へ引き継ぎ、タブ分離と共通の座標変換で表示前に配置する。元tab／未保存編集は保持し、子は元ファイルを独立Openする。旧native位置回帰の失敗→修正後成功、3幅×3密度、非有限座標／stale／重複・失敗時保持を確認。可視の生成動画／PNG／無音WAVで位置／foreground、編集済み元動画と未編集子の画素一致、画像の均一色144点と音声終端を確認。1404496のCI34508201666は成功。mixed-DPI／monitor端／全media・codec／UIA・IME／style／latency・資源と全UX台帳は継続する。

2026-09-11 U08 detach-position checkpoint: 外drop後の新windowがrelease位置を無視する欠落を可視操作と旧コードで失敗するnative回帰で確認し、先頭slotのgrab offset／source density／client原点から表示前に配置する。3幅×3密度×3tabのheadless回帰、native位置／未保存画像の保持と、可視の先頭／2番目動画tabの分離・再結合／映像48,140画素一致を確認。所有する別process windowで覆った背面には誤結合せず、覆いを外すと結合する。e773cbeのCI34506640146は成功。全UX台帳、mixed-DPI／monitor端／filmstrip分離の可視確認／全media／latencyと資源測定は継続する。全体テストで別経路の断続的失敗も観測したため、再現性の監査は残す。

2026-09-11 U08 visible-merge checkpoint（確認記録訂正）: 可視の二windowでlast-tab右の空白dropが拒否される欠落を再現し、native captionを除く空白での末尾追加へ修正。挿入線・未保存回転／focus・media領域拒否・Welcomeへの戻し、通常native window dragを確認。2幅×3密度の回帰、既存scroll／取消と全体チェックが通過。84f86e1のCI34503758408は成功。Welcomeへ戻した動画の黒画面という先の判定は保存画像の誤読だった。既存3枚と診断コードなしReleaseの新しい3回の往復後で、移動前の映像48,140画素との完全一致を確認し、不具合判定を撤回する。表示処理の変更は残さない。次は可視分離／遮蔽の実入力を確認する。mixed-DPI／全media／性能と全UX台帳は未完のまま維持する。

2026-09-11 V02/U10 single-preview checkpoint: video単枚／filmstripを補助decoderへ接続し、通常CLI起動を除去。単枚のSAR／向きと正方形内の寸法上限、video専用v4 key、取消／source変更拒否／既存fallbackを維持する。独立timestamp参照、全既存互換素材、portrait／rotation／cache再利用を検証。Releaseの小素材は約54→3ms、1080p長GOPは約91→97msで後者の改善なし。所有windowでfilmstrip／scrub／tab表示を確認したが初期表示時間は未計測。先行f2ca0b8のCI34500549758は成功。長GOPの再decode、CLI fallbackのtimestamp制約、UI end-to-end／全process peak、全codec／HDR／mixed-DPIおよび全UX台帳は継続する。配布準備は再開しない。

2026-09-11 V02/U10 shared-decoder checkpoint: sheet内で補助input／software decoder／filter graphを共用し、通常経路のコマ別process起動・PNG往復を除去する。RGBA明示のv2 cache、thread指定／pixel上限、協調取消、source変更拒否と既存CLI fallbackを維持する。origin／TS、向き／SAR／色／alpha、B-frame／VFR、best stream／video EOFとfallbackを独立参照で検証。小H.264の新Release full-sheet 26.34ms、1080p長GOPの16コマ取得は同Release条件で約1.33→0.76秒。可視の回転／SAR素材でdragとtab表示も確認した。先行1132f3aのCI34498237382は成功。GOP再decode、UI latency／全process peak／全codec／HDR／mixed-DPIおよび全UX台帳は継続する。

2026-09-11 V02 drag-tooltip／visible-edit checkpoint: 草案の「ドラッグではサムネイルと本画面を同時表示」に対し、通常tooltipがdrag時に消える欠落を再現・修正した。所有gestureだけ強制表示し、画像／動画、track外への移動、保持、disable取消と後続releaseを回帰で確認する。可視の生成SAR素材で直角／自由回転・crop・flip・resize・Cover／panを重ねて実映像とscrubの配置を比較し、別のdisplay-matrix素材でもsource向きを確認。先行e0dc280のCI34496739888は成功。全codec／HDR／編集順序、mixed-DPI、cold生成負荷と全UX台帳は未完のまま継続する。

2026-09-11 V02 main-scrub checkpoint: 動画compact seekの横drag中は一時停止して既存sheet／単枚textureをメイン表示へ使い、release一回でSeek・元が再生中なら再開する。Escape／focus喪失・別操作でSeekなし取消、EOFは停止。crop／回転／反転／resizeとzoom／panを低解像度geometryに反映し、新frame到着までpreviewを保持する。実sessionの固定位置／一回Seek／multipass／取消／終端・復帰とgeometry／mesh、可視の生成素材による停止・再生drag／取消を確認。先行037aac5のCI34494083591は成功。長GOP／cold生成、編集済み素材の可視比較、HDR／全codec、mixed-DPI／peak負荷および全UX台帳は継続し、launch準備は再開しない。

2026-09-11 V02/U10 video-sheet checkpoint: 16コマ単位の有界sheetを現在位置から先読みし、Seek hoverを優先、同sheet内は一つのtextureのUVだけを変える。短編20区間／長編約5秒間隔、共有memory／diskと生成集約、Seekの2枚LRU／tab別texture、単枚の初期応答fallback、世代取消／上限／GPU復旧へ接続。実H.264の生成／cache時間と画素、headlessのtexture uploadなし／優先度／LRUを確認。ownerから可視window操作の許可を受け、所有する生成素材windowで実mouse hover／Seek／tab表示と停止位置不変を確認した。先行a086f6bのCI34481359872は成功、08430ebのCI34481983430はcancelledで成功扱いしない。本画面scrub、全codec／長GOP・mixed-DPI・peak負荷と全UX台帳は継続する。

2026-09-10 I03/U10 prefetch-seed checkpoint: 既存の隣静止画先読みから共有縮小previewを供給し、原寸cache hitからも再decodeせず再登録する。mutexを解放して縮小し、原寸／縮小予算・先読み枚数を増やさず、表示通知を出さない。実PNGの画素／alpha／寸法・filmstrip再利用／disk生成なし・原寸Arc同一と、取消／source変更／close／失敗／animation拒否を確認する。静止画first-decode／cold-storage性能、animation先読み、動画sheet／GPU共有と全UX台帳は継続。先行a086f6bのCI34481359872は確認時実行中。

2026-09-10 I03/U10 first-frame checkpoint: GIF／APNG／animated WebP／AVIFの原寸decodeから最初の借用frameを縮小し、全frame完了前にImagesReadyで通常／readingへ公開する。追加decoder／process／原寸コピーはなく、元寸法・共有preview上限を維持。mailboxの一件枠、原寸成功／失敗時の退役、source変更／取消／close拒否と実codecの画素／timing不変・予算を確認する。AVIFの取消／予算診断が一般consumer停止で隠れる点も修正する。先行8b03da5のCI34479689433は成功。静止画first-decode・cold-storage／可視UI時間・先読みpreview／動画sheet／GPU共有と全UX台帳は継続し、ローンチ準備は再開しない。

2026-09-10 U10 shared-preview checkpoint: hostがPreviewCacheを一つ所有し、window間の画像／filmstrip／recent／tab／seek workerへ共有。RGBA上限64件／16 MiBをwindow数で増やさず、元window閉鎖後も再利用する。同keyのdecode／disk生成を一件へ集約し、別keyは並行、待機取消は独立、失敗／取消後は再試行可能。durationもsource metadata keyで成功値64件を共有し、重複FFprobeを抑える。生成の共用／取消／失敗解放・metadata更新／上限・元window閉鎖後のseed再利用を確認。先行076e486のCI34478547413は成功。cold／可視UIの実時間、未訪問preview先行生成／動画sheet／GPU texture共有と全UX台帳は未完。可視入力許可の返答を待つ間も独立項目を進める。

2026-09-10 U08 launch-routing checkpoint: 同じSID／session／executableからの通常起動を既存hostへ集約し、file／folder／Welcomeを同deviceの新windowとして開く。絶対path-onlyのbounded UTF-16要求、message-only受信窓、session-local lifetime marker、process／SID照合とstartup ackを採用。timeoutでは自動再送・重複fallbackしない。実子processの並行転送／拒否／timeout／owner終了後の再取得、非表示HWNDの実IPC→起動ack／file decode／folder Shell読込み／Welcome・元state保持／同device描画を確認。先行39138e8のCI34476325249は成功。旧独立processのlive state回収はせず、可視Explorer／foreground／window間入力・mixed-DPIと全UX台帳／性能は継続する。

2026-09-10 U08 merge-input checkpoint: tab外releaseの座標をhostへ渡し、同host既存windowの実描画tab strip／Welcomeへgap指定でlive stateを移す。runtimeのroot hit照合と各windowのDPI変換を通し、hover中はfocusを奪わず挿入線／端scroll、成功後に移動先focus。modal／overlay／媒体領域／古いtab列・viewport・densityを拒否し、他windowに隠れた対象へは結合しない。3幅×3密度のheadlessと、OS hit選択だけを注入した非表示HWNDの実drag／GPU indicator／dirty画像移送／Welcomeを検証。独立process入口の所有権、可視windowの実入力／重なり／mixed-DPIと全UX台帳・性能は引き続き未完。

2026-09-10 U08 hosted-filmstrip checkpoint: 通常のサムネイル外dragを同host／device上の新windowへ接続。元tabの移送ではなく独立した元ファイルのOpenとし、元編集・保存先・再生session／位置を保持する。要求の重複・tab／媒体／folder世代・overlayを検証し、初期化前後／missing path失敗では子を残さずfilmstripを保持。受付後の壊れた媒体は新windowの通常診断とする。非表示HWNDで画像・無音音声／動画・壊れた画像の読込み、共有device描画と元state不変を確認。先行a598fe0のCI34473236643は成功。次は既存windowへの通常drag結合／drop indicatorと別process入口の所有権。可視window／mixed-DPI／性能と全UX台帳は継続する。

2026-09-10 U08 image-transfer checkpoint: 画像の外dragも同一hostのstate移送へ接続。移動先contextの全textureを準備してからsourceを外し、静止画／アニメーションの画素Arc・現在frame／deadline・sampling・view・未保存履歴とUndo元を保持する。処理中resampleと未取得readingページだけを再開し、既存ページ／エラー／preview・Shell順を残す。再開時も既存512 MiB decoded予算から保持済みbytesを差し引く。非表示HWNDでactive／retained画像の反復移送・focus・stage失敗保持・共有GPU復旧・close guardを確認。先行2f62d3cのCI34470975468は成功。次はfilmstrip新windowの同host化と既存windowへの通常drop／結合indicator。全UX台帳・U07性能／P010・可視window／物理入力／mixed-DPIは継続する。

2026-09-10 U08 live-transfer checkpoint: 音声／動画の外dragを同一hostの新window初期化→state移送→表示へ接続。sessionの不変originから現在ownerへ通知を配送し、未保存編集・保存先・設定と再生状態を移す。非表示HWNDでactive／非active動画の往復・停止frame／時計・CPU転送0・focus、無音音声の再生／停止・repeat／shuffle・元window削除後の通知、初期化前後の失敗保持を確認する。画像・filmstripの同host化、既存windowへの通常drop／結合indicatorは次工程。先行aeb185dのCI34467998719は成功。全UX台帳と通常window／物理入力／mixed-DPI、U07性能／P010等の残件は継続する。

2026-09-10 U08/U07 shared-recovery checkpoint: hostへlossを集約し、全対象の停止→全surface stage→成功時commitを接続。作成途中の失敗は部分復帰せず、Retryは健全deviceを再利用して要求元だけを復旧する。停止時計がframe PTSより僅かに後の場合の1frame進行を修正し、元PTSとtransport targetを分離。実D3D11VAの非表示2window／retained動画でactive／background／presentation loss・再生／停止・第一／第二surface失敗・Cancel／Retry・stale通知を、WARPで200ns差・RGBA・hidden復帰／意図的Seekを確認。先行a344ff4のCI34465884251は成功。次は移動sessionの通知所有権とstate移送を通常分離／結合へ接続する。実TDR・混在画像／音声／endpoint／HDR／全codec・可視window／物理入力／混在DPI、U07遅延・P010を含む全UX台帳は継続する。

2026-09-10 U08 window-host checkpoint: 通常entryをWindowHostへ移し、再利用しないwindow keyでworker通知を配送、UIAはnative IDで照合、全windowの最早deadlineと最後のwindowだけの終了へ集約する。未保存guard取消／破棄とclose前後の遅延通知拒否、実worker／UIA adapter付き非表示2windowのD3D11VA frame／停止位置保持とreplacementを確認する。試験の描画イベントは明示配送で、通常可視windowの最終確認とは区別する。先行d730c05のCI34464290594は成功。次は全windowのdevice復旧と移動sessionの通知所有権を揃え、state移送と通常分離／結合入口へ接続する。全UX台帳・U07復帰遅延／P010・通常window／物理入力／混在DPIは継続する。

2026-09-10 U08 shared-surface checkpoint: runtimeへ同一D3D11 device上の別window／caption描画先を追加。所有する非表示2windowでCOM同一性・swap chain独立性、3サイズの交互動画／UI画素一致、破棄／再作成後の同じsession保持と次frame進行を確認。通常の分離／結合は未接続で、次は同一event loopのwindow hostと通知／deadline集約、state移送、全windowのdevice復旧を揃える。先行2491639のCI34463040310は成功。物理drag・子window起動／読込、U07復帰遅延・P010、全UX台帳は未完のまま継続する。

2026-09-10 U08 filmstrip checkpoint: visible cardのprimary dragを既存previewで追従表示し、外releaseからpath参照の新window要求へ接続。元tab・編集・保存先・transportは保持し、起動失敗はfilmstripを残す。preview無効化を含む11取消条件・batch input／pointer gap・folder generation／所属／overlay・単一path引数、実GPU復旧前後10点での描画／取消を検証する。先行eefc010のCI34461491979は成功。子windowの実起動／読込・物理dragの最終確認、既存tabのwindow間結合／状態移送は残件。状態引継ぎ範囲の非blocking確認をownerへ送り、回答を採用する。全UX台帳とU07復帰遅延・P010等を維持する。

2026-09-10 U08 drag checkpoint: tab本体を掴んだoffsetのまま追従表示し、隣接tabの即時投影・端の横scrollと既存drop indicatorを接続。release時だけ順序を確定し、取消／batch input／source・構成・画面変化と狭幅3×密度3を回帰確認する。実GPU上で往復／media上の取消を描画し、編集／transport／generationを維持する。先行7d0484dのCI34459310161は成功。次はwindow間結合・分離時の状態移送／filmstrip入口の契約と実装へ進む。現行path-only別process detachを完成扱いにせず、U07復帰遅延・P010・通常window／物理入力／混在DPIと全UX台帳の未完事項を維持する。

2026-09-10 U07 surface checkpoint: 非active化で保持frameだけを同一deviceの独立textureへcopyし、FFmpeg pool参照を解放する。H.264小画像／1080pの24→1枚、画素／PTS保持・5往復・異device拒否時の元frame保持を確認。通常decodeはzero-copy、背景音声／復帰表示契約は維持。先行8557085のCI34457683438は成功。P010用VP9 Profile 2はこの環境のD3D11VA初期化失敗で明示skip、未検証として残す。次は復帰decoder／Seek遅延とU08の連続drag／結合契約を照合する。全resource予算・通常window／混在DPI・全UIA・metadata／preview／seek／性能を含む全台帳は継続する。

2026-09-10 U07 focus checkpoint: media controlをrole／項目pathでtab別に記憶し、再生／読書／audio mode・Seek／timeline／選択辺・playlist／filmstripへ戻す。source再読込／closeで破棄、overlay／focus喪失／新入力を尊重し、初回fullscreen sizingを待つ。実画像source削除後も再読込なしで復帰、headless22役割と実GPU／WASAPIのtab切替・背景EOF／device復旧を確認する。先行749be07のCI34455772918は成功。次はU07の非active動画resource保持量・復帰時decoder再構築／Seek待機を計測・照合する。通常window／混在DPI・全UIA監査、metadata／preview／seek／性能を含む全台帳を継続する。

2026-09-10 U09 keyboard checkpoint: tab名／close buttonからShift+F10・Menu key・UIA ShowContextMenuでfocus対象の既存menuを開く。非active対象／pointer不要・Escape／guard取消後のorigin復帰、close後の現在tab／Welcome復帰を確認。連続矢印の二重focus移動を修正し、reorder後の無効項目skip・3幅×3密度・実GPU復旧前後10点の6入口を検証する。先行71492ddのCI34454309977は成功。次はU07のtab別focus復帰と残るresource／復帰遅延の契約を照合する。U09の通常window／物理Menu key・混在DPI／全UIA監査、他の全台帳を継続する。

2026-09-10 M01 logo menu checkpoint: 右上File／右下Edit／左下Viewの8 logical px drag・releaseを既存submenuへ接続。選択矢印の強調／shaft移動、source/tab・graphics／overlay／focus／resize／DPI・Escape取消、通常click／keyboard／UIAを保持。batched／sparse入力の所有・一回確定、guard／Undo、閉じたmenuの再openと親状態寿命を検証する。実GPU復旧前後10点で3方向の描画／submenu／EscapeとCPU転送0を確認。先行707e82bのCI34451683252は成功。次はU09のtab context menuのkeyboard入口・focusを照合し、M01通常window／物理入力を含む全台帳の残件を維持する。

2026-09-10 U05 toolbar progress checkpoint: 保存jobのsnapshotからtrim／区間編集／rateを反映した推定進捗を上部境界へ接続。normalize二pass、長さ不明／画像のindeterminate、取消時停止、Finishedで解除、UIA非操作進捗に対応。画像移動・読取／dialogには表示せず、既存取消／guardと全画面時の詳細UIを維持する。1物理px／hover不変／二境界、DPI・狭幅・tab切替後の実保存、実GPU復旧前後を確認する。先行a433a69のCI34450453754は成功。E01他画像形式・JPEG残項目を含む全台帳は継続し、次はM01ロゴの方向menu操作を既存menu／focus契約へ照合する。通常window／混在DPI／性能の最終確認も残す。

2026-09-10 E01 JPEG metadata UI checkpoint: 形式別の対応4項目・XML validationをruntimeと共有し、言語別既存値／作者順の非同期表示・Apply／CancelをSaveへ接続。JPEG→JPEGは設定未使用／全Keepでも対象4項目を保持し、表示の省略では元値を切り詰めない。PNGと共通の全項目UIA／focus／compact、実Save／再Save／全Keep復元／Remove・出力形式失敗／guard／source lifecycleを検証する。JPEG他6項目／EXIF・IPTC・COM整合／Extended XMP・他形式と全UX台帳は継続。先行067a225のCI34449201712は成功。次は残る画像metadataの形式別契約と不足項目を照合し、通常window／全素材品質の残件も維持する。

2026-09-10 16:15 E01 JPEG XMP基盤checkpoint: 標準XMPのTitle／Artist／Comment／CopyrightをJPEG→JPEG出力へ接続。言語Alt／作者Seq、namespace URI・Unicode／参照・CR保持を扱い、DTD／複数packet／Extended XMP・未対応構造／過大入力は拒否する。stageの非XMP bytes／EXIF・復号画素不変、独立JPEG decoderのXMP抽出、回転／反転保存とKeep／Set／Remove、取消／source変更／write失敗・target保護を7回帰で確認。他6項目、JPEG UI／全Keep既定保存・EXIF／IPTC整合と他形式は未完。次は形式別項目／既存値表示・UIとKeep保存の契約を揃える。全UX台帳／通常window／全素材品質・性能は継続。先行3a72bf0のCI34447566341は成功。

2026-09-10 15:55 E01 PNG metadata UI checkpoint: File／custom commandを画像へ開き、PNG既存値・keyword対応・PNG入出力／文字のみの説明とApply／CancelをSaveへ接続した。未対応画像・読取待ち／失敗はApply不可。全KeepのPNG保存も元の対象文字情報を保持するよう基盤を揃える。全10項目UIA／compact／focus、実Save／再Save／全Keep復元／Remove・非PNG保存先失敗／guardとsource lifecycleを確認。先行2eb281bのCI34446598113はCargo.lockのnotice照合hash更新漏れで失敗し、今回hashを同期して146 packagesの整合性試験を通す。公開準備は再開しない。次は他画像形式のmetadata契約／実装へ進み、通常window／混在DPI／全codec品質・性能と全UX台帳の未完事項を継続する。

2026-09-10 15:43 E01 PNG metadata基盤checkpoint: PNG→PNGの10文字項目をiTXt UTF-8で保存し、Keepの元text chunk／Set／Removeをstaging内で再照合する。画像decodeを伴わない有界stream処理、CRC／破損／過大展開拒否、取消／source変更／書込失敗時の既存file保護を確認。crop／回転／resize保存の非text bytesと画素はmetadataなしと一致する。画像UIと他形式／EXIF・XMP、通常window／混在DPI・全素材品質と全UX台帳は未完。次はPNG用UI／保存形式の説明とsource lifecycleへ接続する。先行37bd0feのCI34445108179は成功。

2026-09-10 15:18 E01 metadata UI／U02 popup checkpoint: File／custom commandへ10項目のKeep／Set／Remove、非同期global／best stream既存値表示とsource別設定保持を接続。全項目UIA・UTF-8上限・IME／popup Escape・取消／focus／stale結果、実Save／再Save／AudioOnly／離脱Saveを確認。modal中の一律popup閉鎖を原因として、metadataと画像／動画resizeの選択を修正し、全app frameと実GPU復旧前後で4filter選択を確認。次は画像metadataへ進む。全UX台帳・通常window／混在DPI／全素材品質・性能は継続する。先行dc15b09のCI34443028049は成功。

2026-09-10 14:54 E01 metadata基盤checkpoint: 10文字項目のKeep／Set／Removeを通常動画／音声・音声のみ出力へ追加し、staged出力の値／削除を再probeしてからpublishする。6音声形式のtitle、Matroska全10項目、Unicode／改行／引用符、既存target保護と区間編集／normalizeとのPCM一致を確認。ADTS title・RIFF非対応tag・M4A track表記変形は既存targetを置換せず拒否する。設定UI／source別保持・画像metadataは未接続で、次はこれらへ進む。全UX台帳と通常window／全素材品質の残件は維持する。先行f219f76のCI34442221336は成功。

2026-09-10 14:40 E01音声option UI checkpoint: File menu／custom commandの設定modalをSave／Export as／AudioOnlyへ接続。Apply／Cancel・Escape・compact配置・overlay focus・stale token／同path再読込拒否と、実PCMでの再Save／離脱時Saveを確認。設定はtab内の現在sourceだけに保持し、別曲・再読込・closeで解除する。音声のみ出力は動画の未保存状態と通常Save先を保持する。実D3D11VA復旧前後10点のUI操作は履歴／transport不変・CPU転送0。次は個別metadata書換へ進む。通常window／全codec品質と他のUX台帳全残件は維持する。先行590d240のCI34440521631は成功。

2026-09-10 14:15 E01音声option基盤checkpoint: normalizeを編集・channel変換後のsample peak −1 dBFS共通gainと定義し、bounded診断の二pass音声解析／encodeを接続。Monoは左右平均、Stereoはmono複製、Keepは多channel共通gainも保持。静音／微小音／overfull float・6channel、無効／空音声・取消／source変更、独立編集PCMと動画／音声のみ保存の一致・映像不変を確認。解析phase表示は接続済み、optionを選ぶUI／保存設定保持は未接続で次工程。metadata書換と全UX台帳の未完事項も継続する。先行65ab0d2のCI34439302401は成功。

2026-09-10 13:58 E01音声派生出力checkpoint: 動画のFile menu／custom commandから7音声形式の別名保存へ接続。best audio・時間／局所編集を反映し、映像編集を除外。元動画のSave先／saved cursor・未保存guardは保持する。7形式再open、WAV／FLAC PCM、trim／rate／gain・削除／伸縮後の独立sample列、取消／無音声／空出力／既存target保護とnative dialog設定を確認。次はnormalizeの定義とchannel変換の出力optionへ進む。metadata書換・通常window／全codec品質と他のUX台帳全残件は未完のまま維持する。前回b7c030bのCI34437967190はinstaller prerequisite fixtureの15秒timeoutで失敗し、ローンチ作業は再開しない。

2026-09-10 13:34 V04音声微小移動checkpoint: 草案の音声frame相当操作を編集後時間軸の10ms Seekとして採用し、comma／period・View menu／custom bindingへ単位を明記。移動時停止、連続入力、先頭／実EOF・区間削除／伸縮／rate・範囲再生外への移動、履歴／source不変を無音WAV・実WASAPIで検証。動画の実PTS探索は変更しない。次はE01書き出しオプション（音声のみ／normalize／channel変換）の契約と既存保存導線を照合する。通常window・全形式精度／遅延と全台帳の残件は継続する。

2026-09-10 13:23 V05 resize UI checkpoint: timeline内Ctrl+R／Edit menuへ幅・高さ・比率／4filterとGPU previewのdialogを接続。Cancel／identity非編集、snapshot／budget・古いtoken拒否、focus／overlay、Apply／Undo/Redo・実保存再読込と再生状態不変を確認。画像と入力、動画回転とsnapshotを共有し、実D3D11VA復旧前後の10 UI previewはCPU転送0。次はV04音声frame相当操作の契約照合へ進み、V05通常window／mixed-DPI・全素材品質／持続性能を含む全台帳は未完として継続する。

2026-09-10 13:04 V05 GPU resize checkpoint: 4方式を同一deviceの横／縦passへ接続し、符号付きfloat中間・係数再利用・512 MiB合算予算を実装。保存の縮小時色間引きを無効化し、WARPの36寸法条件／24高彩度pattern条件と合成順序、実GPUの1080p／4Kの16条件で独立referenceを照合。操作UIはまだ未接続であり、次はtimeline内Ctrl+R／寸法・比率・filter／Apply・Cancelとpreview／Undoへ進む。通常window／全素材品質・性能と全台帳の残件は維持する。

2026-09-10 12:40 V05 resize基盤checkpoint: VideoResizeのsource寸法／SAR・偶数16px以上の出力、4補間方式とSAR1のsoftware保存を実装。4方式×7条件、crop／回転／再resize、metadata orientationとtrim／rate／区間削除・伸縮／音声、既存target保護を回帰確認。GPU表示と操作UIは未接続でapp入口は拒否を維持。次は縮小時のkernel幅・符号付き中間精度を含む4方式の同一device GPU処理と予算確認。通常window／全素材品質・性能、UX台帳全体は未完。

2026-09-10 12:27 V05表示zoom checkpoint: 動画のtimeline内でCtrl＋wheel／+・-／Actual／Fit／Coverと右drag panを接続。SAR・physical倍率、cursor基点、viewport／UV clip、入力所有権・取消、timeline／fullscreen／tab保持を確認し、zoom前後の保存全5frame画素も一致。実D3D11VAの10回の描画試験はCPU転送0。次は動画resize／resampleの画素編集・GPU／保存契約。通常window／混在DPI・全素材画質／性能を含むUX台帳全体は未完のまま継続する。

2026-09-09 再開指定: ownerが「それは古い指示です。既存のgoalを遂行してください」と明示したため、以下のcheckpoint後の待機指定は失効する。既存のUX_IMPLEMENTATION_PLAN全体を引き続き実装・検証する。ローンチ準備・公開の停止は維持する。

2026-09-10 画像操作checkpoint: I04のPageUp／PageDown等の追加キーと1～10枚jump、I06のreading L／Vを共有commandへ接続。Shell順・読書・未保存確認・設定互換とmenu全項目を回帰確認する。Ctrl+左右は採用済みの一枚移動を維持。通常window入力／IME、preset／自由回転を含む台帳の全残件は継続し、goal全体の完了とはしない。

2026-09-10 08:12 比率選択checkpoint: I06のpreset7種を画像と編集contextの動画へ接続。編集後寸法・SAR・画素格子・custom prefix・非編集・時間選択との分離、PNGの保存／再読込画素と実動画のcrop／Undo/Redoを回帰確認。新menu監査で再現した初回矢印欠落も修正する。自由回転、通常windowの最終操作確認と台帳全体は未完。

2026-09-10 08:26 自由回転基盤checkpoint: I06の画像用角度／外接寸法・非同期raster・PNG exportの共通処理を追加し、合成編集の全画素一致とanimation／Undo/Redo／tab取消を検証。操作UIはまだなく、次は画像command／角度操作へ接続する。動画と通常window／性能を含む台帳全体は引き続き未完。

2026-09-10 10:26 自由回転UI checkpoint: 画像のEdit menu／Ctrl+Shift+R／角度dialog／slider／配置previewを接続。取消／0度非編集、対象・世代照合、custom key保護、keyboard／UIA／pointer、palette／gridを含むfocus復帰とcompact scrollを回帰確認。次は画像hold-drag操作。動画の単一device／SAR／保存、通常windowの操作・外観／性能と台帳全体は引き続き未完。

2026-09-10 10:41 画像hold-drag checkpoint: Alt＋左dragによる水平回転preview／一件の確定を接続し、取消・入力所有権・同一frame release・crop preview／既存編集・logical scale・有界描画を回帰確認。既存角度dialogは維持。次は動画の単一device上の自由回転表示／SAR／編集順序とexport契約を詰める。画像の通常window／性能の認定とUX台帳全体も未完として継続する。

2026-09-10 11:04 動画自由回転基盤checkpoint: VideoRotationのsource／SAR／square-pixel／外接寸法と偶数黒canvas、RGB8の順序付きsoftware exportを実装。全角度境界、独立filterによる合成画素、metadata orientationとtrim／rate／audio、既存target保護を回帰確認。次は同じ順序を単一deviceのGPU中間描画へ接続する。表示とUIは未完・app入口は未公開であり、H1／UX台帳全体は引き続き未完。

2026-09-10 11:22 動画GPU raster checkpoint: runtimeの同一device表示入口と順序付きRGBA stage／寸法別再利用／512 MiB payload予算を実装。WARPで整数編集の全画素一致と自由回転／SAR／metadata orientation／合成順序、黒canvas・再利用・source不変を確認。次は動画の操作UI、適用前予算検証、編集後geometry／selection・preview／Undo/Redoへ接続する。通常app入口はまだ未公開であり、画質／HDR／全寸法性能・通常windowとH1／UX台帳全体は未完。

2026-09-10 11:45 動画角度UI checkpoint: timeline表示中のEdit menu／Ctrl+Shift+R／角度dialog／sliderと映像面のGPU previewを接続。Apply前のgeometry／budget照合、Cancel／0度・古いtoken／context、selection／crop／再回転／Undo/Redoと保存再読込を確認。software実動画と実D3D11VAのapp描画を検証し、後者はCPU転送0を維持。次は動画のAlt保持drag。通常windowの外観・全寸法／HDR品質／性能とUX台帳全体は未完のまま継続する。

2026-09-10 12:12 動画hold-drag checkpoint: Alt＋左dragを画像と共通の入力契約で接続。角度dialogのsnapshot／geometry・budget確認／GPU preview／確定を共有し、1×／2×、release順序、取消12条件、Undo／0度／保存再読込を回帰確認。実D3D11VAの10描画点もCPU転送0を維持。次はV05動画zoom／resizeの採用契約を照合する。通常window入力・外観／全素材品質／性能とUX台帳全体は未完。

2026-09-09 16:48追記: ownerの最新の区切り・push依頼に従い、進行中だったcompact seek端点修正だけを検証してcheckpointに含める。ローンチ準備は再開せず、このcheckpoint後は次の機能・UI実装へ自動着手しない。既存の改善台帳は残件として保持し、次のgoal設定・作業指示を待つ。goal全体の完了を意味しない。

2026-09-09 新goal: ownerが草案とfollow-upに基づく機能・操作・UI改善を指定した。[UX_IMPLEMENTATION_PLAN](UX_IMPLEMENTATION_PLAN.md)の台帳へ要求・既存実装・残件を対応付け、H1内で順次実装・検証する。下記の「新goal待ち」は解消したが、ローンチ準備の停止は維持する。

2026-09-09 16:11追記: owner指定によりローンチ準備はここで停止し、検証済みcheckpointをpushして区切る。次の機能・UI改善の具体的なgoalはownerが別途設定するため、以下の候補を自動的に開始しない。ローンチ全体を完了扱いにはしない。

M0～M7の実装とH1の主要な回帰修正、代表保存・固定Seek・30分4K・Windows 11上の実Setup lifecycleは検証済み。ここからは未公開評価版の見た目・操作感・安定性の仕上げを優先する。以下は以前の日付付き記録の公開／Windows 10必須gateより優先する現在の方針である。

1. 草案と現在のWelcome／画像／動画／音声・menu／timeline／filmstripを同条件で比較し、残る外観・操作上の差を整理して必要な範囲だけ直す → verify: before/after実画面と同じ日常操作flow、関連回帰とM0 checks。草案全機能や無関係なrefactorを自動的に必須化しない。
2. 既知の長GOP Seek遅延と大きい素材のOpen／preview応答を、既に通過した基準素材と区別して評価する → verify: 再現条件と測定、改善時は同条件比較。正確なframeを粗いkeyframe表示へ変えて達成扱いにしない。
3. 利用者のブラッシュアップ要望を反映し、選択した日常flowに重大な不一致が残らないことを確認する → verify: 変更点・残る制約の明示、最終変更に対応した回帰／実画面確認。既存の媒体・native監査を変更なしに反復しない。

公開配布は行わない。署名・公開・配布開始は別指示まで対象外で、公開前のclean-machine VCや追加installer認定作業は区別して管理する。Windows 10をowner環境へ入れず、合理的に用意できる仮想環境がなければ実機確認は省略可・未検証とする。既存CIは`windows-2022`であり、[標準GitHub-hosted runner](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)にWindows 10はない。別途Windows 10環境を用意した[self-hosted runner](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)は可能だが、その構築や費用を暗黙に追加しない。他の物理device／混在DPI／支援技術も実施範囲を正しく記録し、未検証を合格としない。

## 直近の実績

2026-09-09 15:44、ownerが実インストールを許可した。Windows 11基準機の専用per-user配置へ4937a657 Setupを導入し、2b10319aへ実更新、shortcut起動・画像保存／再Open、通常Uninstall.exeの自己copy削除まで終了code 0で確認した。2708→2709 payload全一致、900 frames/drop 0、回転6144画素一致、利用者file／設定8件・共有VC保持、評価登録／shortcut削除を確認。復旧資料約42 MBとsentinelは保持し、M0全278 tests通過。実導入許可待ちは解消したが、Windows 10／VC未導入環境・実中断復旧・実入力／owner外観受入・公開判断は継続する。詳細と試験の範囲はLOCAL_SETUPの実lifecycle記録を参照。

2026-09-09 15:25、評価Setupへ正確なinstaller source 22 filesとmanifestのZIPを同梱した。案内のlink／hash、明示配置／削除一覧と改変拒否4条件を検証し、展開したソースのbuilderを別cwdから実行して再構築に成功。2709 payload filesは全bytes一致し、Setup exe自体は別hashなので完全再現buildとはしない。本体f91b4498／runtime／companionは不変、M0全278 tests通過。実導入・更新・自己copy削除、VC／対応OS、実入力・owner外観受入と公開判断は未完了であり、次は実導入を許可された隔離環境が必要である。

2026-09-09 15:11、評価済みf91b4498とsource c277cebへapp kit・catalog・companion・Setup固定情報を更新した。旧成果物と12 native kits／9本体原文は保持。新しいローカルSetupは4937a657、対応companionはfeba16ba。251 Git blobs、資料・link・欠落／改変拒否の回帰、95 binary／2708 payload paths／非installing検査、M0全278 testsが通過した。生成しただけで実導入／公開はしていない。次は現在のinstaller sourceを正確に提供できる資料化を進め、実環境lifecycle・owner受入・配布承認を別に確認する。

2026-09-09 14:57、同じ通常release f91b4498で30分4K再生も完走した。Seekなしで全107771 frames表示、drop／CPU transfer 0、drift p95 4.812ms／最大17.349ms。通常終了code 0、原本／隣接95ファイル不変、終了後278 tests・format・Clippy通過。単一基準機での代表保存／固定Seek／長時間再生を直接確認した段階であり、長GOPの未達と実環境・owner受入・配布gateは残る。次は新本体のmatching source／資料／Setupを整備する。詳細はDEVELOPMENTの14:57記録を参照。

2026-09-09 14:17、設定／cache耐障害性を含む通常release f91b4498で、代表PNG／WAV／MP4の保存と再Openを再確認した。PNG回転の6144画素、音声trim・速度・音量変更の307254 PCM samplesが参照と一致し、音声tab再利用時の二重編集は再現しなかった。動画は保存前後とも900表示・drop 0。Seekは2秒間隔keyframeの4条件各100回でp95 40.242～108.103msだが、先頭keyframeのみの120秒素材では872.539～979.008msで300ms目標未達。詳細・試験範囲はDEVELOPMENTの14:10／14:17記録を参照。長GOP評価、新本体の長時間4K、対応配布資料と実installer・対象環境・owner受入は未完了である。

2026-09-07、H1の代表保存・再open監査で、cleanな音声folder tabの再利用時に保存済み編集が別sourceへ残る問題を修正した。同sourceとdirty編集の保護を回帰試験で保持し、通常releaseでPNGの画素一致、chirpのPCM一致、動画の4秒/120 framesと再openを確認した。詳細はDEVELOPMENTの14:28記録を参照。H1とlaunch全体は未完了である。

2026-09-08、再生成native FFmpegと別targetの通常releaseで、4条件各100回のSeek、画像／音声／動画の保存と再openを再確認した。Seek p95は31.764～101.288msで300ms以内、PNG画素・chirp PCMは参照と一致した。全268 testsと必須check後、同じbinaryの30分4K60試験も完走した。107,771表示、drop／CPU transfer 0、drift p95 4.808ms・最大30.042msで基準内。5分以降のprivateは222.63～238.92 MiBで、旧15分付近の大きな増加は今回再現しなかったが、原因やリーク不在は未証明。通常終了とexe／source／DLL不変を確認した。配布監査ではZVBIの個別GPL表記と実DLLの対応関数を確認しており、性能合格だけで同梱を承認しない。配布材料／Setup.exe、実環境とowner受入を含むH1全体は継続する。

同日20:43、限定ZVBIを含む新候補の長時間再生で残った303 frames差を追試した。ownerの開始時右矢印誤操作の可能性という補足に対し、30秒素材の停止中／再生中の先頭要求は全892 frames・drop 0、4K原本の先頭0→右5秒も確認した。原本の5.05秒より前は実decodeで303 frames、残107468が前回と一致する。過去keyの確証はないが問題は再現せず、owner指示に従いこの差だけの30分再試験はせず配布準備へ進む。前回を全frame再生の証明へ変更せず、H1／Setup.exe／対象環境・owner受入は未完了のまま維持する。

## M0 — Foundation（完了）

Git、Rust workspace、設計・運用文書、CIを構築する。再生、UI、Shell連携のruntime実装は行わない。

完了ゲート:

- `concepts/`がGit対象外である。
- workspaceがformat、Clippy、testを警告なしで通る。
- Windows CI定義が同じ検証を行う。
- OS、ライセンス、crate境界、D3D11、FFmpeg、WASAPI、Explorer sortの契約が文書間で一致する。
- verified checkpointが`origin/main`へpushされる。

M0は2026-09-04にWindows CIを含めてゲートを通過した。

## M1 — Software playback vertical slice

単一windowと単一fileに限定し、software video decode、D3D11 upload、event-driven WASAPI Sharedによる音声、play/pause/EOFを完成させる。

fixtureは少なくともMP4/H.264/AAC、MKV/HEVC/AAC、WebM/VP9/Opusを含める。UIの再現、tab、folder navigation、編集は行わない。

M1は2026-09-04に完了した。固定FFmpeg 9.0.1で3種類のfixtureを生成・検査し、software decode integration testを通過した。実機ではD3D11 Flip Discard swap chainへのupload、WASAPI Sharedの音声排出、Spaceによるpause/resume、最終frame/sample排出後のEOF遷移を確認した。現在の次工程はM2だが、まだ着手していない。

## M2 — D3D11VA zero-copy

アプリ作成のD3D11 deviceをFFmpegへ渡し、D3D11VA surfaceからVideo ProcessorとSwapChainまでCPU transferなしで表示する。D3D11VA非対応時のみM1のsoftware pathへfallbackする。

adapter LUID、hardware frame count、CPU transfer countを記録し、hardware pathで`CPU transfer = 0`を証明する。

M2は2026-09-04に完了した。基準adapter `00000000:0001311b`でH.264をD3D11VA decodeし、60 hardware frames、0 CPU transfersでVideo ProcessorからSwapChainまで表示してEOFへ到達した。同adapterでhardware初期化できないHEVCとVP9、およびD3D11VA構成を持たないFFV1はM1 software pathへfallbackし、それぞれ60 CPU transfersでEOFへ到達した。

## M3 — Seek, synchronization, and resilience

audio master clock、video-only clock、frame pacing、generation付きSeek、連続Seek、pause/resume、endpoint変更、device removalを完成させる。

基準機で4K60を10分再生してdrop率0.1%未満、30分でA/V drift p95 40ms以下・最大100ms以下、ローカル1080p H.264の100回Seekでp95 300ms以下を目標ゲートとする。

M3は2026-09-04に完了した。demux、video decode、audio decode、WASAPI outputをbounded queueで分離し、`IAudioClock`を通常のmaster、device clock停止時とvideo-only区間を単調時計で補う構成にした。Seekはpipelineをgeneration単位で破棄・再構築し、古いeventを無視する。default render endpoint変更とD3D11 device removalはtyped eventから現在位置でpipelineを再構築する。基準adapter `00000000:0001311b`の30分4K60 H.264/AAC実時間再生では107,768 framesを表示、3 framesをdropしてdrop率0.0028%、A/V drift p95 4.772ms・最大35.759msだった。ローカル1080p H.264の100回Seekはp95 37.750ms・最大56.991msだった。現在の次工程はM4である。

## M4 — Application shell and navigation（完了）

tab、status bar、command registry、command palette、customizable shortcut、audio folder playlist、filmstrip、Explorer folder sort連携を実装する。

Explorer sortは`docs/ARCHITECTURE.md`の`FolderSnapshot`契約と検証matrixを満たすこと。

M4は2026-09-04に完了した。eguiを同一D3D11 back bufferへ統合し、tab、status bar、共有command registry、command palette、prefix対応カスタムshortcut、audio folder playlist、全media filmstripを実装した。`FolderOrderProvider`は専用STAでmatching live Explorer viewを優先し、閉じている場合は非表示`IExplorerBrowser`、失敗時だけWindows自然名前順を用いる。folder変更はoverlapped `ReadDirectoryChangesW`と150 ms debounceでsnapshotを再取得する。fixture付きExplorer integration testでName、Date modified、Date created、Size、Typeの昇順・降順、同値、複数列、およびsort変更後の再取得を確認した。現在の次工程はM5である。

## M5 — Images and reading mode（完了）

静止画、アニメ画像、zoom、selection、crop preview、reading modeを追加する。動画と共有するのはvisual surfaceとpresentation上の概念に限定する。

M5は2026-09-05に完了した。BMP、JPEG、PNG、TIFF、WebPの静止画、GIF・WebP・APNGのanimation、FFmpegによるAVIFを安全なRGBA frameへdecodeし、同じD3D11 device上のegui textureとして表示する。EXIF orientation、deadline駆動animation、cursor anchor zoom、actual/fit、右drag pan、正方形・比率保持・辺resize対応selection、非破壊crop previewを実装した。reading modeは共有`FolderSnapshot`のExplorer Shell順から2～10枚を選び、横・縦配置と表示順反転を行う表示専用modeである。実cropや保存はまだ行わない。現在の次工程はM6である。

## M6 — Non-destructive editing and export（完了）

crop、rotate、flip、trim、volume、rateを非破壊操作として保持し、undo/redo、unsaved indicator、close guard、FFmpeg exportを追加する。

M6は2026-09-05に完了した。tab単位のsaved cursor付きedit historyへcrop、時計回り・反時計回り90度回転、水平・垂直反転、trim端点、volume、rateを保持し、branch対応undo/redoと画像のoperation順previewを実装した。動画crop selectionと動画・音声の編集値もsourceを変更せず保持する。dirty状態はtab/window/statusへ表示し、media移動、tab close、process終了をExport / Discard / Cancel modalで保護する。Save Asと再SaveはmetadataをcopyするFFmpeg software exportを行い、source同一pathは拒否する。画像crop+rotateと音声付き動画trim+rate+volumeの実export・再decodeをfixtureで確認した。現在の次工程はM7である。

## M7 — Advanced presentation and interaction（完了）

HDR、waveform/thumbnail cache、multi-window tab drag、grid menu、hardware encode、詳細アニメーションを追加する。

M7は2026-09-05に完了した。path・size・更新時刻keyと64 MiB上限を持つdisk cache、UI thread外で生成するwaveform / duration / hover thumbnail、click/drag Seek付きtimeline、メディア種別ごとに設定可能な`1234/qwer/asdf/zxcv` grid menu、window外tab dropによる別process window化、Media Foundation hardware encodeの強制要求とsoftware fallbackを実装した。gridは短いopacity transitionだけを使う。PQ/HLG metadataはhardware frameとともに保持し、D3D11 Video Processorが明示的にHDR→SDR変換を保証する場合だけcolor spaceを設定する。基準adapterは同変換を保証しなかったためHDR表示を成功とは扱わず、typed errorを確認した。HDR displayへの10-bit pass-throughとprocess間tab再結合は対象外である。

## H1 — Human evaluation and UX stabilization（進行中）

M0～M7で構築した技術sliceを開発版として人が操作し、日常flowの摩擦、表示と実際の動作の不一致、応答停止、発見性、DPI・入力・error stateの問題を収集して直す。これは草案の未実装項目を一括投入するfeature milestoneではなく、観察できる一つのscenarioを単位にする反復phaseである。

各変更のgate:

- 再現手順、期待結果、対象media・環境が記録されている。
- 変更前後を同じ手順で比較し、実windowで結果を確認する。
- stateやdomain logicの回帰には自動testがある。
- UI差分へ無関係なruntime refactorや次のfeatureを混ぜない。
- format、workspace全targetのClippy、workspace全targetのtestが通る。
- architecture上の決定が変わる場合は実装前に`ARCHITECTURE.md`を更新する。

最初の優先候補は、動画・音声edit値とlive playbackの不一致、同期画像loadと同期exportによるUI停止、timeline/tab/filmstrip/menuの発見性と操作感である。詳細な試用方法と現状差分は`docs/DEVELOPMENT.md`と`docs/KNOWN_GAPS.md`を正とする。PackagingはH1と並行して暗黙に開始せず、FFmpeg binaryとlicense条件を別途決定してから計画する。

2026-09-09、cache directoryの作成不能がアプリ初期化を失敗させる経路と、cache一時fileの使用中が正常なpreviewを破棄する経路を回帰で再現した。cache I/Oだけを補助処理へ変更し、原本・衝突fileの保持、生成／decode失敗・取消の伝達、保存先復旧後の再保存を確認する。実FFmpegのfilmstrip生成でもcache使用中／作成不能からの継続と復帰を検証した。最終通常binaryの実画面／性能と、新本体・source・Setupの対応付けは引き続き必要であり、旧195af870入り評価Setupへこの修正を含むとは扱わない。

続いて、shortcutの一行の記述ミスでwindow作成前の初期化が失敗することを再現した。起動時だけ失敗した設定へ既定値を使い、原本保持・有効な側の設定保持・修正後Reloadと失敗時の現在値保持を回帰で確認する。ownerの画面操作許可後、通常release f91b4498と隔離設定でnative OK警告全文、背景の無効化、Enter解除とmenu／修正後Reloadを確認した。使えないcacheでもfilmstripを表示し、保存先復旧後は同じprocessで保存できた。全エラー種別・screen reader・DPIの確認ではない。APPDATA欠落やgraphics失敗まで成功扱いにせず、配布候補も未差し替えである。

2026-09-07、ownerはmonapadと同様のインストーラーexeを選択した。配布形式の確認待ちは解消し、ARCHITECTURE §7へ反映した。H1の品質gateを維持し、配布準備は次の順序で進める。形式の決定だけを同梱物・公開の承認やlaunch完了としない。

1. 固定FFmpegの推移依存、補助exe、対応source/build設定、第三者表示とVisual C++ runtimeの再配布条件を確認する → verify: 同梱対象と根拠を固定した一覧。必要資料が不足するbinaryは配布候補にしない。
2. その確認後、インストール先を選べるSetup.exeと開発環境に依存しないruntime探索を実装する → verify: FFMPEG_DIRや開発用PATHなしで起動・preview・保存、任意の作業directoryからの起動とtab detachを確認する。
3. 隔離した対象Windows環境で導入・更新・アンインストールを検証する → verify: 代表mediaの再生/保存、安全な更新、sourceと利用者設定を勝手に消さない削除。署名・公開は別途判断し、未署名試験版を公開済み製品と呼ばない。

2026-09-08、Communityの個人開発利用をownerへ確認し、公式VC x64再配布packageと読み取り専用の導入判定を[VC_REDIST.md](VC_REDIST.md)へ固定した。26状態の回帰、原本の署名／hash、32-bit processからの64-bit登録参照を検証済み。実導入・規約同意・再起動／取消・clean-machine検証は未実施で、第2段階へ通過した扱いにしない。

2026-09-09、[評価候補と配布資料の対応付け](CANDIDATE_MATERIALS.md)で、実exe＋94 runtime files、対応する本体source snapshot、13 kitとローカルHTML案内を接続した。名指ししたnotice／data原本の収録から、利用者向けの入口と最終同時提供へ進む。ブラウザーのfile URL制限により実表示は未検証で、アプリ内の入口・Setup.exe・隔離した導入／更新／削除・最終候補品質とowner受入は引き続き未完了。資料収録だけをruntime採用・公開承認としない。

同日01:21、Help／paletteからexe隣のlicense guideをExplorerで選択する入口を追加した。専用STA worker、資料欠落時のpath案内、重複抑止・再生／未保存編集の保持を回帰と通常releaseで確認。273 tests・必須checkが通過した。古い資料集は旧exeの記録として保持し、最終app／source再対応付け、実installer配置・HTML表示・同時提供と、導入／品質／owner受入gateは継続する。

同日、[NSISの安全性fixture](INSTALLER_FIXTURE.md)を先に実装し、配置先選択・既存folder拒否・日本語path・marker照合・明示fileだけの削除・利用者file保持・使用中file失敗後の再試行を検証した。fixture文書だけの通常user権限Setupで、本体／FFmpeg／VC runtime、registryやshortcutは含まない。実アプリのSetup組み込み、更新と隔離対象OSでの導入、最終資料対応付け・品質gateは継続し、この試験で第1～3段階を一括通過した扱いにしない。

続いて現候補195af870とHelp対応source 06588b6を再対応付けし、app kit v2／catalog v11／候補資料v3を検証した。12 native kitsと本体の原文は維持し、224 source files・95実行file bindings・113 linksと全欠落／改変回帰が通過。実資料をexe隣へ配置したHelpの選択表示も確認した。2879 filesの資料を別提供できる約510 MBのportable ZIPへまとめ、全内容一致を検証した。実アプリSetupへの組み込み、VC consent／導入・更新・削除、同時公開と最終品質／owner受入は残る。

同日、本体入りの[local Setup評価](LOCAL_SETUP.md)を組み立てた。95実行fileと2610原本を固定hashへ照合し、短いnotice名と元pathの対応表、別source companion案内、VCのfull UI／post-check wrapperを接続する。共通fixtureの空cwd試験で判明したdrive rootの相対解決を修正した。Setupの検査は配置しないprobeと資料／一覧照合、VCは読み取り専用と16模擬ケースまでで、実アプリや共有runtimeの導入・削除は行わない。registration／shortcut／更新、隔離対象OSのlifecycle、同時提供・最終品質／owner受入と配布採用は継続する。

続いてlocal Setupへ現在userだけのuninstall登録とPrograms shortcutを接続した。既存占有の拒否、payload id／pathの所有確認、変更shortcutと未知registry値の保持、使用中fileの再試行を専用GUID key／仮shortcutで検証した。WSHの日本語path拒否を実測し、Unicode Shell APIで32／64-bitの作成・読み戻しを確認。本体／VCの実導入は行わず、更新、対象OSでの登録・起動・削除、最終品質・同時提供とowner受入は継続する。

既存版更新の前提として、旧marker／新旧inventoryと実bytesを照合し、追加・置換・削除・維持を分ける読み取り専用の判定を追加した。利用者の差し替えDLL、所有外衝突、不正path／DOS別名、使用中file等を拒否し、元fileは変更しない。実2708-file資料でも2707維持・inventoryのみ置換を確認したが、markerとuninstallerは試験用である。実置換・退避復旧・登録切替が揃うまでSetupの既存folder拒否を維持し、更新対応の完了とはしない。

続いてfile単位の退避・配置・復旧を試験配置へ実装した。新旧copyとjournal、直前照合と共有制限付きhandle、旧名の退避後に新名を配置する二段階処理、本体exeの最終配置、receiptを使う中断復旧を追加。32／64-bit PowerShellで更新10か所・復旧10か所の失敗と子process終了後の復旧、hard link先と利用者fileの保持を検証した。実Setupには未接続で、登録切替、journal digestの永続化と復旧UI、cleanup、対象OS・metadata／power-loss検証を残す。file試験の成功を更新全体やlaunch gateの完了とはしない。

復旧は退避原本を戻す方式へ改め、復旧用data copyなしで元のhard link関係・日時・security descriptor・試験用alternate streamが戻ることを32／64-bitで確認した。登録helperには同directoryの所有ID／容量だけを切り替える処理を追加し、二つの書き込み間の失敗と逆方向復旧、未知の値・shortcut保持を試験した。原本の欠落／使用中、不明な登録状態は上書きせず停止する。fileと登録のjournal結合、実Setup／復旧UI、満杯volume・ACL・電源断と対象OSの検証は引き続き未完了である。

旧登録の型付き値とfile plan由来の新値を、一つの更新journalへ結合した。登録の直前検証とexe最終配置前の切替、同じ記録からの共同Rollbackを追加し、file移動20か所・登録書き込み4か所の中断と、登録の二値の間で子processが終了した状態からの復旧を32／64-bitで確認した。未知の登録状態と別directoryへの結合は両側を変更せず拒否する。独立したdigest保存・再発見、uninstaller競合対策と実Setup／復旧UI、cleanup・対象OSのgateは残し、既存folderへのSetup更新はまだ有効にしない。

内部の登録付きentry pointへ、配置変更前のjournal directory／digest永続化と、登録keyからの再発見によるRollbackを追加した。pending値があれば新規更新と新helperによる削除を拒否する。新Setupの導入／削除全処理とentry pointはuser／登録key単位の共通object leaseで排他し、文書fixtureの実section途中に競合を起こして保持と拒否、process終了後の再試行を確認する。旧生成uninstallerの保護、更新childへの所有受け渡し、復旧UI・cleanup・対象OSでの実lifecycleは未完了であり、既存folder拒否は継続する。

schema 3の更新／Rollbackではuninstallerを先に退避し、本体はpayload／登録が一致してから、uninstallerはさらにその後へ戻す。file-only／登録付きの移動40境界で混在中の設置exe不在と公開時の整合を確認し、同一uninstaller bytesと旧schema 1／2の本体だけ退避した状態も復旧する。起動済み旧uninstallerの制御、実Setupの更新／復旧UI接続と対象OSのgateは残す。

続いてlocal Setupへ配置外staging・確認付き更新とpending記録からの旧版復旧を接続した。親が変更前にleaseを解放し、childが通常取得・全体再検証する。復旧後はSetupの再実行で更新をやり直す。実NSIS app branchを生成text payload／GUID key／仮shortcutへ限定した試験で、更新・変更file拒否・登録中断・pending削除拒否・旧状態への復旧・再更新・削除を確認した。NSIS plugin directoryのSystem.dllがPowerShellのframework参照を遮る復旧失敗も再現・修正。実アプリ／VC導入、通常自己copy削除、対象OS・詳細progress／復旧UI・cleanup・同時source提供・最終品質とowner受入は未完了である。

Setupの完了文面を新規導入・更新・旧版復旧に分け、3010の手動再起動案内を保持し、更新childの段階別診断を実行中のdetails logへ接続した。CIで出た短縮一時path由来の新uninstaller拒否をローカル再現し、caller sourceだけを正規化する。inventory名の別名拒否は維持する。実短縮TEMP/TMPでの生成Setup、復旧と再更新、表示値・模擬3010、32／64-bit transaction回帰は通過。実画面の表示／focus／読み上げ・詳細進捗と対象OS／VC／製品lifecycleなどのgateは残る。

ownerの画面操作許可後、生成Setupで三種類の完了画面と失敗後の復旧を確認し、切れていた再試行案内を短い行へ分割した。さらに更新childを復旧記録保存直後で待機させ、設置file未変更・child待機中の時点で、その段階のログが実画面とUIAへ届くことを確認した。試験processの標準providerを読み込むとButton／Invokeも取得でき、Finishへfocusした同じSetupが終了0となる。実製品・VC導入、Windows 10・他DPI・screen reader、最終本体の品質と資料の再対応付けは別gateとして維持する。

2026-09-08、既存preview／保存のhelper探索だけはH1の独立した修正として先に検証する。FFMPEG_DIRによる同梱版の上書きや、不足helperをPATH上の別版で埋め合わせる動作を防ぐ。第2段階のinstaller作成・runtime採用は第1段階の監査後のままとし、helper単体の検証でそのgateを通過扱いにしない。

2026-09-08、第一段階で固定開発FFmpegのChromaprint→GPL FFTW静的リンクを確認し、既存binaryを配布候補から除外した。次の配布作業は[FFMPEG_REBUILD.md](FFMPEG_REBUILD.md)の機能を保つ再buildと対応資料の確定であり、旧DLLをそのままinstallerへ組み込むことではない。KissFFT版Chromaprintの単体試験は通ったが、全体差替え・性能・再配布条件のgateは未完了。

ZVBIについてはownerの許可を受け、未使用の番組制御・放送時刻APIを含めない限定buildを隔離して検証する。合成字幕12条件、実放送TS 2本、新headerでのFFmpeg全体再buildと字幕出力比較は通過した。最終候補の品質・配布監査は残る。限定DLLを汎用ZVBIの全API互換品とせず、H1やinstallerのgateを短縮しない。

2026-09-05、exportの応答停止を改善した。runtimeのbackground job、書き出し時間表示、cancel、成功後だけのtarget置換、exportした履歴位置のsaved判定、dirty guardの成功・失敗・cancel遷移を追加した。生成fixtureと実windowで再生継続、既存target保護、追加編集のdirty保持を確認した。

同日、画像decodeをlatest-only workerへ移し、画像/reading要求のRGBA保持量を512 MiBへ制限した。6000×6000画像の起動時panicをdevice上限の正しい伝達で修正し、上限超過・破損pageのerror表示と相対path起動時のShell順navigationを確認した。H1全体は未完了で、live volume/rate、草案の外観と操作感の再現を引き続き優先する。

続いてvolume/muteをlive playbackへ反映した。WASAPIへ渡す直前のgain、5 ms ramp、undo/redoとpipeline再構築時の保持を追加し、対象processのsession peakで100%・50%・muteを確認した。試験中に再現したmono WAVのdecode失敗も、未指定channel layoutの補完で修正し、mono/stereoの直列・並列decodeと実windowを確認した。rateと草案の外観・操作感は引き続き未完了である。

その後、0.25～4倍のピッチ維持live rateを追加した。速度変更はsource位置を保つgeneration付き再構築とし、音声・映像時計、pause中のframe保持、音声drain後の時計引継ぎを対応させた。tone/clockの自動testと2倍・4倍の実動画trialが通過した。次は草案に沿ったcompact shellと日常操作の改善を進める。H1全体とlaunch auditは未完了である。

compact shellの初回改善として、32px title/tab bar、30px status、logo menu、等分tab、native window操作、名前省略・情報右寄せを実装した。EOF後のPlayも先頭から再開する。実windowで最小幅、移動・resize・最大化・最小化復帰、長い名前の複数tab、dirty close guardを確認した。次は動画をbarに隠れないmedia領域へ収める表示修正と、thin seek bar・日常操作の改善を進める。

動画の表示矩形を同frameのUI layoutから求め、hardware/software共通のaspect-fitとsample aspect ratio、selection位置を対応させた。縦長H.264・非正方形pixelのFFV1で四辺の保持とtimeline追従を確認した。UIのrepaint deadlineも待機へ反映したが、操作後のUI一部欠落の原因確定は残っており、thin seek barより先に描画安定性を調べる。

続くpixel照合で、直近captureに対するUI欠落の目視判定を訂正した。hardware/software各10回のtimeline開閉でもtabとcontrolsは一致した。音声drain後のpauseは実WASAPI testと実windowでFaultedを再現し、正常排出と異常終了を区別して修正した。次はthin seek barとkeyboard中心の日常操作を改善し、launch auditへ進める。

thin seek bar、画像のShell順位置移動、paletteの上下選択・Enter・Escapeを追加した。dragはreleaseで一回だけ確定し、EOFからのSeekはPausedにする。停止中の非frame境界Seekで黒画面になる問題も再現・修正し、最初のframeを保持してから再開できることを確認した。H1は継続中で、次はOpen・folder移動・入力を通した日常flowと残る草案との差を監査する。画像thumbnail、本画面scrub、物理keyboard・IME・DPIのmatrixは今回の完了範囲ではない。

Open Folderの日常flowで、対応mediaがないfolderを選ぶと元のnavigationだけが壊れる問題を修正した。元のsnapshot・tab・編集を保持し、画像移動を続けられることを実windowと回帰testで確認した。同時にShell snapshot待機中の応答停止を確認したため、次は取得待ちをUI threadから外す改善を優先する。

Shell snapshot取得を最新1件の非同期要求へ移し、generationによる失効、同folderの旧snapshot保持、reading更新、待機中のwindow closeを実装・検証した。実Explorer sort matrixも通過した。一方でnative pickerをCancelするだけでも再生遅延が再現したため、次はpicker自体のUI thread待機と復帰時の再生を修正する。H1とlaunch auditは継続中である。

native Open file/folder・Save Asも専用STAへ移し、本体入力のmodal制限とowner寿命を保ったまま描画・再生を継続するようにした。30秒H.264/AACでOpen Folder→Cancelした際のdropは808から0になった。Save AsのCancel時のdirty guard復元、pointerを動かさない再click、書き出し成功後のtab終了も実windowで確認した。次は静止画・pause時のidle負荷と日常入力を監査し、草案との残る表示・操作差を詰める。H1全体は未完了である。

idle監査では再描画eventの自己再予約、静止grid、音声のみの再生に無条件描画を発見し、入力・animation・位置更新deadlineへ限定した。5秒間のCPU時間は静止画/Welcomeで約5.9秒から計測分解能以下、音声再生で約5.9秒から0.47秒へ下がった。必要な動画/GIF更新とmenu操作は実windowで維持を確認した。次はExplorerからのfile dropを含む日常Open操作と、草案のfilmstrip/menuとの差を監査する。launch全体は未完了である。

Explorerからのfile/folder dropを既存のOpenへ接続した。実OLE drag/dropで画像・動画・音声、複数画像、folder、hover取消、dirty編集保持、確認dialog中の拒否を確認した。folderは従来のlatest-only Openで、複数folderのimport機能は追加しない。次はfilmstripのthumbnailとmenuの操作・発見性を、日常flowと草案の両面から改善する。H1は継続中である。

filmstripを中央のthumbnail列へ変更し、画像/動画preview、音声waveform・duration、現在項目の枠と名前を追加した。可視項目だけを最大64件の単一workerで読み込み、古い結果を失効させる。実windowでclick・dirty guard・middle click・Tab前後移動・wheel横scroll・破損previewを確認し、cold cacheの30秒再生も900 frames / 0 dropsで完了した。次はlogo menuの整理・発見性と、日常閲覧に必要な残りの操作を監査する。H1とlaunch全体は未完了である。

logo menuの全command縦列をFile / Edit / Viewへ分類し、関連項目の区切りとcustom shortcutの右揃えを追加した。全commandの一意配置、無効項目、dispatch、最小windowでの末尾到達をtestし、実windowで回転・Undo・dirty closeとpalette起動を確認した。方向gestureは後回しとし、次はfullscreenを含む日常閲覧の表示・入力不足を検証する。launch全体は引き続き未完了である。

F11/View menuからのborderless fullscreenを追加し、persistent bar・timelineを隠してmedia領域を拡大した。Escapeはoverlay/modalを優先し、復帰時に位置・size・最大化を保持する。最大化から直接入る際の旧client領域残りを再現し、解除/復元順序を修正した。画像・reading・hardware/software動画とdirty guardを実windowで確認し、102 testが通過した。次はfullscreenのpointer/操作案内、keyboard・DPIを含む閲覧flowと残るlive-previewの差を監査する。H1とlaunch全体は未完了である。

fullscreenの画像・reading・動画へ2秒idleのcursor非表示を追加した。入力で戻し、button保持・overlay・modal・loading/error中は表示を維持する。右button保持判定と最小化からのstationary pointer復帰で実機上の問題を再現・修正した。静止PNGの追加連続描画はなく、104 testが通過した。次は動画編集のlive previewとkeyboard・DPIを含む残りの閲覧flowを監査する。edge-hover controls、double-clickやlaunch全体の完了とは扱わない。

動画のcrop・90度回転・反転を現在の再生へ反映した。画像と共有する履歴順UV、回転後のSAR・selection、hardware編集時だけの同device内RGBA合成を追加し、CPU readbackは行わない。hardware/softwareの実window、Undo/Redo、停止中Seekと実exportの画素照合を確認し、106 testが通過した。約1分4K60も途中から回転表示にしてdrop 0で完了した。次はcropのpixel丸め・極小範囲と、残るtrim/live表示の不一致を調べる。keyboard・DPI、配布を含むlaunch全体は未完了である。

cropを整数pixel矩形へ変更し、previewとFFmpeg exportの独立した丸めをなくした。1×1 PNGの保存失敗と隣接色のにじみを再現・修正し、画像1 pixel・動画偶数pixelの選択、動画16×16未満の拒否、寸法案内と全領域no-opを追加した。PNGの奇数寸法・回転後再cropと最小H.264 exportを含む114 test、実windowの保存・選択保持が通過した。次はtrim範囲とlive playbackの不一致、入力・DPIなど残るlaunch gateを監査する。H1全体は継続中である。

trim監査では、同じ位置のI/Oを受理しSave Asで初めて失敗する問題を再現した。端点の入力時検証、暗黙のsource先頭/末尾、no-op、timelineの保存範囲・ミリ秒表示、Undo通知を追加し、119 testと動画/音声の実windowを確認した。これはlive trim完了ではなく入力・確認の段階である。次は範囲外の再選択と両立する範囲内再生、音声sample端点・rate・Seek・EOFを検証する。H1とlaunch全体は継続中である。

続いてtrimをlive playbackへ接続した。半開区間内の映像・音声sampleだけを送り、両streamの終端でworkerを回収する。終端で停止して再Playは開始へ戻り、範囲外Seekはpaused source previewとして端点の再選択を可能にする。123 testとhardware/software動画・0.25倍音声、2倍動画、Undo/Redo・tab復帰を確認した。音声sampleのナノ秒丸めで生じた1 sample差も修正した。次は極小範囲・低精度PTS・export境界を追加監査し、keyboard/IME・DPI・metadata orientationを含む残るlaunch gateへ進む。H1全体は未完了である。

境界監査では、exportが余分なframeを残す秒丸めと、低精度音声PTSの16 samples差を再現・修正した。整数tick/sample端点、source時刻維持とstream指定、空trim出力のpublish拒否を追加した。source offset、frame色、非圧縮sample列、既存保存先保護と実windowのH.264/AAC保存を確認した。ただし途中Seek後の音声sub-tick位相差は残る。次はこの差の影響を定量化し、残るmetadata orientation・keyboard/IME・DPIとlaunch gateを進める。H1全体は未完了である。

途中Seekの音声差は対象fixtureで8 samples（約0.167 ms）と定量化した。続いて回転metadata付き動画が横向きになる問題を実windowで再現し、90度単位の回転・反転を編集より先に適用した。hardware/software、crop保存・再open・Undoと130 testsを確認した。上下反転を含むOpenH264保存失敗と、非対応matrixの理由が消える黒画面も修正した。任意行列は明示errorとし、全container・動的metadataの保証はしない。次はkeyboard/IME・DPI、日常操作と残るlaunch gateを進める。H1全体は継続中である。

日本語filenameが欠字になる問題を実windowで再現し、OS日本語fontを既定fontの後ろへ追加した。paletteのIME確定Enterの誤実行と、上下focus移動による確定文字の取りこぼしもevent回帰testで修正した。132 tests、実windowの日本語tab/statusと通常palette操作、静止時の低CPUを確認した。実IME候補操作、物理keyboard、複数DPI/monitorと残るlaunch gateは未完了で、H1を継続する。

小さいwindowのZoom inが固定960×576基準で飛ぶ問題と、画像100%がUI倍率に追従する問題を修正した。現在viewport・編集後寸法とphysical pixel基準を使い、pointer anchorを維持する。実windowで残った二重拡大は固定UI rendererのadapter補正で解消した。133 testsと8×8画像の実pixel照合、拡大UIのwindow controls、hardware動画を確認した。異なる実DPI間のmonitor移動・実IME候補操作と残るlaunch gateは引き続き未完了である。

大きい画像のFitに残っていた2%下限を除いた。16,384px高の画像で隠れていた両端が、480×300と縦横reading modeでも表示される。Fitからの縮小も2%へ跳ねず、小さい倍率を小数表示する。135 testsと実windowの1.08%表示を確認した。H1とlaunch全体は継続中である。

最小windowでgridの両側が切れる問題を修正した。表示領域内の4×4寸法、名前とpathの省略・tooltipを使い、clickもkeyと同じく一回実行して閉じる。136 tests、実windowの列表示・拡大UI・Zoom in clickを確認した。次は入力/focusを含む残りの操作とlaunch gateを継続監査する。

grid上のCtrl+Sが画像回転になる問題を実windowで再現・修正した。physical keyによる位置対応、Ctrl/Alt/Superの除外、palette・modal優先とpalette起動時のgrid closeを追加した。138 testsと保存dialogのCancel、Shift付きcell実行・Undo、palette検索を確認した。実keyboard layout/IMEと残るlaunch gateは引き続き未完了である。

prefix入力の期限切れ案内とfocusをまたぐ継続を修正した。別操作・focus喪失・Escapeで待ちを解除し、prefix自身の案内だけを消す。139 tests、通常のCtrl+K Ctrl+S、402ms以内のfocus往復後にCtrl+Sが独立した保存操作になる実window試験が通過した。H1と残るlaunch gateの監査を継続する。

保存確認がEscapeで閉じない問題を実windowで再現し、編集を保持するCancelへ対応させた。export失敗では最前面の通知だけを閉じる。背景clickの非解除と、tab・dirty・fullscreen保持を含む140 testsが通過した。H1は継続中で、残るmodal操作・layoutとlaunch全体の安定性/性能を監査する。

window縮小・UI拡大で保存確認の左端と操作buttonが切れる問題を修正した。確認・export通知の幅制限、file名の省略/tooltip、button折り返しとエラー詳細の高さ制限を追加した。141 testsと480×300の拡大UIでCancel clickを確認した。次は長時間再生を含むlaunch全体の検証を継続する。物理IME・混在DPIと配布方針は未確定である。

dbf13b4のrelease再検証は30分4K60のEOFまで到達したが、最大A/V driftが281.340msで100msゲートを超えた。p95 4.725ms、全区間drop率0.015774%、CPU transfer 0でも、長時間ゲート全体は未達とする。次は外れ値の発生位置とpacingを調べて再検証し、計測境界を修正した100回Seek試験へ進む。H1は継続中。

UI遅延後に古いframeをpromotionする問題を300msの試験用遅延で再現した。描画直前にも既存late discardを行う修正で、同条件の最大driftは283.758msから27.039msへ改善し、frame/drop総数も整合する。142 testsと通常buildのpaused Seek確認が通った。遅延/traceコード除去後の30分再試験は107,750 presented＋21 dropped、CPU transfer 0、drift p95 4.803ms・最大37.416msで同期ゲートを満たした。全区間drop率0.019486%、先頭10分へ全dropsを割り当てた上限でも0.058455%で、0.1%未満となる。元の長時間runの遅延原因そのものは未特定。次はSeek計測境界の修正と100回再測定を行う。H1全体は継続中。

Seek計測を同期再構築の前から最初の映像Present成功までへ修正した。旧M3の数値は再構築後からVideoReady通知までの部分計測であり、新しい値とは直接比較しない。通常release・ローカル1080p H.264/AACの中断なし100回で、再生中p95 100.004ms（最大105.746ms）、停止中p95 43.291ms（最大74.149ms）となり、300msゲートを満たした。各要求の完了後に次を送る5秒前後移動で、物理入力/DWM走査表示までの遅延や連打の保証ではない。144 testsと実windowの停止中映像保持・正常終了を確認した。次は残る入力・device recovery・日常flowのlaunch監査を進める。物理IME・混在DPIと配布方針を含むH1全体は未完了。

graphics recovery開始時のAccess deniedを実windowで再現し、旧worker・swap chainの解放順序を修正した。交換後のfont/画像再送とpreview失効も追加し、再生動画・編集済み停止動画・回転画像で復旧と編集保持を確認した。試験用呼び出し除去後の146 tests・通常release再生も通過した。前checkpointのCIは新規Seek testsの設定共有で失敗しており、既存のprocess/config分離方式を適用した。次はこのCI修正の確認と、renderer再作成不能時の未保存編集の救済・通知経路を検証する。実driver reset、endpoint切替、IME/DPIとH1全体は未完了。

renderer再作成不能時のnative確認を追加し、Retry、Cancelでの編集保持、描画なしのExport失敗・再保存・正常終了を実windowで確認した。停止動画は待機後も元の位置・1.25倍・停止を保ち、復旧後に残ったFaulted titleも修正した。保存PNGは回転後の64画素が一致し、148 tests・通常release buildが通った。前checkpointのCIも成功。次は複数dirty tabとexport中の故障/取消、実driver/endpoint経路を継続監査する。H1全体は未完了。

native復旧確認中に保存が完了すると保存済みtabを再確認する問題を再現し、確認終了後に残るdirty tabを選び直すよう修正した。実windowで2枚とも保存してから終了し、出力画素も一致した。追加試験で、生成設定の`zoom_in = +`が次回起動時に読めない決定的な不具合も判明した。`Plus`による保存と旧形式の読込を対応し、隔離設定の初回起動・再起動・旧設定Reloadを確認した。以前のCI原因を「競合」とした説明は未証明で、この生成/読込不一致を訂正根拠とする。151 tests・Clippy・releaseが通過。次はexport取消を含む残る組合せとdevice/endpointの監査を続ける。H1全体は未完了。

export完了と取消が重なると保留中の終了が実行される問題を回帰testで再現し、保存成功の記録と自動離脱を分離した。置換前の取消は既存出力・未保存編集を保持し、置換後は保存出力を残して自動離脱だけを止める。実windowの描画なし取消を両時点で確認し、完了PNGの64画素も一致した。試験用故障コード除去後の152 tests・Clippy・release buildが通過し、前checkpointのCIも成功。次は実device/endpoint経路と残る入力・日常flowの監査を続ける。物理IME・混在DPI・配布を含むH1全体は未完了。

音声device無効化のAPIエラーが一般Faultへ落ちる抜けを修正し、HRESULTで既存endpoint復旧へ分類した。実clientへの一回限りのエラー注入で再生/停止・rate・mute・回転を保持し、停止映像149,350画素が一致した。注入除去後の153 tests・Clippy・releaseと、無音の実WASAPI drain/rate 2 testsが通過した。OS設定や物理deviceは変更していないため、実切替matrixの完了とはしない。次はtrim範囲外・非稼働sessionの復旧とD3D11 removal経路を監査する。H1全体は継続中。

trim開始前の停止previewでは、復旧Seekが終了済み音声workerへPauseを送りFaultedになることを実windowで再現した。Pausedを新pipelineの作成条件へ渡す変更により、復旧前後311,736画素・位置・編集を保持し、Playでtrim開始へ戻れた。終端後preview、通常releaseのEOF→seek bar→Paused→再Playと153 tests・Clippy・buildも通過した。再現/修正比較は制御されたnative試験であり、物理device切替の証拠ではない。次は非稼働sessionの通知とD3D11 removal経路を監査する。H1全体は未完了。

停止sessionの切断callback登録と、音声結果公開後のgeneration付きAudioReadyを追加した。実WASAPIのpollなし完了待ちは修正前に失敗・修正後に成功し、folder pollを外した停止windowでも入力なしで復旧した。153 tests・実WASAPI 2 tests・Clippy・buildが通過。通常releaseの停止中closeは4回56～84msだが、制御試験の一度の5秒超過は未特定として残す。D3D11の公式強制TDR試験は他アプリへ影響するため未実行で、D3D12専用RemoveDeviceを代用しない。次は終了時の残る観測とlaunch全体の未達項目を整理する。H1全体は継続中。

終了traceではworker回収前のevent loop待機に遅延を確認した。固定版winitがAboutToWait後も以前の期限で待つため、終了確定時だけPollへ切り替える。3秒期限を残した同条件のnative試験は3,112msから113msになり、通常releaseの未保存Cancel・Undo後終了も64msで通過した。試験用trace除去後の153 tests・Clippy・buildも成功。最初の5秒超過との同一原因までは未証明。次は未達launch条件を全体で整理し、ownerへ確認中の配布方針と切り分けて品質確認を進める。H1全体は継続中。

tab barのrelease時並べ替えと挿入線を追加した。同じ3枚の画像で変更前の順序不変と変更後の左右移動を比較し、dirty画像・active tabの保持、Escape/bar外dropの取消を実windowで確認した。identity・saved targetとpointer操作の回帰を含む156 tests、Clippy、debug/release buildが通過。KNOWN_GAPSの古いexport記述を訂正し、launch条件を安全性・性能/復旧・操作/外観・環境/入力・配布に分けた。次はWelcomeの導線と草案の見た目を評価する。H1全体と実機/配布gateは未完了。

Welcomeの説明と左端buttonが分離した配置を、草案に沿うwordmark・START・Open操作の中央columnへ変更した。空状態の見出し、current shortcut、hover/focus、狭い画面のscrollを追加。native Open File/FolderのCancelと、480×300で画像を開いて最後のtabを閉じる復帰を確認し、158 tests・Clippy・debug/release buildが通過した。recent履歴は未実装として残し、次はtimelineの狭いwindow操作とtrim範囲の調整を評価する。H1とlaunch全体は継続中。

timeline上端のdragが高さを変えずSeekになることを旧版の実windowと回帰testで確認した。既存panelの可変高さと画面高に応じた上限を使い、高さだけを変更するようにした。停止動画の位置・trim保持、縮小windowの上限/下限、通常releaseの音声timelineを確認し、狭いtrim表示も端点優先へ変更した。160 tests・Clippy・debug/release buildが通過。次はtrim端点をpointerで調整する導線を評価する。H1全体と実機/配布gateは未完了。

trimの開始/終了gripを追加し、releaseだけを既存の検証・履歴・live再生・保存へ接続した。同じdragが旧版ではSeek、新版では停止位置を保持したtrim編集になることを実windowで比較した。Escape取消・逆転拒否・Playの範囲復帰と、音声のpointer範囲を4.957625秒のWAVへ保存するflowを確認。元fileのhashは不変で、狭い表示のgrip/案内も非重複にした。163 tests・Clippy・debug/release buildが通過。次はfullscreen中のmouse操作と操作部の発見性を評価する。H1と実機/配布を含むlaunch全体は未完了。

fullscreenの下端hoverに既存status/seek操作と解除buttonを追加した。media領域を変えず、操作部を離れたら隠し、dragはreleaseまで保持する。status操作後にSeekの下半分が遮られる問題を実windowと回帰testで再現し、子レイヤーで修正。再生/一時停止、停止中Seek、下端外へのdrag、元のtimelineへの復帰を確認し、164 tests・Clippy・debug/release buildが通過。上端tab/menuとdouble-clickは未実装。次は残る日常閲覧と環境依存のlaunch gateを監査する。H1と配布を含むlaunch全体は未完了。

映像2秒・音声30秒のfileで、映像のない区間への約4秒hoverがthumbnail要求106回になることを再現した。失敗を区間ごとに保持し、同じ手順を1回へ抑えた。取得不能表示、再openによる再試行、旧世代の成功/失敗の拒否と正常区間のthumbnailを確認。165 tests・Clippy・debug/release buildが通過した。一方、このfixtureは本画面が黒いままであり、Seek時刻の更新だけを映像再生の成功とは扱わない。次はこの再生経路を調査する。H1とlaunch全体は未完了。

黒画面をEOFまで追跡し、hardware確認前の音声1,377 chunksの同期排出が映像通知を遅らせ、60枚すべてdrop・表示0になることを確認した。hardwareだけを事前確認する試作は通常再生には有効だったが、停止中の0.625秒Seekでは黒画面が残ったため除去した。既存queueとcallbackの音声backpressure、hardware成立前のfallbackを一体で扱う必要がある。runtimeの修正は未完了で、次は両条件を満たす供給経路と停止Seekの回帰検証を優先する。

映像・音声のinput/demuxを分離し、各streamを既存のbounded queueへ接続した。最初の映像/映像終端で音声decodeを一度だけ開始するため、hardware確認前の音声蓄積を除去できる。元fixtureの通常releaseは60枚表示・drop 0・CPU transfers 0でEOFとなり、停止Seekも復旧した。167 testsとClippy/buildが通過し、100回Seekのp95は再生中101.286ms・停止中42.494msで300ms以内。二系統読取の30分4K60試験も107,746枚表示・25枚drop・CPU transfers 0、drift p95 4.806ms・最大37.785msで完走した。先頭600秒の35,925 frameへ全dropsを割り当てた上限でも0.069589%で、長時間/10分drop gateを満たす。次は残る入力・実機環境と日常flowの監査へ戻る。H1/launch全体は未完了。

Windows日本語IMEの実入力で、各文字のpreedit直後に空の確定が来て文字が消える不具合を再現した。固定eguiのrequest_focusがIME中断も要求するため、既にfocusがあるframeでは再要求しないようにした。OSへ出す中断flagを確認する回帰testは修正前に失敗・修正後に成功。実候補の上下選択・日本語確定・Escape取消・確定後の独立EnterによるOpen、通常releaseの候補表示と168 tests・Clippy/buildが通過した。前回17ef1f5のCIも成功。次はfocusとkeyboard-onlyの日常操作を監査する。物理keyboard・他IME・混在DPI・実device復旧・配布を含むH1全体は未完了。

通常releaseでIME候補中のfocus離脱・復帰後の文字保持とEscapeを確認した。未保存確認もTab/Shift+TabとEnterでCancel、Save As取消からの編集保持、明示DiscardによるWelcome復帰を確認し、source hashは不変。keyboard focusと全3 decisionのwidget出力を回帰testへ追加し、169 tests・Clippy/buildが通過した。今回はproduction動作の変更なし。次はmenu/Welcomeを含む残りのkeyboard-only操作を監査する。環境・配布を含むH1全体は継続中。

logo menuの矢印操作が背後のWelcomeへfocusを移し、意図しないfolder pickerを開くことを通常releaseで再現した。既存popupの最深menuにfocus移動を限定し、上下/Tabの有効項目循環、左右の階層移動、focus時だけのscrollと再open時の先頭復帰を追加した。旧コードで失敗する回帰、pointer操作、通常releaseの末尾到達・再open・Welcomeから両pickerの起動/取消を確認し、170 tests・Clippy/buildが通過。直前の16454a6と5f5a612のCIも成功した。次は残る日常閲覧と環境依存gateを監査する。H1/launch全体は未完了。

画像のLeft/Rightが無反応だった基本閲覧操作を補い、画像専用Previous/Next commandと既定bindingを追加した。動画・音声のSeek、Ctrl+左右、既存custom binding優先は維持。追加前に失敗するキー解決回帰、Shell順とdirty Cancel、通常releaseの画像往復・reading移動・menu dispatchを確認し、173 tests・Clippy・debug/release buildが通過した。839137fのCIも成功。Home/End等の追加aliasは含めず、残る日常閲覧と環境依存gateの監査を続ける。H1/launch全体は未完了。

Ctrl+wheelの画像zoomが無反応になる不具合を通常releaseで再現した。固定eguiが変換済みのzoom倍率を使い、cursor基点と同frame描画を保つよう修正。旧testの処理済みscroll値の直接変更を実wheel eventへ置き換えると旧コードで失敗し、修正後は倍率・30/120fps相当・overlay遮断も通過した。通常releaseの往復zoom、palette/menu/dirty guardでの背景保持、173 tests・Clippy・debug/release buildを確認。物理mouse/trackpadとDPIのmatrixを含むH1全体は継続中。

選択dragが認識された時点の位置を始点としていたため、移動eventが少ないと選択が消える不具合を通常releaseで再現した。画像/動画共通の処理で押下位置を保持し、release位置を適用後にpixelへ丸める。新しい回帰は旧コードで失敗し、疎なevent・move/release同frame・逆方向・辺resize・画像外開始・click previewを含めて修正後に通過。通常releaseの選択・辺resize・crop/Undoと174 tests・Clippy・両buildを確認した。移動直後のrelease注入がclick扱いとなった別試行は未解明として残し、高速native入力とmodal/focusを次に監査する。H1全体は継続中。

未解明だった高速releaseをtraceし、WindowsのMouseInput(Released)が最終CursorMovedより先に届くことを確認した。runtime所有のevent-loop message hookでbutton messageのclient座標を先にwinitへ反映し、後の実cursor位置で過去入力を書き換えない。実cursorを動かさないqueued button回帰は補正なしで失敗・ありで成功し、負の座標と一回ずつのdispatchを確認。trace除去後の通常releaseで高速の辺resize、通常click、WelcomeからOpen File/Cancelと176 tests・Clippy・両buildが通過した。c5857daのCIも成功。次はselection/panのmodal・focusをまたぐ取消を監査する。H1全体は継続中。

選択drag中のEscape後にmove/releaseで選択が復活する例を通常releaseで再現した。選択・panを一件の一時操作として開始前状態を保持し、Escape・focus喪失・modal/overlay・別commandで復元して新しい押下まで再開しないよう修正。panのrelease位置も含めた差分と、fullscreenでのdrag取消優先を回帰testした。通常releaseの選択/pan Escape、focus往復による辺resize取消、dirty guardと編集保持、178 tests・Clippy・両buildが通過。58f6024のCIも成功。引き続き日常操作と環境依存gateを監査する。H1全体は未完了。

押下・移動・releaseが同一frameに届くと選択を開始できないケースを固定event列で再現し、選択/pan共通でbutton event自身の座標を使うよう修正した。release後のhover、無効/clip外/前面layer/他widget所有の遮断、再レイアウトでの二重適用防止を回帰確認。通常releaseの選択・click preview・panも確認したが、直接postしたWindows message試行は結果が揃わず、native同一frame配送の証拠には数えない。179 tests・Clippy・両buildが通過し、be93e5fのCIも成功。残る入力配送・日常操作・環境依存gateを継続し、H1全体は未完了。

native traceで同frameの押下～release～後続hoverを確認した。前回の「選択なし」は保存PNGの読み取り誤りと判明し、実pixelで境界・内外の明暗を確認、連続試行の細い範囲も左辺の再resizeとして説明できた。この記録を訂正する。別途、描画前の押下→Escapeから選択が復活する欠陥をtraceで再現し、取消時にegui-winitの保留primary/secondary押下を破棄するよう修正。後続入力、新しい押下、既存selection/fullscreen保持を回帰確認し、通常releaseでも取消・再操作を確認した。180 tests・Clippy・両buildと041019eのCIが通過。次はtimeline等の残る日常操作へ戻り、H1全体は継続中。

timelineの右dragで約10秒から22秒へSeekする誤操作を通常releaseで再現した。timeline・compact seek・画像folder barのdrag確定をprimary buttonに限定し、compact候補位置も同じbuttonだけで動かす。全5 buttonのclick/drag回帰で旧コードはsecondary dragに失敗、修正後は左だけ一回確定する。通常releaseでも両Seek表示の右dragはSeek件数不変、左dragは各一回増加を確認。181 tests・Clippy・両buildが通過。trim処理・再生pipelineは未変更とし、残る取消・release時刻・環境依存gateを引き続き確認する。H1全体は未完了。

Seekのrelease後hoverが確定位置へ混ざるケースを回帰で再現し、timeline・compact media/image bar共通でrelease eventの座標を使うよう修正した。Seek所有widgetを一件だけ保持し、同時Escape/focus喪失・command/modal・media切替で取消し、新しい押下まで確定しない。fullscreen Escapeも取消を優先する。通常releaseで22秒のrelease位置保持、fullscreen維持とSeek件数不変、所有windowへのfocus往復取消・押し直し成功を確認。182 tests・Clippy・両buildが通過し、2912cbaとa5524e6のCIも成功。短いgestureやtrim gripを含む残る日常操作・環境依存gateを継続し、H1全体は未完了。

trim gripにもrelease後hoverが端点へ混ざる不具合があり、通常releaseでx=600に離してからx=800へ動かすと開始25.196秒になることを再現した。端点はrelease eventから求め、同frameのfocus離脱・復帰も取消とする。開始/終了のhover・PointerGone・同時Escape/focus回帰と、通常releaseでの開始18.833秒・終了22.015秒、Undo/Redo、全編集Undoを確認。182 tests・Clippy・両buildが通過し、sourceは不変。入力処理だけを変更し、履歴・export・再生pipelineは未変更。短いgestureと環境依存gateを含むH1全体は継続中。

押下と移動が同frameに届く短いtrimは、旧releaseで編集されず背景Seekへ化けた。Seek/gripの入力を共有し、押下位置のclip/layer判定、grip優先、一度使った押下の同frame再利用防止、releaseまでの距離判定を追加した。開始/終了の短いdragとclick、全frame内Seek、遮蔽/無効/clip外、複数layout passを回帰確認。通常releaseでも同じ入力がtrim開始18.833秒となり、短い背景Seek22.015秒・trim Escape・Undoを確認した。184 tests・Clippy・両buildが通過し、85b70e3のCIも成功。履歴/export/runtimeは変更せず、残る日常操作・環境・launch gateを継続する。

Open監査から、非zero開始PTSのTSが長い黒画面になり、offset MKVの長さが過大になる問題を再現した。decode/Seek/exportをinput原点からの時刻へ揃え、Matroskaのdurationを補正。別途TSの途中Seekが空になるため、実packet keyframeを段階的に探すprerollをdecodeとpreviewで共有した。188 tests・Clippy・両build、通常releaseのhardware/software表示、MKV中央Seek、TS Seek/preview/trim保存が通過。4K60 TSの45秒Seekは単発277.736 ms、以後899枚drop 0で完走した。長いGOP・低速storageの応答、実入力/環境、配布を含むlaunch全体は引き続き未完了。

30秒GOPのSeek中に終了すると通常releaseが897 ms待つことを確認した。pipeline単位の取消flagを追加し、probe/Seek境界・preroll探索・demux・出力破棄中でも停止を確認する。取消後のEOF/失敗通知を抑止し、既存join・同一device所有を維持。通常releaseの同手順は63 msとなり、音声付き素材の再Seek・再生・tab closeも通過した。189 tests・Clippy・両buildと04be7aaのCIが成功。進行中のFFmpeg call/OS I/O強制中断とlaunch全体の完了ではない。

別tabを閉じると停止中の動画まで先頭から再生し直す問題を通常releaseで再現した。active identityが同じままのclose・再click・単一tab巡回ではmediaを再loadしない。画像/動画/音声の位置・pause・view・世代・編集保持を回帰確認し、通常releaseでも4,200点の動画画素一致、decode開始件数不変、再生継続と最後のtabからWelcomeへの復帰を確認。190 tests・Clippy・両buildが通過。active tabを閉じた場合の既存遷移・guard・runtimeは変更せず、H1とlaunch全体は継続中。

最後のtabを閉じたWelcomeに旧waveformとtrimハンドルが残る問題を通常releaseで再現した。media固有のtimeline・preview・時計・Seek計測・一時statusを破棄し、media不在時のtimeline描画とsession不在時の再生通知を拒否する。旧コードで失敗する2回帰を追加し、192 tests・Clippy・両buildが通過。同じ通常release windowで動画/音声close後の空状態と音声/画像の再openを確認。3017e12のCIも成功。次は再openをまたぐ非同期結果の識別を監査する。H1/launch全体と環境・配布gateは継続中。

再openでsession内の世代が0へ戻り、古いEOF通知が新しい実sessionをEndedへ変える問題を、非表示の実D3D11 windowと制御した通知順で再現した。既存thumbnail世代をmedia読み込み番号へ共有・改名し、再生・duration・waveformにも付けて同じpathの旧結果を拒否する。旧世代の成功/失敗と現在の通知、Seek世代の二段照合を回帰確認。194 tests・Clippy・両buildが通過し、通常releaseでも同じ動画のclose/reopen、waveform・pause・Seek・再生継続を確認した。次は残る日常操作・資源上限を監査する。開始済みpreview processのcancelや実機/配布gateは未完了で、H1全体は継続中。

preview要求ごとのthread生成を、duration/波形/hover画像それぞれ常設1 worker＋最新待機1件へ変更した。実FFprobeの結果通知を待機させる回帰は旧コードで並列worker増加を検出し、修正後は待機要求をまとめる。1,000要求の置換・clear・実行中をjoinしないcloseも確認。197 tests・Clippy・両buildが通過し、通常releaseでcache未生成のduration/波形/thumbnailとWelcome復帰後5秒のCPU増分0 msを確認した。開始済み1件の取消とdecoder個別メモリ上限は残るため、次はその待ち時間を評価する。H1全体は継続中。

2時間音声のwaveform生成中に通常releaseを終了すると、本体終了40 ms後もFFmpeg子processが残り、500 ms後にCPU・メモリ増加を確認した。要求単位の取消tokenへowned Childを登録し、置換・clear・dropで停止、worker側でpipe排出と終了回収を行う。spawnとの競合と取消後の再起動を防ぎ、filmstripとTS preview準備にも接続。停止を外すと失敗する回帰を含む199 tests・Clippy・両buildが通過。修正後の同じ素材では本体終了86 ms、直後/500 ms後の子process残存なし。tab closeから別音声の波形・再生復帰も確認。次は長時間素材の個別waveformメモリ使用量を評価する。filesystem/native probeの強制中断、実機/配布gateとH1全体は未完了。

2時間AACのwaveform完了までを測ると、旧showwavespic子processが約1.36 GBを保持した。PCMの逐次集計・隣接bin併合へ変更し、集計を最大width×1024個のu64、入力を64 KiBに限定。短い素材の画素一致、長い素材/急変のbar差1 pixel以内、奇数byte分割・末尾不正、診断pipe上限を確認した。203 tests・Clippy・両buildが通過。通常releaseで子process約42 MB、本体合計約191 MB、生成観測3.3→2.4秒、波形本体41,078画素一致、生成中終了65 msと子process残存なしを確認。7660b7eのCIも成功。次は残る日常media/error flowを監査する。H1全体・環境/配布gateは継続中。

複数streamの通常release試験で、本体は既定の青い映像を再生するのにthumbnailは先頭の赤い映像、waveformは先頭の無音になった。previewの選択を再生と同じFFmpeg best-streamへ揃え、cache keyをv3に更新した。旧コードで失敗する回帰、204 tests・Clippy・両buildが通過し、同じ素材の通常windowでthumbnail・waveform一致と120 frames/drop 0を確認。a7274b4のCIも成功。次はexportのstream選択一致を監査する。H1全体・実機/配布gateは未完了。

既定指定のない複数stream素材では、青い本画面からSave Asすると赤い別映像が保存される問題を通常releaseで再現した。trim時だけだったbest-stream指定を全動画/音声exportへ適用し、trimなしのtimestamp処理は維持。無編集・crop・trim・音声のみのdecode照合を含む205 tests・Clippy・両buildが通過し、同じ素材のSave As→再openでも青160×96/monoを確認した。次は残る日常edit/error flowと外観・操作感の監査を続ける。H1全体・実機/配布gateは未完了。

Seek previewがhover位置ではなく左端に現れる問題を通常releaseで確認し、指している位置の上へ中央揃えにした。160×108 logical px以内のaspect-fitと空き高さ制限で縦長・小windowのtrack重なりも防ぐ。描画48組を含む206 tests・Clippy・両build、通常windowの横長/縦長・右端・320×240 timelineが通過。94f97cbのCIも成功。次は画像folderのseek previewなど日常閲覧の外観・操作感を監査する。H1と実機/配布gateは未完了。

画像folderのSeek hoverへ移動先画像とreading page群のpreviewを追加した。Shell順・枚数・縦横・反転を使い、filmstripの単一worker/cacheを共用する。hoverでは移動せず、離脱/overlayで要求を取消し、確定は既存dirty guardを通す。実PNGの非同期完了・描画配置・失敗再要求抑制を含む208 tests、Clippy、両buildが通過。通常windowで単画像/reading/反転・filmstrip・破損page・保存確認と移動を確認し、03e2bd8/90c2021 CIも成功。次はfolder端点へのkeyboard移動など残る日常閲覧を監査する。H1と実機/配布gateは未完了。

画像folderのHome/Endを共有commandとして追加し、現在のShell順の最初/最後の画像へ移動できるようにした。reading modeと保存確認を維持し、現在端点・対象消失・未取得時は再loadしない。menu/palette/custom shortcutへ接続し、210 tests・Clippy・両buildが通過。通常windowで先頭/末尾、dirty端点のno-opと別端点の保存確認、paletteの文字編集とcommand実行を確認した。次は残る画像/再生の日常操作を監査する。H1と実機/配布gateは未完了。

単一項目folderの右矢印でzoomがFitへ戻り、同じ画像なのに保存確認が出る問題を通常releaseで再現した。共通Navigate guardの入口で同じpathをno-opにし、playlistの現在項目clickにも適用。旧コードで失敗する世代・view・clock・編集保持の回帰を含む211 tests、Clippy、両buildが通過。通常windowで100%・dirty画像の保持、停止音声の現在行clickを確認した。次はaudio playlistのクリック領域と草案layoutを監査する。H1と実機/配布gateは未完了。

音声playlistを番号付き32pxの全幅行へ変更し、現在曲の強調・名前省略・window幅内の全文tooltip・可視行描画を追加した。1万曲の描画と全幅click/scroll選曲を含む213 tests、Clippy、両buildが通過。通常windowの右端click・小window・日本語tooltip・scroll後の選曲を確認し、12322d5 CIも成功。次はplaylistで現在曲が画面外にある場合の導線を監査する。曲長の列は追加していない。H1と実機/配布gateは未完了。

音声の途中曲を直接開いても現在行が見えない問題を通常releaseで再現し、open/選曲/tab再表示/順序変更時だけの最小scrollを追加した。同一曲の手動scrollとguard Cancelを維持する。1万曲の表示回帰を含む214 tests、Clippy、両buildと通常windowの選曲・Cancel・tab復帰が通過。次は残る日常の再生操作を監査する。a356646 CIは進行中で、H1と実機/配布gateは未完了。

消音解除で50%から100%へ上がる問題を通常releaseで再現し、active tabの適用済み履歴から直前の非zero音量を復元するよう修正した。音声/動画・Undo/Redo・tab分離の回帰を含む215 tests、Clippy、両buildと通常windowの50%復帰が通過。a356646 CIは成功、eeb8e0b CIは進行中。次は残る再生操作と入力の不一致を監査する。H1と実機/配布gateは未完了。

動画面と動画/音声status barの音量表示へwheel操作を追加し、raw入力だけを既存編集へ渡す。playlistのscrollとmodal/overlay等は分離した。217 tests、Clippy、両build、通常windowの動画90%・Undoと音声一覧/音量の使い分けを確認。pointer移動直後のwheel不達が一度あり、次はその入力境界を監査する。eeb8e0b CIは成功。H1と実機/配布gateは未完了。

foreground確認後もwheelの古い座標参照を通常releaseで再現した。winitのposition-less wheelへ各Windows messageのsigned screen座標を先行反映し、縦/横・負座標・button列の回帰を更新した。217 tests、Clippy、両buildと通常windowの即時一覧↔音量移動が通過。次は同一frameに異なるtargetのwheelがまとめて届く場合を監査する。H1と実機/配布gateは未完了。

同一frameのwheel音量が最後のhoverへ誤配送される問題をheadlessで再現し、各event時点の位置・layerで対象分だけを集計するよう修正した。前frame・離脱・複数target/passの回帰を含む218 tests、Clippy、両buildが通過し、通常windowのqueued一覧/音量入力も90%となることを確認。4ff9e00/664d7ce CIは成功。次はScrollArea側の複数領域を跨ぐsmooth scroll配送を監査する。H1と実機/配布gateは未完了。

音量wheelの後でpointerを一覧へ移すと一覧までscrollする問題を通常releaseで再現した。音声playlistの対象eventだけを専用egui入力状態で平滑化し、一覧のoffsetへ適用する。scroll状態のID参照も実際のScrollAreaと一致させた。30/120fps・入力単位・中断・target間移動を含む220 tests、Clippy、両buildが通過。通常windowで音量70%と一覧位置を両方向の移動で分離し、Undo後に正常終了した。8df753e CIは成功。日常再生の残る操作を引き続き監査する。H1と実機/配布gateは未完了。

再生終了後に左右矢印のSeekだけが無効になる問題を通常releaseで再現し、Endedを既存の停止preview経路へ通した。trim外previewからのPlay復帰・Loading/Faultedの拒否を含む221 tests、Clippy、両buildが通過。通常動画の末尾→先頭frame停止と、30秒音声の末尾→約24.9秒停止を確認。試用中のSendKeysによる余分な操作は原因未確定として記録し、単一のkeydown/up messageによる試験と分けた。H1と実機/配布gateは未完了。

2秒動画の末尾からRightで6秒へ進み黒画面になる問題を通常releaseで再現した。既知durationへSeek/表示を制限し、末尾停止・Play時の先頭/trim開始復帰を追加。parallel decodeは対象以降のframeがないEOFに限り最後の一枚を返し、bounded trimの選別は維持する。222 tests、Clippy、両buildと通常D3D11VA末尾frame/再開、音声Right連打の30秒上限・Leftで25秒復帰が通過。8f9081e CIも成功。追加の長時間性能・実環境matrixを含むH1全体は未完了。

映像1秒・音声8秒の素材で、再生中に約6秒へSeekすると末尾previewがlate-frame dropされる問題を通常releaseで再現した。このframeだけをdrop/同期誤差計測から除外し、通常frameの遅延処理は維持。MP4/MKVの長短stream・VFR末尾画像/PTS照合を含む223 tests、Clippy、両buildが通過。通常D3D11VAで最終映像を保持して音声EOFに達し、Seek後の表示1/drop 0/CPU transfer 0を確認した。音声尾区間のhover thumbnailは別経路として残る。H1と実環境/配布gateは未完了。

音声尾区間のhoverでFFmpegが成功終了してもPNGが空になる問題を再現した。thumbnail/filmstrip共通でこの場合だけ選択videoの最後のPTSを既存worker内で調べ、一度だけ再生成する。複数streamの最終画像/cache、TS短長GOP/B-frameと取消を含む224 tests、Clippy、両buildが通過。通常windowの7秒hoverで再生と同じ0.9秒の最終frameを確認した。09537c6 CIは成功。H1と実環境/配布gateは未完了。

4b7721b通常releaseで再生中/停止中各100回Seekを再測定し、p95 102.204/42.260msで300ms基準を満たした。1分4K60は3,594表示/drop 0/CPU transfer 0、drift最大4.979ms。EOF直後のCPU増加を観測したため追加試験し、落ち着いた後の5秒sampleは二回とも0msだった。224 testsと必須checkも再通過。測定条件・hash・制限をDEVELOPMENTへ記録。これは30分性能・実DPI/device・配布gateの代替ではなく、H1は継続中。

status barの4つの操作buttonでshortcut案内がSpace/T/B/F11へ固定されていた問題を修正した。現在の単一key・prefix・未割当とclick動作を実描画で確認する回帰を含む225 tests、Clippy、両buildが通過。独立設定の通常windowでもKによる停止とK/Ctrl+Tの案内を確認した。既定bindingは変更せず、草案の動画L=Seekと既存回転の競合はownerへ確認中。812fb23 CIは成功。H1と実環境/配布gateは未完了。

通常windowでCtrl+Tabがtabを切り替えない問題を再現し、eguiへ渡す前に現在のTab binding/prefixを共有shortcut処理へ接続した。filmstripは修飾なしのTab/Shift+Tabだけを項目移動に使う。226 tests、Clippy、両buildに加え、3 tabの前後移動、filmstrip内の使い分け、独立設定のprefix、button focus中の切替、menu/paletteの入力保持を通常windowで確認した。de31aaa CIは成功。H1と実環境/配布gateは未完了。

prefixの途中で別のprefix・同じprefixへ打ち直すと新しい入力まで捨てる問題を通常windowで再現した。最後のkeyからの再判定を単一commandだけでなくprefixへも適用し、案内と期限を更新する。227 tests、Clippy、両buildが通過。独立設定の通常音声windowで打ち直し後のpause/resume、Escape・未割当・期限切れでの不実行を確認した。H1と実環境/配布gateは未完了。

草案との再照合からcommand paletteを暗いcompact panelへ変更し、見出し枠を除き、検索欄を全幅、shortcutを右揃えにした。長いprefixの幅を制限し、window内で全文tooltipを表示する。960/480/240pxの配置・非重複・行全幅click、既存IME/検索を含む228 tests、Clippy、両buildが通過。通常windowでも変更前後、Zoom out実行、小窓の末尾候補、長いprefixの省略/全文表示を確認した。f21154e/81cfdc9 CIは成功。H1と実環境/配布gateは未完了。

小窓で5 tabを開くと現在tabが画面外に隠れる問題を通常releaseで再現した。active identity/index・tab幅・表示幅の変更時だけ、既存ScrollAreaでtab全体を最小scrollする。12 tabの切替/追加/並べ替え/resizeと手動位置保持を含む229 tests、Clippy、両buildが通過。通常windowでも追加・前後切替・手動scroll保持・resize・close後の隣接tab表示を確認した。media loadや編集へ新しい動作を加えず、H1と実環境/配布gateは継続中。

compact paletteのnative日本語IMEを通常releaseで再確認した。通常幅/480px幅の候補位置、候補上下移動、日本語確定、Escapeの二段階取消、F10でLatin化後の確定Enterとcommand実行Enterの分離、別の所有windowからのfocus復帰が通過した。229 testsと必須checkも再通過。新しい実装不具合は見つからず、条件と観測上の除外をDEVELOPMENTへ記録。afe7469 CIは成功。物理keyboard・別IME・mixed-DPIを含むH1全体は未完了。

51b43bf通常releaseの30分4K H.264/AAC再試験は、同一processでEOFへ到達し、107,771表示・drop 0・CPU transfer 0、drift p95 4.808ms・最大32.055msだった。全区間drop 0のため先頭10分のdrop基準も満たす。5分以降のprivate memoryは粗いsampleで221.86～235.78MiB、EOF後は178.62MiB。条件・hash・限界をDEVELOPMENTへ記録し、KNOWN_GAPSのtrim grip・wheel volume・preview worker/cacheの古い記述も現行実装へ合わせた。実装変更なし。物理入力/DPI/device・配布を含むH1全体は継続中。

reading modeの固定8pxの隙間と等分枠による中心ずれを確認し、横は高さ・縦は幅を揃えた連結画像全体の中央fitへ変更した。seek hoverも同じ配置を使い、画像previewだけをpaddingなしcacheへ更新。異なる比率・縦横・反転・失敗page・cache移行を含む230 tests、Clippy、両buildと通常windowの見開き/hover/filmstripが通過。ページ送り・編集や動画decodeは変更しない。e0d1c0d CIも成功。残る日常操作・実環境/配布gateを含むH1は継続中。

6000×6000 PNGへ戻るたび約185msの再decodeを確認し、既存worker内に静止画8件・256 MiBのLRU cacheを追加した。RGBAをArc共有し、file size/更新時刻の失効、cache hitの要求予算、animation/大容量の除外を検証。通常windowの5往復でtitle完了までの中央値は220.651→49.705ms、初回/cached表示の248,004 pixelsは一致し、所有fixture差替えも新画像を表示した。232 tests、Clippy、両buildと3fddd7a CIが通過。初回decode・GPU upload・先読み・実環境/配布を含むH1全体は継続中。

続く再訪計測で反復texture変換/uploadを確認し、同じdecode identityの静止画textureを最大8件・RGBA相当256 MiBで再利用するようにした。graphics復旧開始で破棄し、古いfile内容をpathだけで再利用しない。同条件5往復のtitle完了中央値は49.705→16.500ms。連続31入力とscratch差替え後の正しい画像、upload deltaのないhit、上限/除外/復旧を確認し、233 tests・Clippy・両buildと9be0d14 CIが通過。初回表示・実device/配布などのlaunch gateは残り、H1は継続する。

初回画像の画面用変換も測定し、行内が全opaqueの場合だけalpha変換を省くようにした。透明/半透明の既存egui変換とsource RGBAを維持する。通常releaseで未cacheの6000×6000 PNGを5枚開くtitle中央値は233.814→219.187ms、表示248,004 pixelsは一致。全alpha値と混在行の回帰、234 tests・Clippy・両buildが通過。OS file cacheはwarmであり、cold-storage、decode/uploadの無停止化やlaunch全体の完了ではない。

アニメ画像の長いdeadline遅延で過去の全周回を数える処理を発見し、周期の整数剰余で省略するようにした。合成2日gapのrelease単発計測は20.918→0.005ms。同じframeへ戻る場合はuploadを省き、2日/3650日・端数delay・境界の正確なframe/期限を検証。236 tests・Clippy・両buildと通常windowのGIF更新が通過。OSスリープ復帰の実試験ではなく、実環境/配布を含むH1は継続中。

日常選択操作の監査で、Shift正方形/比率付きresizeが画像端で比率を失うことをtestと通常windowで再現した。共通の寸法上限、固定辺/中心とdrag開始比率を保持する修正により、同じ入力が端で正方形のまま止まる。縦横/全方向/zero縮小後と既存取消/cropの回帰、238 tests・Clippy・両build、25cc2e1 CIが通過。読書モードの見開き区切り方はownerへ確認中で、現行navigationは変更していない。実環境/配布を含むH1は継続中。

画像errorからの継続操作を通常releaseで確認し、不正PNGのreading error枠/隣の正常page、左右移動、同じpathの修復後再読込、最後のtabからWelcomeへの復帰が通過した。実workerを使うBMP回帰を追加し、正確な修復RGBAと旧error解除も検証。挙動変更なしで239 tests・Clippy・両buildと8796ad8 CIが通過。全codec/実環境/配布を含むH1の完了を示すものではない。

20:04のlaunch再監査で、現行build/回帰、過去binaryの性能記録、未実装と実環境未検証をKNOWN_GAPS §6へ分離した。固定egui-winitのOS text clipboard連携が無効で内部fallbackのみ、Windows accessibility bridgeも未接続と確認し、次の実装候補とする。Windows 10 22H2での確認、最終候補の性能/保存確認、実device/input/DPI、配布判断は残る。239 tests・format・Clippyを再実行して通過したが、launch完了とは扱わない。

続いてOS text clipboard用の固定egui-winit featureとlockを変更し、検索欄のUnicode paste/copy/cut回帰を追加した。ownerの書込み許可後、通常releaseでOSからのpaste、copy/cut後の完全一致、外部変更後の再pasteを確認。変更前の同じpasteは空欄のままで、最終feature buildでは通過した。240 tests・format・Clippy・両buildも通過。clipboard競合、accessibility経路と既存launch gateは残る。

Windows accessibility bridgeを初回表示前に接続し、初期tree要求/action/無効化を既存event loopへ統合した。UI AutomationでWelcome子要素0→12、menu→palette→検索文字列設定→Open file実行/取消→終了が通過。空白名、固定TextEditのSetValue不処理、候補行のToggle扱いを修正し、headless tree/action回帰を追加した。243 tests・format・Clippy・両buildが通過。5秒idle CPU増分0msはこの通常windowだけの観測で、screen reader・custom widget・実環境/配布gateは残る。

続く保存ガード監査で、UI AutomationのInvokeから確認中の背景menuを開ける問題を実windowで再現した。背景rootの無効化、popupの解除/overlay表示保留と、最前面の確認以外の配送済みUiAction拒否を追加。244 tests・format・Clippy・両buildが通過し、通常windowでも古いmenu参照のInvoke拒否、Cancel後のdirty保持/再有効化を確認した。modalの意味情報/読上げ順と既存launch gateは継続する。

保存関連3 modalの名前と子要素階層を追加し、既存layout回帰で通常background exportとの区別も確認した。244 tests・format・Clippy・両buildが通過。実windowでは保存確認と継続前export待ちのIsModal/子buttonが通過したが、export失敗試行はUIA timeout後に子要素0となりnative semantics未確認。次は同一経路のtree喪失を比較buildと切り分ける。screen reader/focusと既存launch gateは未完了。

21:39の再調査ではexport失敗のIsModal/OK取得が成功する試行も得たが、その後の照会停止が残った。保存もpickerも使わないClose window/Cancelの反復へ絞ると、通常/意味情報追加前相当の比較でともに3回目に停止する。headless 5往復・244 tests・format・Clippy・両buildは通過。production変更なしで再現条件を記録し、次はWindows provider取得/イベント配送を調べる。起動経路や短命client、名称追加だけを根本原因とせず、launch gateを閉じない。

22:43の比較では独立Rust UIA clientでも通常buildの3回目timeoutを再現した。一方、本体の依存関係・window設定・renderer初期化・常駐workerを使う最小providerは15往復通過。Focus→Clickとfocus IDのheadless検査を強化し、244 testsと必須checkは通過したが、production修正は未採用。次はmedia読込み後の本体state/event処理との差を絞り、通常buildの反復試験で修復を確認する。

22:55、UIA反復停止をShell STAの仕事待ちへ絞り、Windows messageも処理できるevent待機へ修正した。通常buildの30往復、同じprocessでexport失敗のOK→保存確認Cancel、続く10往復が通過。message待機の回帰は変更前と負の比較で失敗し、修正後は通過する。UIやShell順を変えず、次は残るcustom widget・screen reader/focusと既存launch gateを監査する。

23:30、compact seek/timelineへUIA Sliderの名前・値・範囲・値操作とfocus keyを追加した。source秒/画像Shell順を維持し、直接値変更はpointer gestureを取消、未保存画像の移動は既存guardで保護する。focus中のSpace/R/Undoなども既存shortcutへ接続。248 tests・必須check・両buildと通常releaseの値変更/再生切替/編集保持/disabled拒否が通過した。trim grip・selection・全screen reader/focus順と実環境/配布gateは残り、H1は継続中。

23:42、先頭tab終了後に取得済み02.pngのUIA参照が03.pngへ変わる問題を通常buildで再現した。TabId由来の明示UI IDでactivate/closeの対象とfocusを維持し、close名へ対象fileを追加。通常buildの同じ参照による正しいtab終了と未保存Cancel、249 tests・必須check・両buildが通過した。外観・dragや保存処理を変えず、残るcustom widget・実環境/配布gateへ継続する。

23:54、playlistでもShell更新後にcached UIA行が別曲を指す問題を再現し、playlist/filmstripの可視項目をpath由来のIDへ固定した。filmstripの名前/Focus、playlistのInvoke、path/現在項目の説明を追加し、同じ参照での正しい曲選択・未保存Cancelを通常buildで確認。251 tests・必須check・両buildが通過。可視範囲限定の描画と既存操作を維持し、画面外項目の支援技術操作やselection/trim・実環境/配布gateは残る。

2026-09-07 00:15、fullscreen操作部へTabだけで入れない問題を通常buildで確認した。既存入力処理を通過したTab/Shift+TabでExit buttonへfocusし、操作中は表示を維持、内容click/window focus喪失/overlayで解除する。画像の移動、動画の5秒Seek、逆方向focus移動とEnterによる通常window復帰、252 tests・必須check・両buildが通過。全screen reader/実入力/配布を含むH1 gateは引き続き未完了。

2026-09-07 00:44、trim開始/終了へsource秒の値操作とfocus keyを追加した。編集後も同じUIA対象を保ち、両端点の数値要求は受信順に既存検証・Undoへ渡す。通常releaseで値変更・逆転拒否・focus/Undo・確認中の拒否・従来dragが通過し、253 tests・必須check・両buildも通過。selection、画面外項目、全screen reader/実環境/配布などのH1 gateは継続する。

2026-09-07 00:54、playlistの画面外行へ上下/Home/End/Pageでfocus移動できるようにした。移動は再生曲を変えず、Enter/Spaceで既存guard付き選曲へ渡す。1万曲で可視行限定のまま到達する回帰と通常releaseの末尾移動・選曲・未保存Cancel、254 tests・必須check・両buildが通過。全screen reader、filmstripの画面外操作、selectionと実環境/配布gateは継続する。

2026-09-07 01:04、filmstripのTab移動後にfocusが旧項目へ残る問題と、項目focus中のEscapeがoverlayを閉じない問題を通常buildで修正確認した。5万項目の限定描画/focus回帰、連続Tab/逆移動、保存確認Cancel後の保持、255 tests・必須check・両buildが通過。全screen reader/selection・実環境/最終候補/配布を含むH1 gateは引き続き未完了。

2026-09-07 10:34、filmstripの背後のplaylist行をclick/UIAで選曲できる問題を修正した。overlay/popup/modal中の背景操作・hoverを無効化し、既存opacityと行ID/配置、foreground選曲と解除後の操作を維持する。5条件の回帰、通常releaseの背景拒否/復帰、256 tests・必須check・両buildが通過。全screen reader/selection・実環境/最終候補/配布のH1 gateは継続する。

2026-09-07 10:45、32967f9 releaseの1080p Seekを通常/UIA tree取得後・停止/再生の各100回で再測定し、p95 33.651～103.416msで300msゲートを通過した。source/状態・計測境界を固定し、256 testsと必須checkも再確認。続けて同じbinaryの30分4K60再生を開始したが、完走/drop/driftはまだ未判定。古い長時間結果を流用せず、同じprocessのEOFを確認する。H1全体と最終候補/実環境/配布gateは継続する。

2026-09-07 11:19、上の同一processがEOFへ到達した記録を確定し、通常終了を確認した。107,758表示＋13 drop＝全107,771枚、CPU transfer 0、drift p95 4.704ms・最大30.109msで30分基準内。再decodeした先頭600秒35,925枚へ全dropsを割り当てても0.036187%以下で10分基準内。5分以降のprivateは最初223.16/最後225.42 MiBだが一時332.19 MiBへ増え、次標本で223.99 MiBへ戻った。原因やリーク不在は断定しない。EOF idleの5秒CPU増分0、hash不変、256 tests・必須checkも確認。DEVELOPMENTへ条件と限界を記録した実装変更なしの性能再確認であり、H1と最終候補/実環境/配布/外観受入は継続する。

2026-09-07 11:48、画像・動画の全体選択commandと四辺のpixel値/focus操作を追加し、pointerなしの作成・調整・crop/Undoを接続した。受信順の値更新、逆転/零長の拒否、modal/overlay遮断、reading/text入力の区別とcustom bindingを回帰で確認。通常releaseで画像398×560・動画1716×878へのcrop/Undo、画像端handle外側からのdrag、dirty Cancelを確認し、259 tests・必須check・両buildも通過した。zoomで画面外に出る辺のfocus/視認性と全screen reader flowは次の監査対象。実入力/DPI/device、最終候補/配布/外観受入を含むH1全体は継続する。

2026-09-07 12:02、拡大画像の選択辺へfocusすると画面外に留まる問題を修正した。一回のreveal要求から最小pan差分だけを適用し、倍率・選択・履歴と手動panを維持する。中間試験で判明した同じ辺へのUIA再Focusも対応。通常releaseのTab/逆Tab・値変更・手動pan/再Focusとcrop/Undo・dirty Cancel、260 tests・必須check・両buildが通過した。画面間の読み順/focus、実入力/DPI/device、最終候補/配布/外観受入のH1 gateは継続する。

2026-09-07 12:18、palette取消後に選択辺のfocusが消える問題を修正した。開く直前のwidgetへ取消時だけ戻し、command実行は新しいfocus先を優先する。通常releaseの画像/動画四辺・検索Ctrl+A・継続矢印、拡大画像と動画crop/Undo、260 tests・必須check・両buildが通過。保存確認Cancelなど残る画面間focusと、全screen reader・実環境/最終候補/配布/外観受入を含むH1 gateは継続する。

2026-09-07 12:32、未保存確認のCancel後にもfocusが消える問題を修正した。直前のwidget/tabを保持し、前passのmodal制限解除後に一回だけ戻す。再確認・Save As取消をまたいで保持し、離脱/media loadで破棄する。通常画像/動画の四辺・継続矢印、画像の実Save As取消→確認Cancel、回帰と260 tests・必須check・両buildが通過。全画面間focus/読み順と、screen reader・実環境/最終候補/配布/外観受入を含むH1 gateは継続する。

2026-09-07 12:40、logo menuのEscape後にfocusが消える問題を修正した。commandを選ばず取消した時だけlogoへ戻し、Enter/Spaceでの再openを可能にした。Welcome/画像の親menu・submenu、通常SelectAll実行、背景click・idleとの区別を確認し、261 tests・必須check・両buildが通過。連続するoverlay操作と全screen reader・実環境/最終候補/配布/外観受入のH1 gateは継続する。

2026-09-07 12:56、menu→palette→Escapeで消えた項目へのfocusがnative consumerをpanicさせる問題を修正した。commandへlogoを復帰先として引き継ぎ、完全root treeの配送前にもfocusの存在を検証する。通常Welcome/画像の生存と再open、263 tests・必須check・両buildが通過。一方でlogo focus中のR不達と連続menu再openの不成立を観測し、次の監査対象とした。全screen reader・実環境/最終候補/配布/外観受入を含むH1 gateは継続する。

2026-09-07 13:07、logoなど通常controlのfocusがR/Undoを飲み込む問題を修正した。有効な現在binding/prefixだけをeguiより先に処理し、UI操作key・text・overlayと既存値操作を維持する。通常logo/reading button/tabのRとUndo、menu Space・palette文字入力、選択/crop/確認Cancel、264 tests・必須check・両buildが通過。連続menu再openの不成立と、全screen reader・実環境/最終候補/配布/外観受入のH1 gateは継続する。

2026-09-07 13:19、上記の連続menu再openの不成立を訂正した。試験helperがEdit buttonと通知Textを名前だけで混同しており、Button型も照合すると同じ通常binaryで回転/Undo・dirty close/Cancelを繰り返せる。製品コードは変えず、連続flowの回帰と検証記録を追加。264 tests・format・Clippyが通過した。次はgrid/filmstrip間のfocusを監査し、全screen reader・実環境/最終候補/配布/外観受入を含むH1を継続する。

2026-09-07 13:34、grid button focus中にEscape一回では閉じない問題と取消時のfocus復帰を修正した。paletteとの復帰先共有、menu/modal/prefixの優先、command固有focusを回帰で確認。通常releaseの四辺復帰・矢印・重なったmenu・grid回転/Undo・既存palette flowと265 tests・必須check・両buildが通過した。次はfilmstripを跨ぐfocusを監査し、全screen reader・実環境/最終候補/配布/外観受入を含むH1 gateを継続する。

2026-09-07 13:46、filmstripの初期focusと閉じた後の復帰を接続した。同じmediaなら呼出元、変更後は現在tab/fullscreen Exit/Welcome logoへ一回だけ戻す。通常画像の取消・矢印調整・移動後のtab・fullscreen復帰・dirty Cancel/Undoと266 tests・必須check・両buildが通過。音声playlist・重なったoverlay・全screen readerと、実環境/最終候補/配布/外観受入を含むH1 gateは継続する。

2026-09-07 13:59、palette/grid/menu背後のfilmstripを操作できる問題と、一回のEscapeで上のgridとfilmstripが同時に閉じる問題を修正した。通常音声のplaylist往復、背景Invoke拒否と解除後の選曲、二段階取消、267 tests・必須check・両buildが通過。次は最新binaryの代表保存/再openを確認し、全screen reader・実環境/最終候補性能/配布/外観受入を含むH1 gateを継続する。
