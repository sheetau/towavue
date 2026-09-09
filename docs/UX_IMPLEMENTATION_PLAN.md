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
| U01 | ネイティブ角丸・境界・caption controls、重くないwindow drag | 未完。現状はdecorationsなし＋egui controls。runtime内のnon-client境界設計と通常／最大化／fullscreen／DPI／Snapを確認する |
| U02 | modalのnative利用をコード量・操作性で判断 | 要照合。通常egui、graphics故障時native。guardと入力・focus・保存取消を維持して判断を記録する |
| U03 | Codicon、Figtree＋日本語UI font、数字の等幅 | 未完。現在は既定font＋Windows日本語fallbackと手描き／文字glyph。参照アプリのasset・ライセンス・tabular figuresを確認する |
| U04 | grayscale配色、barの2境界、logo／tabの中央揃え・左寄せ・一定padding | 未完。chrome.rsとmain.rsに現在色／寸法。黒／白／#808080／#181818／#4C4C4Cへ一貫した役割を与え、狭幅でも測定する |
| U05 | 重い保存等の進捗はtoolbar下境界、軽い画像移動で点滅させない | 未完。現在はexport window／一時status。実際の進捗・取消・失敗との整合が必要 |
| U06 | Welcome tab常在、Open file/folderと最近開いたfile | 一部実装。Welcome表示・Openは存在、recent永続化とtab identityは未完。履歴はpath参照に限定し未保存backupを加えない |
| U07 | tabごとの全表示／再生状態保持、背景音声・複数動画、非activeの表示負荷抑制 | 未完。現在はactive切替でsessionを再構築。単一device維持・session所有・audio共存・bounded resource／終了順の設計が必要 |
| U08 | tab dragの連続性、window分離／結合、filmstripから分離、drop indicator | 一部実装。並べ替えと別processへのpath detachのみ。結合と状態移送を未完として扱う |
| U09 | tab context menu、閉じる操作群、path copy／開く、reopen closed | 未完。sidebar／pin／preview-tab／追加button／未保存backupは要求外 |
| U10 | tab hover／filmstrip／recent／seekの低解像度preview共用と速い表示 | 一部実装。filmstrip可視worker／metadata cache、seek区間cacheあり。tab hover、先行生成、共有・実遅延は未完 |
| U11 | 選択線は反転色1pxのみ、不要なgrip／shadow／暗幕なし | 未完。現在のselection描画とkeyboard／UIA hit領域を分離して改善する |
| U12 | compact seekのhoverつまみを両端内に収め、非hoverは全幅1px | 未完。現状は中心が端へ来るため半分はみ出す。描画とpointer→値の対応・DPIを検証する |
| I01 | Fitの余分な8px余白を除き、Cover表示command／shortcutを追加 | 対応済み。core/image.rsのCover、共有CoverWindow commandとShift+C、main.rsのviewport。3新規回帰と通常releaseの960×576画素比較によりFit全幅／Cover全領域・bar非侵入を確認。下記実績参照 |
| I02 | 読書modeは隙間なし連結、重複しない見開き送り、先頭枚数offset、枚数／offset drag＋shortcut | 一部実装。連結配置とhover見開きは存在、現在の一枚送り・offset不在は未完。cursor固定dragの所有／取消も検証する |
| I03 | 高速な画像移動、decode／表示分離、取消、先読み・段階表示の適切な採用 | 一部実装。latest-only decode＋8枚／256 MiB CPU/GPU cache。初回／連続移動の測定と黒いloadingを減らす改善は未完。Shell順と画質を偽らない |
| I04 | 画像の移動keyと端点・複数枚jump、reading時の役割 | 左右／Home／Endは実装済み、今回のreading変更と草案alias／jumpは要照合。既存custom bindingとCtrl+左右の同種移動を黙って奪わない |
| I05 | 画像／選択範囲のclipboard copy、resize/resample、interpolation | 未完。現在のclipboardは文字のみ。編集後画像・alpha・元file保護とnearest表示を検証する |
| I06 | preset aspect selection、自由回転、readingの回転／反転alias | 要照合。90度・反転・正方形／比率保持は存在。任意角度の境界／export契約と入力競合を別途決める |
| V01 | seek上dragでtimelineを開く、専用buttonを除く、timeline中thumbnailなし | 未完。現在はT／button、timelineにもhover preview。Seek／selectionとのgesture判定を明確にする |
| V02 | 区間低解像度previewの先行生成・即時hover、drag中も同じpreview表示 | 一部実装。20区間の遅延取得はあるが先行sheetと本画面scrubは未完。bounded生成、初期応答・長GOP負荷を測る |
| V03 | timelineの時間選択・範囲再生・内外削除・連結、部分音量／速度、rubber-band | 未完。現在の単一区間trim gripは要求と異なる。非破壊区間model／source↔編集時刻／映像音声境界／Undo／export一致の設計が必要 |
| V04 | 動画の閲覧／編集contextでshortcut競合を解消し誤編集を防ぐ | 未完。R/L回転、左右Seek、comma/period速度は存在。frame移動・J/L/K・長押し2倍と編集modeを一貫させる |
| V05 | 動画のzoom・resizeと既存crop／rotate／flip／fullscreen | 一部実装。zoom／resizeは未完、単一device・preview/export一致を維持する |
| A01 | 音声の自動次曲、repeat all／one／off、shuffleとbuttons | 未完。Shell playlist選曲はあるがEOFで停止。履歴／再生順／dirty guard／tab状態保持との結合を検証する |
| A02 | 音声timeline常時、動画共通の選択編集・音量／速度操作 | 一部実装。waveform／trimは存在、V03と共通の編集契約へ進める |
| E01 | metadata書換、音声抽出、normalize、stereo／mono export | 未完。現在はmetadata copyと固定export。明示optionと非破壊保存・再openの一致が必要 |
| M01 | logoの三方向menu gestureと最小限の状態表示 | 未完。クリック／keyboard menuとlogo描画は存在。閾値・角度・mouseup・取消を共有dispatchへ渡す |
| G01 | menu／palette／custom prefix／media別grid／dirty guard／Shell順 | 実装あり。追加commandの全入口と重なり・keyboard／IME／UIA・Undo／保存を変更ごとに再検証する |

