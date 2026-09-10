# 機能・操作・UI改善（2026-09-09 owner goal）

この計画はローカル草案の `concept.txt` と `follow-up-plan.txt` を現在の実装へ照合した、新goalの作業台帳である。草案自体はGitへ含めない。ローンチ準備・署名・公開・インストーラーの再認定は再開しない。

## 優先規則と検証

- single D3D11 device、core/runtime/appの境界、Shell順、非破壊編集と保存先保護、WASAPI Shared、D3D11VA→softwareは維持する。草案のCUDA／Exclusive／wgpu案へ戻さない。
- 以前のH1記録で「この変更には含めない」とした機能は、永久的な却下ではない。follow-upの明示要望を未完事項として扱い、構造を変える際は先にARCHITECTUREへ採用契約を追記する。明確な採用仕様との競合は理由を記録する。
- 既存実装・過去のtest成功だけで草案全体の達成とはしない。下記の各項目に実装箇所、対象操作の自動回帰、同条件の実画面／runtime検証を対応させる。外観は座標・寸法・色と画面の両方で確認する。
- 一度に全機能を混ぜず、依存順に検証可能な変更へ分ける。format／Clippy／全target testsと関連実画面を通してcheckpointをpushする。Windows 10実機は必須化せず未検証を明記する。

