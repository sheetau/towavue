# 機能・操作・UI改善（2026-09-09 owner goal）

この計画はローカル草案の `concept.txt` と `follow-up-plan.txt` を現在の実装へ照合した、新goalの作業台帳である。草案自体はGitへ含めない。ローンチ準備・署名・公開・インストーラーの再認定は再開しない。

## 優先規則と検証

- single D3D11 device、core/runtime/appの境界、Shell順、非破壊編集と保存先保護、WASAPI Shared、D3D11VA→softwareは維持する。草案のCUDA／Exclusive／wgpu案へ戻さない。
- 以前のH1記録で「この変更には含めない」とした機能は、永久的な却下ではない。follow-upの明示要望を未完事項として扱い、構造を変える際は先にARCHITECTUREへ採用契約を追記する。明確な採用仕様との競合は理由を記録する。
- 既存実装・過去のtest成功だけで草案全体の達成とはしない。下記の各項目に実装箇所、対象操作の自動回帰、同条件の実画面／runtime検証を対応させる。外観は座標・寸法・色と画面の両方で確認する。
- 一度に全機能を混ぜず、依存順に検証可能な変更へ分ける。format／Clippy／全target testsと関連実画面を通してcheckpointをpushする。Windows 10実機は必須化せず未検証を明記する。

## 作業台帳

「未完」は新goalの残件。「要照合」は採用済み契約・実装・実画面の追加確認が必要であり、完了扱いではない。