## 実装順

1. I01の画像viewportを実装・検証し、U04／U12など画面の寸法・操作境界を整える。native caption（U01）、font/icon（U03）は独立して設計・導入する。
2. I02～I05、U06／U09／U10の閲覧flowを実装し、連続操作を測定する。
3. U07のtab/session所有を確立してU08とA01へ進む。複数sessionのdevice・音声・終了契約を実証する。
4. V03の編集modelと時間軸を確立し、V01／V04／A02／E01へ接続する。他の未完項目も台帳から落とさず、操作・外観・性能の最終照合まで進める。

この順序は小さな項目だけでgoalを完了するための縮小ではない。各行の未完／要照合が残る間はgoal全体を完了扱いにしない。

## I01検証実績（2026-09-09 16:35 JST）

- 通常画像はmedia領域を8px縮めずFit。Coverは二軸比率の最大値、pan中央、resize追従。Shift+CとView／paletteの共通commandを使用し、custom prefixへ変更後は旧keyを残さない。reading／動画／音声／Welcomeでは無効。
- 回帰: coreの縦横・極小／大画像・resize・手動zoom、shortcutのcontextと設定往復、実UI shapeのFit／Cover×通常／fullscreen×100／125／200% DPI×縦横resize×回転×crop previewを確認。選択／編集列を保持し、clipとmesh寸法を独立した比率計算へ照合した。
- 旧8px viewportを固定値にしていた選択辺reveal testは失敗を確認し、新viewportの0..960／32..546と最小pan 330へ期待値を更新した。焦点辺が完全に見えること・手動pan保持の試験は維持。
- 同じ生成32×16 PNG、同じ960×576 windowの通常release比較: 青色領域の両端を含む座標は、旧Fit `(8,53)..(951,524)`、新Fit `(0,49)..(959,528)`、Cover `(0,32)..(959,545)`。Fitの縦letterboxは元画像の2:1比率による正しい余白であり、Coverのみそれも覆う。status／toolbarへは侵入しない。
- paletteのCover候補とShift+C表示、選択実行、正常終了0を確認。生成media原本は不変。実画面の100%以外は自動layout試験であり、OSの混在DPIを実証したものではない。
- 診断を除いた最終通常release bfb6ce0bでもFit／Shift+C／palette検索・Enter／Shift+Wと終了0を再確認。checkpointのFit／Cover／palette画面は前の通常buildの対応PNGと全画素一致。最終format／Clippy／281 tests通過、実機依存3 testsはignoredとして区別する。
- 途中の「画像がtoolbarを覆う」という目視判定は誤り。問題と判断した保存PNGそのものの上32行を走査すると青色侵入は全て0画素、tab背景(28,28,28)／bar背景(8,8,8)も保持されていた。診断で追加したGPU前の出力ログは除去し、rendererの変更は残さない。バグ修正済みという記録にはしない。
- 証拠はignored `target/tmp/image-viewport-20260909/` のbefore-capture／final／診断trial。生成物・実行ログ・UI操作helperはGitへ含めない。残る全体外観／性能／機能は台帳の別項目として継続する。