## 作業台帳

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
| U01 | ネイティブ角丸・境界・caption controls、重くないwindow drag | 主実装をcheckpoint化。DWM caption＋同一deviceの入力透過child surface。角丸、標準button hit、drag／double-click／最小化／最大化／復元／fullscreen、PNG・短いhardware動画、UIA操作とguard、pointer resize、画像と再生／一時停止動画の復旧を確認。標準system menuのpopupも確認。混在DPI・物理キーのmenu操作・drag遅延の定量比較は継続。Snap候補は基準機の設定で無効なため表示未検証 |
| U02 | modalのnative利用をコード量・操作性で判断 | 要照合。通常egui、graphics故障時native。metadataと画像／動画resizeのpopup保持を全app frameで修正・回帰確認。guardと入力・focus・保存取消、全modalのnative選択判断は継続 |
| U03 | Codicon、Figtree＋日本語UI font、数字の等幅 | 主対応済み。monapadのFigtree／Monaco Codiconを同梱、既存tnum字形を再生成可能な派生fontへ固定。Yu Gothic UI Regularのfaceを優先し、glyph・等幅・UI配置と実日本語画面を確認。今後追加する操作のiconとnative caption後の最終照合は継続 |
| U04 | grayscale配色、barの2境界、logo／tabの中央揃え・左寄せ・一定padding | 一部対応。基本バー・共通widget状態色・clear色、logo／tab中央と左10px余白、timeline上へ移る2境界を実装・検証。overlay固有色／全media状態での最終照合、font/icon変更後の配置確認は残る |
| U05 | 重い保存等の進捗はtoolbar下境界、軽い画像移動で点滅させない | 主経路実装。保存jobのsnapshotへtrim／区間編集／rateを反映し、workerの出力時刻から推定。normalize二pass、静止画／未知長はindeterminate、取消停止／完了・取消・失敗で境界復帰。既存取消／guard維持、UIA進捗、100／125／200%×狭幅の1物理px・hover不変、画像読込非表示、tab切替後の実保存と実GPU復旧前後を確認。通常windowの見え方／物理入力／mixed-DPIの最終照合は継続 |
| U06 | Welcome tab常在、Open file/folderと最近開いたfile | 主要実装済み。空選択をWelcome identityへ置換し、初回Open／最後のcloseを往復。直近40件のpath-only履歴をworkerで永続化し、複数window更新をlock下でmerge、破損fileは保持して警告する。可視cardだけ既存filmstrip worker／低解像度cacheを共用し、thumbnail／waveform・duration・左揃えfile名から開く。Windows 11でPNG／MP4／WAVの履歴、順序更新、正常終了後の再起動復元、UIA OpenとWelcome復帰、狭幅gridを確認。session復元や未保存backupは追加しない。全screen reader／物理入力matrixはG01で継続 |
| U07 | tabごとの全表示／再生状態保持、背景音声・複数動画、非activeの表示負荷抑制 | 一部実装。画像の画素／view／読書状態に加え、通常UIで動画／音声session・clock・view・bar開閉・取得済みduration/waveformを保持。複数sessionの背景音声・既知／未知終端・停止位置・fault隔離・close・全sessionの同一device復旧を検証。既知終端の非active動画はdecode停止、未知ならclock同期の有界処理。音声は復帰時に再Openせず、映像も開いた入力を再利用する。保持した最終frameを復帰直後に再描画し、現在位置の新frameへ置換する。非activeのhardware frameは同一deviceの1枚へ独立化し、NV12の24枚pool参照解除を確認。P010実decodeは明示skip。decoder再構築／Seek／置換待機と全VRAM予算は残る。playlist／表示中filmstripのscrollとtimeline高さに加え、media controlのrole／項目path別focusを保持。22役割・画像再読込なし復帰と実GPU／WASAPI切替・復旧を確認。通常window／物理入力／混在DPI／全UIA、全resource予算、device／endpoint失敗の全組合せは未完 |
| U08 | tab dragの連続性、window分離／結合、filmstripから分離、drop indicator | 一部実装。同SID／session／exeの起動をpath-only IPC＋startup ackで同hostへ集約。通常window間drop／gap挿入線・inactive端scroll／Welcome結合、filmstrip独立Open、画像／音声／動画tabのlive state／未保存編集移送を接続。実process転送／timeout／owner終了、非表示HWNDで起動／state／GPU共有、OS hitを注入したdirty画像結合を確認。画像frame／画素Arc・不足ページ／preview／resample・予算、再生／repeat／shuffle・通知・focus／復旧を維持。可視Explorer／foreground／物理入力／重なり／mixed-DPI・移送速度は未完。旧独立processのlive stateを回収する仕様ではない |
| U09 | tab context menu、閉じる操作群、path copy／開く、reopen closed | 主要実装済み。tab ID固定のright-click menu、close／other／left／right／all、path copy／Explorer選択、Ctrl+Shift+Tによる直近32件のpath-only再表示。menu／palette／custom shortcutで共有commandを使い、dirty tabごとのSave／Discard／Cancelとexport中の保護を確認。Windows 11の実windowで非active対象・clipboard一致・右側／他／全tab close・Cancel／Discard／Welcomeから再表示・Explorer選択を確認。tab名／close buttonのShift+F10・Menu key・UIA入口とorigin復帰、guard取消・close後の現在tab／Welcome復帰を追加。reorder後の連続矢印／disabled skip、3幅×3密度、実GPU復旧前後10点の6入口を確認。通常windowの物理Menu key／混在DPIと全体UIA監査は残件。sidebar／pin／preview-tab／追加button／未保存backupは要求外 |
| U10 | tab hover／filmstrip／recent／seekの低解像度preview共用と速い表示 | 一部実装。同hostの全window／workerで64件／16 MiBのRGBAを共有し、同keyの生成を集約。duration成功値も別の64件枠で共有・probe集約する。原寸読込後の240×160以内のpreviewは元window閉鎖後も再利用。GIF／APNG／animated WebP／AVIFは原寸の先頭frameから初回読込中にも供給する（別decodeなし）。先行変更で既読PNG＋GIFのfilmstrip初回観測301.356→84.384ms、warm再表示は約65～85msで明確な差なし。今回の共有変更の実時間は未測定。tab hoverの画像／音声はfilmstrip、動画はseekの16コマsheetをCPU／diskで共用し、非active動画は最後の観測位置。recent可視gridも同じcacheへ接続。隣の静止画先読みからも共有previewを供給し、縮小eviction後は保持中原寸から再供給する。動画sheetの現在位置先読み／hover優先と単枚fallback、可視windowの停止中hover／Seek／tab表示を確認。animation先読み、GPU texture共用と全素材性能は未完 |
| U11 | 選択線は反転色1pxのみ、不要なgrip／shadow／暗幕なし | 主経路実装。画像／動画／時間選択を1物理pxの反転枠へ統一し、暗幕・grip・辺focusの追加四角を除去。数値focusはstatusへ表示しhit領域・keyboard／UIAは維持。125%の角重複を修正し、geometry／app回帰とWARP／実GPUの厳密pixel readbackを確認。通常exeは前面確認で入力前に中止したため、通常windowでの見た目・操作と混在DPIの確認は未完 |
| U12 | compact seekのhoverつまみを両端内に収め、非hoverは全幅1px | 対応済み。つまみ半径を除いた移動区間を描画・hover・releaseで共有。100／125／200%の自動描画・座標回帰と通常releaseの画像両端表示を確認。実OSの混在DPIは未検証 |
| I01 | Fitの余分な8px余白を除き、Cover表示command／shortcutを追加 | 対応済み。core/image.rsのCover、共有CoverWindow commandとShift+C、main.rsのviewport。3新規回帰と通常releaseの960×576画素比較によりFit全幅／Cover全領域・bar非侵入を確認。下記実績参照 |
| I02 | 読書modeは隙間なし連結、重複しない見開き送り、先頭枚数offset、枚数／offset drag＋shortcut | 主要対応済み。8837892の固定ページ分割にcursor固定dragを追加。上下／左右の主軸を固定し、release確定・Escape／focus／resize／離脱／overlay取消で解放・復元する。active/edit対象保持、drag直後の左右送り、Windows 11の通常／fullscreenとUIA Toggle、1px固定範囲から元のdesktop範囲へ戻ることを確認。follow-up追記のdirty開始禁止／読書中の編集・Undo/Redo禁止／同輪郭filled iconも追加し、実windowでdisabled状態・別dirty tabへの通常表示復帰・読書解除後のRedo再開を確認。100／125／200%の相対移動は自動回帰、実OS混在DPIは最終matrixで継続 |
| I03 | 高速な画像移動、decode／表示分離、取消、先読み・段階表示の適切な採用 | 一部実装。latest-only foregroundと独立した一件先読み、8枚／256 MiB共有decode cache。先読み結果と既存原寸hitから元寸法付き縮小previewも共有し、追加decode／表示通知なし。6000×6000 PNGのtitle完了中央値220.113→31.840ms、31連打後のtargetを確認。見開きの逐次公開に加え、共有memoryに元寸法付きpreviewがあれば独立workerで取得して通常／readingの原寸読込中に表示する。大GIF再訪は約36ms標本でpreview（原寸Loading）を確認、baselineは約214ms標本まで黒。原寸のzoom／crop／rotate座標と同じ描画、旧結果／読込済みページへの上書き拒否を検証。初回GIF／APNG／animated WebP／AVIFは原寸decoderの先頭frameを縮小し、世代付き一件mailboxで全frame完了前に通知する。取消／source変更／close、原寸成功／失敗のpreview退役と実codec画素一致を確認。静止画のfirst-decode／cold-storage、animationのpreview先行生成と可視UI／連打全般の待機低減は未完。近似previewと原寸を混同しない |
| I04 | 画像の移動keyと端点・複数枚jump、reading時の役割 | 主実装済み。左右／Home／EndにPageUp／PageDown／Backspace／Space／A／D、Ctrl+数字で1～10枚、Shiftで逆向き、Ctrl+Space／Backspaceで5枚を追加。Shell順の画像数で数え、jumpは端で停止、通常reading移動は見開き単位。採用済みCtrl+左右＝同種一枚は維持。旧設定の限定移行、customキー／prefix・記号・text focus保護、dirty Cancel、実画像の非同期読込・画素／tab／見開き／端点no-reloadを回帰確認。通常window／keyboard layout／IMEの最終確認は残る |
| I05 | 画像／選択範囲のclipboard copy、resize/resample、interpolation | 主経路実装済み。Ctrl+Cの編集後frame／selection／透過RGBA、Ctrl+Rの寸法／比率／4補間／非同期処理／Undo/Redo／保存PNG一致を検証。View／paletteに表示専用Smooth／Nearestも追加し、画像・animation・読書・共有cache・復旧dataとcopy／履歴不変を回帰。固定rendererのtexture options無視をsource-only修正版で解消し、WARPの混在sampler／partial更新画素試験をCIに追加。実windowでもnearest領域246015画素が原色のみ、smoothの245692画素は中間色、copyは元16×16と一致。resize固有の実GPU復旧・混在DPIの追加監査は未完 |
| I06 | preset aspect selection、自由回転、readingの回転／反転alias | readingのR／L配置切替・H／V順反転を接続済み。比率preset7種をEdit menu／palette／Ctrl+Kの後に1～7へ追加し、編集後寸法・SAR・context・履歴・PNG保存画素と実動画cropを回帰確認。画像自由回転のcore値／非同期raster／export、menu／Ctrl+Shift+R／角度dialog／slider／近似配置previewに加えAlt保持左dragを接続。0.1度、外接寸法、alpha補間、PNG一致、animation・Undo/Redo・tab／stale拒否、取消／0度非編集、focus復帰、custom binding衝突、320×300 scroll、drag所有権・release順序・scale・有界描画を回帰確認した。動画のcore／SAR／software保存・単一device rasterに角度UI・適用前budget／編集後geometry・selectionを接続し、実動画／D3D11VAで検証。動画Alt保持dragも同じGPU preview／確定へ接続し、1×／2×、取消12条件、入力所有権・release順序・Undo／0度・保存再読込を回帰確認。HDR／全素材画質、通常window入力／appearance・性能は未完 |
| V01 | seek上dragでtimelineを開く、専用buttonを除く、timeline中thumbnailなし | 実装・検証済み。click閾値後の初動が上優勢なら展開のみ、横／下が先ならrelease時Seek。T／View／paletteは動画専用、fullscreenから展開時は通常windowへ戻る。専用buttonとtimelineのhover thumbnail生成／表示を除去。通常releaseで20秒を保持する上drag、横→上でもSeek維持を確認。V03の時間選択編集は別の未完事項 |
| V02 | 区間低解像度previewの先行生成・即時hover、drag中も同じpreview表示 | 一部実装。短編20区間／長編約5秒間隔、16コマ4×4 sheetを現在位置から先読みし、hoverで別sheetを優先。Seekは2枚LRUで同textureのUVだけを切替、tabも共有memory／diskを参照する。準備中は単枚fallback。実H.264の16コマ生成約1.119秒／memory取得約0.56ms、同textureの16位置で追加uploadなしを確認。可視960×576の生成MPEG-4で停止中hoverの非Seek、前半／後半・クリックSeek・tab表示を確認。本画面scrub、長GOP／全codec・mixed-DPI／初期応答分布／peak負荷は未完 |
| V03 | timelineの時間選択・範囲再生・内外削除・連結、部分音量／速度、rubber-band | 部分実装。区間model／export／再生／appの時間軸・Undo/Redo・波形再配置は接続済み。旧trim gripを横dragの時間選択へ置換、CTI drag／clickはSeek。Delete／Ctrl+Y／Ctrl+A／I／O／UIA端点、tab保持・取消・stale拒否、通常releaseの0.5～1.5秒選択→Delete→Undo→Keepと選択履歴export再decodeを確認。部分音量線の縦drag／Alt+横drag stretchと数値UIA・keyboardを追加し、release確定／取消・混在gain・区間速度制限・選択保持を回帰確認。通常releaseでも部分mute→50%→2秒から2.5秒へstretch→Undoを確認。Shift+Spaceの範囲再生／Space停止再開／Escape解除、元の編集軸・履歴保持、背景末尾停止とaudio repeat／auto-next抑止も実装し、通常releaseで1.5秒停止→通常2秒終端復帰まで確認。difference枠はU11で共通実装しWARP／実GPU pixelを確認。通常window・全focus/style監査は残る。長い削除区間のdecode負荷・初期Seek位相・継ぎ目の音質、最終UI/export一致も残る |
| V04 | 動画の閲覧／編集contextでshortcut競合を解消し誤編集を防ぐ | 部分実装。visual選択・crop・回転・flipはtimeline表示中だけ有効。共通command判定とpointer／UIA／辺focusを揃え、閉じる／全画面で途中操作を取消し確定選択・編集結果を保持。視聴・保存・Undo/Redoは維持する。compact seekを毎frame取消す初期不具合を同一frameの回帰で修正。既存の動画export画素照合も維持。J/K/Lを既存Seek／再生commandの追加bindingへ接続し、動画編集中は主bindingのL回転を優先する。custom主binding／prefixの保護、旧設定の限定移行、複数bindingの保存・再読込、hidden window/sessionのSeek／K停止再開を回帰確認。動画comma/periodを実PTS探索へ接続、速度はCtrl+comma/period、旧標準だけ移行。32操作の順序・世代／tab／modal取消・表示frame基準・一時停止・Delete/stretch区間往復をhidden-window回帰で確認。DTS索引のB picture欠落を再現し、video Seekのkey PTS確認で修正。全参照PTS／画素、VFR／B picture／TS／第2streamを照合。長押し2倍速を動画視聴面／共通再生buttonへ接続。400ms静止保持、短いclick維持、元rate/pause復帰、履歴／編集plan／選択再生範囲の保持をhidden-windowと無音WASAPIで確認。focus／Seek／tab／EOF取消、描画frame不在時のreleaseとnative短click経路も回帰対象。音声frame相当操作は10ms微小Seekとしてcomma／period・Viewへ接続し、停止・累積・実EOF／編集時間軸・rate・範囲外移動と非編集を確認。通常window・長GOP／全形式の微小Seek精度・遅延は未完 |
| V05 | 動画のzoom・resizeと既存crop／rotate／flip／fullscreen | timeline内のCtrl＋wheel／+・-／100%／Fit／Coverと右drag panを接続。SAR／physical倍率・cursor基点、viewportとUVのclip、modal／overlay／取消、timeline／fullscreen／tab保持と保存画素不変を回帰確認。実D3D11VAの復旧前後もCPU転送0。resize／resampleのcore値・4filterのsoftware保存・SAR1／合成順序・source照合、GPU4方式／符号付き中間／係数再利用／512 MiB予算と代表画素・速度を確認。Ctrl+R／Edit menuの寸法・比率・4filter／preview／Apply・Cancel UI、identity／snapshot／古いtoken拒否、focus／overlay・Undo/Redo・保存再読込と実D3D11VAも接続。通常window／混在DPI・全素材品質／持続性能は未完、単一device・preview/exportの対応を維持する |
| A01 | 音声の自動次曲、repeat all／one／off、shuffleとbuttons | 主要経路実装。tab別のShell順auto-next、repeat off／all／one、shuffle一巡、前後操作、status buttons／View／palette／音声Ctrl+R。曲末にShell順を非同期再取得し、初回取得前のtab切替にも対応。実WASAPIでactive／背景の次曲・loop・dirty guard・古い通知拒否・失敗隔離を検証。通常releaseでbuttons／shortcut／自然EOFの次曲を確認。modeのrestart永続化、gapless、手動選曲の独立した履歴stackは提供しない |
| A02 | 音声timeline常時、動画共通の選択編集・音量／速度操作 | 音声timelineはfullscreenでも常設、Tでは閉じずcompact seek／専用buttonなし。V03共通の時間選択・Delete／Keep・UIA端点を接続。共有rubber-band音量／Alt+drag stretchと数値操作も接続。共有範囲再生も接続し、repeat off/all/oneと背景停止を実WASAPIで確認。最終操作監査は引き続き未完 |
| E01 | metadata書換、音声抽出、normalize、stereo／mono export | 部分実装。音声のみ出力、normalize／Mono・Stereo設定とSave連携済み。動画／音声の10文字metadata設定UI・非同期既存値・source別保持・再probe／既存target保護を検証。PNG→PNGの10項目とJPEG→JPEGのXMP4項目もFile／custom command・既存値UI・Save／再Save／guard／source lifecycleへ接続済みで全Keep／設定未使用でも保持。JPEGは言語Alt／作者Seq・Keep／Set／Remove、非XMP bytes・EXIF／画素不変、有界parse・取消・target保護を確認。JPEG残る文字項目、他形式／EXIF・IPTC・COM整合／Extended XMP、通常window／全codec品質は未完 |
| M01 | logoの三方向menu gestureと最小限の状態表示 | 主経路実装。8 logical px・右上File／右下Edit／左下View、releaseでsubmenuだけ開く。shaftの80ms移動と非選択矢印の半透明、取消／所有・source/tab／graphics世代、普通のclick／keyboard／UIA、guard／一回Undoを検証。3幅×100／125／200%の再openと形状、実GPU復旧前後10点の3方向／Escape・履歴／transport不変・CPU転送0を確認。通常windowでの物理pointer／混在DPI・全focus/style監査は継続 |
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