| ID | 要求・到達状態 | 初期証拠／残件 |
|---|---|---|
| U01 | ネイティブ角丸・境界・caption controls、重くないwindow drag | 主実装をcheckpoint化。DWM caption＋同一deviceの入力透過child surface。角丸、標準button hit、drag／double-click／最小化／最大化／復元／fullscreen、PNG・短いhardware動画、UIA操作とguard、pointer resize、画像と再生／一時停止動画の復旧を確認。標準system menuのpopupも確認。混在DPI・物理キーのmenu操作・drag遅延の定量比較は継続。Snap候補は基準機の設定で無効なため表示未検証 |
| U02 | modalのnative利用をコード量・操作性で判断 | 要照合。通常egui、graphics故障時native。guardと入力・focus・保存取消を維持して判断を記録する |
| U03 | Codicon、Figtree＋日本語UI font、数字の等幅 | 主対応済み。monapadのFigtree／Monaco Codiconを同梱、既存tnum字形を再生成可能な派生fontへ固定。Yu Gothic UI Regularのfaceを優先し、glyph・等幅・UI配置と実日本語画面を確認。今後追加する操作のiconとnative caption後の最終照合は継続 |
| U04 | grayscale配色、barの2境界、logo／tabの中央揃え・左寄せ・一定padding | 一部対応。基本バー・共通widget状態色・clear色、logo／tab中央と左10px余白、timeline上へ移る2境界を実装・検証。overlay固有色／全media状態での最終照合、font/icon変更後の配置確認は残る |
| U05 | 重い保存等の進捗はtoolbar下境界、軽い画像移動で点滅させない | 未完。現在はexport window／一時status。実際の進捗・取消・失敗との整合が必要 |
| U06 | Welcome tab常在、Open file/folderと最近開いたfile | 主要実装済み。空選択をWelcome identityへ置換し、初回Open／最後のcloseを往復。直近40件のpath-only履歴をworkerで永続化し、複数window更新をlock下でmerge、破損fileは保持して警告する。可視cardだけ既存filmstrip worker／低解像度cacheを共用し、thumbnail／waveform・duration・左揃えfile名から開く。Windows 11でPNG／MP4／WAVの履歴、順序更新、正常終了後の再起動復元、UIA OpenとWelcome復帰、狭幅gridを確認。session復元や未保存backupは追加しない。全screen reader／物理入力matrixはG01で継続 |
| U07 | tabごとの全表示／再生状態保持、背景音声・複数動画、非activeの表示負荷抑制 | 一部実装。画像の画素／view／読書状態に加え、通常UIで動画／音声session・clock・view・bar開閉・取得済みduration/waveformを保持。複数sessionの背景音声・既知／未知終端・停止位置・fault隔離・close・全sessionの同一device復旧を検証。既知終端の非active動画はdecode停止、未知ならclock同期の有界処理。音声は復帰時に再Openせず、映像も開いた入力を再利用する。保持した最終frameを復帰直後に再描画し、現在位置の新frameへ置換する。decoder再構築／Seek／置換待機、hardware surface pool保持量の削減は残る。playlist／表示中filmstripのscrollとtimeline高さもtab別に保持。全focus状態、全resource予算、device／endpoint失敗の全組合せは未完 |
| U08 | tab dragの連続性、window分離／結合、filmstripから分離、drop indicator | 一部実装。並べ替えと別processへのpath detachのみ。結合と状態移送を未完として扱う |
| U09 | tab context menu、閉じる操作群、path copy／開く、reopen closed | 主要実装済み。tab ID固定のright-click menu、close／other／left／right／all、path copy／Explorer選択、Ctrl+Shift+Tによる直近32件のpath-only再表示。menu／palette／custom shortcutで共有commandを使い、dirty tabごとのSave／Discard／Cancelとexport中の保護を確認。Windows 11の実windowで非active対象・clipboard一致・右側／他／全tab close・Cancel／Discard／Welcomeから再表示・Explorer選択を確認。context menu内の連続矢印／Enterは自動回帰、Shift+F10等のkeyboardからの呼出しと全体UIA監査は残件。sidebar／pin／preview-tab／追加button／未保存backupは要求外 |
| U10 | tab hover／filmstrip／recent／seekの低解像度preview共用と速い表示 | 一部実装。preview worker間で64件／16 MiBの低解像度RGBAを共有し、原寸読込後の240×160以内のpreviewもfilmstrip／画像seekへ供給。既読PNG＋GIFのfilmstrip初回観測301.356→84.384ms、warm再表示は約65～85msで明確な差なし。tab直下のhover previewとpathを追加し、画像／音声はfilmstrip、動画はseekの区間thumbnailを共用。非active動画は最後の観測位置で、U07の状態復元ではない。recentの可視gridもfilmstrip worker／cacheへ接続した。未訪問画像の先行生成、動画sheetとGPU texture共用は未完 |
| U11 | 選択線は反転色1pxのみ、不要なgrip／shadow／暗幕なし | 主経路実装。画像／動画／時間選択を1物理pxの反転枠へ統一し、暗幕・grip・辺focusの追加四角を除去。数値focusはstatusへ表示しhit領域・keyboard／UIAは維持。125%の角重複を修正し、geometry／app回帰とWARP／実GPUの厳密pixel readbackを確認。通常exeは前面確認で入力前に中止したため、通常windowでの見た目・操作と混在DPIの確認は未完 |
| U12 | compact seekのhoverつまみを両端内に収め、非hoverは全幅1px | 対応済み。つまみ半径を除いた移動区間を描画・hover・releaseで共有。100／125／200%の自動描画・座標回帰と通常releaseの画像両端表示を確認。実OSの混在DPIは未検証 |
| I01 | Fitの余分な8px余白を除き、Cover表示command／shortcutを追加 | 対応済み。core/image.rsのCover、共有CoverWindow commandとShift+C、main.rsのviewport。3新規回帰と通常releaseの960×576画素比較によりFit全幅／Cover全領域・bar非侵入を確認。下記実績参照 |
| I02 | 読書modeは隙間なし連結、重複しない見開き送り、先頭枚数offset、枚数／offset drag＋shortcut | 主要対応済み。8837892の固定ページ分割にcursor固定dragを追加。上下／左右の主軸を固定し、release確定・Escape／focus／resize／離脱／overlay取消で解放・復元する。active/edit対象保持、drag直後の左右送り、Windows 11の通常／fullscreenとUIA Toggle、1px固定範囲から元のdesktop範囲へ戻ることを確認。follow-up追記のdirty開始禁止／読書中の編集・Undo/Redo禁止／同輪郭filled iconも追加し、実windowでdisabled状態・別dirty tabへの通常表示復帰・読書解除後のRedo再開を確認。100／125／200%の相対移動は自動回帰、実OS混在DPIは最終matrixで継続 |
| I03 | 高速な画像移動、decode／表示分離、取消、先読み・段階表示の適切な採用 | 一部実装。latest-only foregroundと独立した一件先読み、8枚／256 MiB共有decode cache。6000×6000 PNGのtitle完了中央値220.113→31.840ms、31連打後のtargetを確認。見開きの逐次公開に加え、共有memoryに元寸法付きpreviewがあれば独立workerで取得して通常／readingの原寸読込中に表示する。大GIF再訪は約36ms標本でpreview（原寸Loading）を確認、baselineは約214ms標本まで黒。原寸のzoom／crop／rotate座標と同じ描画、旧結果／読込済みページへの上書き拒否を検証。未訪問／cold-storage、preview先行生成と連打全般の待機低減は未完。近似previewと原寸を混同しない |
| I04 | 画像の移動keyと端点・複数枚jump、reading時の役割 | 左右／Home／Endは実装済み、今回のreading変更と草案alias／jumpは要照合。既存custom bindingとCtrl+左右の同種移動を黙って奪わない |
| I05 | 画像／選択範囲のclipboard copy、resize/resample、interpolation | 主経路実装済み。Ctrl+Cの編集後frame／selection／透過RGBA、Ctrl+Rの寸法／比率／4補間／非同期処理／Undo/Redo／保存PNG一致を検証。View／paletteに表示専用Smooth／Nearestも追加し、画像・animation・読書・共有cache・復旧dataとcopy／履歴不変を回帰。固定rendererのtexture options無視をsource-only修正版で解消し、WARPの混在sampler／partial更新画素試験をCIに追加。実windowでもnearest領域246015画素が原色のみ、smoothの245692画素は中間色、copyは元16×16と一致。resize固有の実GPU復旧・混在DPIの追加監査は未完 |
| I06 | preset aspect selection、自由回転、readingの回転／反転alias | 要照合。90度・反転・正方形／比率保持は存在。任意角度の境界／export契約と入力競合を別途決める |
| V01 | seek上dragでtimelineを開く、専用buttonを除く、timeline中thumbnailなし | 実装・検証済み。click閾値後の初動が上優勢なら展開のみ、横／下が先ならrelease時Seek。T／View／paletteは動画専用、fullscreenから展開時は通常windowへ戻る。専用buttonとtimelineのhover thumbnail生成／表示を除去。通常releaseで20秒を保持する上drag、横→上でもSeek維持を確認。V03の時間選択編集は別の未完事項 |
| V02 | 区間低解像度previewの先行生成・即時hover、drag中も同じpreview表示 | 一部実装。20区間の遅延取得はあるが先行sheetと本画面scrubは未完。bounded生成、初期応答・長GOP負荷を測る |
| V03 | timelineの時間選択・範囲再生・内外削除・連結、部分音量／速度、rubber-band | 部分実装。区間model／export／再生／appの時間軸・Undo/Redo・波形再配置は接続済み。旧trim gripを横dragの時間選択へ置換、CTI drag／clickはSeek。Delete／Ctrl+Y／Ctrl+A／I／O／UIA端点、tab保持・取消・stale拒否、通常releaseの0.5～1.5秒選択→Delete→Undo→Keepと選択履歴export再decodeを確認。部分音量線の縦drag／Alt+横drag stretchと数値UIA・keyboardを追加し、release確定／取消・混在gain・区間速度制限・選択保持を回帰確認。通常releaseでも部分mute→50%→2秒から2.5秒へstretch→Undoを確認。Shift+Spaceの範囲再生／Space停止再開／Escape解除、元の編集軸・履歴保持、背景末尾停止とaudio repeat／auto-next抑止も実装し、通常releaseで1.5秒停止→通常2秒終端復帰まで確認。difference枠はU11で共通実装しWARP／実GPU pixelを確認。通常window・全focus/style監査は残る。長い削除区間のdecode負荷・初期Seek位相・継ぎ目の音質、最終UI/export一致も残る |
| V04 | 動画の閲覧／編集contextでshortcut競合を解消し誤編集を防ぐ | 部分実装。visual選択・crop・回転・flipはtimeline表示中だけ有効。共通command判定とpointer／UIA／辺focusを揃え、閉じる／全画面で途中操作を取消し確定選択・編集結果を保持。視聴・保存・Undo/Redoは維持する。compact seekを毎frame取消す初期不具合を同一frameの回帰で修正。既存の動画export画素照合も維持。J/K/Lを既存Seek／再生commandの追加bindingへ接続し、動画編集中は主bindingのL回転を優先する。custom主binding／prefixの保護、旧設定の限定移行、複数bindingの保存・再読込、hidden window/sessionのSeek／K停止再開を回帰確認。動画comma/periodを実PTS探索へ接続、速度はCtrl+comma/period、旧標準だけ移行。32操作の順序・世代／tab／modal取消・表示frame基準・一時停止・Delete/stretch区間往復をhidden-window回帰で確認。DTS索引のB picture欠落を再現し、video Seekのkey PTS確認で修正。全参照PTS／画素、VFR／B picture／TS／第2streamを照合。長押し2倍速を動画視聴面／共通再生buttonへ接続。400ms静止保持、短いclick維持、元rate/pause復帰、履歴／編集plan／選択再生範囲の保持をhidden-windowと無音WASAPIで確認。focus／Seek／tab／EOF取消、描画frame不在時のreleaseとnative短click経路も回帰対象。通常window・長GOP応答性・音声frame相当操作は未完 |
| V05 | 動画のzoom・resizeと既存crop／rotate／flip／fullscreen | 一部実装。zoom／resizeは未完、単一device・preview/export一致を維持する |
| A01 | 音声の自動次曲、repeat all／one／off、shuffleとbuttons | 主要経路実装。tab別のShell順auto-next、repeat off／all／one、shuffle一巡、前後操作、status buttons／View／palette／音声Ctrl+R。曲末にShell順を非同期再取得し、初回取得前のtab切替にも対応。実WASAPIでactive／背景の次曲・loop・dirty guard・古い通知拒否・失敗隔離を検証。通常releaseでbuttons／shortcut／自然EOFの次曲を確認。modeのrestart永続化、gapless、手動選曲の独立した履歴stackは提供しない |
| A02 | 音声timeline常時、動画共通の選択編集・音量／速度操作 | 音声timelineはfullscreenでも常設、Tでは閉じずcompact seek／専用buttonなし。V03共通の時間選択・Delete／Keep・UIA端点を接続。共有rubber-band音量／Alt+drag stretchと数値操作も接続。共有範囲再生も接続し、repeat off/all/oneと背景停止を実WASAPIで確認。最終操作監査は引き続き未完 |
| E01 | metadata書換、音声抽出、normalize、stereo／mono export | 未完。現在はmetadata copyと固定export。明示optionと非破壊保存・再openの一致が必要 |
| M01 | logoの三方向menu gestureと最小限の状態表示 | 未完。クリック／keyboard menuとlogo描画は存在。閾値・角度・mouseup・取消を共有dispatchへ渡す |
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
