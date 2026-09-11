# towavue アーキテクチャ

## I07: 最後のtab closeで原寸cacheを解放（2026-09-12）

最後のtabを閉じたらwindow所有の原寸texture cacheを空にし、ImageLoaderへ取消とdecoded cacheの非同期解放を要求する。復号workerが実行中の旧要求を抜けてからcacheを解放し、UIはcodec完了・cache lockを待たない。解放要求は直後の再openで上書きせず、新しいforeground要求の前に処理する。先読みは取消をcache挿入のlock内でも確認する。別tabが残る場合のcache／retained presentationと共有thumbnail cacheは維持する。GPU textureの解放は既存eguiのfree deltaを次の描画でrendererへ渡す。allocator／driverの予約とprocess-lifetime peakの低下を解放の必須条件にはしない。

## I03/I06/I07: 通常画像送りは描画成功後に逐次実行（2026-09-12）

原寸表示済みの通常画像から始める次／前の1枚送りは、最新要求優先ではなく受理順に処理する。最初の移動から新原寸のmeshを含むframeのPresent成功までを待ち、追加の方向を最大256件だけ保持する。Pathやdecoded画像のqueueは増やさない。上限超過は通知して追加を受理せず、既存の受理分は維持する。復号完了だけでは次へ進めない。描画出力からmedia instance付きtokenを取得し、成功したPresentと同frameのUI action処理の後にtokenを検証して次の1件を開始する。描画失敗、旧handoffだけのframe、stale tokenは進行条件にしない。

待機開始は共通の原寸handoffに結び付け、dirty guard承認後や直接指定した移動先についても、後続の通常1枚送りが最初の新原寸を飛ばさないようにする。直接指定どうしの上書きは残りの方向を破棄して新しいhandoff／tokenを作るため、最新の明示指定を優先できる。

直接の飛び先指定、別sourceのload／tab離脱、画像一覧の順序・構成変更、復号失敗、編集・modal／export等への移行では残りの送りを取り消す。内部の逐次送りだけがloadによる取消をまたいで残りの方向を引き継ぐ。dirty guardは各移動で再利用し、未保存編集やdialogを越えて進めない。reading mode、初回原寸なし、種類を跨ぐ移動や明示ジャンプの最新要求優先は変更しない。後続節の「連打は最新要求優先」は通常1枚送りについてこの契約で置き換える。CPU-only testの合成描画ackと、実rendererのPresent成功は証拠として区別する。

## I06/I07: 復号待ちの原寸表示引継ぎ（2026-09-12）

GPU画素の回帰検証にはruntimeの非default `render-verification` featureをappのdev-dependencyからだけ有効にする。safeな検証専用入口が同じrendererのRGBA back bufferをstaging textureへ読み戻し、所有されたbytesだけを返す。COM／mapped pointerはruntime外へ出さず、通常buildの描画経路にCPU readbackは追加しない。hidden test windowの画素／Present検証は、可視画面・実key入力・アプリ全体のpeak memory証明とは区別する。

同じ検証feature内で、現在processのworking set／private commitとOSのprocess-lifetime最高値、同じadapterのnode 0に対するDXGI local／nonlocal使用量を読み取れる。GPU値は画像到達後のsampleであり真のpeakではない。GPU counter未対応は理由付きで省略し、ゼロの測定値に置き換えない。100枚GPU計測はreadbackなしの通常sampling走行と、Nearestの64点画素照合を行う別走行を区別する。後者の待機が前者のdecode先読みに時間を与えないよう別caseとして扱い、性能改善の根拠にしない。

同一tabの通常画像navigationでは、直前に表示した原寸presentationを一枚だけ表示専用handoffとして保持する。path・view・表示transform・取得済み容量も旧sourceと一緒に固定し、原寸の復号／texture準備が成功した同じ処理で新sourceへ置き換える。保持中は中央の縮小previewとloading captionを出さず、旧画像のanimationも進めない。tab target／titleは最新要求先、statusの画像情報／pathは表示中の旧sourceを表す。旧presentationを新targetのself.imageへ戻さない。

保持中の編集・Undo/Redo・保存／書出し／metadata options・画像／pathコピー・Explorer表示と画像view操作はcommand gateで無効にし、直接の編集／書出し／画像コピー入口も保護する。中央描画は入力を扱わない。navigation・tab／window操作は維持し、明示した飛び先の変更は最新要求優先で最後に表示できた一枚を引き継ぐ。通常1枚送りは前節の逐次実行を使う。古いgeneration／pathの完了では置換しない。失敗、別sourceのload、tab離脱／transfer、最後のtab closeで解放し、未完了の新targetをretained tabへ保存するときに旧画像を混入させない。graphics復旧では保持中textureも既存decoded pixelsから復元する。

ImagePresentationの既存Arc／texture handleを共有し、handoffのためのRGBAコピー・worker・画像データを持つqueueは追加しない。ただしcacheからevictされた場合も直前一画像を生存させるため、既存の一画像decoded上限512MiBに収まる旧decodedデータと表示textureの寿命が延び得る。既存decoded cacheの10件／256MiBとtexture cacheの8件／256MiBは変更せず、これらをアプリ全体のpeak memory上限とは呼ばない。初回open、reading mode、表示可能な旧原寸なし、編集中decode待ち／errorはhandoff対象外。直列100枚の全原寸到達と、連打で全要求を表示すること、実GPUでの無ちらつき・資源認定は別々に検証する。

## I06/I07: 完了済み原寸を描画前に取り込む（2026-09-12）

image_loading中かつUI contextが準備済みなら、draw_uiの最初のlayout passで既存finish_image_loadを呼び、worker mailboxに既にある結果だけを取り込む。RedrawがImagesReadyの処理に先行しても、完成している原寸より空表示／縮小previewを優先しない。通知は引き続き必要であり、後から届いた同じwake-upは空のmailboxとして無害に処理する。context準備前に結果を消費せず、eguiの追加layout passで別の完了結果へ再切替しない。既存のgeneration／path／chunk順序／errorとtexture cache処理を再利用し、UIでの復号待機・filesystem query・新timer／worker／cacheは追加しない。texture準備の既存CPU費用はUI側に残る。

この先行修正は完了通知と描画の順序に由来する不要な一回のloading表示を防ぐ。未完了decodeの表示引継ぎは上の後続節で扱い、旧画像保持を新画像の準備完了として数えない。初回loadなどhandoff対象外ではpreview／空表示があり得る。100枚原寸のseamless gateには実GPU表示・全画像到達と資源確認も必要なままとする。

## G01/V05: 動画wheelの即時・非active反映（2026-09-12）

動画も平滑化済みinput.zoom_deltaとframe末尾のhover／modifier判定から、画像と共通のevent単位zoom parserへ移す。動画用の薄い入口でCtrl必須・Alt／mac_cmd除外を各MouseWheelへ適用し、画像側のmodifier契約は変えない。単位・倍率速度・上限は従来値を維持する。複数wheelはevent時点のpointer位置で順にzoom_videoを適用し、その実倍率でpanと次event用centerを更新する。Ctrlをframe末尾に離しても既に届いたwheelを捨てず、空frameや後続Ctrl入力へ平滑化残量を持ち越さない。SAR・編集後寸法・physical倍率・viewport／UV clip・表示専用stateは既存経路を使う。

timelineによるvisual_selection_enabledと共通overlay／modal／popup gateは維持し、通常視聴ではzoomしない。MouseWheelはwindow focusを要求しないが、明示Zoom／multi-touchにはfocusと動画modifier条件を要求し、動画のbutton-down抑止も残す。右dragはupdate_panのfocus／取消契約を維持する。WindowFocused(false)を含むframeはzoomを破棄し、次frameから有効な非active wheelを再開する。新hook・OS設定変更・active化は行わない。先行画像wheel節で別経路として残した動画の平滑化はこの節で置き換える。生成FFV1／SAR素材のsoftware decode→GPU描画と3倍率回帰は行うが、実マウス入力や全D3D11VA経路の認定とは区別する。

## G01/I07/I08: 非active画像wheelの外側gate（2026-09-12）

通常画像のwheel入力は、overlay／modal／popup等の共通許可条件と「dragなし」を確認し、window focusは要求しない。従来のimage_scroll_deltaだけでなく呼出元もview_drag_allowedを使っていたため、先行変更では非activeの実画像panが止まる余地を残していた。共通overlay条件をview_input_allowedへ抽出し、drag／選択／scrollbar入力では従来どおりfocus必須と取消を重ねる。wheelは縦・Shift横・Ctrl倍率を現在frameの描画へ反映し、keyboard focusやwindowのactive化を要求しない。

zoom_eventsではMouseWheelだけ非activeを許可し、明示Zoom eventとmulti-touchはfocus必須を維持する。WindowFocused(false)が含まれるframeは全量破棄し、次frame以降のposition付き入力を受け付ける。event時点のmodifier・座標・単位・layer／rect／enabled、button競合、追加layout passで再生しない契約は変更しない。古いzoom即時化節の「非active禁止」はこのwheel例外で更新する。固定winit 0.30.13はWM_MOUSEWHEEL／WM_MOUSEHWHEELでmodifier更新後にwheelを発行するが、これは実OS入力の検証ではない。OS設定変更・focus強制・新たな入力hookは追加しない。動画側のzoom／timeline有効条件と平滑化は別経路として残す。

## U03: Welcomeのfont適用漏れ（2026-09-12）

Welcome中央の32pxアプリ名もUI本文と同じProportional family（Figtree-tabular先頭）へ揃え、明示Monospace指定を外す。色・font size・column幅・action／shortcut・Codicon専用familyは変更しない。一般MonospaceのLatin font定義をFigtreeで上書きせず、コード表示用の既存契約と日本語fallbackは維持する。app内の明示font指定、TextEditと値非表示Sliderの利用箇所、固定eguiの関連既定値を確認した。現在のproduction widgetに明示Monospace指定は残っていないが、OS所有dialog／captionや全UIの実表示を一括認定するものではない。

## U03/U04: status容量の非同期取得（2026-09-12）

status描画はfilesystem metadataを読まず、現在sourceの容量結果だけを表示する。windowごとにruntimeのLatestTaskを一つ所有し、path・media instance・folder snapshotのgeneration／captured_atが変わったときだけ問い合わせる。既存folder watcher／refreshによるsnapshot更新で同じpathも取り直す。容量cacheは現在source一件だけで、未取得・失敗は容量を省略し、失敗も次のsource更新まで再試行しない。空fileの0 bytesは既知の値として区別する。source変更時は古い容量を消し、完了eventでも現在sourceを再照合したうえで単調ticketを確認する。タブ復元／別windowへの移動では移動先のsourceとして再取得し、他tabの容量を表示しない。

要求は一件実行＋最新一件待機に制限する。source消失／最後のtabを閉じると未着手要求を破棄し、worker破棄でUIをjoin待ちさせない。metadata問い合わせ自体のOS待ちを強制中断するものではなく、完了前後のcancel確認とticketで古い結果を捨てる。追加timer・polling・全folderの容量cacheは設けない。変更通知が利用できない環境で毎frameの外部変更検出を保証せず、既存snapshot refreshへ従う。実windowのdrag latency全体の改善量は未測定。

## U03/U04: 左寄せ時計と軽量な影調整（2026-09-12）

音声statusが340 logical px未満では、短縮時計を可変幅領域の中央へ置かず、明示left-to-right／上下中央の子UIへ配置する。領域の最小幅・24px高さ・残りのvolume／repeat／shuffle用領域と、時計の省略／全文tooltipは維持する。右端resizeに追従するのは余剰幅であり時計の開始位置ではない。単なるLabel.halignではadd_sizedの領域内中央配置が変わらないため、配置を所有する子UIを変更する。通常幅のtab名／list名／status path・time・volumeは既存配置を維持する。

egui window／popup shadowの横offsetを0、黒alphaを112へ揃える。既存の縦offset（window20／popup10）・spread0は維持し、blur幅をwindow15→18、popup8→10 logical pxへ少し広げる。既存Shadowのfeathered plain meshだけを使い、shader・texture・blur pass・workerを追加しない。window／popup×3寸法×4倍率では旧版とvertex／index数・texture／primitive数が同じ。CPU側tessellation標本は測るが、blur外周の塗る面積は増えるためGPU時間不変とは断定しない。Gaussian／CSSと同等のぼかしを新設したものではなく、負荷優先の既存方式の調整とする。native DWMの影には触れない。

## U01/U04: native境界に沿うタブ余白（2026-09-12）

toolbarのnative button下端／1 physical px区切り線と左右配置を変えず、タブの上下に3 logical pxずつの余白を確保する。通常時は上端のnative内側border 1 physical pxを別途予約し、最大化時は不可視insetを避けるがこのborderを重ねて予約しない。rootのsafe areaは二重加算しない。固定26pxではなく残りをタブ高とし、headlessの32px toolbarにも同じborder／余白計算を使う。極端に低い行では余白を利用可能高の1/4までに制限し、負の高さを作らない。タイトル行だけeguiの最小interact高を行高へ合わせ、buttonの縦paddingを0にする。tab文字の横10px／close幅24px、ID・guard・drag・native glyph／hit処理は維持する。任意UI倍率・native fullscreen／全DPIの実入力認定ではない。

## V04/A01/A02: 音量HUDと音声リスト（2026-09-12）

master volumeは従来の0～2倍・edit history／live playback／export共有を維持する。変更時とvolumeを変えるUndo/Redoではtab ID・media instance付きHUDを1.2秒表示し、status左を音量通知で置換しない。HUDはmedia領域左中央、内側8 logical px、幅3px・最大長244pxの非操作描画とし、全長を0～200%へ対応付ける。動画左側に24pxの余白がなく上側に24px以上あれば上中央の横向きにする。小窓では内側へ縮める。期限の再描画だけを予約し、focus・新規Area・常時animationを追加しない。modal等では表示を抑止し、期限／owner変更で破棄する。

音声音量wheelはtoolbar／tabより下の領域へ広げ、playlistのscroll viewportとscrollbar全体を除外する。timeline・status・list外余白では既存のevent時点の座標・layer・modifier・button・overlay／取消判定で一度だけSetVolumeをdispatchする。list全体の空白もscroll領域として保護し、scrollを音量へ流用しない。非activeで届いたwheelをfocus状態だけで捨てない契約を維持する。

playlistの現在行はinactive文字だけ白にし、背景はhover時だけ描き、keyboard focusでは文字の白表示を保つ。行の右端へsource durationを表示し、長い名称は従来どおり一行truncateする。UIAのpath／current説明に取得済みdurationを追加し、行の安定IDと全幅clickを維持する。

durationはwindowごとにruntime所有LatestTaskを一本追加し、可視行の未取得一件を順に要求する。既存PreviewCacheのmetadata-key付きduration経路を使用し、UI threadでprobe／filesystem I/Oしない。scroll等で先頭の未取得対象が変われば進行中／待機要求を取消し、完了はrequest ticketとfolder path／snapshot generation／capture timeで照合する。失敗もsnapshot内で記憶し毎frame再試行しない。app側label cacheは256件または可視行数の大きい方を上限とし、非可視項目を除く。snapshot変更・最後のtab closeで破棄し、非audio表示では待機を取消す。大量folderの全件probe、追加texture、再生sessionの変更は行わない。

## U09/U10: 説明tooltipの表示寿命（2026-09-12）

アプリ内の説明tooltipは共通のHoverHelpで表示する。pointerがclipped interact rect内にあり、元要素のlayer／hit判定が有効な間だけ許可する。自分のtooltip層が元要素を覆う場合は保持できるが、元要素外のtooltip本体やそこへ向かうpointer移動だけでは保持しない。説明は非操作型とし、egui既定のdelay・still判定・grace・クリック抑止と有効／無効要素の区別を維持する。元要素を失って閉じる際は次frameの再描画を要求し、eguiの前frame所有権から次の説明へ移れるようにする。media previewは別の即時表示契約を維持する。egui本体の変更やOS tooltipは導入しない。

## U09/U10/V02/I02: 即時・非操作型media preview（2026-09-12）

seek／image-reading／tab previewは、通常tooltipのdelay・still判定・前frameのtooltip所有権から分離した共通の非操作型Areaを使う。fade待ちを設けず、初回／内容寸法変更はeguiのdiscard passで同一frame内に再配置する。seekはhover位置の上中央、tabはtab中央の下へ4 logical px離してanchorし、画面端では画面内制約を優先する。pointerが離れれば非表示とし、既存seek dragの所有中だけ領域外でも保持する。標準tooltip全般の寿命やinteractive tooltipを強制終了するAPIは追加しない。

preview用hoverはclipped interact rectとlayerを確認し、focusを要求せず、Tooltip層だけでは背後のpreview元を抑止しない。通常の別overlay層・disabled・menu／modalとapp側overlay条件は維持する。tabの生成要求も同じhover判定を用い、非active時に表示だけでなく対象要求も追従する。preview自体はfocusを取得せずclick-throughとする。一般tooltipの残留／全経路と実OS hoverは別途監査する。

動画seek cardは未生成時から160×最大108 logical pxの画像slotを確保する。既存の上方空間制限内で、到着画像は比率を保持して中央へfitし、sprite UVを変更しない。時刻・失敗captionは一行truncateし、画像有無／横長・縦長／失敗でcaptionの高さを変えない。時刻表示は既存Figtree-tabularを含むProportional familyへ統一し、Monospace指定を外す。原寸品質・生成worker／cache／世代と取消契約は変更しない。

## V03/A02: timeline背景と時間選択合成（2026-09-12）

timeline panelの外側は黒とし、左右・上のmarginを8 logical px、下は0とする。#181818・corner radius 3の背景を置き、その内側3 logical pxを波形・選択・CTIの共通領域とする。内側へ配置してもpanel全体の領域を確保し、既定96px・tab別resize／小窓時上限を維持する。追加borderを出さず、toolbar下とtimeline上（非表示時はstatus上）の二境界を維持する。

画像選択の既存1物理px反転枠は変更しない。時間選択だけ、波形上／文字とcontrol線の下に白alpha51（20%）difference塗りと、左右端だけの白1物理px点線（2物理px描画／2物理px空白）を描く。上下borderは描かない。pixel境界へ丸め、細い選択でも最小1px、左右が同じpixelなら一度だけ描く。既存のplain InvertMesh callbackと通常meshを順に使い、native handleや画素をappへ渡さない。

反転blendのdestination項をINV_SRC_ALPHAにし、premultiplied whiteのalphaをaとした `a*(1-dst)+(1-a)*dst` とする。destination alphaは保持し、a=1の画像枠は従来と同じ結果。通常meshでは通常blendへ戻す。既存mesh描画経路を使い、合成専用texture／shader／readback／CPU画像処理は追加しない。WARP／hardwareのoffscreen readbackで0／20／100%、重なり・clip・alpha・後続通常描画を検証し、実window／mixed-DPI入力の認定とは区別する。

## V03/A02/U12: timelineのドラッグ所有権とhover進捗（2026-09-12）

timelineのCTIは領域内の白1物理px線と上端の下向き三角markerとする。markerの8 logical px高／左右各6 logical px内だけをseek dragの開始対象にし、端では領域内へ切り詰める。線全体の特別なhit判定は廃止し、線からも通常の範囲選択dragを始められる。通常timeline clickによるseekは維持する。

選択端から6 logical px以内の通常dragは近い側の端を変更し、反対端とpress時の掴みoffsetを固定する。media端と反対端の1ns手前までclampし、端の交差で選択を消したりseekへ変更しない。marker、選択端、音量線、通常選択の順に判定する。既存のAlt＋選択内dragによるstretchは優先し、音量線は最初の移動方向でgain／通常選択を固定する。端／stretchはResizeHorizontal、音量線はResizeRow、markerはGrab／Grabbingを使用する。既存の一回commit、取消／focus喪失／modal、keyboard／UIAを維持する。音量線は白alpha128（約50%）。timeline背景／余白と白20% difference選択塗り・左右点線は別の未完gateとして保持する。

compact seekbarのpointer hover中は既存trackと同じ高さでhover位置まで白alpha64（約25%）を描き、再生進捗の白より後ろ・背景より前に置く。hoverだけではseekしない。端のhandle内側travel、非hoverの全幅1px、disabled／keyboard／既存drag処理は変更しない。

## U04: メディア通知のステータス集約（2026-09-12）

画像読込／再サンプリング／読書ページ／再生失敗のmessageをmedia表示面へ描かず、status左のpathと置換する。読書drag中の値、長押し速度、期限内の一時通知、処理中／失敗状態、folder処理の順に解決する。既存のselection keyboard focus hintは保持し、読書drag中はその値を優先する。一時通知は既存の4秒で失効し、処理／失敗が残っていればその状態、それ以外はpathへ戻る。失敗詳細はstatusのtooltipへ全文保持し、読書の複数失敗も集約する。画像成功時に寸法・形式を一時通知へ書き込まず、形式／寸法／frame数はstatus右へ常設する。読書drag確定値は4秒残し、取消値は残さない。

fullscreenの上中央通知を廃止し、通知中は既存の下端control barを表示する。focusを取得せず、非active／modal／overlay等の従来の表示抑止は維持する。通知終了後は通常のedge／keyboard／dragによる可視条件へ戻る。Welcome案内や保存確認等のmodalはこの集約の対象外。音量変更の通知は上記HUD契約へ分離する。

## U04/G01: Debug版のID交替診断枠（2026-09-12）

共通styleでDebug版eguiのwarn_if_rect_changes_idだけを無効にする。同じrectに異なるtabのcontrolが現れる等の遷移を赤い診断枠として画面へ出さない。通常のfocus stroke／keyboard／UIA／tab別focus保持、同一pass内の実ID重複を検出するwarn_on_id_clash、他の診断設定は変更しない。固定eguiのこの診断描画自体がdebug_assertions限定のためReleaseの表示は変えない。これはID安定性の全監査や、利用者の全環境で赤枠が消えたことの認定ではない。

## U07: 未保存indicatorと可逆transformの同一性（2026-09-12）

tab名末尾のasteriskを除き、未保存は既存close領域のdotで示す。領域をhover／keyboard focusするとclose glyphへ戻し、button ID・操作領域・close／dirty guardは維持する。UIAのclose名は維持し、未保存の説明を追加する。

未保存判定は保存済みのoperation snapshotを保持し、履歴cursor一致に加え、連続した直角回転／反転を同じdihedral transformへ正規化して比較する。Undo／Redo用のoperation列は削除しない。crop／resize／自由回転／timeline等を境界として扱い、異なるraster処理を同一視しない。保存中の追加編集や分岐後も、実際にexportしたsnapshotだけを保存済み基準とする。選択／pan／zoomは従来どおり編集履歴へ入れない。任意の画素一致や全timeline同値性の判定を完了したとは扱わない。

## M01/G01: 固定位置の直接menuと方向領域（2026-09-12）

logoの通常clickはFile／Edit／View／Helpだけを持つroot menu、方向dragは対応sectionの中身だけを同じbutton下のanchorへ開く。pointerのrelease位置はpopup配置に使用せず、選んだsectionをpopupが閉じるまで保持する。command描画・enabled／shortcut・keyboard移動は両入口で共用し、Image jumpはViewの子menuに置く。画面端ではeguiの画面内配置を維持する。

8 logical pxの既存閾値と所有／取消を保ち、上向き0度から時計回りでFile [0,112.5)、Edit [112.5,157.5)、View [157.5,270]を採用する。左上の残りは無効。logo button背景は変えず、線は通常#808080、hover／focus／menu表示中は白、方向選択中は選択矢印のみ白で他を#808080とする。toolbar内のlogo左にも既存の右側gapと同じ余白を置く。native captionやtab寸法の新たな追記は別gateで維持する。

## U04/G01: 9月12日追記の配色と非アクティブwheel（2026-09-12）

共通HOVERを#4C4C4Cから#2C2C2Cへ置換し、既存のhover／active／open widget・tab／seekbarへ同じ定数を適用する。他の黒／白／#808080／#181818は維持し、DWM所有captionの描画は独自化しない。以降の配色はこの指定を優先する。

OSが対象windowへ届けたwheelは、音声リスト・画像scroll・既存の音量操作領域でwindowのfocused状態だけを理由に捨てない。event時点のpointer位置・layer／enabled・modal／popup・button競合の既存判定を維持し、windowのactive化やkeyboard focus移動を要求しない。WindowFocused(false)の遷移frameでは従来どおり操作／scroll残量を取消し、その後の有効なposition付きwheelを受け付ける。OSの非アクティブscroll設定は変更しない。画像Ctrl＋wheel zoomのfocus条件、音声の音量領域拡張やHUDはこの変更に含めず、追記の残件として扱う。

## I03: 不透明行のブロック判定（2026-09-12）

appの表示用ColorImage変換は、行内の最大32画素ごとにnative-endian u32のANDを取り、同じendianのalpha maskで全alphaが255か判定する。非不透明blockで打ち切り、画像全体の事前走査はしない。不透明行のpremultiplied構築と混在行のegui標準unmultiplied変換は維持し、透過色の丸めを独自実装しない。unsafe・追加buffer・worker・cache変更はない。全alpha値とblock境界前後の混在画素で標準処理との完全一致を回帰確認する。これはCPU準備の最適化であり、GPU転送や可視切替の完了を意味しない。

## I03: 引継ぎ可能な先読みと表示準備の並行化（2026-09-12）

画像切替が進行中先読みを直接引き継ぐ要求置換を前提とし、通常画像の最新世代・正しいpathの原寸読込成功を受け取った時点で、CPU texture pixelの準備より先に近隣先読みを開始する。同じcompletionの末尾では再submitして取り消さない。readingのchunk／見開き先読みと読込失敗は従来経路を維持し、worker数・cache予算・世代取消・対象順は変更しない。前提修正前に効果を確認できず撤回した開始順の案とは、引継ぎの成立条件を区別して測定する。

## I03: 画像切替で進行中先読みを維持する要求置換（2026-09-12）

新しい画像への切替は空要求で一度取消してから移動先を要求するのではなく、最終的な対象path列の要求で一度に世代を置換する。これによりruntimeの既存契約どおり、対象pathを復号中の先読みだけを引き継ぎ、無関係な処理は取消す。非画像への切替・保持済みtabへの復帰と、読書表示の再構築で読込対象がなくなる場合は明示取消を維持する。表示stateの初期化と世代／pathによる古い結果の拒否、worker数・cache予算・対象順は変更しない。

## I09: crop preview撤去と通常の範囲zoom（2026-09-12）

crop previewの状態と選択による描画UV切出しを撤去する。画像の選択内primary clickは、辺／角hitを除き、選択がviewportへ収まる通常Custom倍率とpanへ移る。編集後の画像寸法とphysical densityを使い、既存倍率上限とI08の画像端clampを保つ。表示画像全体・選択枠・編集履歴・textureは保持し、同じ描画回で新しい倍率・barへ反映する。選択内hoverはZoomIn、保持中はCrosshair。範囲を拡大した後も通常zoom／Fit／選択移動／解除を使い、preview解除という別操作は持たない。動画の範囲内clickは変更しない。

menu／grid／palette／status／既定Ctrl＋Shift＋Yは非toggleのZoomSelectionへ置換する。旧設定のtoggle_crop_preview名は読込aliasとして引き継ぐが、出力はzoom_selectionのみ。旧M5のCtrl＋YからApplyCropへの既存移行は維持する。crop編集と保存処理は変更しない。

以下の過去checkpointで保持／未対応としたcrop previewの記述は、この契約で置き換える。ImageViewStateはzoom・pan・selectionだけを保持し、Fit／Actual／Cover／copy／rotationはpreview状態を参照しない。画像用barの表示軸は当該描画回のoverflowから求め、出現animationを行わず、範囲zoomの初回から操作位置を表示する。

## I09: 表示面の外側clickによる選択解除（2026-09-12）

visual selectionの外部clickは、選択枠から離れた表示面へのprimary pressと短いreleaseの組として扱う。画像外の余白も含むが、menu／overlay／scrollbarなどが所有する入力は対象外。辺・角のhitを優先し、そこへの短いclickでは解除しない。選択外で始まり選択外で終わるclickだけが選択を解除し、表示pan・zoom・編集履歴は変えない。画像内からのdragは従来どおり新しい範囲を作り、余白からのdragは選択を変更しない。取消・長押し・同frameの後続gestureを誤ってclickへ変換しない。動画は既存のtimeline表示時だけのvisual selection規則を維持する。

範囲内clickのcrop previewから通常zoomへの変更は別の未完作業として保持する。

## I09: 選択角のresizeと画像内の選択移動（2026-09-12）

共通のvisual selection hit testでは、8 logical pxの既存許容幅内にある最も近い角を辺より優先し、二辺を同時に変更する。対角はdrag開始時の選択へ固定し、反対側へ交差した場合は反転せずそこで縮退する。Shift時は開始時の比率を保ち、pointerが要求する縦横の大きい方まで拡げつつ、対角から画像端までに収める。辺だけのShift操作は従来の反対辺固定・直交軸中央保持を維持する。hoverでは辺／角の向きに合うresize cursor、primary保持中はCrosshairを使う。確定時の画像／動画のpixel整列と取消は既存経路を使う。

通常画像でcrop previewが無効な時、選択矩形内のsecondary pressは画像panより先に選択移動が所有する。開始時の選択とpointerを保持し、編集後の画像pixelへ換算した相対移動を整数へ丸め、幅・高さを保って画像端まで移動する。範囲外のsecondary pressはI08のpanを使う。移動中はAllScroll cursor、release自身の位置で確定し、共通のgesture取消では開始時の選択を復元する。画像pan・texture・編集履歴は変えず、同じreleaseをpanへ渡さない。readingと動画のsecondary panは変更しない。

この変更は外部clickによる選択解除やcrop preview撤去／範囲への通常zoomを完了するものではない。それらはfollow-upの未完要件として保持する。角の共通geometry実装と、実動画での全入力／品質認定を混同しない。

## I08: 画像の有界panとscrollbar（2026-09-12）

follow-up追記を採用し、通常画像の表示panは各軸で±max(表示寸法−viewport寸法, 0)／2 logical pxへ制限する。収まる軸は中央固定とし、拡大・縮小、編集後の寸法変更、window resize、保持state復帰と元寸法付きpreviewにも適用する。Ctrl＋wheelのpointer基点はこの範囲内だけ維持し、余白を作ってまで基点へ追従しない。右dragは少なくとも一軸にはみ出しがある時だけ開始し、保持中はGrabbing cursor、取消時は既存の開始位置復帰を使う。動画とreadingの表示・移動規則は変更しない。

縦wheelは縦pan、Shift＋wheelは横panへ入力回で反映する。Point／Line／Pageの既存換算、event時点の座標／layerと修飾keyを使い、Ctrl／Alt／別gesture・modal／overlay中はscrollしない。新たな平滑化残量やtab別cacheは持たない。表示panを唯一の位置として、既存egui ScrollAreaの両軸barへoverflow／2−panを渡し、bar入力後の位置を同じ描画回へ戻す。barはoverflow軸だけのfloating表示でviewportを縮めず、画像meshより上へ描く。bar上のwheelは受け付け、selection／右dragの開始領域からbarの最大幅を除く。画像textureと編集履歴には触れない。

選択辺focusによる自動revealも画像端で止める。端にある画像用の不可視14px操作領域はviewportに交差する部分だけを公開し、操作領域全体を見せるため画像外の余白を作らない。4辺のID・値・focus／keyboard／UIA編集は維持し、動画の操作領域は変更しない。実windowの全入力・bar外観、全DPI／資源とscrollbarの完全なUIA操作は別の検証残件とする。

## I07: 画像wheel zoomの即時反映（2026-09-12）

画像のCtrl＋wheelはeguiの平滑化済みzoom_deltaではなく、受信した各MouseWheelの修飾key・単位・量から倍率を計算する。既存InputOptionsのzoom modifier／line speed／zoom speed、Point・Line・Pageの換算と指数倍率を維持し、Moveだけをその描画回で全量反映する。Start／End／Cancelは倍率を変えず、Ctrlなしの後続eventや空frameへ残量を持ち越さない。通常scrollと動画zoomの平滑化には変更を加えない。

各event時点のpointer位置と対象layer／rectを使い、frame最後のhoverへまとめない。複数eventは順番に既存倍率上限とpan補正を適用し、同じ描画回のmeshへ反映する。画像texture・原寸画素・編集履歴は変更しない。明示Zoom eventとeguiのmulti-touch倍率経路は保持する。非active／無効なsurface、overlay／modal／popup、別drag／button押下中は背景zoomを行わない。eguiの再layout passで消費済みwheelを再生しない。これは入力の平滑化待ちを除く契約であり、OS入力から実画面までの時間や全GPU／DPIでの連続性能を保証するものではない。

## I03/I04: 前後移動で全pathを複製しない（2026-09-12）

通常の前後移動は既存FolderSnapshotから現在のraw indexを求め、進行方向のsliceと端からのsliceを順に走査する。同種filter・Shell順・端の循環を保ち、選ばれた一件のpathだけをguardへcopyする。入力ごとの全path Vec生成は行わず、並びのcache・worker・snapshot所有権は増やさない。現在path不在／同種に不一致なら移動せず、候補が自身だけなら従来の同一path guardによるno-opを維持する。音声同種移動のqueue規則と読書modeの見開き送りは変更しない。

## U07: 現在の音声sourceの外部再Open（2026-09-11）

外部Openが既存のcleanな音声playlist tabを選び、そのtabをすでに表示中でpathも一致する場合は、正常／読込中／終端の状態を保持して再loadしない。再生session・clock・位置・選択・view・focusと取得中workerをそのまま使う。Faultedの場合は同じpathでも従来どおり再試行する。背景tabは既存のactivate／保持state復帰経路を使い、別曲への置換では従来の編集／export設定初期化を維持する。

画像・動画の外部Open、明示的な新規tab、dirty／export中のplaylistを別tabで保護する契約は変更しない。source内容の外部変更を監視して自動再loadする機能ではない。

## V03/A02: 選択再生のtempo入力と出力終端（2026-09-11）

選択再生は同じ編集位置からの通常再生のPCMを選択終端で止める操作とする。選択終端をatempo入力のEOFへ置き換えるとwindow処理の結果が変わるため、最後の編集区間内では選択終端後のsourceも必要に応じて入力する。gain／stretch／Deleteで区切られた実際の区間境界は越えて混ぜず、従来の区間ごとのtempo変換を保つ。出力は整数sample境界の必要数のみで、選択外のPCMをWASAPIへ送らない。

必要な出力数が得られたらconsumer停止を正常完了として扱い、残るsource全体を読まない。利用者の取消・consumer拒否・別のdecode errorは成功へ変換しない。実際の区間終端／source EOFでは従来どおりtempoをflushして必要なら不足分をpaddingする。初期Seekの位相や本来の編集区間の短いtempo品質、削除区間の負荷は別の残件であり、この試聴範囲の補正で解決済みとしない。

## V03/A02: 音声timelineの整数sample境界（2026-09-11）

再生と保存の編集区間境界は共通の整数計算で `ceil(編集ns × sample rate / (10^9 × master rate))` を求め、その差を区間の出力frame数とする。検証済みmaster rate 0.25～4.0のf32はすべて2^-25の整数倍なので、この固定単位へ正確に変換しi128で計算する。秒の浮動小数点丸めにより17ms等の境界へ余分な一sampleを足さない。非整列境界は従来どおり切り上げ、局所stretchのtempo処理・gain・source切出し・真に短いatempo末尾のpaddingは維持する。

この修正は音声producerとexport filterの予定sample数を一致させるもので、初期Seekで失われる低精度PTSの位相復元や、長い削除区間を復号しない最適化ではない。WASAPIの所有権・queue・clockとUI／編集modelは変更しない。

## G01/U06: native file dialog取消後のfocus（2026-09-11）

file／folder／export pickerの開始成功時に、そのwindowのegui focusとactive tab（Welcomeならなし）・media generationを保持する。取消／失敗後も同じtabとgenerationで、別modal／guardが続かない場合だけfocusを戻す。選択成功、source／tab変更、残るguard、対応するpending dialogがない遅延結果は保持先を破棄する。再描画でボタンを再登録した後の既存accessibility検査により、消えたmenu項目などの非live focusは除去する。勝手な再実行／ファイル再選択はせず、Enter等の次の操作を待つ。

## E01: JPEG Dynamic Media文字metadata（2026-09-11）

JPEGの対応項目へAlbum／Composer／Genreを加え、[Adobe Dynamic Media定義](https://developer.adobe.com/xmp/docs/xmp-namespaces/xmp-dm/)のxmpDM:album／composer／genreへ対応させる。namespace URIで判定し、RDF Descriptionのattributeまたは単純なtext elementを読み、出力は単純elementとする。配列・修飾値・同一property重複は推測して変換せず拒否する。既存のdc言語Alt／作者Seqと混在でき、Keepは3項目も保持、Setは単一文字値、Removeはproperty除去となる。UIは対応先と元の文字値を表示し、7項目の形式別validation・既存の非同期読取／tab世代／保存guardへ接続する。

文字とXML／APP1の予算・取消・source／既存target保護・stage再読取照合は変更しない。metadata差替処理ではJPEGの非XMP bytes／画素を変更しないが、通常画像保存の再encodeは従来どおり行う。Album artistの対応先、Dateの意味と日付型、Trackの整数／総数表記はこの文字列契約へ混ぜず残件とする。EXIF／IPTC／COM同期、未知／技術XMP・Extended XMP、他形式の完全保持も未完である。

## I03: 通常画像の前後先読み（2026-09-11）

通常画像の表示とShell順取得が完了した後は、現在画像を除く近隣最大9枚を既存の一つの先読みworkerへ渡す。画像だけのShell順で距離1、2、3…と近いものを優先し、同じ距離では直前の移動方向を先にする。右移動なら次1／前1／次2／前2…の順で、単枚移動と同じ端の循環を使い、現在画像と重複候補は除外する。名前で再sortせず、件数上限に達したら候補探索を止める。readingでは既存の隣見開き全体を先読みし、この近隣単枚規則へ置き換えない。

現在画像を含む原寸cache最大10件／256 MiB、batch合計256 MiB、foreground要求512 MiBとworker数は変更しない。準備できる枚数はサイズと形式次第であり、9枚の原寸常駐を保証しない。アニメーションは先頭previewだけを共有縮小枠へ供給する。新しい移動では同じ要求に含まれる実行中の一件だけを既存経路で採用し、古いbatchの残りを継続しない。取消・source stamp・予算超過時の通常読込fallbackは維持する。これは前後数十枚の無制限常駐ではなく、既存予算内で待機中の準備範囲を広げる変更とする。

## I03/U10: アニメーションの先頭preview先読み（2026-09-11）

既存の隣画像／次の見開き先読みで原寸cache対象外になったGIF・APNG・animated WebP・AVIFは、同じ一つのspeculative workerで最初の合成済みRGBA frameだけを取得する。GIF／APNG／WebPは原寸と同じimage decoder、AVIFは既存FFmpeg decodeを使用し、最初のvideo frameでconsumerを停止する。この正常な早期停止だけConsumerClosedを受理し、取消・予算超過・他のdecode失敗は成功にしない。単フレームGIF／AVIFもpreview対象に含む。通常静止画の原寸先読みとforegroundの全frame復号・delayは変更しない。

この一時frameは残りの先読みRGBA予算以下だけ受理し、従来のnearest samplingで240×160以内へ縮小後に解放する。先読みbatchの10件／256 MiB原寸枠、host共通64件／16 MiBの縮小枠は増やさず、アニメーションの先頭だけを原寸cacheへ入れない。codec内部のbufferや最初のframe取得前の入力走査を含めたprocess全体のpeak保証ではない。先読みは全frame列を常駐させる機能ではなく、未訪問画像への最初の表示を準備するものとする。

既存のsource key・元寸法・非blocking生成leaseを静止画先行previewと共用する。同keyが生成中なら重複／待機せず、取得済みpreviewは再利用する。decode／縮小中はUI/cache mutexを保持せず、generation・close／取消・source stampを登録前に確認する。失敗・変更・取消は登録せずleaseを解放する。先読みから表示通知・GPU upload・disk encodeを行わない。通常のforeground要求やfilmstrip等がこのmemory previewを使い、原寸完成／失敗時の退役は既存契約を維持する。先読み対象外の画像や先読み完了前の移動について黒い待機をなくす保証はしない。

## U04: overlayと操作状態の共通色（2026-09-11）

eguiのpanel／window／menu／popup／入力欄の背景は共通の黒、枠線と弱い面は#181818、通常・補助文字は#808080、選択・hover・focusの文字は白を使用する。hover面は既存の#4C4C4Cを維持する。command paletteも黒と共通枠線を使い、常時focusする検索文字は白、枠線を含む最大幅600 logical pxを保つ。playlistは文字の固定色を持たず、selectable buttonの状態色に従う。filmstripとdrag cardの下地は#181818へ揃える。

この規則はUIの基準色であり、警告色・無効時の減衰・アニメーション／anti-aliasingの中間色、OS所有のnative dialog、mediaの画素・透過checkerboardは変更しない。描画命令の色・寸法と入力回帰は実画面の全DPI／media検証の代用ではない。

## U02: native modalの採用範囲（2026-09-11）

草案の「可能ならnative、コード量と操作性で判断」を次のように採用する。すべてを独自UIまたはnativeへ統一すること自体は目的にしない。

| 対象 | 採用と理由 |
| --- | --- |
| File／Folder／Save As | 既存のIFileDialogを維持。Shellの場所・形式選択と上書き確認を利用する |
| 未保存確認、同windowのexportなし | TaskDialogIndirectで明示名の3ボタンを表示。OSが配置・Tab・既定Cancel・Escape／close・アクセシビリティを所有する。Exit時は全編集を破棄するボタン名を明示する |
| 同windowのexport中のguard／継続保存の進捗 | eguiを維持。既存jobの進捗・cancel状態とSave無効化を直接反映し、native callbackや別threadへの進捗同期を増やさない |
| 通常描画時のexport失敗 | eguiの有界ScrollAreaを維持。長い診断を表示しつつ狭いwindowでもOKを到達可能にする |
| metadata／audio export／画像・動画resize／自由回転 | eguiを維持。検証・popup・非同期既存値・編集preview・世代取消を既存フォームと共有し、native controlの独自組立を増やさない |
| graphics復旧不能／configuration警告 | 既存GPU非依存native promptを維持。未保存確認だけ同じTask Dialogへ移し、他の故障通知は既存MessageBoxを使う |
| tooltip／メディアhover preview | eguiを維持。既存texture／UV・有界配置・入力所有権を使い、単純な文字tooltipのためだけのHWND管理を増やす利点がない |

[TaskDialogIndirect](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-taskdialogindirect)のCommon Controls v6要件を、appとruntime test hostのMSVC linker manifestへ埋め込む。新dependency、別配布ファイル、WinUI runtimeは追加しない。`MANIFESTDEPENDENCY`だけではRustの既定link設定でresourceが生成されなかったため、`MANIFEST:EMBED`も明示する。既存のwinit DPI初期化を変えず、native色・寸法を独自に再描画しない。

runtimeの専用STA workerがownerのArcとUTF-16文字列・button配列をmodal終了まで保持する。appへ戻すのは既存の選択結果だけであり、COM／HWND／callback pointerを渡さない。[TASKDIALOGCONFIG](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/ns-commctrl-taskdialogconfig)でowner中央配置と既定Cancelを指定し、既存のguard／file dialog／export継続へ接続する。通常native確認は可視window・exportなし・errorなしの場合に開始し、native promptとfile dialogの二重起動を拒否する。裏側のegui確認は描かない。非表示test hostとexport中の既存egui経路は維持する。確認の起動／worker失敗ではCancelとして編集を残し、statusへ理由を出す。自動再表示ループにしない。

実Windows SDK UIAクライアントで96／192 DPIの3ボタンがButton型・Invoke対応と確認した。旧.NET UIAutomationでは同じnativeボタンがPane・patternなしと見えたため、その観測だけを実装欠陥の証拠にしない。一方で全クライアント互換とも主張しない。168文字の日本語・`&`を含むPNG名を通常／全画面×96／192 DPIで確認し、末尾と全ボタンが収まる。Cancel後は画像選択の同じ辺へfocusが戻り、右キーで1 source pixelずつ操作を続けられる。native titleのCloseもCancelとなる。app回帰は、blocked UIがfocusを解放した後のCancel／worker失敗で辺・選択・履歴を保持し、キー調整を再開することを確認する。全素材・最大長・全倍率・全クライアント・故障時の実UI監査は継続する。

## U11: fullscreen画像のselection focus（2026-09-11）

画像の選択辺がfocusを持つ場合も、既存fullscreen controlsのkeyboard保持条件へ含める。status説明の表示に新しいoverlayを作らず、Exit fullscreenへのfocus要求も発生させない。既存のmodal・window focus・outside pressの優先順位を維持し、画像viewportと選択範囲は変えない。100／125／200%の全app frame回帰で4辺のUIA focus・説明・選択保持・modal抑制を確認。実UIAの96／192 DPIでも4辺を確認し、statusを除く2,016,000／3,942,400画素はfocus移動前後で一致した。音声では96／192 DPIの通常windowと192 DPIのfullscreenで1px反転枠を確認し、4数値対象のfocus前後でtimeline全画素が一致した。全比率・全UIAクライアント・全素材の監査とは区別する。

## U11: 時間選択の数値focus表示（2026-09-11）

時間選択の開始／終了・局所音量・選択長さの4コントロールにも、追加のfocus四角を描かない規則を適用する。既存の位置・hit領域・数値ラベル・keyboard／UIA値は残し、focus中の対象名と値、左右キーの案内をstatus欄へ表示する。対象IDが現在のfocusと一致しない場合、disabled／popup／modal中、timeline非表示時には古い説明を出さない。画像のselection focus状態へ混ぜず、時間側のIDで保持する。描画順の都合でstatusは直前のtimeline描画から得た説明を使い、通常の再描画で現在値へ更新する。

可視の生成PNGとSAR動画で、96／192 DPIの選択枠全画素が元のRGBの反転、内外の追加塗りなしと一致した。時間選択では既存In／Out／Volume／Lengthラベルを枠の欠陥と混同しない。これらの数値領域外で1px反転枠を検証し、focus前後はtimeline全域245,760画素の一致を別途確認した。音声・全画面・全比率・全UIAクライアントの最終監査は継続する。

## U01/U04: native captionに合わせたtitle bar（2026-09-11）

runtimeはnative controlsの予約矩形をphysical client座標で返す。appのtitle bar下端はそのbottom直後の1 physical px区切り線までとし、固定32 logical pxで生じていた隙間を残さない。最大化時に画面外となる上端insetを避け、既にrootへ反映済みのsafe-areaを二重加算しない。行高と上下余白は上記U01/U04の3px契約に従う。native captionがないheadless構成では32 logical pxのtoolbarを維持する。

title barの左右6 logical px外側marginを除き、tab名内部の10 logical px余白、status barの6／3 logical px margin、native controlsの予約領域とWindowsのリサイズ境界は維持する。行内の2 logical px間隔はphysical pixelへ丸め、125%表示でscroll originが半画素に乗ることによる分離anchorの変動を避ける。DWM buttons自体を拡縮・独自描画しない。旧可視画素では96 DPIでボタン下端29と区切り31の間に1px、192 DPIで56と63の間に6pxの隙間があった。通常・最大化の96／192 DPIで区切りを各30／57へ揃えた。全DPI比率・Windows 10・任意UI倍率の外観認定ではない。

## U01/U04: native captionの非アクティブ背景（2026-09-11）

黒いclientとnative captionの背景を揃えるため、作成時にDWMWA_CAPTION_COLORへ黒のCOLORREFを指定する。Windows 11 build 22000以降の公開属性であり、拒否される旧環境では既存のnative表示を維持して起動を失敗させない（[Microsoft DWMWINDOWATTRIBUTE](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute)）。native glyphの非アクティブ色・hover／pressed色・hit test・UIA／close経路は変更しない。WM_NCACTIVATEの偽装やボタンの独自描画で常時activeに見せない。草案の灰色化への対応は背景の不一致を解消することで行い、OSが示す非アクティブ状態は残す。

所有するneutral windowへfocusを移した可視baselineでは、caption背景がRGB(43,43,43)だった。同じHWNDへの属性設定でRGB(0,0,0)になり、最終exeでも96／192 DPIの通常windowと192 DPIの最大化で黒を確認した。ボタンboundsは96 DPIで(487,0,633,30)、192 DPIで(976,0,1268,57)のまま。赤いclose hoverとnative click終了を確認。DwmGetWindowAttributeによるこのset用属性の取得は基準機でも失敗するため、getterの成功や失敗を色の適用／対応可否の証拠にしない。色は可視画素で検証する。左右padding／bar下端の間隔、全DPI比率・Windows 10の外観は別の未完事項として維持する。

## U01/U08: fullscreenのmonitor移動と復元（2026-09-11）

Win+Shift+Left／Rightで移動する全画面windowは、移動先monitorの全領域に収める。可視試験でOSのWM_DPICHANGEDは正しい移動先矩形を通知していたが、winit 0.30.13が旧client寸法のまま位置変更を再要求し、逆方向のDPI通知を招いた。runtimeは通知の提案矩形からmonitorの実rcMonitorを取得し、そのWM_DPICHANGED処理中だけCellへ保持する。同期WM_WINDOWPOSCHANGINGの座標／寸法をその矩形へ合わせ、NOMOVE／NOSIZEだけを外してwinitへ渡す。winitのmonitor追跡・DPI通知・activation／Z順は維持する（[Microsoft WM_WINDOWPOSCHANGING](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-windowposchanging)）。ネスト終了で以前のCell値へ戻し、callbackがsubclassを外してもstateが残るよう一時Rcを保持する。通常移動・最大化・描画ごとのresizeには適用しない。

NativeCaption::set_fullscreenがcaption stateとwinitのborderless切替を一体で所有する。入る前の論理client寸法を記録し、終了時はwinitの配置復元完了後に実client／outer差から一度だけ寸法を整える。SetWindowPlacementとDPI換算による二重拡大を残さない。最大化状態の退避／復元はappが既存どおり担当し、captionの切替は最大化解除後に行う。元のwindow配置へ戻る規則は維持するため、全画面中の最後のmonitorへ通常windowを移す新仕様ではない。新device／dependency／per-frame補正は追加しない。

可視baselineは左2560×1600からprimary1920×1080への移動で(-1136,0)、2560×1600のままとなり、反対方向でも旧1920×1080が残った。移動だけの中間修正では全画面終了時に1280×960が2560×1920へ倍増した。最終Releaseでは3台を両方向に2周し、各monitorの原点／寸法／DPIが一致。元の1280×960へ復元し、位置だけを96 DPI側へ動かした640×480で編集済み映像48,140画素が一致。最大化→全画面→別DPI→最大化復帰とguardのmouse取消／破棄も確認した。非表示native試験はpending proposalの補正／解除・flag保持、実monitor移動と論理復元寸法を検証するが、それだけでは旧可視OS shortcutの不具合は再現しない。全DPI比率・全media・長時間移動／drag latency・UIA／IME／styleの認定は継続する。

## U01/U08: custom captionのDPIサイズ保持（2026-09-11）

通常の復元windowは、DPI変更前のclient寸法を旧DPIから新DPIへ倍率換算する。WM_NCCALCSIZEで通常枠をclient化しているため、winit 0.30.13のWM_DPICHANGEDが加算する標準枠の余白は実際のclientに不要である（[Microsoft custom frame](https://learn.microsoft.com/en-us/windows/win32/dwm/customframe)、[WM_DPICHANGED](https://learn.microsoft.com/en-us/windows/win32/hidpi/wm-dpichanged)）。runtimeのcaption subclassで前回DPIを保持し、winitの状態更新・ScaleFactorChanged通知・提案位置を通した後、実測client／outer差を使って寸法だけを補正する。位置・Z順・activationは変えず、描画ごとの補正や別deviceは追加しない。appはInnerSizeWriterで別の寸法を要求しない現行契約を前提とし、winit更新時はこの境界の回帰を再確認する。

DPI記録はfullscreen／maximizedでも更新するが、それらの寸法へ通常windowの倍率補正は適用しない。DWM frameは通知処理後に更新する。同threadのCellと同期native呼出しに限定し、COM／HWNDはappへ渡さない。

旧native回帰は1920×1152に対し1946×1223で失敗。修正後は3台を2周し、960×576→1920×1152→960×576、作業領域とgrab位置を保持した。異なるDPIのmonitorがない場合は専用のskip理由を報告する。可視の生成動画は3周で640×480→1280×960→640×480、往復前後の編集済み映像48,140画素が一致。96／192 DPIそれぞれで最大化／fullscreenからの復元寸法も確認した。その状態のまま別DPIへ移す操作は上記fullscreen節で追加対応した。全DPI比率／全media／latency・資源は未認定。

## U08: drop先monitorの作業領域とDPI（2026-09-11）

画面端ではgrab位置の完全一致より、新windowの操作領域の可視性を優先する。tab／filmstripの要求はrelease点と論理grab offsetを別に保持し、sourceのUI densityでrelease点を物理screen座標へ変換する。monitorはwindow原点や矩形の最大重なりではなく、release点で選ぶ。runtimeの同期read-only境界でMonitorFromPoint／GetMonitorInfoWのrcWorkを取得し、負座標とtaskbar領域を扱う（[Microsoft MONITORINFO](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-monitorinfo)）。native handleはappへ渡さない。

hidden windowを選択monitor内へ一度配置して実際のDPI／サイズを確定し、移動先DPI×UI zoomでgrab offsetを計算し直す。client／outer差を除いて作業領域へclampしてからmediaを公開する。通常位置ではoffsetを保持し、端では位置だけを補正する。既存windowの手動移動・サイズ／最小サイズ規則は変更しない。window自体が作業領域より大きい軸は左／上へ揃えるため、その場合に全操作部が収まる保証はしない。

可視の右端dropはclient (2833,231)、幅960で右端3000を793px超えた。修正後は(2040,231)へ収まり、閉じるボタン位置(2978,246)のOS hitを確認。左の192 DPIへは実cursor (-2220,277)からclient (-2420,132)、1946×1223を作業領域内へ配置し、200pxの横grab offsetを保持。主monitorのfilmstrip右下dropは(960,456)、960×576でtaskbar上の1032に収まる。大きい表示の4色点と、96 DPIの640×480へ揃えた編集済み48,140画素／未編集112,572画素も確認した。

位置clamp時点で別件として確認した960×576→1946×1223→989×651のサイズ増加は、上記custom captionのDPI補正で対応した。ここに記録した大きい寸法は補正前の測定値である。全DPI比率／全media／HDR／UIA／IME／latency・資源の認定ではない。

## U08: filmstripから開くwindowの位置（2026-09-11）

filmstripの外dragは元ファイルの独立Openであり、tabのlive state移送ではない。release点からカード内のgrab offsetを引いた、浮遊カード左上のsource-client座標を要求へ保持する。tab分離と同じsource density／client原点の変換とclient／outer inset補正を共用し、新しいhidden HWNDを配置してからmediaを開き、成功後に表示して元filmstripを閉じる。要求のfolder generation／元tab／media instance・modal・重複拒否と、失敗時の元window／filmstrip保持は維持する。非有限座標はqueueへ入れない。新windowのサイズ規則と、tab以外を既存windowへ結合しない契約は変更しない。

可視のrelease (973,246)に対し、旧実装は既定(104,104)へ開いた。修正後はカード内grab (60,40)からclient (913,206)へ開き、foregroundも移る。元の未保存90度回転動画48,140画素と独立した未編集動画112,572画素が各参照と完全一致。生成PNGの均一色144点、無音WAVのPlaying→Ended、元動画の保持も確認した。3幅×3密度のカード座標と非表示nativeの画像／動画／音声配置を回帰検証する。生成素材・単一monitorの証拠であり、mixed-DPI／monitor端／全codec／latency・資源／全UIAの完了とはしない。

## U08: タブ分離時の位置保持（2026-09-11）

外dropにはrelease座標と、先頭slotへ移した時のgrab offsetを引き継ぐ。sourceのclient原点とUI densityから新windowのclient原点を物理座標へ変換し、hidden HWNDのclient／outer差を補正して、state移送と表示の前に配置する。sourceの元tab indexやscroll量は新windowの位置へ持ち込まない。位置取得を含む初期化の失敗時は既存のrollback経路で新windowを片付け、元tabを保持する。結合側のOS topmost root照合とgap選択は変更しない。

可視の生成動画で、release (973,246)に対し既定位置(234,234)へ開く旧挙動を確認した。修正後の先頭tabはgrab (100,15)を保つclient原点(873,231)、2番目tabはslot間の2pxを除いたgrab (98,15)に対応する(875,231)へ開く。未保存90度回転・foregroundと、640×480へ揃えた映像48,140画素の一致を確認。所有する別processの空windowで結合先を覆うと新windowへ分離し、覆いを外すと結合する。tab tooltipが重なったcaptureは全面画素一致の証拠に使わない。これは単一monitorでの確認であり、mixed-DPI／monitor端／全media／遅延・資源matrixは未完。

## U08: 可視の結合操作と末尾の空白drop（2026-09-11）

incoming tabのdrop領域は、実際のtab stripに加えて、その右のnative window-drag空白まで含める。空白では末尾gapを選び、挿入線は可視stripの右端へ置く。caption controlsの予約幅は除外する。これはdrop判定だけの拡張であり、通常のnative drag hit region、local tab並べ替え、tab幅／scroll領域は変更しない。空白でのincoming hoverは端scrollを開始せず、既存のclipped strip内の端scrollは維持する。media／logo／caption／modalと古いlayoutの拒否を維持する。

所有する二つの可視640×480 windowで、修正前はlast-tab右12px前後の空白が拒否され、tab本体へのdropのみ成功することを再現した。修正後は同じ空白で挿入線・末尾追加・destination foreground・未保存90度回転の保持が成立する。media領域のdrop拒否、Welcomeへの戻し、通常空白dragによる50×30pxのwindow移動も確認した。これはmixed-DPI／全media／遮蔽matrixや遅延測定の完了を意味しない。

確認記録の訂正：Welcomeへ戻したEOF動画が黒いという先の判定は、保存画像の誤読だった。既存の初回／二回目returnと入力なしcaptureは、移動前の映像領域48,140画素と完全一致する。診断コードを除去した同じ実装のReleaseでも、可視二window間の3回の往復後に同領域の完全一致を確認した。この観測に対する表示処理の修正は不要であり、試作したcaption順序／surface class／swap effect変更やGPU readbackは残さない。画面の見た目だけで黒画面と断定せず、保存画素を照合する。可視の分離、遮蔽、mixed-DPIと性能の残件は維持する。

## V02/U10: 単枚動画プレビューの補助デコーダー利用（2026-09-11）

Seek／tabの初期単枚とvideo filmstripは、sheetと同じworker-owned software decoder／filter helperを一つのtargetで使う。通常時のCLI起動を省くが、単枚request間でinputを保持する変更ではない。縮小後のRGBAを一回PNGにして既存cacheへ渡す。非取消エラーは従来CLIへfallbackし、取消と生成中のsource metadata変更をpublish前に拒否する。画像・音声の生成経路、再生session／D3D11、共有cacheの件数／容量は変更しない。

動画単枚は`width × width`内のaspect-fit、`reset_sar=1`、paddingなしとする。従来の幅だけのscaleでは縦長入力の高さが膨らみ、SARも表示ピクセルに反映できなかった。filmstripは既存240×160のpadded fitを維持する。両方RGBAを明示し、video専用v4 keyへ分離して旧寸法／色のcacheを再使用しない。単枚keyはtarget nanosecondsを含め、従来のmillisecond切り捨てによる別targetの衝突も避ける。画像のthumbnail-v3／filmstrip-image-v4は維持する。

TSで音声が映像より先に始まる生成素材では、従来CLIのtimestamp rebasingがvideo開始をzeroにし、format originに対する指定位置と異なるframeを選ぶケースを確認した。独立ffprobeのformat start、CLI `-copyts`、timestamp `select`による小素材の全走査参照ではnativeと画素一致する。終端以降は独立reverse参照を用いる。参照の全走査はテスト限定であり、productionへ追加しない。fallback自体の旧timestamp制約や全形式の精密Seekは未解消である。

Release測定（PNG生成・disk登録を含むcache miss、同じRGBA filter）：小さな32×96／SAR 2:1素材はCLI 53.5022ms、native 3.0056ms。1080p／GOP 180の4.8秒位置はCLI 91.345ms、native 96.7887msで、この長GOP条件では改善なし。後者memory hitは80.6µs。OS cacheをflushしたcold-storage測定、UI end-to-end latency、全process peak／全codec／HDRの保証ではない。可視の所有素材で回転／SARを保つfilmstrip・scrub／tabを確認し、240×160の新しい単枚cacheも生成した。初期単枚が画面に出る瞬間の時間は未計測である。

## V02/U10: sheet内の補助デコーダー共用（2026-09-11）

従来のコマごとの子process生成を、worker内で所有する一つのFFmpeg input／software decoder／filter graphへ置き換える。各sampleの前に既存のorigin／TS-aware seekを行いdecoderをflushし、最初のtarget以降のframe、または実video終端の最後のframeを使う。decoderとfilterのthread指定は1、decoderのmax_pixelsは128 Mi pixelとする。これは再生用session／D3D11 deviceの共用や変更ではなく、補助処理自身の再利用である。GOPの先頭からの再decode自体はまだ残る。

scale／pad／RGBA変換前にsourceのquarter-turn／reflectionを適用する。graphはwidth／height／pixel format／time base／SAR／color space／range／orientationをkeyに再利用し、異なるframe propertiesでは再構成する。native frameはworker内に留め、外へ渡すのは一コマの縮小済みowned RGBAだけである。PNG往復はシート全体を既存cacheへ登録する一回のみ。従来PNG encoderの自動RGB24選択とRGBA経路には数LSBの差があったため、sheetとCLI参照を明示RGBAへ統一し、cache variantをv2へ更新する。比較の許容誤差を広げる変更ではない。

packet／decoded frame／filter／publishの境界で取消を確認する。取消後はCLI fallbackを開始せず、完成していないsheetをcacheへ登録しない。native処理の非取消エラーでは従来の単枚生成で全sampleを作り直し、既存の失敗診断とsource変更検査を保つ。これはlibav内の一回のI/O／codec呼出に期限を保証するものではない。既存の共有64件／16 MiB memory、64 MiB disk、appの二枚LRUと最後のscrub sampleの参照保持は変えない。

H.264、B-frame付きMPEG-4、VFR、SAR／回転／反転、full-range BT.709、alpha付きQTRLE、default video stream、MP4／MKV／TSの非zero origin、長い音声末尾を比較する。RGBA指定の独立CLI参照と画素一致を要求し、45度display matrixによる実fallbackと取消も確認した。旧TS input-seek参照が最後のframeを返せない場合は、小さな生成素材を先頭からscan／reverseした独立参照で確認する。新経路自身の結果を参照画像にはしない。

観測値：小さなH.264のdebug full-sheetは従来1.126秒→新0.260秒、新Release full-sheetは26.34ms。1080p／30fps／6秒／GOP 180の16コマ取得は同じRelease比較でCLI 1.327秒→shared 0.759秒（sheet PNG登録前）。これらは生成素材の個別測定であり、UI応答、全codec／HDR、CPU総量／process treeのpeak memory、mixed-DPIや長編全体の性能保証ではない。

## V02: compact seekのメイン映像scrub（2026-09-11）

可視比較で通常tooltipがdrag時に抑制される欠落を確認し、compact seek自身が保持するdragの間だけサムネイル／時刻tooltipを強制表示する。単なるpress・他widgetのdrag・取消後は従来のhover判定に戻す。画像のcompact seekにも同じ規則を使う。実マウスでSAR 2:1の320×180素材に90度回転→35度自由回転→crop→左右反転→200×232 resizeを重ね、Fit、Cover、右drag panを保持したcompact scrubの配置と実映像を比較した。別のdisplay-matrix 90度／SAR 2:1素材でも向きと比率を確認し、source orientationを二重適用していないことを確認した。低解像度補間・輪郭の差は残り、HDR／全編集順序／全素材の品質認定ではない。

以下は従来の「本画面scrubを含めない」という段階制限を更新する。動画のcompact seekで横方向dragと判定した時だけsession／clockを一時停止し、既存sheetまたは単枚fallbackのtextureをメイン映像にも描く。press／hoverだけでは停止しない。上方向dragのtimeline展開、画像移動、音声timelineは変更しない。drag中は実Seekを発行せず、releaseで通常Seekを一回だけ呼ぶ。元がPlayingなら通常SeekのEOF／trim停止規則を評価後に再開する。Escape・focus喪失・別command／source／tab・graphics recoveryではSeekなしで取消し、元の再生状態へ戻す。

eguiのdiscarded passを跨いでreleaseを保持するCommitting状態と、Seek後の新frameまで最後のpreviewを保持するAwaitingFrame状態を分ける。runtimeのvideo_refresh_pendingが解除された時に実映像へ戻し、低解像度meshの表示を実Seekのpresentation計測に数えない。別sheetが未到着なら最後の低解像度sampleを保持し、一枚もない時は通常の停止映像を残す。編集後時刻からsourceへの写像と世代管理は従来経路を使い、追加decoder／device／原寸readbackを導入しない。

sheetはsource orientation／SAR適用済みである。appでは元の向きの画素座標から、cropの凸多角形clip、直角回転／反転、resize、square-pixel化を伴う自由回転を順番に適用し、補間したUVを同じtextureへ参照する。黒い余白・偶数canvas paddingと、既存Fit／Cover／zoom／panを保つ。これは低解像度の概形確認であり、正確なframe時刻・resample filter・HDR色・export品質を再現するものではない。新たに保持するのは最後のsampleのtexture参照一枚と小さな頂点列である。

停止／再生中の固定位置、release一回、discarded pass、Escape／focus取消、終端停止と新frameへの復帰を実session回帰で確認する。所有する可視960×576 window／生成40秒MPEG-4素材でも停止・再生からの実mouse dragと取消を確認した。長GOP／全codec、編集済み素材の可視比較、混在DPI、cold latency／peak負荷の認定は残る。

## V02/U10: 有界の動画サムネイルシート（2026-09-11）

source durationからmax(20, ceil(seconds/5))個の区間を作り、その中央をsampleする。1枚16コマ、4×4の960×640 RGBA、各コマ240×160のFit＋黒paddingとする。source path／metadata／duration／sheet index／versionをkeyに、既存PreviewCacheの同key生成集約・取消・64件／16 MiB memory／64 MiB disk枠を共用する。既存のbest-stream／TS Seek／向き・SAR／末尾frame fallbackを使い、各sheetの最大16回の子process取得を逐次実行する。途中取消では未完成sheetを公開しない。原寸の再生decoderやGPU deviceは追加しないが、補助codec／processの負荷自体はある。

通常動画の現在のsource位置をUIの描画時に先読み要求する。UI threadではmetadata／file I/Oをせず、既知durationからsheet位置を計算する。専用latest-only workerは一件だけ生成し、Seek hoverが別sheetを求めたら優先する。現在位置の先読みで進行中のhover要求を取り消さない。Seek側はcontext内に最大2枚のLRU、失敗keyは32件まで保持し、同sheet内のhoverでは同じtextureとUVを使う。slotのtexel中心をUV端とし、rounded-rectのAAが隣cellへUVを広げないようmesh経路で描画する。source切替／timeline toggle／graphics recoveryで世代更新と取消・texture破棄を行う。通常video timeline内にはhover previewを表示しない。

初期応答をsheetの全16コマ完了まで待たせず、Seekは従来の単枚取得を併用し、sheet完了後に置換する。tab hoverも共有memory sheetがあれば直接UV表示し、なければ単枚を先に通知してsheetを続ける。duration／sheet生成失敗時は単枚経路を残す。tab位置は編集後の現在時刻をsourceへ戻してからsourceのsampleを選び、Seekのsource sheetと整合させる。tab hoverのGPU textureは別所有の一枚であり、SeekとのCPU／disk共用をGPU texture共有と表現しない。先読みは現在位置のsheetだけで、長編全体の事前走査は行わない。

固定H.264素材のdebug検証で16コマ生成約1.119秒、4コマの終端sheet約0.327秒、memory clone取得約0.56～0.59msを観測した。これはUI表示遅延の保証ではない。実画素と単枚reference、density／端／UV、同textureで16位置を描画して追加uploadなし、世代／優先度／2枚LRU／texture上限を確認する。所有する可視960×576 windowと生成40秒MPEG-4素材では停止中hoverの非Seek、前半／後半thumbnail、クリックSeekとtab hoverを確認した。software decode条件の一例であり、長GOP／全codec／HDR／混在DPI／資源peak・本画面scrubは未認定・未完。

## I03/U10: 保存したサムネイルを原寸前のpreviewへ戻す（2026-09-11）

直接生成した静止画のcache PNGに、向き補正後の元width／heightを保持する。IHDR直後のprivate ancillary chunk tvSzを20 bytes（length=8、type、各u32 big-endian寸法、type＋dataのCRC32）として追加し、元画像や書き出し画像へは追加しない。既存filmstrip-image-v4 keyとPNG cacheを継続し、sidecar／新DB／追加依存は作らない。load_or_generateの生成／disk hitでこの寸法を共有memoryへ戻す。

cached_imageはmemoryの既知元寸法を優先し、なければworker側で該当cache PNGを最大1 MiB＋超過検出1 byteだけ読む。codec呼出前にsignature／IHDR／専用chunk位置と長さ／CRC、preview幅1..240・高さ1..160、元寸法の非ゼロ・checked RGBA計算と128 MiB上限を検査する。通常のPNG pixel decode・取消・source key再照合を通したものだけmemoryへ登録する。I/O／PNG decode中にmemory mutexを保持せず、生成leaseの取得待ちはしない。CRCは破損検出でありcacheの暗号学的真正性保証ではない。

原寸loaderとappの既存preview workerはこの元寸法付き画素をそのまま使い、原寸のgeneration／成功・失敗時の退役を維持する。新worker・原寸再decode・GPU texture経路・編集座標を追加しない。旧cache／不正寸法／破損／大きすぎるfileは原寸用previewとして採用せず、削除しない。寸法のない旧PNGはfilmstripとして使い続け、元画像が後で供給された際の既存寸法upgradeも維持する。同じsource keyの後着PNGに寸法がなくても、memory側で既知になった寸法は消さない。全旧cacheを起動時に走査／再生成する仕様ではない。

4形式のdirect thumbnailのmemory／fresh cache再利用、EXIF8向きの元寸法と全縮小RGBA、legacy／CRC／ゼロ／overflow／上限／不正preview寸法／truncation／1 MiB超過・取消／source変更を回帰確認する。原寸workerを制御して止めた実PNG試験では、fresh cacheから先にpreview通知が届き、再開後は元の全RGBAと一致してpreviewが退役する。可視UI／cold-storage latencyと全UX台帳は未完で、これを原寸decode自体の高速化とは呼ばない。

## U10: キャッシュ済みカードを生成待ちより先に公開（2026-09-11）

filmstrip／recent等のPreviewLoaderは、最大64件の要求を既存worker上で二段階処理する。先にsource stamp付き共有memoryを調べ、取得済みcardを即時通知する。残ったmissだけを元の要求順に生成する。表示位置・Shell順・選択は変更せず、worker／GPU texture／cache予算を増やさない。sourceのmetadata読取は必要だが、この先行lookupでmedia probe／decode／disk PNG読込・生成lease待ちは行わない。動画・音声はdurationもmemoryに揃う場合だけ先行公開し、欠ける場合は既存経路で補完する。

cache hitの直後もsource stampと取消を確認し、結果公開は生成経路と同じmailbox generation／closed検査を通す。スクロール・close中にlookupが終わっても古い結果や通知を公開しない。遅い未生成一枚が後続のwarm cardまで待たせることは避けるが、slow metadata I/O／未生成card同士／diskのみのhitの待機まで解消する契約ではない。

## U10: 静止画サムネイルの直接復号（2026-09-11）

画像filmstripでmemory／diskと既存JPEG・BMP専用previewが使えない場合、既存の静止画専用decode_image_for_prefetchを原寸RGBA上限128 MiBで試す。原寸の向き補正・depth／alpha変換・読取境界取消を再利用し、所有するRGBAをcopyせず画像bufferへ移し、nearestで240×160内へ縮小する。小画像は従来filmstripと同様に拡大する。小さなPNGへencodeして既存load_or_generateのmemory／disk・同key生成lease・取消をそのまま使う。PNG往復自体を除いた経路ではないが、対応静止画のFFmpeg別process／入力準備を除く。

GIF／APNG／animated WebP／AVIFは静止画経路でskipし、上限超過／失敗も従来FFmpegへ戻す。変更中sourceの生成結果はcache key再照合で拒否する。128 MiBはこの直接経路の原寸RGBA判定であり、codecの一時buffer・他worker・fallback processを含む全体peak上限ではない。原寸cacheには登録せず、foregroundの原寸decode・JPEG/BMP先行表示の上限・GPU texture・編集／保存を変えない。原寸読込と同時に別workerが同sourceを復号する可能性は残る。

生成6000×4000 PNG・warm file-cache・各7回Releaseの最終API比較では、従来CLI＋PNG cache取得の中央値514.1387ms、新filmstrip取得140.5008ms。新経路のkey／先行preview試行／stamp照合／PNG保存も含む。fixture warming・空cache作成・assertion／cleanupは除く。480×320のPNG／alpha BMP／WebP／JPEGは全240×160 sampleのRGBAを原寸decoderと照合し、budget境界／途中取消・disk再利用／GIF fallback／不正入力を回帰確認する。nearestは縮小品質より応答を優先するサムネイル用で、従来FFmpeg縮小との全画素一致やcold／可視UI／全素材・peak性能を保証しない。

## U10/I03: 未訪問静止画のサムネイルへ高速previewを共用（2026-09-11）

filmstripの画像要求は既存memoryを優先し、disk cacheがない時だけforegroundと同じprepare_image_previewを試す。大きなJPEGの1/8復号と24bit非圧縮BMPの疎な行読取、240×160以内・原寸budget・取消・source stamp・同key生成leaseを共用する。得られたRGBAと元寸法をhost共有64件／16 MiBへ入れ、PNG encode／disk保存／FFmpeg別processは追加しない。tab hover／recent／画像seek等のfilmstrip consumerと、その後の原寸読込のpreviewが同じ画素を再利用する。新しいworker・原寸decode・GPU所有は作らない。fast生成結果はmemoryのみのため、そのcacheが失われれば再生成する。

既存diskがあれば従来の読込を優先し、fast非対応／失敗／別generatorが所有中なら既存load_or_generateへ戻る。同keyの実行中生成は既存leaseで集約し、別key・取消は独立する。小画像／PNG／WebP等の従来FFmpeg fallbackとdisk永続化は残す。画像の先頭frameだけは入力-ss 0を付けない。固定FFmpegのimage2 JPEGはこの不要なseekで唯一のpacketを失いNoFrameになったためで、非ゼロのanimation位置と動画のseek／stream選択は変更しない。

生成6000×4000 JPEG／BMP、warm file-cache・空のpreview cacheからRelease各7回で測定。先頭seekを修正した従来CLI＋PNG cache経路の中央値90.7328／280.3775msに対し、新filmstrip取得は13.7722／1.2578ms。cache key計算／生成／結果取得を含み、fixture読取とcache生成・後片付けは計時外。旧NoFrame経路の速度比較でも、cold-storage／可視UI／全画像品質の保証でもない。代表色／alpha・元寸法・共有・既存disk優先・小画像fallbackを回帰確認し、原寸decode／編集／保存は変更しない。

## I03: 実行中の静止画先読みをforegroundへ引き継ぐ（2026-09-11）

ImageLoaderの先読みjobに固有のlease・現在のpath・要求generation・実行状態・取消tokenを保持する。新要求に実行中先読みのpathが含まれると、そのページの仕事を新generationへ引き継ぐ（readingの先頭以外も対象）。foreground workerは既存cache／小previewを確認後、該当原寸の完了だけをCondvarで待ち、元のArc<DecodedImage>をcacheから受け取る。UI thread・UI mailbox lockを保持して待たず、追加の原寸copy／decoder／workerやbyte予算は作らない。未開始のqueued仕事は採用しない。採用後は処理中のページだけを終え、古いbatchの後続ページへ進まない。

別path・空要求・closeは取消または所有権の無効化で待機を解き、新要求を古い先読みの完了に従属させない。同pathの再要求でも最新generationだけへ公開する。lease終了は成功／失敗／非対応／source変更のどの経路でも通知し、古いleaseの終了で後続jobを消さない。原寸cache登録後は縮小thumbnail作成を待たずにforegroundを起こす。queued leaseの破棄とcache evictionをUI mailbox lock内で実行しない。

先読みは静止画のみ・256 MiB／10件、source stamp照合と取消を維持する。通常表示は移動方向の隣画像一枚、readingでは隣見開き全体をShell画像順で選び、既存worker一本で逐次処理する。最大10入力の重複pathを除き、成功した原寸（warm hitを含む）のbyte数をbatch残予算から引く。後のページのために先に準備したページを追い出さないよう、warm hitもLRU末尾へ移す。失敗／非対応／残予算超過は飛ばし、予算がゼロなら終える。空batchは待機を解除してqueued仕事も取り消す。採用後もforegroundの残る512 MiB budgetへ照合し、入り切らない結果はTooLarge、先読み失敗／非対応は通常decodeへ戻す。cold／OS read強制取消や全window間での原寸job共用は未実装。

実PNGの読取途中で要求を切替え、全画素一致とforeground decode呼出0を回帰確認する。生成6000×4000 PNG、warm Release各7回、同じ100回目の読取／取消確認境界から再開する比較では、明示取消して再要求する基準の中央値118.4876ms／再decode1回に対し、引継ぎは48.1838ms／0回。計時は結果取得で終了し画素比較は含めない。同じ最終codeで強制再始動と採用を比較する制御試験であり、旧exe対比・可視UI end-to-endや任意の移動時刻の性能保証ではない。

## I03: 静止PNG／WebPのdecoder再利用（2026-09-11）

foregroundとprefetchは、内容推定済みreaderをPNG／WebPの型付きdecoderへ移し、そのinstanceでanimation判定後に静止画も復号する。static_frameはreaderを再度decoder化せずImageDecoderを受け取る。非対応の別backendやPNG先行decodeは追加しない。静止画は従来のImageReader既定limitsを適用し、PNGはheader parseから同じ既定512 MiB上限を使う（APNGのheaderにも適用する）。原寸RGBA budget・EXIF／alpha／16bit→8bit変換・取消・animation first-frame通知を維持する。

生成した16 MiB tEXt付き小PNGは、旧foregroundが4,107回、再利用後のforeground／prefetchが2,058回のread／seek／取消確認を行う。固定版pngの8 KiB buffered readに基づく上限回帰で二重header parseを検出し、全RGBAも一致する。8種類の8／16bit・RGB／gray・alphaとEXIF8向き、予算を両経路で検証する。通常24MP PNGのRelease内訳はheader約0.1ms／画素復号約105ms／RGBA変換約15msであり、header再利用を普通のPNG全体の大幅高速化と扱わない。

同じ24MP PNGをFFmpegのPNG decoderへ直接packetとして渡す追加測定でも、7回Releaseの総中央値約220ms（既存約118ms）、全96,000,000 bytes一致だったため採用しない。通常demux経路の測定とは分けて記録し、PNG等の初回段階表示、進行中先読みとforegroundの重複、UI latency／cold／資源と全UX台帳を残す。

## I03: 非圧縮BMPの疎な先行表示（2026-09-11）

JPEGと同じforeground preview生成枠へ、内容判定による24bit BI_RGB／40-byte BITMAPINFOHEADERのBMPを追加する。[file headerの画素offset](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapfileheader)と[上下方向・4byte stride](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/ns-wingdi-bitmapinfoheader)を検査し、240×160以内の各出力行の中心に対応する原寸scanlineだけを読む。横方向も中心のnearest sampleとし、BGRから不透明RGBAへ変換する。原寸全体の一時allocationや追加worker／process／unsafeはなく、原寸decode・編集・保存はimage crateのまま。

4,194,304画素以上かつ原寸budget内だけを対象にし、row allocationは1 MiB、総sample row読取は32 MiBまで。offsetと全画像末尾を実file長へ照合し、破損header／非対応のpalette・bitfields・alpha・profile形式・small／budget超過は先行previewを諦める。既存のcancellable reader、共有cache／source stamp／generation／mailboxと原寸成功・失敗時のplaceholder退役を共用する。疎なnearest表示のaliasingを許容する一時表示であり、原寸画質を置換しない。cold-storageでの多点Seek負荷は未測定。

生成6000×4000 BMP、warm file-cacheのRelease各7回で先行なし原寸中央値75.8184ms、先行取得0.8922ms、先行あり原寸完了75.7522ms。総時間の差はばらつきの範囲であり、原寸高速化を主張しない。sample行は2,880,000 bytesで原寸72,000,000 bytesの4%。portrait／landscape・top-down／bottom-up・paddingの全sample画素、原寸不変、取消／上限／alpha BMPへのfallback、loader原寸前通知／terminal lifecycleを自動検証。Computer Use接続失敗のためこのcheckpointの可視表示確認は未実施。

PNG／BMPを既存FFmpeg通常decode_fileへ渡す代案は、同じ生成24MPのRelease中央値で約119→434ms／74→199msとなったため採用しない。この測定は通常demux／変換込みの経路であり、FFmpeg codec単体や他の構成の一般的な性能判定ではない。PNG等の初回段階表示、全UI end-to-end／資源評価と全UX台帳は継続する。

## I03: managed textureの画素共有と行転送（2026-09-11）

vendored egui-directx11はImageDataのArc<ColorImage>をmanaged textureのCPU backingとして保持する。全texture生成では画素Vecをcloneせず、同じimmutable画素を同期CreateTexture2Dへ渡す。partial更新だけArc::make_mutで共有元を保護し、唯一のownerなら同じallocationを更新する。free／置換でbackingを解放する。これはGPU textureのwindow間共有、decode RGBAの直接upload、常駐backing自体の除去ではない。

partial更新は矩形と画素数を検証してCPU backingへ反映し、WRITE_DISCARD後に全行を復元する。[MapのRowPitch](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/ns-d3d11-d3d11_mapped_subresource)に従い行頭を計算し、CPU側のpacked幅と同一と仮定しない。実GPUの3×3 textureで旧実装の下2行がゼロになることを再現した。native pointerは従来のrenderer内に留め、MapからUnmapの間だけ借用する。device、alpha変換、画素format、sampling、blendは変えない。

6000×4000の全texture生成を同じoffscreen実GPUで各7回測定し、Release中央値30.540→22.692ms。96,000,000 byteの一時コピー／allocationを除去したことはArc同一性でも確認する。ドライバー初期化・allocation/cacheの影響があり、UI全体の速度や常駐memory削減を意味しない。WARP／実GPUの7幅、共有元不変／COW／unique backing再利用・不正入力・freeを回帰化し、既存のsampling／inversion readbackも維持する。

## I03: 古い画像の復号を読取境界で取り消す（2026-09-11）

foregroundと静止画prefetchの既存generation判定を、Fileを包むRead／Seek adapterへ渡す。外側のBufReaderで小さなcodec読取をまとめ、大きな一回のreadも64 KiBまでにする。取消時のI/O errorはcodecが包み直す場合があるため、decode結果の返却前にgenerationを再検査し、既存のCancelledへ統一する。画像内容による形式推定と拡張子fallbackを併用し、animated AVIFのように推定だけで判定できない入力も維持する。

静止画はdecode後・向き変換後にも検査して不要な後続変換を省く。thread／cache／画素予算は増やさない。実行中のOS readやcodec内部のメモリ内計算を強制中断するものではなく、JPEGなどは次の検査まで待つ。AVIFのFFmpeg処理は既存のframe境界取消を維持する。通常画像の初回低解像度表示、cold-storageとUI end-to-end latencyは別の未完事項とする。

## I03: 大きなJPEGの初回低解像度preview（2026-09-11）

foregroundの原寸cache miss時に、既存previewがあれば再利用し、なければJPEGだけ同梱FFmpegのMJPEG decoderへ圧縮packetを直接渡す。独立process／demux走査／追加workerは作らない。低解像度復号は[lowresとcodec上限](https://ffmpeg.org/doxygen/trunk/structAVCodec.html)を確認して1/8寸法を要求し、一件・一threadのsoftware decoderとscalerをworker内で所有する。画像の原寸decode・編集・保存やD3D11 deviceを置換しない。

内容からJPEGと確認し、4,194,304画素以上かつ残る原寸budget内を対象とする。先行処理の圧縮入力は32 MiBまで、出力はEXIFの8向きを適用後240×160以内のRGBA。元の向き補正済み寸法は縮小画素と別に保持し、通常／readingの配置・倍率を変えない。低解像度frameが期待した寸法でない場合やcodec失敗は先行previewだけを諦め、従来の原寸経路に診断を任せる。取消は読取・処理境界で検査し、native decoder内部を強制中断しない。

PreviewCacheの既存64件／16 MiB枠とsource stamp、ImageLoaderの世代付き一件mailbox・ImagesReadyを共用する。同keyの生成枠を非blockingで取得し、他window／filmstripが生成中なら重複生成も待機もせず原寸へ進む。生成中にUI/cache mutexを保持しない。原寸が先に完了した場合はpreviewを表示せず直接原寸へ進み、失敗／取消／closeでもplaceholderを退役させる。先読みだけの原寸decodeはこの追加の縮小復号を行わない。

生成6000×4000 JPEGのwarm file-cache・Release各7回では、先行なし原寸中央値42.377ms、先行ありpreview 12.984ms／原寸55.549ms。早い低画質表示と引換えに原寸完了は約13ms遅くなる。この追加decode・圧縮コピー・software負荷を許容し、大きなJPEGの初期応答を優先する。UI end-to-end、cold-storage、全JPEG方式／色管理・資源peakの保証ではない。他の静止画形式、総decode高速化、libjpeg-turbo採否は引き続き測定対象とする。

追加品質検証では2571×1933の生成9種類（通常／progressiveのYCbCr・gray・CMYK、黒版ありCMYK、直接RGB）を、Pillow 12.3.0による独立復号の4代表色と原寸decoderへ比較し、各RGBA channel差5以内・元寸法・縮小寸法・全alpha不透明を確認する。これは任意fixture生成付きopt-inで、通常CIの実行証拠ではない。外部生成不要のgray黒／白／中間階調回帰は通常テストに含む。production経路は変更せず、YCCK・ICC色管理・写真の全画素誤差・可視表示時間は未検証のままとする。

## I03/U10: 静止画先読み結果のpreview共用（2026-09-10）

既存の隣画像一件／256 MiBの静止画先読みは、原寸cacheへの登録後、decoded-cache mutexを解放してから同じ原寸の借用frameをPreviewCacheへ渡す。既存の240×160以内の縮小、source metadata key、host全体64件／16 MiBを使い、追加decode／補助process／disk encodeはしない。原寸cache hitでも元寸法付きpreviewを再供給し、縮小側だけがevictionされた場合に再decodeしない。先読みの静止画限定／元画質／枚数／順序は変えず、animation／AVIFの先読みやvideo sheetを追加したという意味ではない。

画像request generation／closed／取消とsource stampを確認して供給し、closeはmailbox終了前に先読みtokenを取り消す。codec／縮小／preview-cache操作中にdecoded-cacheやUI mailboxのmutexを保持しない。先読みからImagesReady／LoadedImagePreviewを通知せず、表示・編集・GPU textureは変更しない。通常のforeground requestが来たら従来の原寸Arcを再利用し、必要な低解像度consumerは共有previewを使う。実PNGで先読み・alpha／寸法・filmstrip再利用／disk生成なし・原寸同一Arc、制御したworkerで取消／変更／close／失敗／animation拒否を確認する。可視UIの速度とcold-storage資源評価は別途残る。

## I03/U10: 原寸デコードの最初のフレームを段階表示する（2026-09-10）

GIF／APNG／animated WebP／FFmpeg AVIFのforeground decodeで、メモリ予算内の最初のフレームを得た直後に借用RGBAと実際の表示寸法をPreviewCacheへ渡す。既存の240×160以内の縮小・source metadata key・64件／16 MiB共有枠を使い、追加の原寸コピー、別decoder／process、disk encodeは行わない。後続フレームは同じdecoderで続ける。静止画の先行decodeや最初のフレーム自体の待ち時間削減は含めない。

ImageLoaderのmailboxは進行中画像の縮小previewを最大一件保持し、通常のImagesReadyで通知する。原寸の成功／失敗時は未消費previewを退役させ、原寸が先に到着すればそちらを使う。要求更新・closeで破棄し、生成前後にgeneration／file stamp／closedを検査する。appは原寸request generationに加えて既存のpath／未読込位置／image_loading条件を照合し、通常とreadingの同じ一時texture描画へ渡す。previewは原寸成功や編集可能状態を意味しない。途中の予算超過や破損でも原寸の診断を隠さず、表示placeholderを除去する。既存cache-only workerはwarm hitを即時取得するため残す。後段のI03 cache-only記述はこの初回animation通知で拡張する。

FFmpegのconsumer停止に伴う一般エラーより、画像側で判定したCancelled／TooLargeを優先して返す。実生成GIF／APNG／WebP／AVIFでfirst-frame画素と原寸全frame／時間の不変性、予算と取消を検証する。制御したworker順序の検証を、可視windowの速度測定やcold-storage全般の解消とは扱わない。

## U10: host全体のpreview共有と同一要求の集約（2026-09-10）

`WindowHost`が一つの`PreviewCache`を所有し、各ApplicationのImageLoader／filmstrip／recent／tab hover／seek workerへcloneを渡す。既存の低解像度RGBA上限64件／16 MiBをwindow数で増やさず、元windowを閉じても残りのwindowと後から開いたwindowが使える。egui textureは引き続きcontext別に所有し、GPU texture共有を達成したとは扱わない。

同じsource metadata／variant keyの生成は一件にまとめる。生成中keyのleaseを共有し、他のworkerは完了後にmemory／diskを再確認する。別keyは並行に進み、mutexを保持したままdecode／disk I/Oしない。待機中は10ms間隔で自分の取消を確認し、待機側の取消で生成側を止めない。生成側の失敗／取消でもleaseを解放して、残るconsumerが再試行する。foregroundの原寸decodeはこの待機へ入れず、従来どおり縮小画素をmemoryへ供給する。memory hitは生成待ちより優先する。

filmstripの動画／音声等で使うdurationも、同じsource path・size・更新時刻を含むkeyで成功値だけ64件のLRUへ保持する。同時probeを集約し、失敗／取消は保存しない。metadataが変われば別keyになり、古いdurationを返さない。RGBAの上限は従来のままで、durationは小さな別metadata枠とする。一時PNG名にはprocess内の連番も加え、独立cache instanceの同時保存でも一時ファイルを取り合わない。diskが使用不能／busyの場合の画素返却・後の保存再試行という既存契約は変えない。

これは重複decode／probeとwindowごとのmemory cache複製を減らす変更であり、cold-storage全般や可視UIの表示時間を認定するものではない。動画sheet・GPU texture共用・未訪問媒体の先行生成と実時間／資源評価は引き続き未完。

## U08: 起動要求を同じhostへ集約（2026-09-10）

通常起動は引き続き新windowを一つ開く。同ユーザーSID・logon session・canonical executable pathの既存hostがあれば、ファイル／folderの絶対pathまたはWelcome要求だけをそのhostへ渡す。既存tabへ勝手に追加せず、同deviceの新しい非表示windowを初期化してから表示する。起動元のrelative pathは転送前に解決する。異なるinstall場所やユーザー／sessionは別hostとし、すでに独立している旧processのlive stateを奪わない。

runtimeのmessage-only HWNDとsession-local named mutexを使用する。mutexの名前hashは起動競合の調整だけで、転送先はprotocol class／完全title・process executable／SIDを照合する。既定token DACLとWindowsのdesktop／UIPI制約を維持し、メッセージfilterを緩和しない。ネットワークlistener・永続service・path交換用file・async runtimeは追加しない。受信するWM_COPYDATAは不信な入力としてversion／長さ／UTF-16 path構造・絶対pathを検証し、path以外のcommand／編集／native objectは受け付けない。同じdesktop上の任意codeに対する認証境界とは扱わない。

受信threadはpathを所有bufferへcopyしてUI eventへ渡し、window初期化成功のackを最大4秒待つ。起動元は送信結果を最大5秒待つ。起動競合ではreadyを待つが、送信後のtimeout／拒否／終了は「まだ開く可能性がある」として診断し、自動再送や独立windowへのfallbackで重複を作らない。async media decode完了はack条件にせず、通常Openと同じ新window内診断とする。最後のwindow終了時は受信を止め、所有thread上でHWNDを破棄・joinしてからmutexを解放する。process異常終了時もkernel handleの寿命に従いmarkerが消える。

参照: [WM_COPYDATAのbuffer寿命](https://learn.microsoft.com/en-us/windows/win32/dataxchg/wm-copydata)、[message-only window](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features)、[SendMessageTimeout](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendmessagetimeoutw)、[session-local mutex](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-createmutexw)。可視Explorer起動／foreground・mixed-DPI・実install更新との組合せは別の未検証項目とする。

## U08: 同host window間の通常tab drop（2026-09-10）

外dropはtab IDに加えてrelease時のsource client座標をqueueし、hostが同deviceの既存windowへの結合か新window分離かを選ぶ。runtimeだけがClientToScreen／WindowFromPoint／root照合／ScreenToClientを扱い、appはsource／target各contextのpixels-per-pointで物理座標を変換する。別windowに隠れたtab barやpopupの背後へは結合しない。独立processの既存windowへはまだ移送しない。

結合範囲は実際に描画したtab stripと空windowのWelcome領域。通常の挿入gap規則とclipを共用し、hover時は2 logical pxの縦線を表示する。対象windowはactivateせず、端で横scrollできる。source dragの取消・release・対象の閉鎖／modal／overlayでindicatorを解除する。releaseで対象tab列・viewport／density・描画世代とgraphics／modalを再検証し、前節で導入したstage→state移送を実行する。既存host window上でも媒体領域や無効なstripなら移送せず、診断して元tabを残す。どのhost windowもhitしなければ従来の新window分離へ進む。元の未保存編集を再読込や保存確認で置き換えず、最後のtab移送後はWelcomeを残す。

検証はheadlessの複数幅／densityでのgap・indicator・scroll・古いlayout拒否と、所有する非表示HWNDでの実drag action／GPU描画／state移送に分ける。非表示HWNDではOSのhit選択だけを注入するため、可視windowの実pointer capture／occlusion／mixed-DPIを認定したことにはしない。通常の別process入口は上節の起動集約を使い、可視入力と全体の時間／資源評価は未完。

## U08: filmstripからの同host新window（2026-09-10）

通常のfilmstrip外dragは`WindowHost`へ新window要求を渡す。要求には元tab ID・media instance・folder generation・対象pathを持たせ、処理前にfilmstripの表示／modal／overlay／所属を再確認する。重複queueは増やさず、元tab・folder・媒体が変わった要求や閉鎖済みownerは無視する。source deviceが利用でき、pathを正規化・判別できる場合に、同device上の非表示HWNDを初期化して通常のOpen処理を開始する。

これは既存tabの移送ではなく、元ファイルを独立した新tabとして開く操作である。元tab・未保存履歴・保存先・再生位置／sessionは変更しない。window作成前後の初期化失敗やOpen受付前のpath失敗では新windowを破棄し、元filmstripを残して診断する。window作成とOpen受付成功後に新windowを表示して元filmstripを閉じる。初回decode完了は起動受付と区別し、壊れた媒体などの読込失敗は新windowの標準診断に任せる。元の未保存編集を複製したり、読込失敗を理由に元tabを閉じたりしない。

通常のtab分離とfilmstrip新windowは同じevent loop／D3D11 device／通知所有権／終了管理を使う。既存windowへのdrag結合／drop indicatorと通常起動の集約は上節の同host処理を使う。可視windowの実入力／mixed-DPIと起動・描画時間の評価は引き続き未完。

## U08: 画像tabのcontext移送と未取得ページの再開（2026-09-10）

画像の通常外dragも音声／動画と同じhost内の移送transactionを使う。移動先の作成と全画像textureの準備を終えるまで元tabを外さない。active／retainedの静止画・アニメーション・読みかけページ・読み込みpreviewは、元の画素を参照しつつ移動先contextへ新textureを登録する。上限を超えた画像／ページ／previewがあればstageを破棄して元tabと編集を残す。texture IDとsamplingの可変状態はwindow間共有せず、decoded Arc・現在frame index・次frame deadline・sampling設定を保持する。retained画像にもlocal media instanceを持たせ、pathが同じでも古い移送要求を区別する。

画像編集は履歴・元decoded Arc・確定済みの描画結果を移す。処理中のresampleだけ新ownerのworkerで元画素から再開し、旧ownerの完了は従来の世代照合で拒否する。取得済みのreadingページ／エラー／previewは保持し、未取得のsuffixだけ新ownerのImageLoaderへ要求する。appのrequest offsetでworkerのchunk index／totalを元のページ列へ対応させる。同じShell順の更新では再要求せず、隣接ページの構成が変わった時は従来どおり再構築する。previewはColorImageをArcで持ち、新textureへの移送に使う。取得済み画素のために元sourceファイルを再読込せず、永続バックアップも作らない。

不足ページの再開でも、元の512 MiB decoded-image予算を増やさない。appが保持中のprimary／readingページの画素bytesを渡し、ImageLoaderはそれを差し引いた残量からcache hit／decodeごとの消費を計上する。残量不足は既存のTooLarge診断で、そのページをエラー表示として残す。新規の全ページ要求は従来の予算を使う。

同context内のtab復帰も取得済みreadingページを保持して不足分だけ再開する。新window側の読み込み／GPU upload実行時間、可視ウィンドウのdrag・mixed-DPIは別の検証項目とする。filmstripからの新windowと既存windowへの通常drag結合／indicatorは上節の同host処理を使う。

## U08: 音声／動画tabのlive移送（2026-09-10）

host配下のtabを新windowへ分離する際は、同じD3D11 deviceで新しい非表示windowを初期化し、成功後にtabを移して表示する。作成失敗・古いtab/path/media instance・閉鎖／modal／graphics復旧待ち・対象tabのexport中は移送しない。移送する未保存編集は破棄せず、保存guardを出さずに履歴ごと新しいtab IDへ移す。最後のtabを移した元windowはWelcomeを残す。既存windowへの挿入位置指定の移送も同じ手順を使い、通常dropは上節のhit／gap検証から呼ぶ。

sessionは作成時の`(WindowKey, media instance)`を不変の通知originとして保持する。hostはactive／retained sessionの現在の所有者を検索し、宛先のlocal instanceに置き換えてPlayback通知だけを配送する。移動前にqueueへ入った通知、元window削除後、反復移送をforwarding chainや永続aliasなしで扱う。session破棄後のoriginには配送しない。background音声の次曲で新sessionを作る時は、その時の所有windowを新originにする。UIA／その他のwindow workerは従来の固定宛先を維持する。

active sessionのdecoder／WASAPIを開き直さず、時計・再生状態・表示frame・選択・view・filmstrip／playlist位置・編集履歴・保存先・音声／metadata export設定を移す。非active動画は既存のbounded suspensionから通常の再表示経路で戻す。新contextで無効な波形textureは再生成し、duration workerは元ownerから取り除き必要なら再要求する。playlistの古いwidget IDは捨て、tabの意味的focus roleとtimeline panel寸法を新IDへ移す。音声queueのrepeat／shuffle／順序を保持し、EOF instanceを付け替え、Shell順workerの通知先を新ownerへ再作成する。

画像tabは上節のcontext移送を使う。通常window間drop、可視windowの実入力／mixed-DPI、全codec／endpoint／HDRの組合せは未完として維持する。

## U08/U07: 共有deviceの復旧transactionと停止frame（2026-09-10）

host管理下のappは、active／retained decoder・presentationからのdevice lossを復旧要求として記録する。hostが各windowの時計位置／再生状態を先に取得し、対象のactive／retained sessionをすべて停止・旧surfaceを解放してから、新deviceと全対象のsurfaceをstageする。全部のsurface作成が成功した時だけappへcommitしてsession／UI textureを復帰させる。最初または途中の作成失敗ではstageしたsurfaceを解放し、全対象を編集／位置保持のnative Retry／Cancel待ちにする。session固有の再開失敗はそのwindowの既存診断に従い、共有deviceを別deviceへ分岐させない。

Retryだけの場合は要求元を対象とし、他windowに健全なdeviceがあればそれを使う。Cancel済み／まだ選択待ちのrendererなしwindowを自動復旧しない。健全とみなしたdeviceの除去が確認された時は、そのdeviceを使うwindowも含めて新deviceへ復旧する。旧stream generationの通知は既存照合で拒否し、失敗後のrendererなしwindowからの遅延lossも自動Retryにしない。音声／動画の新window分離は上節のlive移送を使う。

停止時計は表示frameのPTSと一致するとは限らない。runtimeの`suspend_for_graphics_recovery`は呼出元の現在時計位置とframe PTSだけを保持し、旧deviceのtextureを残さない。同じ位置へのpaused復旧ではaudio／transport targetを保ち、videoだけ元frame PTSから再開する。非active動画では再表示までこのtimestamp対を保持する。意図的なSeek・別target・再生状態変更では古いframeを流用しない。appとretained tabの復旧時計は、作成とpauseの間のns差を足さず正確な位置へanchorする。

生成H.264の非表示2window／各retained tabで、loss通知とpresentation errorを注入し、再生中／停止中の復帰、相手windowのrendererへのhardware cross-draw、第一／第二surface作成失敗、Cancelと個別Retry、遅延通知を検証する。WARP/software試験は時計がframeより200ns後のvisible／hidden復旧で元PTSとRGBA一致、transport target保持、意図的Seek／別targetでの無効化を確認する。実TDR・物理device取り外し、複数windowの混在画像／音声・WASAPI endpoint／HDR・全codec・通常可視window／混在DPIを検証済みとはしない。

## U08: window host・通知と待機の所有権（2026-09-10）

通常のevent-loop entryを`WindowHost`とし、window別の`Application`を保持する。worker通知を再利用しない`WindowKey`と`AppEvent`の組で包み、window-localなmedia instance／playback generationの衝突だけでは別windowへ配送しない。Playbackは上節のorigin照合で現在のownerへ、その他は固定windowへ配送し、宛先のない通知を無視する。AccessKit通知はnative `WindowId`で選び、従来のapp内window照合も維持する。raw motionは各appの既存focus判定を通し、非focus側の古いdragも解除できるようにする。

各appの`schedule`は希望する`ControlFlow`を返し、globalな待機／終了を変更しない。hostはPollを優先し、WaitUntilの最小値を採用する。既存の保存／離脱guardが`exit_requested`を確定したwindowだけを除去する。HWNDを保持したまま描画surfaceを解放し、最後のwindowがなくなった場合だけPoll＋event-loop exitを行う。別windowが残る間は終了しない。

追加windowの初期化は既存rendererのdeviceを引き継ぐ。画像／音声／動画tabの通常分離はlive移送、filmstrip起動は元ファイルを開く同host処理へ接続する。共有deviceの復旧は上節のtransactionで扱う。

headlessの通知／close guard／deadline集約に加え、実worker・AccessKit adapter付き非表示2windowでD3D11VA動画を確認する。window-local IDを意図的に一致させ、合成UIA通知の宛先・未保存取消／破棄・survivorのframe／generation・replacement keyと遅延通知破棄・最後のwindow終了を検証する。非表示HWNDでは通常のpaint通知待ちで表示frameが進まなかったため、試験だけが所有windowへのRedrawRequestedを明示配送する。[winitのWindows描画要求](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html#method.request_redraw)はWM_PAINTに対応する。通常の可視window／物理入力／混在DPIの証明とは区別する。

## U08: 同一deviceの複数描画先基盤（2026-09-10）

`FrameRenderer::with_graphics_device`／`with_native_caption_on_device`は、opaqueな既存`GraphicsDevice`を受け取り、そのdeviceのadapterから取得したfactoryで別HWNDのswap chainを作る。新しいdeviceやFFmpeg sessionは作らない。既存constructorの単独device作成とflip-discard／RGBA8の描画設定は変えない。deviceの指定は[MicrosoftのCreateSwapChain契約](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgifactory-createswapchain)に従う。

共有するrendererは同じevent-loop threadで逐次描画し、decode workerとの既存multithread-protected immediate contextを維持する。各rendererはswap chain／UI renderer／動画処理resourceを別に所有する。片方のsurface解放でcontext stateを解除しても、残るrendererは次の描画で自分のstateをbindする。egui context間のTextureHandle移送を許可するものではない。

音声なし生成H.264の実D3D11VA sessionでCOM device／context同一性とswap chain独立性、3サイズの交互動画／UI描画の画素一致、surface破棄／再作成後の継続と次frame進行を確認する。テストだけのstaging readbackは再生のCPU転送0とは区別する。runtimeの描画基盤はwindow host・通知routing・編集／再生state移送の所有者ではない。hostと共有device復旧は上節で扱い、通常分離への接続にはsession通知とstate移送の所有権をさらに揃える。

## U08: filmstripからの新window要求（2026-09-10）

filmstripの項目をprimary dragし、window外でreleaseすると、その項目のpathを新windowへ開く。元tabを移動／closeせず、現在の編集・再生を変更しないためdirty guardは不要とする。既存tabの分離／結合や状態移送とは別の「参照先を新規に開く」操作。6 logical px超から低解像度previewとfile名をpointerへ追従表示し、追加decodeはしない。window内release・Escape・focus喪失・overlay・folder snapshot／current source・screen／density変更・filmstrip終了で取消する。既存primary click／middle click／UIAは維持する。capture中の一時的なpointer離脱は後続座標を待つ。

UIはpathとfolder snapshot generationを渡し、appが現在のfilmstrip／snapshot所属を再検証して同じexecutableへ一つのpath引数として起動要求する。要求成功時だけfilmstripを閉じ、失敗時はそのまま診断を表示する。spawn成功は子windowの読込完了や描画成功の保証ではない。元sourceの保持と未保存backupを作らない方針は変えない。

source再読込／GPU復旧でpreviewを無効化する時もgestureを破棄し、旧contextのtexture handleをdragだけに残さない。通常の可視項目更新では、掴んだ低解像度textureだけをgesture終了まで保持する。

## U08: tab dragの追従表示（2026-09-10）

前frameの可視tab labelとprimary pressを対応させ、6 logical pxを超えた移動をdragとして所有する。close buttonはdrag開始点にしない。端12 logical px内で保持すると既存ScrollAreaを360 logical px/sで横scrollし、同じframeの再passで重複加算しない。barの描画が途切れた場合や非active source path変更も取消対象。capture中のCursorLeft／PointerGoneだけでは分離操作を破棄せず、追従描画と新しいscroll要求を止めて後続座標／releaseを待つ。直前に受理済みのscrollは次layoutで反映する。実focus喪失は取消する。

bar内のdrag中は掴んだ位置を保ってtab本体をpointerへ追従させ、挿入先の隣接tabはease／animationなしで即座に場所を空ける。TabSetの実順序はrelease時に一度だけ確定し、drag中の表示順は投影に留める。active／編集／保存先／再生sessionを切り替えず、Escape・focus喪失・modal／popup・source／tab構成変更・resize／DPI変更で投影を破棄する。bar外のwindow内releaseは取消、window外releaseは既存のguard付きdetachへ渡す。dragged tabの描画はwindow内へclipし、下のmedia操作へ入力を渡さない。既存の挿入線だけを動かす表示をこの契約で置換する。window間結合・状態移送・filmstrip分離は別の未完項目として維持し、pathだけのprocess起動をそれらの達成と扱わない。

## U07: 非active動画の保持surface（2026-09-10）

非active化でvideo worker／queue／decoderを停止し、開いたinputと最後の表示frameを保持する既存契約は維持する。最後のAVFrameが24枚のtexture arrayをpinすることをH.264で実測した。保持時だけ同じdevice上のArraySize=1 textureへ同一format／寸法の全subresourceをcopyし、AVFrameを解放する。通常表示中はdecode surfaceを直接使い、CPU readback／色変換／再encodeは追加しない。PTS／orientation／pixel aspect／transferとpaused-frame対応を保持し、復帰直後の描画を維持する。既に独立化したframeは再copyせず、新frameで置換する。

copyは[CopySubresourceRegion](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-copysubresourceregion)の同device／同format・全slice条件に従う。確保失敗は元frameを残して明示診断し、通常のdevice removal経路を妨げない。対象はset_video_visible(false)でdecodeを休止する時点であり、終端不明の背景decodeを維持する既存方針は変えない。decoder再構築／Seek待機の解消やprocess全体のVRAM上限とは別の変更で、texture descriptor由来の保持量と実VRAM予約量は区別して測定する。

## U07: tab別のmedia control focus（2026-09-10）

表示／再生tabの最後のmedia control focusを、egui widget IDではなく役割keyでwindow内に保持する。再生／読書／repeat・shuffle、crop preview、Seek／timeline値／選択辺、playlist／filmstrip項目が対象。項目はpathで識別する。tab名・native caption・menu／palette・設定modalの一時focusはmedia状態として保存しない。復帰先のenabledで可視のcontrolへ描画時にfocusを戻し、存在しない場合は現在tabへ戻す。読込中は待つが、新しいpointer／key／UIA操作は保留復帰より優先する。source変更／同tab再読込／close時に破棄する。focus復帰自体ではactivate／Seek／編集／再生変更をしない。未確定modal入力の永続化や別windowへのfocus移送は行わない。

保存量は開いているtabごとに一つのrole hashと、現在passの可視control ID一覧だけとする。Play／Pauseとrepeat状態違いは同じrole、通常Seekとtimeline値は別の表示領域として識別する。全画面の保存済みbar操作を戻す際は既存のkeyboard表示機構を使い、初回Area sizing passを一度待ってから不存在を判定する。全画面で見つからないcontrolは非表示のtabへfocusを要求しない。native window／focus喪失中とmodal／command overlay中は復帰しない。実混在DPI／物理入力／全UIA監査、tab間のresource pool／decoder復帰遅延は別の残件。

## U09: tab context menuのkeyboard入口（2026-09-10）

focusされたmedia tab名／close buttonからShift+F10、修飾なしのWindows Menu key、UIA ShowContextMenuで既存tab menuを開く。activeではなくfocus先のTabIdを対象とし、表示だけではactivate／履歴変更しない。keyboard／UIAではtabの可視部分をanchorにし、pointerの位置に依存しない。right-clickのpointer anchorは維持する。Escapeは呼出元へfocusを戻し、command選択は既存dispatch／dirty guardへ渡す。guard／保存が終了するまでfocus復帰を保留し、呼出元tabが残れば元widget、閉じたなら現在tab、全close後はWelcomeへ戻す。modal／他overlay／別popup／fullscreen／drag中／focus喪失では新しい呼出しを受け付けず、repeatで再openしない。通常Enterによるtab activationとmiddle-click closeは変更しない。Welcomeはmedia用menuの対象外。共通MenuKeyboardが矢印／Tabを処理したら、eguiがpass開始時に予約した空間focus移動も取消し、独自移動との二重適用を防ぐ。

## M01: ロゴからの方向menu操作（2026-09-10）

左上の既存logo buttonへprimary pointerの方向dragを追加する。8 logical px以上、右下45度±45度はEdit、その上側の右上はFile、下側の左下はViewへ対応させる。左上方向／閾値内へ戻したreleaseは取消。releaseで既存のcategory submenuを開くだけで、commandは実行しない。選択はrelease位置で再評価する。保持中は選択した矢印だけ白、他を半透明にし、左上shaftを右下へ80msで移す。release後は通常logoへ戻す。

既存Popup／MenuKeyboard／command有効性・shortcut表示を共用し、普通のclick／keyboard／UIA menuは維持する。方向dragのpopupはrelease点の右下8pxへanchorを固定し、離したpointerが別categoryへ直ちにhoverしないようにする（画面端の配置調整は既存Popupへ任せる）。通常clickのanchorは従来どおり。press所有とsource/tab／graphics世代を固定し、Escape・focus喪失・pointer消失・resize／DPI・他overlay／modal・toolbar非表示で取消。取消後の遅いreleaseをclickやcommandに変換しない。同frameのpress／move／releaseと描画再passでも一度だけ受理する。既存menuのEscape／左右移動／focus復帰・保存guardと非破壊編集を変更しない。

2026-09-11追記: nativeのCursorLeftはeguiへ渡し、PointerGoneをpress／releaseと同じイベント順で処理する。release前の退出は保持中のgestureを取消し、release後の退出は確定済みsubmenuを取り消さない。Windows通知の段階で先に取消すと、描画待ちのreleaseを追い越してしまう。同frameへまとめられたpressの有無×退出の前後×3方向を回帰確認する。focus喪失・Escape・resize等の既存取消は変更しない。

eguiは同frameのpress／moveを最後のpointer位置でhit-testするため、logoで受理したpressが保持中なら、そのframe内に既存のdrag owner APIへ明示登録する。これにより次frameの遅延drag判定が移動先の画像操作面やtabを選ぶことを防ぐ。既存のforeign owner検査は残し、release済み／取消済みにはownerを登録しない。実画像操作面とtab上へのbatched移動・保持・3方向submenuと、短い通常click／release後のowner解除を回帰確認する。

## U05: 保存中のツールバー境界進捗（2026-09-10）

保存jobがある間だけ、ツールバー下の既存境界に非hoverのSeekと同じ白／#181818・1物理pxの進捗を表示する。別の境界やhit領域／focus stopは追加しない。通常の画像移動・preview／metadata読取・保存先選択には表示しない。既存の取消／保存先・解析状態の表示と、離脱Saveのguardは維持する。全画面でtoolbarがない時は境界も表示せず、既存の取消UIを利用する。

保存開始時のsource長とrequestのtrim／区間編集／全体rateから出力長をsnapshotし、workerの出力時刻に対する推定比率を使う。normalize時は解析／encodeの二passへ半分ずつ割り当てる。これは壁時計の残り時間や厳密なbyte進捗ではない。hardware fallbackで再encodeが始まればそのpassの実報告へ戻る。publish完了前は99%以下に留める。長さ不明・静止画は数値を捏造せず有界のindeterminate表示にし、取消要求後は止め、Finishedの成功／取消／失敗すべてで元の境界へ戻す。source/tab切替後も保存jobのsnapshotだけを参照する。UIAには非操作の進捗と解析／encode／取消状態を伝える。

## E01: JPEG文字metadataのUIとKeep保存（2026-09-10）

JPEG UIはruntimeが公開する形式別の対応項目とvalidationを使い、Title／Artist／Comment／Copyrightだけを選択可能にする。非同期の既存値には言語と作者の順番を表示し、表示だけを1024 UTF-8 bytesへ切り詰める。読取待ち／失敗・XMLに不正な文字／未対応項目はApply不可。Setは言語別値／複数作者を一つへ置換し、Removeはその項目の全値を除去することを説明する。PNGと同じsource/tab世代guard・設定lifecycleを共有する。

基盤段階のSet／Remove限定を置き換え、JPEG→JPEG保存は全Keep／設定dialog未使用でも対応4項目の文字・言語・作者順序を保持する。元の未知／技術XMP、EXIF／IPTC／COMとの同期は保証せず、JPEG以外の保存先への明示設定は拒否する。元sourceと既存targetを守る有界読取・stage照合・source stampを既定保存にも適用する。他6項目・他画像形式・通常window／物理IME／全素材品質と全UX台帳は引き続き未完。

## E01: JPEG XMP文字metadataの保存基盤（2026-09-10）

JPEGは標準APP1 XMPのdc:title／dc:creator／dc:description／dc:rightsを、Title／Artist／Comment／Copyrightへ対応させる。[Adobe Dublin Core定義](https://developer.adobe.com/xmp/docs/xmp-namespaces/dc/)に従いtitle／description／rightsは言語別Alt、creatorは順序付きSeqとして扱う。Keepは文字値・言語・順序を保持し、XMLのprefix／空白のbyte一致ではない。Setは単一値（Altはx-default）、Removeは該当propertyを除去する。他の6項目は未対応として明示拒否し、今後の形式別契約へ残す。

標準XMP一packet、UTF-8、65502 bytes以内、32階層／4096 elements／128文字値へ制限する。namespace URIで識別し、通常の属性形式／文字要素／Alt・Seqを読み、DTD／外部entity・不正参照・複数packet／Extended XMP・未対応の対象property構造は拒否する。元EXIF／IPTC／COMとXMPの相互整合はまだ行わず、XMP文字だけの処理として区別する。未知／技術的な元XMPを編集済み画像へcopyしない。

JPEG入力→JPEG出力で、既存encode後のstageに標準XMPを差し替え、再読取照合してからpublishする。JPEG marker／entropy bytesをstreamingで複写し、metadata処理で再decode／再encodeせず、stageのEXIF／ICC／画像bytesを変更しない。source stamp、取消、既存target保護を共用する。基盤段階ではSet／Remove時だけ有効だったが、現在は上記UI契約に従い全Keep／設定未使用のJPEG→JPEG保存にも本経路を使う。他項目／形式・EXIF／IPTC整合、通常window／全素材品質と全UX台帳は未完のまま維持する。

## E01: PNG文字metadataのUIとKeep保存（2026-09-10）

File／custom commandのMetadata export optionsを画像にも開く。PNGは既存の10項目・非同期既存値・Apply／Cancel／IME／source/tab世代guardを共用し、PNG入力→PNG出力・文字項目だけであること、Author／Creation Time／Album Artistへの対応とEXIF／XMP対象外を明示する。他画像形式も制約を確認できるがApplyは無効にする。PNGの読取完了前／読取失敗時もApplyは無効。Apply／Cancel・Save／再Save／保存先選択の取消・tab保持／source再読込解除は動画／音声と同じで、設定変更は履歴／dirty／画素を変更しない。

UIのKeepと保存結果を一致させるため、前段基盤の「Set／Removeがある時だけcopy」を置き換え、PNG→PNG保存では全Keep／設定dialog未使用でも10項目の元text chunkを保持する。対象はPNG文字chunkだけであり、未知keyword／EXIF／XMPの完全保持や他形式への保持を保証しない。source／outputの有界CRC確認と文字chunk差替えを既定PNG保存にも適用する。明示Set／Removeのある画像でPNG以外の保存先を選んだ時は、既存workerの形式拒否を表示しsource／target／設定／dirtyを保持する。保存先を勝手に変更・拡張子付替えしない。画像UIの通常window／物理IME／混在DPI・他形式／技術metadataと全UX台帳は継続する。

## E01: PNG文字metadataの保存基盤（2026-09-10）

画像はまずPNG入力→PNG出力の10文字項目に対応する。Title／Artist／Album／Album artist／Composer／Genre／Date／Track／Comment／Copyrightを、PNG keywordのTitle／Author／Album／Album Artist／Composer／Genre／Creation Time／Track／Comment／Copyrightへ対応させる。既存の同名keywordと動画用keyをASCII大小文字を無視して読む。Setは非圧縮iTXtのUTF-8、Removeは該当keywordの全重複・言語variantを除去し、Keepは該当する元のtEXt／zTXt／iTXt chunkをそのまま保持する。[PNG仕様](https://www.w3.org/TR/png-3/#11textinfo)に従い、Dateは文字列であり時刻の変換・検証はしない。

既存の画像encode後、隔離staging内で文字chunkだけを差し替え、指定とKeepのraw chunkを再読取照合してからpublishする。画像画素の再encodeは従来の保存経路だけで行い、metadata処理ではIDATを含む他のchunkを変更しない。元のEXIF／XMP／未知keywordを編集済み画像へ追加copyしない（向き・色の二重適用を防ぐ）。これは全metadata保持や無変換画像保存ではない。基盤段階ではSet／Remove時だけ有効だったが、現在は上記UI契約に従い全KeepのPNG→PNG保存にも本経路を使う。

入力／出力のsignature・chunk長・CRC・終端をstreamingで確認し、文字chunkは最大128件、格納bytes／展開UTF-8 bytesはそれぞれ合計1 MiBへ制限する。圧縮文字の過大展開・不正UTF-8／構造・CRC破損は明示拒否し、IDAT全体は保持しない。取消をchunk／64 KiBごとに確認し、source長／更新時刻を読取前とpublish前で照合する。失敗時は既存source／targetを保持し、所有stageを片付ける。他形式への文字設定は拒否する。PNG用UIは接続済みで、他形式／EXIF・XMP対応、通常windowと全素材認定は次工程として台帳に残す。

## E01: metadata設定UIと非同期の既存値表示（2026-09-10）

metadataと画像／動画resizeのmodal内popupはframeを越えて保持する。背景menuはmodalを開く時に閉じ、modal全体を毎frame閉じる処理でそのfield／filter選択まで消さない。metadataのCancel／source変更時は所有popupも閉じる。文字入力のUIA SetValueは既存resize用のbridgeを共有し、複数行roleを維持する。IME候補中とcommit同frameのEscapeはdialogを閉じず、popup内Escapeはpopupだけを閉じる。

動画／音声のFile「Metadata export options」から10文字項目を選び、Keep／Set／Removeと値を指定する。既定keyは追加しない。現在fileのglobal／再生と同じbest video・audio streamの値を、window/GPUを所有しないlatest-only workerで取得する。表示値は各1024 UTF-8 bytesへ文字境界で制限し、省略を明示する。これは表示上限で、Keepの元tagを切り詰めない。古いtoken／source／tab／generationの結果は破棄し、Cancel／source変更／close時に待機要求を取消する。進行中のFFmpeg probeを強制中断する保証ではなく、UI threadを待たせない。

Applyは出力設定だけを確定し、次のSave／Export as／AudioOnlyに適用する。再生音・画素・編集履歴・dirty状態は変更しない。設定保持は音声export optionsと同じcurrent-source/tab内sessionに限定し、別file／次曲・再読込・closeで解除、tab切替／再Saveは保持する。Cancel／Escapeは未適用draftを破棄し前focusへ戻す。modal中の移動・編集・離脱とexport中の設定変更を防ぎ、無効文字／上限超過はApplyできない。保存先dialogのCancelは適用済み設定を戻さない。AudioOnlyは動画の未保存guard／通常Save先を保持する。画像metadataは次工程として残す。

## E01: 非破壊metadata出力の文字項目（2026-09-10）

草案の書き出し時metadata書換は元fileを変更しない出力設定とする。まずTitle／Artist／Album／Album artist／Composer／Genre／Date／Track／Comment／Copyrightの文字項目をKeep（既定）／Set／Removeで指定する。空文字SetはRemoveと同じ意味で扱う。技術的な回転・色・durationや任意のFFmpeg optionを編集対象にしない。文字列はUTF-8で1項目1024 bytes・全項目4096 bytes以内、NULは禁止する。Unicode・改行・引用符・等号は文字として保持し、shellを介さず個別argumentとして渡す。

通常動画／音声と音声のみ出力へ同じ設定を渡し、指定項目だけcontainer／出力streamへ上書きまたは削除する。Keepは従来のmetadata copyであり、全formatを越えた完全保持を意味しない。encode後のstaged fileを再probeして指定値／削除を照合し、非対応形式・値の切捨て／変形はpublish前に拒否する。既存の取消・source別名保護・stagingを共用する。設定は画像／音声sample・時間軸を変えず、再encode自体は従来の保存経路に従う。

動画／音声のruntime基盤と実file回帰に加え、上記の設定UI・source別保持・取消／再Save／離脱へ接続する。画像はFFmpegのformat metadata指定だけではPNG等の文字chunk／EXIFに反映されないため、未接続の間は明示拒否し、画像metadata対応を台帳から除外しない。

## E01: 音声export設定UIと保持範囲（2026-09-10）

動画／音声のFile menu「Audio export options」とcustom commandから設定modalを開く。既定keyは増やさず、動画のtimeline有無を問わない。Peak −1 dBFS normalizeのOn／OffとKeep／Mono／Stereoを提示し、再生や編集履歴には作用せず次のSave／Save As／AudioOnlyへ適用されること、LUFS／true-peakではないこと、mono／stereo変換は1／2channel入力が必要なことを説明する。Applyは設定だけを確定し、Cancel／Escape・古いtoken・source／tab／media世代変更は破棄する。前のfocusを復元し、modal中の編集／移動／離脱を共通guardで防ぐ。実行中exportがある場合は新たな設定modalを開かない。

設定はtabに属する現在sourceのsession内状態で、タブを行き来しても保持する。同じsourceの再Saveは現在の設定を使い、書き出し開始時にoptionsをworkerと進捗表示へcopyする。設定変更自体はdirty／saved cursorを変更しない。AudioOnlyは既存どおり通常のSave先／saved cursorを変更せず、設定も勝手に戻さない。別sourceへの移動、外部置換の再ロード、background音声の次曲、tab close時は設定を初期化する。pathだけを持つclosed-tab再open／別windowへの引継ぎやapp restartでは設定を引き継がない。native保存dialogもmedia世代を照合し、同じpathのsource再ロードを古い選択で開始しない。設定modalのCancelと、Apply後の保存dialogのCancelは別操作であり、後者では適用済み設定を保持する。

## E01: 音声出力optionの採用契約（2026-09-10、runtime基盤から接続）

normalizeは任意の「sample peak −1 dBFS」一括補正として採用する。既定はOff、LUFS／RMS／true-peak／動的音量追従ではない。trim・区間削除／伸縮・局所／master gain・rateとchannel変換を適用した音声全体を解析し、全channel共通の一定gainを最後に適用する。無音はgain1で保持する。これはencode前のsample peak目標で、lossy codecの再構成peakや知覚音量の一致は保証しない。master gainを変えてもnormalizeにより全体のgainが打ち消されることがあるが、部分的な強弱は保持する。

channelはKeep（既定）／Mono／Stereo。stereo→monoは0.5L+0.5R、mono→stereoは同一sampleの左右複製、既に目的のchannel数なら恒等変換とする。mono／stereoへの変換元は1／2channelに限定し、多channelを黙って切り捨てたり別のdownmix規則を追加しない。Keepでのnormalizeは多channelも共通gainで保持する。無音声素材・画像への非既定音声optionは既存target変更前に拒否する。

通常動画／音声保存とAudioOnlyへ同じtyped optionsを渡す。runtime workerは固定FFmpegのdouble sample形式・最終音声filterの同一列を二回使う。第一passは音声だけをnull出力へ流してastatsの全体peak／sample数／NaN・Infを取得し、第二passでgainを追加して通常encodeする。全PCM・全長中間音声をmemory／diskへ保持せず、既存16 KiB診断tail・staged filter file・取消／publish保護を共用する。空音声／不正・欠落した解析値は公開しない。解析とencodeの進捗を区別し、hardware失敗時のsoftware fallbackにも同じgain・channel filterを渡す。UI／保存先に紐づくoption保持は後続sliceで接続し、未接続を機能完了とは扱わない。sourceが二つのpass間に外部変更される場合の整合性は開始／解析後／publish前のfile情報照合で拒否し、同userによる同一情報への偽装まで保証するものではない。

## E01: 音声のみの派生書き出し（2026-09-10）

動画のFile menu「Export audio only」から別名の音声を作る。既定は`元のstem-audio.wav`、WAV／FLAC／MP3／M4A／AAC／Ogg Opus／Opusを対象とする。元動画のSave先・saved cursor・編集履歴・再生状態は変更せず、離脱guardのSaveにも代用しない。再実行は毎回保存先を選び、既存の非同期dialog／export worker・進捗・キャンセル・staging／publish保護を共用する。dialog中にsource／tabが変わった場合は開始しない。

runtimeは通常mediaとAudioOnlyの出力目的を明示的に受け、AudioOnlyではworker内のrequest copyから画像・動画の空間編集だけを除く。元requestのkindと編集列は保持する。再生と同じbest audio streamだけを選び、trim／master rate・volume／区間削除・伸縮・局所音量を既存音声filterへ渡す。無音声素材・画像kind・非対応出力拡張子は既存targetを変更せず拒否し、映像・字幕・添付画像を出力しない。音声は再encodeでありpacket抽出ではない。metadataは既存copy方針を維持し、個別metadata書換・normalize・channel変換は次のsliceで接続する。通常windowのphysical dialog操作・全codec品質認定とは分ける。

## V04: 音声のフレーム相当移動（2026-09-10）

草案の音声`,／.`は前後10msの微小Seekとして採用する。音声には表示fpsがなく、codecのdecoded frame／packet長を使うと同じ操作の移動幅が形式で変わるため、編集操作として一定の時間単位を選ぶ。これはPCMの1sample移動や圧縮frame境界への移動ではない。専用の「Step audio backward/forward (10 ms)」command／View menu／custom bindingへ単位を明記し、動画の実PTS探索とその設定は変更しない。

移動は現在の編集後時間軸を基準とし、先頭0／既知終端でclampする。再生中・停止中・EOFから利用でき、hold-speed解除後に一時停止して既存Seekを使う。rateによって10msを増減せず、履歴・時間選択は変更しない。範囲再生の外へ出る場合だけ既存Seekの規則で範囲再生を解除する。modal／別media／未ロード・故障状態は対象外。新しいdecoder／worker／queueを追加せず、連続入力は直前の停止位置から累積する。sample精度の波形編集・auditionや全形式のseek遅延を認定するものではない。

## V05: 動画resize／resample採用契約（2026-09-10、UI接続済み）

UI接続方針: 動画の回転・resizeでsource／tab／世代／orientation／編集列／device寸法上限のsnapshot照合を共有する。画像と寸法・比率・4filterの入力部品を共用し、動画だけは入力SARを含めた表示比率と偶数格子を使う。動画resize専用commandをtimeline内Ctrl+R／Edit menuへ追加し、既存custom bindingは維持する。右下の入力透過でない透明backdrop modalで全canvasのGPU previewを表示し、Apply前のsnapshot／budget照合、Cancel／identity非編集、focus復帰とUndo/Redoを回転と同じ契約にする。

2026-09-10 13:23接続結果: 上記UIと共通snapshotを実装。linked寸法は入力SARを含む元の表示比率から最近傍2pxへ丸めるが、手入力の奇数寸法は黙って修正せずApplyを無効にする。初期寸法も偶数へ丸め、SAR正規化と実出力比率を表示する。古いdialog token・対象snapshotは破棄し、Cancel／identityはview・selection・履歴を変えない。有効なApplyだけ既存visual editへ渡し、表示をFitへ戻す。previewは確定済み編集の後ろへ一時ResizeVideoを付けた同一device rasterであり、描画passと同じframeのgeometry／UVを使う。共有filter候補はpointerだけでなくkeyboard／UIAの確定でも閉じる。通常window／mixed-DPI／全素材認定と全台帳の残件は保持する。

動画の画素resizeは表示zoomと区別し、専用VideoResizeへ操作直前のsample寸法／SAR、指定出力寸法、補間方式を保持する。出力は指定どおりの偶数幅・高さ（各16～16384px、128M pixels以内）、SAR1とし、encoderの都合で黙ってpadding／cropしない。入力の奇数寸法は許可する。元がSAR1かつ同寸法なら補間方式によらず非編集とする。SARだけが異なる場合は同寸法でも表示比率変更として一件の編集になる。UIは元の表示比率を保持する設定を既定にし、丸め後の寸法と比率を表示してApply前に確認できるようにする。 同梱OpenH264の実exportで16px未満の拒否を確認したため、最小寸法は既存動画cropと同じ16pxへ揃える。

Nearest／Bilinear／Bicubic／Lanczosの4方式を画像と共有する。保存はRGB8の明示scaleとSAR1を履歴順に挿入し、crop／quarter turn／flip／自由回転／再resizeも変更後の座標系を使う。source geometry／SAR／orientationと各操作snapshotを保存先配置前に照合し、不一致では既存targetを変えない。動画以外への適用と画像resizeの動画流用は拒否する。これは再サンプリング・再符号化であり、無損失を保証しない。

先にcore／software保存と合成順序を検証し、続いて4方式の同一device GPU表示、適用前budget／最終geometry、timeline内Ctrl+Rのmodal／Apply／Cancel／Undoへ接続する。GPU／UI未接続の段階ではapp入口を公開しない。補間方式の削減やCPU readbackで表示契約を代用しない。HDR・dynamic geometry・全素材画質／性能と通常windowの最終認定は別途必要であり、UX台帳全体をこの基盤へ縮小しない。

GPU接続前の固定source照合: FFmpegのlibswscale/utils.cではBicubicの既定B=0／C=0.6、Lanczosの半径3を用い、縮小率に応じてkernelの幅を広げ、境界の係数を端へ集約して正規化する。libswscale/swscale.cの水平中間は符号付き精度を保持する。単純な固定2／4／6点sampleや各軸後のRGBA8 clampを同じfilterの証明にしない。GPU側も縮小のalias抑制と中間精度を考慮し、画素・memory予算・速度を実測してからUIへ公開する。

GPU resize実装方針（2026-09-10追加）: 横／縦のseparable passとし、横中間はRGBA16 floatで負の補間値を保持、最終出力でRGBA8へ戻す。source／target長・filterに対応する正規化係数はCPU上で一度計算し、同一deviceの読取専用RG32 float textureへ転送・再利用する。media画素のCPU readbackは行わない。geometry planでは係数を生成せずspecだけ扱い、寸法・format別の中間textureと係数payloadを合わせて512 MiB以内へ制限する。隣接stageのsource／target分離と各pass後のunbindを維持する。縮小率に応じたtap数を使い、固定tapへ切り詰めない。画素比較・精度・速度の検証が終わるまでUIは公開しない。

2026-09-10実装・画素照合: 上記GPU処理を接続し、WARPの拡大／縮小／奇数・極小source／高彩度pattern／metadata orientation／crop／回転／再resizeと、実GPUの1080p／4Kを独立FFmpeg出力へ比較する。固定libswscaleは偶数幅RGBから半分以下へ縮小する時、標準flagでは入力色を間引くことがあり、64×48→30×18のNearestでも元にない色を再現した。動画ResizeVideoのexportに限り`full_chroma_inp`を明示し、GBRP8の各色を保持する。GPUのNearestは完全一致、他方式は固定小型fixtureで各色最大3以内、実GPUの16条件で最大1以内を確認。一般素材のbyte一致保証ではない。横中間を含めたpayload予算を超える縦横比変更は出力寸法が範囲内でも拒否する。512 MiBはdecode／入力画像／driver／CPU係数生成時の一時領域を含むprocess全体の上限ではない。UIは次のsliceで接続する。

## V05: 動画の表示zoom／pan契約（2026-09-10）

動画のzoomは画素編集ではなくImageViewStateの表示状態とする。草案の操作制限に合わせ、倍率変更・右dragはtimeline表示中だけ許可し、閉じた視聴中／fullscreenでも確定したzoom／panは保持する。Zoom in/out・Actual size・Fit・Coverは既存command／custom binding／View menuを画像と共有し、音声は対象外。Ctrl＋wheelは現在のpointerを基点に実zoom_deltaを使い、Ctrlを離した後の平滑化残量では変更しない。overlay／modal／別gestureが入力を所有する間も変更しない。keyboard/menuのzoomは画像と同じ中心基準、右dragの取消しは開始前のpanへ戻す。window geometry／focus・cursor喪失／別操作では途中panを取消する。

倍率は表示向きのpixel行をphysical pixelへ対応させ、横方向を編集後SAR倍する。Actualは1行＝1 physical px、Customはその倍率、Fit／Coverは現在のlogical viewportをDPIとSARで補正した二軸比率から求める。SARを整数寸法へ丸めない。任意回転後は採用済みの最終canvas／SAR1を使う。手動倍率の上下限は既存ImageViewStateに従い、window寸法の変更でCustom倍率を再計算しない。panはlogical座標で、保持済みplayback tab viewへそのまま格納する。

選択・辺操作には切り出し前の映像rectと最終pixel寸法を使い、選択線はviewportでclipする。GPUへはviewportとの交差rectと対応する補間UVを渡し、表示clipを整数pixel cropや編集履歴へ変換しない。既存のsoftware／hardware UV表示と順序付きGPU rasterを使い、CPU readback・別device・新しいtexture poolは追加しない。hardwareの非identity UVでは既存RGBA中間textureを使用する。全体が画面外なら動画を描かず、編集viewportの右dragまたはFitで復帰できる。自由回転preview中だけ全canvasをFitし、取消しで元のzoom／panへ戻る。保存は表示clipを参照しない。

resize／resampleは別の画素編集として未完。通常windowの物理入力／混在DPI、全解像度・HDR画質／性能の認定は残件であり、限定fixture・hidden-windowの証拠で置き換えない。

## I06: 動画自由回転の保存・表示・角度UI契約（2026-09-10）

動画hold-drag契約（2026-09-10追加）: timeline表示中の映像上でAlt＋左pressから左右dragし、画像と同じ1 logical px＝0.5度・±180度・0.1度単位へ丸める。角度dialogのsnapshot／geometry・budget検証と確定処理を共有し、previewの全canvasを動画viewportへFitする。Alt保持のmouse releaseだけ一件を確定し、0度は非編集。Alt先離し／Escape／focus・cursor喪失／wheel／secondary press／window geometry・source・context変更で取消し、履歴・選択・transportをpreviewのために変更しない。releaseとmodifier解除が同じframeでも、release時の修飾状態と位置を使う。入力ownership／別overlay／既存gestureを侵さず、確定までselection操作を抑止する。

角度UI契約（11:43追加）: 動画専用Free rotate video commandをEdit menu／palette／grid／custom bindingへ追加し、既定Ctrl+Shift+Rは画像commandとcontextで分離する。timeline表示中のみ開け、既存custom key／prefixが重なる場合は追加defaultを抑止する。右下の有界scroll modalで±180度・0.1度単位の数値／sliderを操作する。透明な入力遮断backdropで他の操作を止め、映像面に現在の編集列＋仮の一回転を同一deviceでpreviewする。dialog更新前にそのframeの寸法と操作列を一緒に捕捉し、角度変更時は次の再描画を要求してrectと画素の一時不一致を防ぐ。再生／停止・clock・履歴・selectionはpreviewのために変更しない。modal中の選択枠は隠し、Cancel／Escape／0度で元へ戻す。Applyは既存visual編集と同じ一件の履歴追加・selection解除・Fitを行う。

適用契約: dialog token、tab／path、media／playback世代、操作列、実frame寸法／SAR／orientation、device辺上限を確認する。古いactionは新dialogを閉じず、source交換やcontext変化は取消する。runtimeの非確保video_edit_geometryで入力snapshot／編集順／中間payload予算を確認し、無効な角度やbudget errorはApplyを無効にする。後続visual editとUndo/Redoの候補も履歴を変える前に確認する。自由回転を含む表示は最終canvas寸法・SAR1・identity UVとし、全操作をGPU rasterへ渡す。preset／cropも同じ最終pixel座標を使う。timelineを閉じても確定済み編集を描画する。画像dialog／Alt操作の契約は維持し、HDR／dynamic geometry／全寸法性能・通常window認定は残件。

VideoRotationは操作直前のsample寸法とpixel aspect、square-pixel寸法、0.1度単位の角度、回転raster寸法と偶数出力canvasを保持する。SARが1より大きければ幅をSAR倍、小さければ高さを1/SAR倍して最近整数へ丸め、元のdetailを減らす縮小はしない。各寸法は16384px／128M pixels以内。角度0は履歴にもfilter列にも入れず、pixel aspect正規化やpaddingも行わない。

非ゼロ角度の保存は、GBRP8→bilinear square-pixel scale→GBRP8明示→SAR1→回転→右／下へ最大1pxの黒いpadding→SAR1とする。回転は画像と同じceil外接寸法、90度倍数はtranspose／flip、他はbilinearで黒い余白を生成する。scaleの後にもGBRP8を固定し、複数回転の間で自動交渉によりYUVへ変わらないようにする。既存crop／quarter turn／flipとの順序を保持し、次の操作はpadding後の寸法を使う。これは動画の再サンプリング・再符号化であり無損失の契約ではない。

保存前にはbest video streamの寸法、container／codecの表示用SAR、display matrixの軸交換を解決し、操作列をたどって各回転の入力と照合する。FFmpegのav_guess_sample_aspect_ratioを、export workerが所有するinputと借用streamの寿命内で読み取り呼出しし、native pointerを外へ出さない。寸法／SAR不一致、範囲外crop、非video操作との混在は配置変更前に拒否する。frame途中で変わる寸法／SAR／orientationやHDRの品質認定は別残件である。

表示側は既存の四隅UVだけでは回転後の黒いcanvasや、その後のcrop／再回転を保持できない。順序付きの中間raster処理を同じD3D11 device上へ接続し、角度dialogと確定済み履歴の表示に使う。ソフトウェア保存の基盤テストをGPU表示、hardware encode品質、通常windowの操作確認の代用にはしない。

GPU raster契約（2026-09-10）: runtimeのdraw_current_editedは現在frameの寸法／SAR／orientationから操作列を検証し、source orientation→crop／quarter turn／flip→square-pixel scale／自由回転／paddingを履歴順に描く。各回転の入力snapshot不一致、非video操作、無効cropは描画前にtyped errorへする。表示用UVは最終rasterに対する一時cropだけを表し、元frameのUVを二重適用しない。decode session・PTS・音声・履歴は変更しない。app UI側も同じplanによる事前検証と編集後geometry／selectionを使う。

softwareは既存RGBA upload、hardwareは既存Video Processorの色変換／HDR能力判定後のRGBA textureを入力にする。中間textureは同じdeviceのGPU専用RGBA8・RTV／SRVとし、CPU readback／再uploadはしない。寸法が一致し直前のsourceと異なるslotを再利用し、同じ寸法の連続stageでも最大2枚。layoutが変わる時だけ旧poolを解放して再確保し、通常経路へ戻る時も解放する。所有する中間RGBA payloadは合計512 MiB、各辺はdevice上限以下とし、事前に超過を拒否する。source upload／Video Processor出力・decode pool・driverの保留resourceは別枠であり、process全体やGPU物理使用量の上限ではない。幅／高さの最大値を組み合わせた巨大な正方形textureは確保しない。

各stageはscissor／blend／depth状態を初期化して全画素を描き、source／target viewをunbindしてから次へ進む。任意角度はpixel中心を逆写像し、exportと同じ1px境界拡張・黒背景・8bit切捨て補間を使うが、GPU浮動小数点／samplerとFFmpeg固定小数点／scaleの違いによりbyte一致は保証しない。offscreen WARPの限定fixtureで整数編集の全画素一致、自由回転／SAR／全8 orientationと合成順序の最大channel差3/255以内を確認する。readbackはcfg(test)だけに隔離する。hardwareの実行／復旧試験をHDR品質、全寸法・全角度の画質、通常window／性能認定の代用にはしない。未編集・既存UVだけの従来経路は維持する。

## I06: 自由回転の画像基盤（2026-09-10）

画像hold-drag契約（2026-09-10）: 草案で未決だった保持キーはAltを採用する。原寸画像上でAlt＋左pressから左右dragし、1 logical pxを0.5度、-180～180度・0.1度単位へ丸めてプレビューする。Ctrl／Super併用は対象外。既存textureを画像の中心・現在のscale／panで回し、確定前のview／selection／履歴は変更しない。crop preview中も対象は選択だけでなく編集後の全画像とする。Alt保持でmouse releaseすると一件だけ確定し、移動0は非編集。Altを先に離す、Escape、focus／cursor喪失、window geometry変更、別command／source変更では取消する。canvas上限超過はpreviewで知らせ、確定を拒否する。数値dialogはCtrl+Shift+Rとmenuのまま維持する。動画と通常window／性能の認定は別残件。

画像UI契約（2026-09-10）: Free rotate image command（既定Ctrl+Shift+R）をEdit menu／palette／grid／custom shortcutへ追加する。原寸画像が利用できる非reading時だけ角度dialogを開き、数値入力と横sliderで-180～180度を0.1度へ丸めて調整する。dialogの有界canvasでは現在の編集後画像meshを回転させ、透明余白と配置を近似previewする。これは保存用画素を生成するものではない。確定後の全frameは共通raster workerで生成する。previewは履歴／selection／zoom／原本を変えず、Cancel／Escape／角度0のApplyは完全な無編集終了とする。

Applyはdialog token、tab、media／raster世代、source画素identity、path、編集列、現在寸法を再確認してから一件だけ追加する。古いactionは新dialogを閉じず、無効なcontextでは適用しない。既存modal入力保護・復帰focus・離脱時のApply/Cancel要求へ接続し、source交換ではdialogを破棄する。hold-dragも同じsnapshot取得・適用検証とmesh回転を使う。動画、通常windowの最終操作／外観と大画像性能は未完として維持する。

自由回転を画像の非同期raster編集から実装する。角度は時計回り正の0.1度単位、-180～180度。操作直前の寸法と、回転した画素領域を切らず収めるceil済み外接canvas寸法をcoreの検証済み値に保持し、最大16384px／128M pixelsの既存画像上限へ収める。角度0は履歴を増やさずRedo枝も変えない。±90／180度は既存transpose／flipを使い、任意角度は透明黒の余白とpremultiplied-alphaで補間する。固定FFmpegのrotateが対応するGBRAP8を明示し、透明色の色漏れを防ぐ。元画素と直前のcrop／resize／反転／回転の順序を保持する。

表示用workerと画像exportは同じvisual filter列・固定canvas寸法を使用する。ImageViewのUVだけで自由回転を表現しない。処理待ち／失敗中は既存raster待機・errorを表示し、完了後のRGBAを既存同一deviceへ載せ、materialized状態では履歴を二重適用しない。Undoでraster編集がなくなれば保持元画像へ戻す。原本と共有cacheの画素は書換えない。workerの取消・世代・tab境界、512MiB上限と全animation frameのdelayを維持する。動画の単一device上での任意角度表示とSAR／export契約はまだ接続せず、自由回転全体の残件とする。

## I06: アスペクト比の選択プリセット（2026-09-10）

1:1／4:3／3:4／3:2／2:3／16:9／9:16を共通command・Edit menu・palette・変更可能なprefix shortcut（Ctrl+Kの後に1～7）へ公開する。現在の編集後画像に内接する中央の最大矩形を作り、既存のimage1px／video2px格子へ丸める。動画はorientation／回転後のsample aspect ratioを含む表示比率で解釈するため、整数格子による比率・中心の誤差は許容する。極小／非対応寸法では無効な矩形を作らない。

画像のreading／原寸未読込／resample待機、音声、動画のtimeline非表示では実行しない。成功時だけ以前のvisual選択とcrop previewを置換し、時間範囲選択を解除してCtrl+Yをvisual cropへ渡す。既存の辺focus・移動・Shift比率保持resizeを使い、持続する比率lock／設定／新dialogは追加しない。選択自体は非編集で、tab保持・取消・コピー・実crop・Undo/Redoは従来の経路へ接続する。新しい暗黙prefixが宣言済みcustomキー／prefixと競合する場合は追加せず、明示した新commandは既存の優先規則に従う。

menuの初回表示passでは前回の有効項目一覧がまだない場合がある。直後の上下矢印は項目収集後へ引き継ぎ、初期focusから一段だけ移動する。項目がない場合は実行せず、既存のscroll／submenu／Enter取消規則を維持する。

## H1: timeline表示とcompact seekのgesture（2026-09-10）

V04一時倍速契約（2026-09-10）: 動画の視聴面（visual編集contextを除く）と、動画／音声共通のPlay/Pause buttonで、主buttonを移動なしに400ms保持すると絶対値のmaster2倍速で再生する。元が一時停止なら保持中だけ再生し、release／移動／キー／focus・pointer喪失／resize／別command／Seek／編集／tab切替／modalで元の速度とpause状態へ戻す。EOFでは速度を戻してEndedを保ち、一時停止からの試聴終了は次曲へ送らない。短いbutton clickは既存TogglePauseで、長押しや取消済みpressのreleaseからclickを発生させない。保持中の2倍速をstatusとfullscreen messageで明示する。

入力は1pressにつき1ownerとtokenを持ち、eguiの複数pass／まとめられたイベントで重複開始しない。開始actionはmedia／playback generation／入力tokenと有効contextを確認する。native releaseでも解除し、短いclick候補だけは通常UIへ渡す。rate変更中にframeが不在でも動画面のrelease処理を維持する。一時速度はappの非保存状態であり、EditHistory／export／immutable timeline／選択再生範囲を変えず、別tabへ保持しない。runtimeのset_rate_atは既存session／同一D3D deviceとWASAPI Sharedを使い、現在位置でrateを切り替えてpipelineを再開する。したがって境界の無音・再開待機を無くすものではない。長GOP応答性・任意素材の音質／gapless・通常windowの物理操作は別途確認する。

V03の移行契約: 一つのsourceの半開区間列を編集後の時間軸へ並べる。Keep／Delete／部分音量／Stretchは、そのoperation直前の編集時間で選択を解釈し、source時刻には履歴を順に再生して対応付ける。joinの時刻は後続区間、EOFは最終source端点へ対応する。区間の表示長は整数nsで保持し、選択stretchは各区間の相対速度を維持して指定した総時間へ配分する。source境界も整数計算し、無効値／overflowは原子的に拒否する。全削除は空timelineとして保持し、Undoで復元できる。空timelineは再生対象時刻を持たず、空mediaのexportは開始せず拒否する。局所音量は0～200%、局所速度は0.25～4倍。既存の全体音量／速度は最後のmaster調整として維持する。

旧trim履歴はsource上の初期区間として解釈し、その後に新しい区間編集を適用する。UIを新モデルへ接続する時には旧trim gripから時間選択へ置き換え、以降の切り詰めはKeep操作に統一する。保存は同じ区間列から選択streamを切り出し、timestampを零起点へ揃えて連結する。段階移行中は再生と保存の契約が揃うまで新しい区間編集commandをUIに公開しない。これはV03完了ではなく、選択UI・再生・waveform時間軸・export一致までが実装対象である。

現在のcore planはsource順の非重複区間を保ち、同じ速度／音量の隣接source区間を統合する。exportは選択したvideo/audio streamのみをsplit→半開trim→局所処理→concatへ渡す。音声の予定境界は編集時間からsample数へ変換し、atempoの端数・短いtailはtrim／無音padで揃える。音質・seamless再生をこれだけで保証するものではない。長いfilter graphはWindowsのcommand-line上限を避けて一時staging内のUTF-8 fileへ置き、成功／失敗／取消の全経路で回収する。source duration不明、無効な範囲、空の編集結果はexportを開始せず拒否する。

V03再生の接続契約: sessionはimmutableな区間planを保持し、target／表示frame PTS／audio clockは編集時間を使う。video producerは開いたsource入力を区間順にSeek／decodeし、映像PTSを編集時間へ対応付ける。audio producerは初期Seek後に一続きのdecodeから必要区間を切り出し、区間ごとの粗いPTS Seekによるsample位相のずれを避ける。音声は局所速度×master速度を保存と同じatempo chainで一度だけ処理し、局所gainと予定sample数へ揃えて、一つのWASAPI出力の有界queueへ1024 frames以下のchunkで連続投入する。WASAPI内で速度を二重適用せず、clockだけmaster速度で編集時間へ進める。master音量は従来どおり出力側で即時変更できる。区間境界ではsession／generation／WASAPIを再作成せず、選択試聴中を除き、全区間の終端だけEOFとする。Seek／plan交換／device復旧は世代を更新し、非表示videoの停止と音声継続の既存所有権を維持する。

動画はcompact seekのpress後、click許容距離を初めて越える方向で操作を固定する。上方向が横方向より大きければtimelineを開き、Seekは発行しない。横／下が先なら従来のrelease時Seekを維持し、途中から上へ動かしても開閉へ変えない。T／View menu／paletteは動画timelineの開閉に共用し、専用status buttonは置かない。fullscreenからの展開はfullscreenを終了する。音声timelineはfullscreenでも常時表示し、音声の開閉commandとcompact seekは提供しない。

timeline内にはhover thumbnailを表示・生成せず、compact video seekのpreviewは維持する。単一区間trim gripは時間選択へ置換した。入力取消・所有権、tab別panel高さ、編集履歴と再生位置は表示変更だけでは変えない。

V03 app接続契約（2026-09-10）: source duration／waveform textureは入力のmetadataとして保持し、active／backgroundの位置・EOFとstatus／Seek／timelineの長さはsessionの編集planを使う。編集履歴をpushする前にcandidate全体を検証し、無効な操作ではUndo/Redo枝も変更しない。plan変更とUndo/Redoでは旧位置をsourceへ戻して新planへ対応付け、削除された位置は次の残存区間、残存tailより後は編集EOFへ移す。空planからの復元は先頭へ戻す。master音量だけなら再生を再作成せず、master速度は編集軸の長さを変えない。元波形のUVを残存source区間ごとに切り出して編集長・局所×master gainに合わせて描き、panel内へclipする。これは元波形のoverviewで、tempo後のPCMから再計算した精密波形ではない。previewは編集時間でsampleを選んでsourceへ対応付ける。新plan上で旧source trimの追加・grip表示は行わない。時間選択とDelete／Keepを通常UIへ公開した。rubber-band／部分stretchと下記の範囲再生も接続した。

V03選択操作契約（2026-09-10）: 音声と表示中の動画timelineでは、CTIの8px以内から始めたdragをSeek、それ以外の横dragを時間範囲選択とする。clickはSeekと選択解除、CTI dragは既存範囲を保持。press時に役割を固定し、releaseで一度だけcommit、Escape／focus loss／modal／tab切替で未確定操作を取り消す。確定範囲は編集履歴ではなくtab状態として保持し、source交換／closeで破棄する。Delete／Keep only selected timeは選択とtimeline表示を必要とし、DeleteとCtrl+Yを既存のcommand経路へ接続する。Ctrl+Yは時間範囲がなければ従来のvisual cropを維持。Ctrl+A／Escapeは時間全選択／解除、I／Oは非破壊の選択端点で、旧設定用のset_trim_start/end識別子は維持する。UIA端点は同一frame内でも順に候補を検証し、selection actionはTabId／generation／duration／modalを確認する。選択枠は下記U11の1物理px反転線を共有し、通常windowと全selection focus/style監査は残件。部分gain／stretchは下記の契約で接続した。範囲再生は下記で接続したが、最終操作／style／export照合までV03完了とはしない。

V03部分調整契約（2026-09-10）: CTIを優先し、音量線4px以内のpressはclick閾値を越える最初の方向で縦のgain／横の選択へ固定する。gainは選択内にだけ、未選択なら全体に絶対値0～200%を設定し、最低位置をmuteとする。選択外の線では新しい範囲選択を優先する。Altのみを押して選択内からdragすると開始点を固定して右端を相対移動し、全区間の既存相対速度を保ったstretchにする。修飾keyもpress時に保持し、途中の解放では役割を変えない。previewは線／選択枠と数値だけで、音声・historyはrelease時に一度だけ変更。coreのcandidate検証で丸め・overflow・局所速度範囲を最終判定し、無効／変化なしのgestureは追加しない。gain後は選択を保持し、stretch後はその長さへ更新、Undo/Redoによるplan変更では選択を解除する。

Local volume (%)／Selected duration (seconds)をUIAとTab focusへ公開し、左右／Home／Endで変更可能。混在gainの表示は先頭値とMixedを示し、明示SetValueは同じ先頭値でも全選択をその値へ統一する。同一frameの部分調整数値操作は最初の有効な変更だけを採用する。新しい調整actionはTabId／generation／期待する選択／modal／timeline表示を確認する。master音量・速度は別の既存調整として保持する。範囲再生は下記で接続し、全focus/style、native保存再openの総合監査とseam品質は引き続き残件。

V03範囲再生契約（2026-09-10）: Play selected time／Shift+Spaceは表示中timelineの確定選択を先頭から再生する。Spaceは範囲内のpause／resume、終端からなら選択先頭へ戻る。Escapeの選択解除、選択変更、選択外へのSeek、時間編集によるplan変更で一時的な範囲再生を解除する。master音量・速度は維持し、時間軸・全長・history・export対象は変更しない。範囲はtab sessionとともに保持し、source交換／closeで破棄する。範囲末尾ではRepeat off/all/oneによらず停止し、audio queueの自動送りを抑止する。動画timelineを隠すだけでは再生範囲を変更しない。

runtimeは元のimmutable planを保持したまま、各segmentの処理終端を選択末尾へ制限する。映像PTSは元の編集時間、音声は同じ初期Seek／元の局所速度を使い、選択終端の予定sample数で切る。選択部分だけの別planへ作り替えて零へ戻さない。terminal preview／device replacement／非表示復帰でも境界を保持し、範囲外のplan要求は状態変更前に拒否する。active／backgroundのEOFは共通の既知durationと実再生rangeの最小値を使い、最後の映像frameの表示開始時刻だけで時計を止めない。音質・任意素材でのgaplessや物理loopbackを保証するものではない。

U11選択枠契約（2026-09-10）: 画像・動画・時間選択はruntimeのsafe painter helperを共有する。境界を現在のpixels-per-pointで物理pixelへ丸め、内側の互いに重ならない4本のstripとして1pxの枠を描く。極小の正の選択は最低1pixelを維持し、clip範囲の外や内部を塗らない。選択外の暗幕・見た目のgrip・辺focus用の追加四角は除去し、操作用hit領域／keyboard／UIA値は維持する。画像のfocus中の辺と値はstatus欄へ表示し、fullscreenを含む最終focus監査は残る。crop preview／copy／exportの対象画素や編集は変更しない。

UIのdraw order内にplain meshのInvertMesh callbackを置き、vendor rendererはそのpayloadだけを認識する。同じdeviceの既存shader／font atlas白texel／scissorを使い、gamma UNORM targetのRGBを1-destinationへ反転、alphaは保持する。通常meshごとに通常blendを再設定し、後続menu等の描画を反転しない。source／backbufferのcopyやCPU readbackは本処理に不要で、appにはCOM pointerやnative handleを公開しない。任意GPU callbackの実行APIは追加しない。WARP／実GPUのtest readbackは本体のzero-copy経路とは別の検証用であり、通常windowやOSの混在DPIを証明するものではない。

## 1. 目的と優先順位

V04フレーム探索のruntime契約（2026-09-10、UI接続済み）: adjacent_video_frameは指定時刻より厳密に前／後にある、選択video streamの異なるPTSを返す。source時間または既存EditTimelineの編集時間を受け、削除区間を飛ばし、局所stretchの整数対応付けと半開区間境界を共有する。fpsから固定間隔を作らず、時刻なしframeは明示エラーにする。次／前の候補がなければNone。逆方向はkeyframe境界で候補が得られない時だけprerollを区間先頭まで拡大する。通常containerは対象streamを指定してSeekし、TSは既存のkeyframe byte-position処理を使う。

これはworkerで実行する同期探索APIであり、UI threadで呼ばない。独立したsoftware decoderからPTSだけを読み、RGBA変換・readback・画像cache・WASAPI・別D3D deviceは作らないが、codec内部のframe decode負荷は発生する。packet/frame/再試行境界で取消し、進行中のFFmpeg I/Oやcodec callの即時中断は保証しない。single-device presentationは維持する。appは表示済みPTSを基準に一時停止し、専用workerへ要求する。最大32方向入力を順序通り処理し、次の要求には解決済みPTSを使う。tab／media instance／playback generation／request serial／modal／paused状態を照合し、Seek・他command・編集・tab変更・focus喪失・pointer pressで取消す。1回のpaused preview以外は表示frameを保持し、master clockの先行で余分なframeを進めない。通常video Seekと探索は共有helperで選択streamのbackward Seekを行い、最初のkey packetのPTSが目標を越える場合は前GOPへ戻す。DTS基準の索引によるB picture欠落を防ぎ、検査後は同じ位置へ再Seekする。TSの既存byte-position経路は維持する。長GOP性能・音声側契約・通常windowの最終操作確認は未完。

V04追加binding契約（2026-09-10）: J／K／LはSeekBackward／TogglePause／SeekForwardへの追加KeySequenceであり、別commandや固定key例外ではない。主bindingのexact／prefixを追加bindingより優先するため、動画timeline表示中と画像では主bindingのL回転、動画視聴中と音声では追加bindingのL Seekが解決される。custom主bindingも同じ優先規則を使う。setは追加bindingも含めて置換、addは重複を除いて追加する。menu／palette／status hintは解決可能な追加bindingだけ表示し、disabled commandの主bindingは学習用に残す。v2／v3設定は明示headerと縦棒区切りの列、旧形式は従来の単一prefix列として読む。旧ファイル中の未変更標準Left／Right／Spaceだけ新しいJ/K/Lを補い、既存fileの自動書換えは行わない。 v3では動画のcomma／periodをPreviousVideoFrame／NextVideoFrameへ、速度をCtrl+comma／Ctrl+periodへ割り当てる。v3以前の単独・未変更の速度comma／periodだけ新標準へ移行する。変更済み／複数キーを保持し、v3で明示したcomma速度も保持する。主キー同士の既存exact／prefix優先規則は変えず、新frame commandは既存commandの後に登録する。音声にframe commandは提供しない。

H1/V04（2026-09-10）: 動画のvisual selection／crop／90度回転／flipは、実際にtimelineが表示されている状態に限定する。menu／palette／grid／custom shortcutは共通CommandDefinition判定を通し、pointer／keyboard／UIAの選択操作にも同じ可視条件を適用する。fullscreenで隠れたtimelineは編集contextではない。閉じる際は進行中の選択dragを取消し、辺focusを解放するが、確定選択と編集結果は保持し、枠は編集contextへ戻るまで表示しない。視聴中のSeek／音量／master速度／再生／保存は維持し、明示的なUndo/Redoも修正を戻せるよう有効とする。画像と音声のcontextは変えない。無効な選択処理を毎frame呼ぶだけでcompact seek gestureを取消してはならない。J/K/Lは下記の追加binding契約で接続した。動画frame移動と上記の一時倍速を接続したが、音声frame契約・長GOP応答性・最終操作検証まではV04未完。

towavueはWindows向けの画像・動画・音声ビューア兼プレイヤーである。設計上の優先順位は次のとおり。

1. 正確で安定した再生、Seek、同期
2. 起動・アイドル・再生時の低オーバーヘッド
3. GPU常駐を維持する単純な表示経路
4. メディアを遮らない最小UI
5. 読めるコードと検証可能な境界

対象はWindows 10 22H2以降のx86-64。Rust 1.98.0、Edition 2024、MSVC ABIを使用する。

## 2. 採用技術

| 領域 | 決定 | 理由 |
|---|---|---|
| Window/Event | winit 0.30.13 | Win32の詳細へ降りられる薄いイベント境界 |
| UI | egui 0.35.0 | 最小UIとカスタム描画を少ないコードで構築できる |
| UI renderer | egui-directx11 0.13.0 | wgpuを介さずD3D11 render targetへ描画できる |
| Windows API | windows 0.62.2 | COM、D3D11、DXGI、WASAPI、Shell APIの公式Rust projection |
| Media | FFmpeg 9.0.1 / ffmpeg-next 9.0.0 | demux・codec・resampleの広い対応範囲 |
| Image | image 0.25.10 + FFmpeg AVIF decode | 静止画、アニメ画像、orientationを安全なRGBA frameへ統一 |
| Graphics | D3D11 + DXGI Flip Model | decode surfaceからpresentationまで同じdeviceを維持できる |
| Audio | event-driven WASAPI Shared | 他アプリと共存しつつ十分に低遅延 |

依存は使用を開始するmilestoneでのみ追加し、`Cargo.lock`へ固定する。`egui-directx11`は薄いadapter内に閉じ込め、必要ならUIやmedia境界を変えずに自前rendererへ交換できるようにする。

`eframe/wgpu`は採用しない。D3D11VAが返すNV12/P010等の外部・マルチプレーナresourceをD3D12側へ安全に取り込む経路が中核リスクになるためである。独自UI toolkitも、文字入力、DPI、accessibility、複数windowまで自前化するコストに見合わない。

## 3. Workspace境界

### towavue-core

OS非依存の値、状態遷移、コマンド、編集履歴を置く。unsafe、COM、FFmpeg、native handleを禁止する。

主な中核型は次のとおり。

- `MediaTime(i64)`: ナノ秒単位の時刻。
- `SessionId`: 開いているmedia sessionの識別子。
- `PlaybackGeneration(u64)`: Seekや再openの前後を識別し、古い結果を破棄する番号。
- `MediaCommand`: open、play、pause、seek、rate、volume、stop、close。
- `MediaEvent`: opened、state、position、ended、fault。
- `MediaInfo`: 種別、duration、stream、codec、解像度、sample rate、色空間。
- `CommandId` / `CommandContext`: menu、palette、shortcutが共有するcommand identity。
- `FolderSnapshot`: Shellが返したfolder identity、ordered media、sort columns、source、generation。
- `TabSet` / `ShortcutBindings`: platform非依存のtab targetとprefix対応key sequence。
- `ImageViewState` / `ReadingSettings`: 正規化selection、zoom・pan・crop preview、2～10 pageの表示軸と反転。
- `EditHistory` / `EditOperation`: source非破壊のcrop、90度回転、反転、trim端点、volume、rateとsaved cursor付きundo/redo。
- `PixelCrop`: preceding visual edit後の整数pixel crop矩形。UIの正規化selectionを確定し、previewとexportで共有する。

### towavue-runtime-windows

FFmpeg、D3D11/DXGI、WASAPI、Windows Shell、worker、queue、clockを所有する。FFI/COMとunsafeはこのcrateから外へ出さず、安全なcommand/event境界へ変換する。

GPU frameはruntime内部のRAII `FrameLease`で保持する。FFmpegの`AVFrame`、`ID3D11Texture2D`、array slice、COM pointerをappやcoreへ公開しない。

### towavue-app

winit event loop、egui、tab、command dispatch、利用者向け状態を所有する。runtimeへcommandを送り、eventと描画結果だけを受け取る。

M4ではmenu、command palette、shortcutの全入口を同じ`CommandId`へdispatchする。画像・動画の外部openはfile単位の新規tab、音声は同じfolderの既存playlist tabを再利用し、明示的な新規openだけ別tabにする。filmstrip、playlist、全種移動、同種移動は一つの`FolderSnapshot`を共有する。

M5ではruntimeがBMP、GIF、JPEG、PNG、TIFF、WebPを`image` crate、AVIFを既存FFmpeg software pathでdecodeし、native handleを含まないRGBA frame列とframe durationだけをappへ返す。appはegui texture、animation deadline、画像操作状態を所有する。画像と動画が共有するのは同じD3D11 device、visual surface、Presentだけであり、画像frameを動画decode queueやVideo Processorへ流さない。

M6では各tabが独立した`EditHistory`と直近export先を持つ。appは画像の履歴をUV meshへ順番どおり適用してpreviewし、動画上のcrop selectionを同じ正規化矩形で保持する。動画・音声のtrim、volume、rateを含む全operationはsource playbackを変更せずstatusへ反映し、runtimeのFFmpeg exportで初めて出力へ適用する。dirtyなtabの移動・closeとprocess終了は、入力を遮るExport / Discard / Cancel modalを必ず通る。

## 4. 再生・表示契約

- app event loopがruntimeの安全なfactoryを呼んでD3D11 deviceを作成し、M2ではFFmpegの`AVD3D11VADeviceContext`へ正しいCOM参照寿命で渡す。appへCOM pointerは公開しない。
- decode、D3D11 Video Processor、DXGI presentation、eguiは同じadapter/deviceを使う。
- immediate/video contextの利用箇所を限定し、FFmpegとの共有に必要なmultithread protectionを有効にする。
- SwapChainはFlip Discard、2～3 buffers、frame-latency waitable objectを使う。
- FFmpeg D3D11VA surfaceをVideo Processorへ直接渡す。hardware pathでCPU readbackや再uploadを行わない。
- hardware decodeが成立しない場合だけsoftware decodeへfallbackする。CUDA/QSV decode fallbackは設けない。
- HDR metadataは失わないが、正しいtone mapping/pass-throughが完成するまではHDR対応を宣言しない。

### M1 software path

M1ではFFmpegを動的リンクし、映像をsoftware decodeしてtightly packed RGBAへ変換する。runtimeの`FrameRenderer`が単一D3D11 device、immediate context、2-buffer Flip Discard swap chainを所有し、CPU frameをRGBA textureへuploadしてfull-screen shaderでpresentする。このCPU uploadはsoftware fallbackだけの基準経路であり、M2のhardware pathでは使用しない。eguiは同じdeviceとback bufferへ続けて描画し、1回のPresentに合成する。

音声はsource sample rateのinterleaved stereo `f32`へ変換し、専用MTA thread上のevent-driven WASAPI Shared clientへ渡す。映像queueは2 frame、音声channelは32 chunk、WASAPI手前の蓄積は約2秒へ制限する。M1のdecode workerはdemuxとvideo/audio software decodeを直列実行するが、COM、FFmpeg型、native frame handleはruntime外へ出さない。M3の役割別workerでも、この安全なcommand/event境界を維持する。

### M2 D3D11VA path

`FrameRenderer`が作成したD3D11 deviceのopaqueな`GraphicsDevice`参照をdecode workerへ渡す。FFmpeg用`AVD3D11VADeviceContext`にはcloneしたCOM参照の所有権を移し、`AVCodecContext`が`AVBufferRef`とともに解放する。immediate contextにはmultithread protectionを有効にする。

hardware frameはruntime内部のFFmpeg `AVFrame`がD3D11 texture arrayとsliceを保持し、bounded presentation queueを経て同じdeviceの`ID3D11VideoProcessor`へ渡す。appが受け取るのはpresentation timeとeventだけで、COM pointer、FFmpeg frame、texture handleは公開しない。hardware pathではmap、readback、software scaling、back-buffer uploadを行わない。

codec metadata、device、driverのいずれかがD3D11VAを成立させられず、まだhardware frameを公開していない場合だけ入力を開き直してM1 software pathへfallbackする。最初のhardware frame後のdecode errorはfallbackで隠さずsession faultとする。終了時にadapter LUID、hardware frame count、CPU transfer countを記録する。

### M3 synchronization and recovery

appのmedia読み込み番号はwindow内の単調な採番元からopen（失敗を含む）と最後のtab closeごとに割り当て、再生callback・duration・waveform・hover thumbnailの結果へ付ける。保持した再生tabへ戻る時は元の番号を使い、採番元は巻き戻さない。再生通知はactiveまたは保持tabの番号へ配送し、close／path置換後の旧結果は破棄する。再生はその上でruntimeのaccepts_eventでstream世代を照合する。音声／全decode完了はSeek/recovery世代、映像の準備・decode path・device fault／VideoFailedは映像worker停止時にも進む独立世代を使う。全decode完了通知はwakeとして扱い、現在のruntime完了状態を再確認する。映像だけの復帰前にqueueへ入った完了／故障を、新しい映像へ誤適用しない。

renderer再作成不能時は、GPUを使わない所有window付きnative確認を専用workerで表示する。Retryは失敗前のsource位置と再生/停止状態を使い、Cancelは編集を保持する。以後の終了要求はnativeのExport/Discard/Cancel確認から既存のSave As・background exportへ接続する。export失敗もnative通知にし、未保存編集を消さない。native確認とfile dialogは同時に一つだけとし、確認中の別操作を受け付けない。通常描画時の未保存確認については、上記2026-09-11 U02のnative採用契約が後継となる。

native確認中にexportが完了した場合、保留された終了/移動は確認を閉じてから現在のdirty状態で再判定する。保存済みの旧tabへ再度保存を求めず、未保存tabが残ればそちらを確認する。file dialog・実行中export・未確認export errorがある間は継続せず、Cancelで取り消したguardを復活させない。

export取消が出力先の置換に間に合った場合だけCancelledとして既存fileを保持する。置換後の取消は出力を巻き戻さず、成功したexport履歴を保持するが、保留中の終了/移動は実行しない。確認中に既に完了していても、nativeの取消選択は自動離脱を止める。表示では保存完了と取消要求を区別する。

graphics recoveryでは旧decode/output workerを停止し、旧rendererの参照を解放してから同じwindowへ新しいflip swap chainを作る。[D3D11の遅延破棄契約](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-flush)に従い、runtime内でClearState/Flushを行う。PresentやUI描画からのerrorもdevice removal理由を確認して同じ復旧経路へ渡す。新rendererにはfont atlasと保持中の画像/reading frameを再送し、再生成できるfilmstrip・waveform・hover previewだけを失効させる。tab・編集・選択・zoom・再生設定は保持する。GPU全体が再作成不能ならpanicや自動再試行loopにせず、Faultedのtitleと診断を残し、上記native確認へ移る。renderer不在でも描画要求は安全に戻る。

動画の`seek_latency_ms`はappの`seek_to`開始から、新generationの映像を描画した最初のPresent成功までとする。同期的なworker停止・再構築、decode待機、UI描画とPresent待機を含め、VideoReady通知では完了しない。失敗・別mediaへの移動・device recoveryで中断した要求と、映像を持たない音声は標本にしない。連続要求では未表示の旧要求を置き換える。rate/trim編集など同じSeek経路を使う再構築も含むため、性能ゲートは操作を限定した試験で測る。物理入力の配送前やDWM/scanoutの実表示時刻はこの計測範囲外である。

再生では映像と音声に独立したFFmpeg input/demuxとdecode workerを持たせ、各streamの既存packet/decoded output queueとpresentation/WASAPI queueをboundedのまま接続する。片側の出力待ちで、もう片側のdemuxや映像の初期表示を塞がない。H1 U07ではworkerの寿命も独立させ、pipeline開始時に音声producerを一つだけ開始する。映像のhardware fallback／表示復帰では音声producer・WASAPI output・clockを再作成せず、従来の「hardware成立まで音声開始を待つ」契約を置き換える。確認後の映像故障は既存fault/recoveryへ渡す。映像のみ・音声のみ、長さの異なるstreamも同じ境界を使い、全decode完了通知は両stream完了後に一度出す。二系統の入力読取による負荷は性能gateで確認する。packetはdropせず、遅れたdecoded video frameだけをdropする。既存の両stream一括decodeはfixture検証用に残す。

PlaybackSession::set_video_visibleは非表示時に映像queueを解放して映像workerだけを取消・joinする。runtime内のParallelInputがFFmpeg入力を排他的に所有し、scoped demux workerへ一時的に貸し、join後はsessionへ戻す。復帰時は同じ入力を現在source位置へSeekし、decoder／queueを再構築する。D3D11VAからsoftwareへのfallbackも同じ入力を使う。EOF／取消後も先頭へ戻せるよう、再利用時の0秒Seekを明示し、TSは時刻Seekによる先頭GOP欠落を避けてbyte位置0へ戻す。開いた入力はclose／path変更で解放し、読み込み途中の取消・open失敗・worker開始失敗等で入力を保持できなかった場合は再open対象となる。音声／sessionの世代、pause・volume・rate・rangeと音声workerは保持する。取消はpacket/output境界で確認し、receiverを先にdropして満杯のbounded producerを解放する。実行中のFFmpeg／storage callまで即時中断する保証はない。Seekとdevice交換は全pipelineを停止・再開するが、映像入力とhidden指定は維持する。入力はdeviceを所有せず、新decoderは引き続きwindowと同じdeviceを使う。U07で終端が既知の非active動画に適用する。復帰時のdecoder再構築／Seek／最初のframe待機短縮は未完である。

U07の復帰表示では、非表示化だけなら最後に選んだ映像1frameも保持し、worker／queueは停止・解放する。復帰直後はこのframeを同じdeviceで再描画し、現在source位置の結果が届いたら置き換える。同じ一時停止位置へ戻る時は保持frameの時刻をdecode開始に用い、clockがframe境界の間でも1コマ先へ進めない。保持frameは新しいSeek結果や新規presented frameとして数えず、置換待ちflagを持つ。一時停止位置がframe境界と異なっても最初の新frameを受理し、背景EOFへの復帰では終端previewを古いframeの存在によってdropしない。Seek・device復旧・closeでは旧frameを解放する。hardware frameはdecoder surface poolへの参照を保持し得るため、これは1frame分だけのGPUメモリ上限を保証しない。非activeで新frameをdecodeする最適化には戻さない。

RetainedPlaybackTabはsafeなPlaybackSession・clock・pause/EOF/error・表示view・bar開閉・取得済みduration/waveform・playlist操作状態・folder snapshot・計測をtab IDに結び付ける。音声通知は非activeでも処理し、故障／endpoint復旧は対象sessionへ限定する。length workerは読み込み番号ごとに最新一件を持ち、非activeでも完了を受理して終了後に回収する。終端が既知なら映像を停止し音声clock、drain後または無音動画では保持clockで終端へ進める。長さ不明なら有界映像queueをclockに合わせて一回最大4 frameだけ進め、実decode EOFを使う。これらの処理は背景の描画を要求せず、playingの保持sessionがある間だけ最大20ms間隔でserviceする。activeと背景のposition計算を共有し、復帰時の音声Seek／Openを行わない。

graphics復旧では保持sessionの位置を固定して全pipelineを先に停止し、一つの新deviceへ接続し直す。失敗中の背景clockは進めず、Retryで同じ位置を使う。取得済みCPU情報／viewは保持し、waveform textureは復旧時に再生成する。close／同tabのpath変更は対象session・長さworkerを解放し、ApplicationのDropで全sessionをrendererより先にjoinする。queue単位の上限はprocess全体の上限ではなく、複数tabの入力・WASAPI workerは増える。全focus状態、映像復帰の待機、device／endpoint失敗時の全組合せは追加監査対象である。

通常再生は`IAudioClock`をmasterとする。running中のdevice positionが供給停止で進まない場合に限り、audio clientのstart/stopとpauseを追跡した単調時計を下限にして永久停止を防ぐ。Seekはgeneration更新後に旧workerと全queueを破棄し、`avformat_seek_file`、decode/discard、audio/video primingを新しいpipelineで行う。default render endpoint変更とD3D11 device removalはtyped eventとしてappへ渡し、現在位置と新しいendpointまたはD3D11 deviceでpipeline全体を再構築する。

audio masterに対して40msを超えて遅れたdecoded frameは、待機スケジュール時だけでなく描画直前のpromotion前にも既存queueから破棄する。待機判断の後にUIが遅延しても古い判断でframeを進めず、破棄数を既存metricsへ含める。PausedのSeek previewとaudio masterがない経路にはこの破棄を適用しない。

音声の正常排出後も残りの動画はpause/resumeできる。WASAPI workerはcontrol receiverを閉じる前に正常終了を公開し、その場合だけ閉じたcontrol channelへのpause/resumeを成功したno-opとして扱う。device変更・失敗・予期しない終了はこの扱いに含めず、既存のerror/recovery経路を維持する。

WASAPI呼び出しが`AUDCLNT_E_DEVICE_INVALIDATED`を返した場合も、通知到着の有無によらず既存のEndpointChanged経路へ渡す。[Microsoftの既定device復旧手順](https://learn.microsoft.com/en-us/windows/win32/coreaudio/recovering-from-an-invalid-device-error)に従い、旧clientを解放して現在の既定endpointで再作成する。文字列照合は行わずHRESULTで分類し、それ以外のAPI失敗は従来どおり診断を残して停止する。再作成そのものの失敗は自動retry loopにしない。

Seek再構築では必要なPaused状態も新pipelineへ渡し、破棄予定の旧WASAPI workerへのPause成功を前提にしない。endpoint無効化後でもtrim範囲外のsource previewを同じ位置・停止状態で復元する。通常のPause/Resumeは引き続き実行中workerを操作し、異常終了を成功扱いしない。

停止中のWASAPI session切断も、`IAudioSessionEvents::OnSessionDisconnected`の登録をclient寿命中保持して既存endpoint復旧へ渡す。callbackでは通知をqueueへ置くだけにし、COM解放・再作成はcallback外の既存worker/app経路で行う。音声workerは結果を公開した後にgeneration付きAudioReadyを送り、UIのfolder pollや次の入力を待たず受信させる。旧generationのwakeは既存のevent失効条件で拒否する。

保存/破棄のguardを解決して終了が確定した場合だけ、`about_to_wait`でControlFlowをPollへ切り替えてからevent loopを終了する。固定版winitのWindows実装はAboutToWait後にも待機処理を呼ぶため、以前のWait/WaitUntilを引き継がせない。通常idle/pauseの待機方針は変えず、workerの同期回収も省略しない。

### H1 tab ordering

tab bar内のprimary dragは挿入位置だけを表示し、release時に一度だけTabSetの順序を変更する。TabId、active tab、編集履歴、export先、再生sessionは保持し、activate/reloadや保存確認は行わない。bar外・window内へのdropとEscapeは並べ替えを取り消す。window外へのdropは既存のguard付きdetachを使い、window間結合は追加しない。長いbarは既存の横scrollを使う。

active tabのidentity・bar内index・tab幅または表示幅が変わった場合は、そのtab全体が見えるまで必要最小限の横scrollを行う。同じ状態の描画では手動scrollを保持する。追従はUIのscroll状態だけを変え、media load・選択・編集・並べ替えを発生させない。

### I05 resize/resample contract

表示用interpolationはResize履歴とは独立したwindow-local設定とし、Smooth（線形）を初期値、Nearestを明示切替とする。共有ToggleImageInterpolation commandをView menu／palette／custom bindingへ公開し、読書中も許可する。元画素・編集・選択・zoom・コピー／exportには影響させず、原寸画像と見開きの全frameに適用する。textureを共有するcache cloneはRc<Cell<TextureOptions>>も共有し、変更時だけ再転送する。固定eguiのTextureMeta.optionsはset後も初期値のままなので、それを現在値の判定には使わない。animationと復旧textureへ同じ設定を渡す。低解像度loading／thumbnail、文字・UIは線形を維持する。

固定egui-directx11はtexture optionsを無視して単一linear samplerを使っていたため、同じversionのsource-only修正版をvendorへ保持する。managed textureのfull／partial更新でoptionsを保持し、drawごとにmin／mag／wrap対応samplerを選ぶ。原HLSLは元のshader model 5.0／O3でD3DCompileし、compiled shader binaryはcommitしない。device・描画順・public APIは維持する。上流license／出典・変更説明をvendorへ保持し、第三者notice generatorはlocal patchを除外せずfile hashesで検証する。WARP offscreen試験をCIへ追加して混在samplerとpartial更新後の実RGBAを検証する。物理GPUや混在DPIの代替認定とはしない。

Resizeは画像専用の非破壊EditOperationとし、指定幅・高さとNearest／Bilinear／Bicubic／Lanczosを履歴順に保持する。各辺1～16384、RGBA出力は一枚512 MiB以内とし、animation全frameの処理結果も合計512 MiB以内に制限する。source decodeや原本fileを置き換えず、Undo/Redoで復帰できる。表示zoom／nearest表示の選択は別のpresentation設定であり、Resize履歴へ混ぜない。

Resizeを含む画像のmaterializationはruntimeのFFmpeg filter graphで行い、exportと同じvisual filter列を使う。crop／回転／反転／複数Resizeの順序を潰さず、Nearest以外は16-bit planar RGBAのpremultiply→scale→unpremultiplyを通して透明画素の色のにじみを抑える。Nearestはstraight RGBAを保持する。最終RGBAのstrideを正規化して返し、FFmpeg frameやgraphをappへ渡さない。表示／copy／exportの画素一致を透明PNGと編集順序で検証してからUI経路を完成扱いにする。

Ctrl+R／Edit menu／paletteは共有ResizeImage commandを使用し、幅・高さ／元の縦横比固定／filterのmodalからApplyしたときだけ履歴へ追加する。Cancel／Escapeは無変更、背景操作・読書中のresizeは無効とする。幅・高さの支援技術SetValueも通常入力と同じ比率更新・寸法検証を通す。window closeはApply／Cancelまで保留する。

appはoriginal decodeのArcをUndo用に保持し、LatestTaskの取消と世代番号で最新のpath／編集列だけに処理結果を採用する。Resizeを含む全履歴をmaterializeし、処理済みframeへ同じ編集を二重適用しない。copyも処理済み画素とその座標のselectionを使う。処理中は待機表示として寸法依存操作・copyを拒否し、失敗時は古い画素をcopyせず診断を表示する。最後のResizeをUndoすると元Arcと通常UV編集へ戻る。frame index／delay／次deadlineを保持し、描画復旧用textureも現在の処理済みframeから再構築する。path変更・最後のtab close・原画像再読込で取消世代を更新する。単一D3D11 deviceや原画像texture cacheのidentityは変更しない。

### I05 image clipboard behavior

Copy image（Ctrl+C）はactive画像の現在frameを原寸で取得し、表示と同じ順序のcrop／90度回転／反転を反映する。通常画像にselectionがあれば、その編集後座標の領域だけをcopyする。zoom／pan／crop previewの表示倍率や低解像度previewはコピー画素へ適用しない。readingではactive画像だけを対象にし、隠れたselectionや見開き全体はcopyしない。文字入力のcopyは引き続き優先する。

appは既存ImageTransformから整数出力寸法とsource UV、decode済み画像のArcとframe indexをsnapshotし、runtimeの一件のcopy workerへ渡す。workerでstraight RGBAを作り、PNG／DIBV5対応の既存固定arboardへ渡す。egui-winitのColor32画素直渡しによる透過色のpremultiply混同を避け、alphaを含む画素を保持する。処理中の追加copyはqueueせず案内し、失敗はstatusへ通知する。tab移動後も明示的に要求したsnapshotのcopyは完了できる。終了時は作成中の画素処理を取消・joinし、OSへのpublish開始後は完了を待つ。source、編集履歴、選択、再生位置、保存先を変更せず、copy結果を自動保存しない。

### H1 Welcome entry

mediaがない時は中央の最大660 logical pxの左揃えcolumnへwordmark、START、Open file/folder、RECENT、drop案内をまとめる。狭いwindowでは余白を縮め、縦scrollで操作を残す。Openは既存CommandIdとnative pickerを使い、shortcut表示は現在のbindingsから求める。上部のWelcomeはU06で非mediaの安定したtab identityへ更新した。唯一のWelcomeのcloseはno-opで、初回Openでmedia tabへ置き換わる。recent履歴とpreviewの契約は下記U06に従い、session復元は追加しない。

最後のtabを閉じてWelcomeへ戻る時はtimelineを閉じ、waveform/hover preview、duration、再生時計・Seek計測、一時statusを破棄する。mediaがない時にtimelineを描画せず、session不在の再生通知は受理しない。path付きpreview結果は既存のpath/世代検査で破棄する。window・shortcut・reading等の設定は維持し、再open時は既存の初期化を使う。

### M5 image presentation

静止画はEXIF orientation適用後、アニメGIF、WebP、APNGは合成済みRGBA frameと10 ms以上のdeadlineへ変換する。app event loopは次frame時刻までsleepし、期限を過ぎたframeを追いつかせてからegui textureを更新する。画像textureも動画・UIと同じD3D11 deviceとback bufferへ描画し、Presentは一回に保つ。

selectionは元画像に対する正規化矩形として保持し、表示scaleから独立させる。左dragで作成、辺dragで変形、Shift付き作成で画像pixel上の正方形、Shift付き辺dragで現在比率を保持する。右dragはpan、Ctrl+wheelはpointer anchorのzoom、選択範囲clickとCtrl+Yはpixelを変更しないcrop previewである。実crop、undo、save/exportはM6まで開始しない。

H1ではShift付き選択の画像端制約を、片方の軸だけの切り詰めではなく共通の最大寸法で適用する。正方形作成は押下点を保持し、比率付き辺resizeは反対側の辺と直交方向の中心を保持して、どちらかが画像端に達したところで拡大を止める。比率はdrag開始前の範囲から求め、一時的に幅/高さがzeroになっても失わない。通常drag、取消、release時の既存pixel整列、crop/exportの規則は変更しない。整数pixel/動画偶数pixelへの最終整列による丸めは残る。

reading modeは表示専用で、同じ`FolderSnapshot`から画像だけをShell view順のまま2～10 page取得する。下記の固定見開き分割を使い、横・縦配置と表示順反転はpresentation状態だけを変更し、個別画像のselectionや編集状態を作らない。

2026-09-09 follow-up追記に合わせ、未保存編集のあるactive画像ではclick／drag／commandによるreading開始を無効にする。reading中の画像編集（回転・反転・crop・Undo/Redo）とselection/crop previewを共有command contextで無効にする。保存済みの編集は表示に反映したままでよく、履歴を削除しない。別の未保存画像tabへの移動は拒否せず、その画像のload時にreadingを解除して編集を保持する。従来のdirty anchorからのreading許可はこの契約で置き換える。readingの枚数・配置設定、閲覧navigation、mode解除は引き続き利用できる。

H1のreading表示は横並びで高さ、縦並びで幅を揃え、各画像の縦横比を保って隙間なく連結する。連結した全体をmedia領域へaspect-fitして中央に置き、外周の固定余白は加えない。反転は並び順だけを変える。読込失敗pageは正方形の場所を残し、後続pageを詰めて順序を偽らない。folder seekのreading previewにも同じ配置を使うため、画像用filmstrip cacheはpaddingなしの縮小画像を保持する。動画/音声preview、読み込み件数・worker・cache上限、page送り・編集状態は変更しない。

### M6 non-destructive editing and export

H1 A01では音声の再生順・repeat・shuffleをtab IDごとに保持する。既定はShell順の自動次曲／Repeat off（末尾停止）、allは末尾から先頭、oneはEOFで同じ曲の有効rangeを再開する。手動の前後はoneを無視して再生順を辿り、allだけ端で循環する。shuffleは現在曲を先頭にした重複のない一巡で、無効化するとShell順へ戻る。同じfolder更新は既存shuffle順を保ち、削除曲を除き新曲を末尾へ加える。これは過去に手動選択した曲の別個の履歴stackではない。順序の候補取得は消費しないためguard取消で変わらない。

音声tabは独立した非同期Shell order providerを持ち、初回のfolder取得前に非activeとなってもEOF後に順序を受理できる。通常のactive folder更新もそのqueueへ反映し、別曲を選ぶ自然EOFではproviderへ非同期に最新順を再要求してから送り先を決める。providerはtab close／異なるfolderへの置換で解放し、tab数に応じてworkerが増える。再生モードはwindow内だけで、再起動・閉じたtabの再openへは保存しない。Repeat／Shuffle button、View menu／paletteを共有commandへ接続し、Ctrl+Rは音声repeat／画像resizeでcontextを分ける。

自然EOFで音声がPlayingだった場合だけ自動送りを行う。一時停止中のEOF Seekは送らず、同じinstanceの消費済みEOFを二重処理しない。device generation交換だけでは停止済みqueueを再開せず、実際のPlaying開始で次のEOFを受け付ける。単曲loopは次のUI tickを待たず再受付可能にする。別曲へ移る前に未保存履歴／当該tabのexportを確認し、自動移動は停止してstatusを残す。手動移動は既存guardを使う。同曲repeatはsource／historyを変えない。背景の次曲は同じtab IDに新しいmedia instanceとPlaybackSessionを割り当て、旧通知を拒否する。選択中tab・画像・表示stateへ切替を起こさず、同じwindowのD3D11 deviceを使う。別曲では旧履歴／export先・取得済みduration／waveformを流用しない。新曲のopen/decode失敗を無限skipせず、対象tabをFaultedで止める。音声のgapless再生はこの契約では保証しない。

`EditHistory`は適用済みcursorとsaved cursorを別に持つ。新しいoperationをundo位置から追加した場合はredo branchを破棄し、破棄されたbranchにsaved cursorがあれば保存済みidentityも失効する。tab titleとwindow titleの`*`およびstatusのUnsavedは、現在cursorとsaved cursorが一致するまで消えない。folder内移動は同じtabの履歴を破棄するためcloseと同じguard対象だが、tab切替は履歴を保持するためguardしない。

外部Openが同じfolderのcleanな音声tabを再利用して別sourceへ移る場合も、通常navigationと同じく編集履歴と直近export先を破棄する。保存済みtrim/volume/rateを次の曲や保存物へ再適用しない。同じsourceの再openは履歴を保持し、dirty/export中のplaylistや明示的新規tabは従来どおり別tabを作って元の編集を保護する。

exportはruntimeだけが`ffmpeg.exe`を子processとして起動し、app/coreへFFmpeg型を公開しない。画像filterはoperation順のcrop / transpose / flip、動画filterはそれらとtrim / PTS rate、音声filterはatrim / PTS / atempo / volumeを適用し、metadataを入力からcopyする。2倍を超える、または0.5倍未満のrateは複数の`atempo`へ分解する。video/audio encodeは固定FFmpeg buildのsoftware codecを使い、hardware encodeはM7まで行わない。Save As後のSaveは同じexport先を更新できるが、sourceと同一pathへの出力は拒否してpartial overwriteによるsource破損を避ける。

### H1 trim endpoint feedback

trim端点のSlider identityはtab・source path・開始/終了へ固定し、pointer gestureを取り消すmedia/trim generationとは分ける。名前・source秒・0～duration・1秒stepを公開し、focus中の左右/Home/Endと数値要求を既存TrimEndpointの検証・履歴へ渡す。両端点の数値actionは描画順ではなく受信順に消費し、有効な変更だけを次のIncrement/Decrementの基準にする。直接変更は古いpointer gestureを取り消し、modal/popup中は受け付けない。focus中のgripだけ既存hover色で示し、通常shortcut/Undoを維持する。

timelineには開始・終了のdrag gripを設け、未指定端点はsource先頭/末尾に置く。開始gripは上側、終了gripは下側に分け、狭い範囲でも両方を操作できる。drag中は有効な候補範囲だけを表示し、逆転/零長の候補は赤いgripで示す。release時だけ既存のtrim検証・履歴・live再構築へ一回渡し、不正な候補は既存状態を保持して理由を通知する。Escape、focus喪失、tab/source generation切替は取消。drag中のSeek・履歴追加は行わず、frame単位snapや複数区間編集は追加しない。

trim gripの確定にはprimary release eventの座標を使い、同frameの後続hoverやPointerGoneで端点を変更・消失させない。同frameにfocus喪失・復帰が揃っていても取消し、Escapeとreleaseが同時のframeでも履歴へ渡さない。

短い入力列でもtimeline操作は押下eventの座標で所有widgetを決める。開始/終了gripを背景Seekより先に判定し、同frameで完了した押下を背景が再利用しない。primaryの押下・移動・releaseを順に処理し、release後の移動はdrag距離へ含めない。drag閾値は固定eguiのinput設定に合わせ、gripの単なるclickは編集しない。共有の一件の所有状態でSeek・trimの取消とmedia/trim identity切替を扱う。

timelineの高さは既定96 logical pxとし、上端dragで変更できる。上限はtitle/statusを除く残り領域の60%、下限は64 px（上限がそれ未満なら上限）として、縮小時も映像領域を残す。高さは既存egui panel state内だけで保持し、source位置・trim履歴・再生sessionを変更しない。範囲表示は幅が足りなければ説明部分を省き、source端点を優先する。pointerでtrim端点を動かすrange handleとは別の操作である。

I/Oの端点はsource時刻で保持する。source時刻はcontainerの絶対PTSではなく、FFmpegが報告するinput開始時刻からの経過時間とする。runtimeは同じinput原点を各streamのtickへ丸め、demux後のpacket PTS/DTSから引いてsoftware/hardware decoderへ渡す。stream間の開始差と負のprerollは維持し、Seek時だけ原点を足してcontainer時刻へ戻す。input開始時刻が不明なら原点は0とする。未指定の開始は0、未指定の終了はsource末尾として扱い、duration取得前・負の時刻・範囲外・開始以上でない終了はUIで拒否する。既存の履歴・saved/redo位置・再生位置は変えず理由を表示する。同じ有効範囲の再指定は履歴を増やさない。export境界でも負の端点・零長・逆転を拒否する。

durationも同じ原点へ揃える。固定FFmpegのMatroska/WebMはsegment終端をformat durationへ返すため、その形式だけ既知のinput開始時刻を引く。他のdemuxerの経過durationから原点を二重に引かない。これは拡張子でなくFFprobeのformat_nameで判定し、不明・負・非finite・表現不能な結果はduration unavailableとする。根拠は固定commitの[Matroska muxer](https://github.com/FFmpeg/FFmpeg/blob/e47273f4d9/libavformat/matroskaenc.c)の最終timestamp＋packet durationによる集計と、[demuxer](https://github.com/FFmpeg/FFmpeg/blob/e47273f4d9/libavformat/matroskadec.c)のsegment duration読取である。MP4/MKV/TSの開始時刻0/5秒を回帰素材で比較する。破損header・全形式のduration精度や欠落timestampの復元を保証するものではない。

固定MPEG-TS demuxerの時刻探索indexは実keyframeを保証しないため、動画Seekでは到達先からtarget以前の実packet key flagを確認する。見つからなければ探索を1秒、2秒、4秒と前へ広げ、先頭を上限として必要なprerollを求める。保持するのは一packetと候補位置・時刻だけで、全file indexやframe cacheを作らない。decodeは確認済みpacketのbyte位置から既存の半開区間filterまで進める。音声専用decodeと他demuxerは既存の時刻Seekを維持する。長いGOPではそのGOPのdecode自体は必要だが、常に先頭から全fileをdecodeする方式にはしない。thumbnail/filmstripも同じ探索開始時刻を使い、FFmpegの入力側Seekと出力側の残り時間discardを組み合わせる。

再生pipelineは起動ごとに独立した取消flagを持ち、停止・新Seek・media切替・終了ではqueueを閉じる前にそのflagを立てる。runtimeはprobe/Seekの前後、TS preroll探索中、demux packetとdecoded outputごとに確認し、target以前のframeを捨てている間も中断する。取消はconsumer終了として扱い、失敗通知・software再試行・EOF通知を出さない。既存workerのjoinと同一deviceの所有順序は維持する。これは処理境界での協調取消であり、進行中のFFmpeg callやOS I/Oそのものの強制中断・即時終了保証ではない。

有効な端点を指定したらtimelineを表示し、除外区間を暗く、保存区間をbracketとミリ秒付きsource端点で示す。fullscreenでは通常windowへ戻って表示する。Undo/Redo・tab復帰は履歴から表示を求める。入力検証の初回段階では「Export trim」と明示した。現在は後述のlive範囲再生と上記のgrip操作へ接続し、範囲外の再選択・音声sample境界・Seek・EOFを扱う。cut/delete、時間軸伸縮は追加しない。

### H1 live trim range

書き出しのtrimも同じsource半開区間を使う。秒文字列はFFmpegでinput time baseへ最近傍丸めされ、境界直前のframeが入るため使わない。runtime workerで再生と同じbest streamを選び、映像はinput tick、音声はsample単位へ端点を切り上げて整数のstart_pts/end_ptsを渡す。trim時はstreamを明示mapし、copytsとstart_at_zeroでinput原点からの時刻を維持してfilter適用後に出力PTSを0へ戻す。

低精度containerの音声PTSは、そのままchunk開始へ使うとsampleの重複・欠落を生む。decode worker内でFFmpegのav_rescale_deltaを使い、累積sample数とinput PTSの精度からsample時刻を復元する。前chunk終端の切り上げinput tickを超えるgapでは状態をリセットし、sourceの時刻飛びを保持する。状態はpipeline/Seekごとに独立し、sampleデータやWASAPI clockは変更しない。

極小trimではFFmpegが成功終了してもcontainer headerだけになる。trim出力はstaging内で要求media種別の非空packetを確認してからpublishする。動画要求で音声しか残らない場合も失敗とし、既存保存先と編集履歴を保持する。packet確認は全fileの再decodeやcodec品質保証ではない。

sample時刻復元は[固定FFmpegのdecoder処理](https://raw.githubusercontent.com/FFmpeg/FFmpeg/n9.0.1/fftools/ffmpeg_dec.c)に合わせるが、途中Seekで元のsample位相を失う低精度PTSは完全復元を保証しない。exportはsource先頭からのdecodeを基準とし、この差は既知の未完項目として扱う。最後のvideo frame長・圧縮音声paddingによりcontainer durationが区間長を越える場合もある。

trimを持つ動画・音声のPlayはsource基準の半開区間[start, end)を再生する。未指定端点は0/自然EOF。範囲終端でEndedになり、Playで範囲開始へ戻る。timeline/shortcutのSeekは全sourceを参照でき、範囲外へのSeekはPausedの素材確認とする。その状態でI/Oを再指定でき、Playは現在範囲の開始へ戻る。範囲内Seekは元のpause状態を保つ。端点変更・Undo/Redo・rate変更は現在source位置でgeneration付きpipeline再構築を行い、位置が範囲外なら素材確認としてpauseする。tab復帰は保存された範囲の開始から再生する。

runtimeのparallel decode収集点で映像PTSを[start, end)へ制限し、音声は開始/終了と重なるchunkのsampleを整数計算で切り出してからtempo/WASAPIへ送る。ナノ秒変換で切り捨てられたchunk PTSを最も近いsample indexへ戻し、開始以上・終了未満になるよう端点をceilする。これによりsample境界上の終端が1 sample増える誤差を防ぐ。各streamの終端を独立通知し、両streamが範囲終端または自然EOFへ達した時点で既存channelを閉じ、workerを回収する。終端まで全fileを無制限にdecode/discardしない。自然EOFの短いstreamとhardware fallbackを維持し、GPU frameをCPUへ戻さない。

終端判定はdecode完了だけでは行わず、最後の映像を表示し音声がdrainしてから行う。動画のみで終端まで残る時間は既存source時計と一回のdeadlineで待つ。位置表示は再生範囲の終了を超えない。素材確認中はこの上限を適用しない。これは単一区間のtrimで、cut/delete、repeat、range dragや新しい時刻軸は追加しない。

### H1 pixel-aligned crop

画像・動画の選択dragはbuttonを押した位置を始点とし、その位置で既存selectionの辺hitを判定する。drag認識までの移動量を失わず、release frameの最終位置を適用してからpixelへ丸める。移動eventが少なくても選択範囲が消えたり辺resizeが新規選択へ変わったりしない。画像外から開始したdragで新規選択は作らないが、既存の辺のhit範囲は画像端の外側も含め、表示されたhandleを掴めるようにする。surfaceのclip/overlay遮断、範囲内clickの一時crop previewとShiftの制約は維持する。

選択dragと画像panは同時に一件だけ保持する一時操作とし、開始前のselectionまたはpan位置を保存する。releaseでその表示を確定し、Escape・focus喪失・modal/palette/grid/filmstrip/menu・別commandでは開始前へ戻して操作を解除する。解除後の保持中buttonやreleaseで再開せず、新しい押下を必要とする。Escapeは既存prefix/overlayの取消を優先し、進行中dragがあればfullscreen解除やselection全消去より先にdragだけを取り消す。履歴・sourceは変更しない。panは押下位置からの差分で計算し、release時の最終位置も含める。

押下・移動・releaseが同一frameに届く場合も、button eventの座標から開始と確定を行う。開始はenabledなsurfaceのclip内かつその位置の最前面layerに限り、他widgetのdragを横取りしない。共有view入力は最初の該当button releaseで区切り、後続gestureの押下・解放やhoverを先の操作へ混ぜない。前frameから保持中なら最初のreleaseだけで確定し、後続pressを開始点にしない。同frameの後続gestureをすべて個別に再生する機構ではない。画像／動画のAlt回転も先のpress/release event自身の修飾キーで判定し、同座標の後続eventや最終frameのAlt状態を借りない。

選択は開始位置・押下時刻・移動閾値を超えた履歴を一時操作へ保持する。eguiの入力設定と同じclick距離／時間閾値を使い、対象pressより前の移動と最初のrelease後の移動を含めない。release自身の距離も検査し、疎な移動eventでも終点を反映する。原点へ戻っても過去の閾値超過を忘れず、後続gestureに由来するeguiの集約click/drag flagで先の選択を変えない。短い範囲内clickは既存crop previewとして扱い、取消後に元の押下eventを再利用しない。

取消時は未描画のegui-winit入力に残るprimary/secondary押下も破棄する。描画前の押下→Escapeやfocus離脱・復帰から古い操作を開始させないためであり、移動・release・keyboard・wheelは保持する。保留押下の取消もEscapeを一回消費し、既存selectionやfullscreenを同時に解除しない。取消後に届いた新しい押下は通常どおり扱う。

Windowsではqueued button releaseが最後のWM_MOUSEMOVEより先に届く場合がある。固定winit 0.30.13はbutton messageの座標をMouseInputへ渡さないため、runtimeがevent-loop builderのmessage hookを設定し、client-area button down/upのdispatch直前に同じwindow・wParam・lParamでWM_MOUSEMOVEを同期送信する。button message自体は一度だけ通常dispatchし、通常の重複move除去・capture・modifier・egui入力経路を維持する。[button message](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-lbuttonup)と[WM_MOUSEMOVE](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousemove)が持つclient座標をそのまま使い、後のGetCursorPos値で履歴を置き換えない。pointerやnative messageはruntime内だけに閉じ、appへは従来のwinit eventだけを渡す。global hook・polling・別threadは追加せず、native STA dialogとnon-client messageは対象外とする。

selectionのdrag中は正規化座標を使い、releaseとcrop確定時に現在の編集後寸法へ丸める。画像は1 pixel、動画は偶数の位置・寸法を使う。各辺を近いgrid境界へ丸め、同じ境界へ潰れた場合は内側の最小1 grid領域とする。現行defaultのlibopenh264は2×2を実際にencodeできず16×16未満を拒否したため、動画cropの確定は16×16以上に限定する。小さすぎる選択は勝手に16×16へ広げず、案内とともにselection・履歴を保持する。非finite・逆転・範囲外の選択と寸法未取得も確定しない。

確定cropは正規化floatではなく、編集時点の整数pixel矩形を`EditOperation::Crop`へ保持する。previewはその矩形からUVと整数寸法を求め、FFmpegへ同じ整数と`exact=1`を渡す。回転/反転/cropの履歴順は変えず、既存の履歴はmemory内だけのため保存形式の移行は生じない。確定時に出力寸法をstatusへ表示し、全領域cropはdirty履歴を増やさない。選択の一時crop previewも同じimage pixel丸めを使う。動画の一般的なresize/paddingやencoder変更は行わない。

crop境界でlinear samplingが選択外の隣接pixelを混ぜないよう、画像meshを端の半pixel帯で分割し、動画shaderでも選択領域内のpixel中心へsample座標をclampする。1×1画像は一色のまま拡大される。textureの再decodeやcrop用CPU copyは追加しない。

### H1 keyboard and accessible selection

H1の選択操作は画像・動画のSelect all command（既定Ctrl+A、menu/palette/custom binding共通）からも開始できる。reading表示では無効とし、検索欄の全選択を奪わない。読込済みの編集後media全体を選び、一時crop previewを解除して左辺へfocusする。選択自体は編集履歴を増やさない。

既存の四辺の表示位置へ名前付きpixel Sliderを公開し、focus中の矢印で画像1 pixel・動画2 pixel、Home/Endで軸端点を指定する。数値要求は四辺をまたいで受信順に処理し、pixelへ丸めた後の逆転・零長を拒否する。identityはtab/path/辺へ固定する。値操作は進行中pointer gestureを取り消し、modal/overlay/menu中は無効。pointerの辺drag・Shift制約・外観と、既存crop確定/Undo・動画16×16制約を維持する。全screen readerの操作完了は別途検証する。

通常画像ではfocusを得た辺、値変更後のfocus辺、明示Select allの左辺へ一回のreveal要求を出す。handleとfocus枠がmedia viewportへ入る最小のlogical pan差分だけを適用し、倍率・選択pixel・編集履歴は変えない。要求消費後の通常再描画や手動panを引き戻さず、overlay/modal中やpointer button保持中はrevealしない。UIA FocusとTabも同じ要求を使う。動画は既存aspect-fit表示のままで、zoom/panを新設しない。

### H1 visual filmstrip

filmstripは中央の横scroll overlayとし、背景を暗くして現在項目の白枠・名前、画像/動画thumbnail、音声waveform、取得できたdurationを表示する。Shell snapshot順を維持し、clickは既存のguard付き移動、middle clickは新規tab、Tab/Shift+Tabは既存の全種移動へ渡す。開いた時と現在media変更時は現在項目を中央へ寄せ、wheelは横scrollに使う。

Tabはeguiのfocus traversalより前にfilmstripへ渡す。ただしpalette、grid、modal確認の最中は横取りしない。縦wheelの横変換はfilmstripのscroll領域だけに設定し、他のUIのscroll方向は変えない。

Tab/Shift+Tabによる移動は、既存のguard付きNavigate後に現在項目への一回のfocus要求も残す。表示可能になった現在項目を中央へ寄せてfocusし、通常再描画ではfocusを奪わない。snapshot未到着・Area sizing中は待ち、clearで要求を破棄する。保存確認中はfilmstripを表示しない既存制約で背景focusを防ぎ、Cancel後は変更していない現在pathへ戻る。項目focusがEscapeを消費する前に既存のoverlay解除へ渡すが、通常windowのmenu popup・palette・modalはそれぞれの入力を優先する。

画面内の項目だけを単一runtime workerへ要求する。待機要求は最新1件、最大64項目、RGBAは各240×160以下とし、結果はpath/generationで照合する。UI textureは現在の可視集合だけ保持し、folder snapshot更新・closeでは失効する。diskは既存のmetadata付き64 MiB preview cacheを共有する。古い要求の未開始項目は処理せず、開始済みFFmpeg/FFprobeは下記preview取消tokenで停止する。window closeでそのprocess完了をjoinしない。decoder作業領域のメモリ上限は保証しない。

### H1 external file drop

Explorerからのfile dropはwinitのowned path eventで受け、既存のexternal Openへ渡す。画像・動画は新規tab、音声は同folderの非dirty playlistを再利用し、dirty/書き出し中のplaylistは別tabとして保持する。folder dropは非同期のOpen Folderへ渡し、Shell順の最初の対応mediaを開く。folder要求は従来どおり最新1件で、複数folderを展開・importするqueueは設けない。hover中は描画だけの案内を出し、外へ戻すかdropしたら消す。native picker、dirty guard、export error、guardからのexportの最中はdropを拒否し、確認対象を切り替えない。fileの移動・copy・source変更は行わない。

### H1 compact window shell

U01のnative caption実装は、runtime内のDWM frameと同一UI threadのsubclassへ限定する。caption buttonsはDWMへ任せ、appは実際のbutton boundsを避けてtab barを描く。closeはwinitのCloseRequestedから従来のguardへ渡す。Flip Modelとnative frame描画を同じHWNDへ混在させるとbuttonsが見えないため、同じdevice／swap chainを入力透過のchild surfaceへ置き、親のcaption領域を残す。GPU device・decode経路・CPU transfer契約を変えず、native handleはruntime内だけで所有する。主要経路のcheckpointと、混在DPI・物理入力などを含むU01全体の完了判定は区別する。

captionとchild surfaceはRcでUI threadへ限定し、親windowを保持する。subclassの追加参照は解除またはnative破棄時に解放し、rendererはswap chainをchild HWNDより先に破棄する。childはHTTRANSPARENTを返し、標準STATIC classの背景描画を抑止する。これを残すと最大化・復元後にcaption背後が白くなる実例があった。親のGDI描画は露出したcaption背景だけを担当する。追加GPU device・CPU転送・別threadは作らない。

最大化時もNCCALCSIZEの上端non-client insetは0とする。上端へnative border幅を残すと、DWMのglyphが見えても最大化中のbutton hit testが失敗した。画面外の上端はeguiのsafe areaとして除外し、現在のwindow scaleとUI zoomで換算する。これは[Microsoftのcustom frame契約](https://learn.microsoft.com/en-us/windows/win32/dwm/customframe)と、同条件を説明する[Chromiumの実装記録](https://chromium.googlesource.com/chromium/src/+/8a0dac94cf639d40c0c00557bf9eb5338ae82146/ui/views/win/hwnd_message_handler.cc)を踏まえた実測対応である。通常／最大化／復元とPNG・短いhardware動画、pointerによる連続resize、画像と再生／一時停止動画のgraphics復旧を確認した。native system menuも所有threadの標準popupとして表示を確認したが、混在DPIと物理keyboardの確認は残る。Snap候補は基準機の設定で無効なため表示未検証とし、設定を勝手に変えない。

固定AccessKit adapterはnative captionの子要素をtreeへ含めないため、[TITLEBARINFOEX](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-titlebarinfoex)が返す各buttonの実bounds・状態をruntimeから値として渡し、appで操作名とClick actionだけを補う。DWMの描画・native pointer hit・hoverを変えず、eguiのbutton描画・不可視hit領域・Tab focus停止点は追加しない。UIA Clickは通常のUI action guardを通してruntimeへ渡し、同じwindowへのWM_SYSCOMMANDをqueueする。Closeは通常のCloseRequestedから未保存確認へ進む。modal中はdisabledかつClick非公開、fullscreenではnode自体を出さず、遅れて来た要求もappで拒否する。native windowを破棄する別の終了経路は設けない。

UX改善ではchromeの共通色を背景#000、通常text/icon #808080、active/focus/progress #fff、境界/active tab #181818、hover #4C4C4Cへ揃える。mediaのletterbox clearも黒とする。32px title bar／30px status barを基準に内容を上下中央へ配置し、26px tab／28px幅logo buttonの高さを文字の有無から独立させる。tab名は左10pxの余白と右24pxのclose領域を持ち、hover背景はtab全体へ描く。通常のバー境界はtitle下とstatus上、timeline表示中は後者をtimeline上へ移し、timeline/status間の線を出さない。既存のwidget identity、menu／tab／resize入力、dirty guardは維持する。native caption、font/icon資産、選択線とtimeline編集modelは台帳の別項目として継続する。

音声playlistはShell snapshotの音声だけを元の順で番号付き表示し、32 logical pxの行全体を選曲対象とする。見出しは省き、現在曲を明るく、他の曲を控えめに表示する。長いfilenameは一行に省略し、行hoverで全文を示す。ScrollAreaは可視行だけを描画し、選曲は既存のNavigate guardへ渡す。曲ごとのduration probeや新しい再生方式は追加しない。

filmstrip・palette・grid・menu popup・modal中は音声playlistのUiを無効化し、wheelだけでなくpointer、keyboard focus、UIA操作とhover tooltipも背景へ通さない。opacityは元の値を保ち、暗幕による既存の見た目を重ねて減光しない。行のID・位置・Shell順を保持し、overlay自身やtitle/statusの操作へ無効化を広げない。閉じた後は同じ行を再度有効にする。

playlistの行focusは再生中pathとは別に保持する。実際のfocus IDと一致する時だけ修飾なしの上下/Home/End/PageUp/PageDownを音声のShell順へ対応させ、移動先を最小限scrollしてfocusする。Page単位は表示高さに収まる行数とし、端点で循環しない。Enter/Spaceはその時点のfocus先を既存選曲へ渡し、同frameの移動と実行も受信順を保つ。disabled/popup中や行外focusでは追加処理を行わず、Tab・修飾key・pointer選曲と可視行限定の描画は維持する。再生中の行追従と手動scroll保持の契約は変えない。

初回のsnapshot到着、選曲/tab再表示、Shell順での現在曲の位置変更時には、現在行を必要最小限scrollして表示する。同じpath/indexの描画では手動scrollを保持する。対象が未取得/消失中は追従済みとせず、再取得後に表示する。guardの確認中/Cancelでは行先へ追従せず、実際にloadが確定してから切り替える。

folder前後移動・playlist項目の再clickなど、Navigateの行先が現在pathと同じなら共通guardの入口でno-opにする。単一項目/同種一件の巡回でも再load・不要な保存確認・export待機を発生させず、zoom/pan/selection・再生位置/pause・編集・読み込み世代を維持する。別pathへの既存guard、明示的なOpenと新規tab作成は変更しない。

tab操作でactive identityが変わらない場合はmediaを再loadしない。現在tabの再clickと単一tabの巡回はno-opとし、非active tabのcloseでは対象のtab/history/export pathだけを削除してbarを再描画する。現在の再生位置・pause、画像zoom/pan/selection、読み込み世代を維持する。active tabを閉じた際の隣接tabへの移動、最後のtabのWelcome、既存のdirty/export guardは変更しない。

UI本文は同梱Figtree Regularを先頭に使う。固定eguiのfont backendはOpenType featureを選択しないため、元fontが持つtnum数字glyphを既定cmapへ固定した派生fontを、pin済み再生成scriptで作る。数字以外の字形・metricsを変えず、元fontとライセンス・変更説明も保持する。専用Codicon familyをicon widgetだけへ指定し、一般textのprivate-use文字をiconとして解釈しない。日本語はruntimeがWindows Fonts内のYuGothM.ttcのface 1（Yu Gothic UI Regular）、Meiryo、MS Gothicの順で読める一つのfontとface indexを返し、Figtreeとegui既定fallbackの間へ登録する。コード等のMonospaceは既定Latin fontを維持する。日本語fontは同梱・download・OS設定変更せず、ない環境でもFigtree/Codiconを導入し診断を残す。Windowsに通常存在しないHiragino Sansを取得・同梱しない。

上部は上記native controlsの高さに合わせた単一title/tab bar、下部は30 logical pxのstatus barとし、暗いneutral色でmedia領域を優先する。U01以前はdecorationsなしのwinit windowにwindow controlsも描画していたが、現在は上記native caption構成を検証中である。window closeは既存のdirty/export guardを必ず通す。tab幅は等分、最大160 px・最小72 pxとし、収まらない場合は横scrollする。path/名前は省略表示と全文tooltipを使い、右側の状態表示へ専用領域を確保する。menuの方向gestureとtab reorderの要求・進捗はUX台帳に従う。

logo menuはFile / Edit / Viewの3分類とし、app内の固定配置で関連commandを区切る。全registry commandを一箇所ずつ配置し、title・有効条件・現在のcustom shortcutは既存registry/bindingsから取得する。shortcutは右揃え、縦に収まらないsubmenuはwindow内でscrollする。commandのdispatch・dirty guardは変更せず、分類のために新commandやruntime処理は追加しない。方向drag gestureは引き続き対象外とする。

logo menuをEscapeで閉じた場合はlogo buttonへfocusを戻す。command選択時も、消えるmenu項目を復帰先として次のpalette/確認へ渡さないよう、dispatch前にlogoへfocusを引き継ぐ。command自身が新しいfocus先を指定する場合はそちらを優先する。親menu・submenu共通で、背景click・通常再描画では復帰を要求しない。

固定eguiは描画した全nodeを含むroot AccessKit treeを毎pass出力するが、初回のprogrammatic focus要求には未描画IDを一pass保持する猶予がある。消えたmenu項目などへの復帰で、このIDをnative consumerへ渡すとpanicする。appはplatform outputの配送前にfocusが同じtreeのnode一覧に存在することを検証し、存在しなければrootへ戻して同じ古いegui focusも解除する。nodeの追加・順序変更・有効なfocusの置換は行わない。これは完全なroot treeの境界検証であり、一般の差分treeから省略nodeを削除扱いする規則ではない。

menuを開いたら最初の有効項目へfocusを渡す。最深menuが上下/Tab/Shift+Tabを循環移動、右をsubmenu展開、左を親へ戻る操作として所有し、背後のWelcomeやmedia操作へfocusを移さない。Enter/Spaceの選択とEscapeによるmenu treeの取消、pointerのhover/clickは既存egui popupを使う。有効項目だけを移動先にし、focusした項目をscroll内に表示する。

### H1 fullscreen viewing

fullscreenの下端48 logical pxへpointerを移すと、既存status barとseek barを映像の上へ表示する。mediaのviewport・zoom・再生sessionは変えず、同じcommand/Seek/guard経路を使う。解除buttonを加え、timeline表示commandは従来どおり通常windowへ戻す。操作部を離れたら隠すが、操作部から開始したdragはrelease frameまで表示を保持する。選択drag中に下端へ来ても新たに表示せず、modal・palette・grid・filmstrip中は隠す。表示中はcursorを隠さず、新たなpollやanimation timerは設けない。

F11をdefaultとする共有Toggle fullscreen commandをView menu・palette・custom shortcutへ登録する。winitのBorderless fullscreenを現在monitorへ適用し、復帰時のwindow位置・寸法・最大化状態もwinitの保存済みplacementに任せる。exclusive display modeやD3D deviceの再作成は行わず、通常のresizeと同じ単一device描画を使う。Enterは確定操作との競合を避けて割り当てない。

最大化から直接入ると、固定winitのWindows経路では旧client領域が残り復帰時のouter boundsも一致しないことを実機で確認した。appは入る前の最大化状態だけを保持し、最大化解除→Borderless、解除→再最大化の順で呼ぶ。通常位置・寸法の保存をappで重複実装せず、display modeやnative placementへ直接触れない。

fullscreenではtitle/tab bar、timelineとwindow resize操作を隠し、status/seek barは下端hoverまたはkeyboardでの呼出し時に重ねて表示して、media領域をwindow全体へ広げる。画像/readingの外周余白も除く。音声playlistとWelcomeは中央contentとして残す。filmstrip・palette・grid、loading/error、export進捗・dirty guardは明示的な操作/通知として引き続き表示する。status通知とEscapeによる復帰案内は期限付きoverlayとする。timelineの表示設定は復帰まで保持し、fullscreen中にToggle timelineを実行した場合は通常windowへ戻ってtimelineを表示する。

既存shortcut/modal/overlay処理を通ってeguiへ届いた修飾なしTabまたはShift+Tabでfullscreen操作部を表示し、初回はExit fullscreen buttonへfocusする。Areaの初回sizing中は要求を保持し、有効なbuttonが描かれたら一回だけfocusする。Tab巡回で一時的にfocusが空になってもkeyboard表示を保持し、操作部への支援技術focusも維持する。内容部分のpointer pressで操作部のfocusを解放し、window focus喪失・modal/別overlay・content drag・fullscreen解除でkeyboard表示と初回要求を解除する。従来のedge hoverと押下/release中の保持は変えず、通常windowのfocusは変更しない。新しいshortcut alias、timer、pollingは追加しない。

Escapeはmodal/menuの入力を優先し、次にpalette/grid、次にfilmstripを一段ずつ閉じ、overlayがなければfullscreenを解除する。解除時に画像selection・編集・再生状態は変えない。上端でのtab/menu表示とdouble-click割当は別の操作監査とする。

filmstripを開く時は現在項目へfocusを要求し、呼出元widgetとmedia読み込み世代を一件保持する。同じ世代で閉じる場合は元の操作部へ戻す。別mediaへ移った場合や呼出元focusがない場合は、現在tab（Welcomeではlogo、fullscreenでは既存Exit操作部）へ戻し、古い選択辺を復帰させない。palette/gridから開く場合は、そのoverlayの元の復帰先を引き継ぐ。close時の復帰は一回だけとし、通常の再描画や手動focus移動で再要求しない。

filmstripの上にpalette/grid/menuがある間は、filmstripのpointer・keyboard・UI Automation操作とhover説明を無効化する。表示opacityと項目ID、限定描画、Shell順は維持し、上のoverlayを閉じると通常操作へ戻る。modal中は従来どおりfilmstrip全体を非表示にする。

保存確認のEscapeはCancelと同じく離脱要求だけを取り消し、編集を保持する。export失敗のEscapeは最前面のエラー通知だけを閉じ、保留中の保存確認は残す。背景クリックではどちらも閉じず、保存・破棄は明示的なbutton操作に限定する。

modal入力を保護している間は背景rootのwidgetを無効化し、既存menu popupを閉じ、filmstrip/palette/gridは設定を保ったまま表示を保留する。背景のopacityは既存backdropに任せる。UI AutomationのClickは固定eguiのpointer遮断を迂回するため、配送済みUiActionも最前面の確認・error解除・export取消以外は拒否する。error通知中の旧Discard等を実行せず、Cancel後は未保存編集と通常UIを復元する。

保存確認・export通知は現在のegui表示領域に幅を制限する。長いfile名は一行で省略してtooltipに全文を残し、確認buttonは横幅に応じて折り返す。エラー詳細の縦scrollには画面高に応じた上限を設け、確認buttonを詳細の外に保つ。OSのDPIや設定は変更しない。

保存確認、export失敗、継続前のexport待ちは、内容を描く前に同じ見出しを名前にしたAccessKit Dialog/modal nodeを作り、説明とbuttonをその子要素にする。通常のbackground export windowはmodal扱いにしない。見た目と既存のTab/Enter/Escape、確認buttonの初期focus方針は変更せず、名前/階層だけでscreen readerの読上げ完了を宣言しない。

画像・reading・動画のfullscreen閲覧中だけ、入力が2秒ないとcursorを隠す。windowがactiveでpointerが内側にあることを条件とし、button保持・selection drag、filmstrip/palette/grid、picker・dirty guard・export、loading/error・file hover中は表示する。pointer移動・button・wheel・key入力、focus/入退出の変化で期限をリセットし、fullscreen解除時も表示へ戻す。音声playlistとWelcomeでは隠さない。eguiのplatform outputでcursorを統一管理し、期限をevent loopの既存待機へ統合する。非表示中という理由だけで再描画やpollを追加しない。最小化からpointerを動かさず復帰するとCursorEnteredが届かない場合があるため、focus取得時も既存のpicker復帰と同じclient座標更新を行う。

### H1 seek bar and command palette

compact seekの操作中は、左右に半径4 logical px（幅が足りなければ幅の半分）の余白を設けた範囲をつまみ中心の移動区間にする。hover／drag候補・release確定も同じ区間から値へ変換し、端の余白は先頭／末尾へclampする。非hover／非focus／非drag時の1 physical px進捗線は従来どおり全幅を使う。hit領域・keyboard／UIA値・取消と一回確定の契約は維持し、timelineの時間座標へこの余白を適用しない。

H1のWindows accessibilityは固定egui-winitのAccessKit adapterを使い、windowを初めて表示する前に接続する。初期tree要求時だけeguiのtree生成を有効にし、actionは対象windowを照合して既存event loop/egui入力へ渡し、既存platform outputでtree更新を返す。appにCOM pointerや独自UI Automation providerを追加せず、支援技術がない通常idleでtree生成や新しいpollを強制しない。標準widgetと既存の意味情報から接続を検証し、custom widgetやscreen reader全体の対応をbridgeの存在だけで宣言しない。

compact seek barとtimelineの再生位置を、source秒の0～duration、画像位置をShell順の画像だけの1～件数を範囲に持つSliderとして公開する。現在値・範囲・stepとSetValue/Increment/Decrementを既存egui出力へ加え、有限の数値だけを範囲内へ制限する。画像の小数は最寄りの位置へ丸める。値変更は既存Seekまたはguard付きOpenMediaへ一回だけ渡し、古いpointer dragを解除する。modal中は部品の公開状態と処理の両方を無効化する。focus中の左右/Home/Endは5秒または1画像の値操作、Tabはfocus移動に使い、それ以外の現在のshortcut/prefixは既存command経路を保つ。合成されたfocus復帰keyからこの追加経路を実行しない。値eventは順番を保って消費し、破棄されたlayout passから再実行しない。focus時のhandle/枠以外の描画や、trim編集・保存・再生時計は変更しない。

固定eguiのTextEditはUI AutomationのSetValueを反映しないため、paletteの検索欄に限り、正しいroot tree/node宛ての文字列SetValueを既存の全選択・削除・paste入力へ変換する。event順、空文字と単一行化、検索結果更新を保ち、OS clipboardやcommand実行には触れない。他widgetや非文字列actionへ一般化しない。

logo menuと検索欄には意味のある名前を付ける。palette候補の選択色はcommand cursorであってon/off設定ではないため、accessibilityではtoggled状態を付けず、通常のbutton実行として公開する。描画・keyboard選択・共有command dispatchは変えない。

tabのactivate/close widgetは配置indexではなくTabIdに結び付けた明示UI IDを使う。並べ替えや隣接tabの削除で、既存のfocusや取得済みaccessibility actionを別tabへ再割当しない。closeの意味名に対象filename、descriptionにfull pathを公開し、既存のguard付きCloseTabへ渡す。表示文字・寸法・pointer dragの契約は維持する。

playlist/filmstripの可視項目は、移動先のfull pathを含む明示UI IDで操作対象を固定する。Shell snapshotの更新でindexが変わっても別pathへ再割当せず、非表示・削除された項目への古いactionから別項目を開かない。playlistの選択色は現在曲を示しon/off toggleではないためInvokeを公開する。両widgetのdescriptionへpathと現在項目の説明を載せ、filmstripへ名前付きButtonとfocus時の既存枠/名前表示を加える。可視範囲だけの描画・preview要求、Shell順、既存OpenMedia/dirty guardを維持し、全項目を常時accessibility treeへ展開しない。

H1のtext入力は、固定egui-winitのclipboard featureを明示的に有効にしてWindowsのOS clipboardへ接続する。検索欄等のcopy/cut/pasteは既存egui入力とplatform outputを使い、app独自のclipboard、監視thread、履歴保持は追加しない。native clipboardへの接続/アクセス失敗時は固定backendの既存fallback/error処理に従い、画像/選択範囲をcopyする新commandとは分ける。テストでユーザーのclipboard内容を暗黙に置換せず、隔離環境または明示許可された内容でnative往復を確認する。

画像folderのHome/Endは現在のShell snapshot内の最初/最後の画像へ移動する共有commandとする。非画像を除き、filenameで並べ替えず、reading modeでも同じ端点画像を起点にする。現在画像がsnapshotにない間は移動せず、すでに端点なら再load・保存確認を行わない。別画像への移動は既存dirty/export guardを通す。menu・palette・custom shortcutへ同じcommandを公開し、既定Home/Endは画像だけに割り当てる。text入力のHome/Endや動画・音声の操作は変更しない。

画像folderのSeek hoverは移動先のpreviewと位置・filenameを表示する。reading modeでは既存reading_itemsで求めた移動後のpage群を、現在の枚数・縦横・反転に合わせる。filmstripが閉じている間だけ同じPreviewLoader・path/generation照合・240×160 cacheを共用し、新しいworkerを増やさない。hover離脱・overlay・media切替・snapshot更新では要求とtextureを失効させる。対象は最大10 page、hoverだけではsource・現在画像・編集を変更せず、release時は既存dirty guardへ渡す。失敗pageは位置を保ってNo previewとし、表示中に再試行loopを作らない。

動画・音声のSeek tooltipはtrack全体の左端ではなく、hover地点の上へ中央揃えで置き、画面端では表示領域内へ収める。動画previewは縦横比を保って最大160×108 logical pxに収め、時刻を中央に添える。縦長素材でもtrackを覆う巨大な画像にしない。既存の20区間cache・非同期取得・失敗表示・Seek確定経路を維持し、本画面scrubは追加しない。

動画hoverのthumbnail失敗は現在mediaの20区間ごとに記憶し、同じ区間への描画・pointer往復でworkerを再起動しない。tooltipに取得不能を示し、理由は一度だけdiagnosticへ出す。別mediaへの移動・再openで失敗記録を解除して再試行を許す。要求にはmedia load世代を付け、同じpathを再openしても旧世代の成功/失敗は反映しない。失敗はdisk cacheへ保存せず、再生・Seek・保存の可否を変えない。

timeline非表示時はstatus上端に1 physical pxのseek barを重ね、hover/drag時だけ太くしhandleを表示する。動画・音声はsource時刻、画像は同じShell snapshotの画像だけの順序へ対応付ける。dragはhandle位置を更新し、releaseで一回だけ既存のgeneration付きSeek/guard付き画像移動を行う。動画hoverは既存の20区間thumbnail tooltip、画像hoverは上記の移動先preview・位置・filenameとし、本画面のscrub previewは含めない。EOFからの位置移動はPausedとし、その位置からPlayできる。停止中のSeekでは音声時計が次のframe時刻へ到達できないため、保持frameがない場合だけ最初のdecode frameを時計待ちせず表示する。

timeline・compact seek bar・画像folder barのpointerによる確定はprimary（左）buttonのclick/drag releaseだけとする。secondary・middle・追加buttonのdragでSeekやfolder移動を行わず、compact barの候補位置も動かさない。既存のkeyboard/accessibilityによるprimary click経路は維持する。

Seekの確定位置はprimary release event自身の座標とし、同frameで後から届いたhoverはtooltip用の位置にだけ使う。進行中のSeekは一件のwidget所有権として保持し、Escape・focus喪失・modal/別command・media切替で解除する。取消後のreleaseは移動せず、新しい押下を必要とする。fullscreenの最初のEscapeもSeek取消を優先する。編集履歴は変更しない。

paletteは検索入力を保ち、上下keyで有効な候補を巡回し、Enterで共有commandへdispatch、Escapeで閉じる。eguiの破棄されたlayout passで消費したkeyのactionも保持し、同一frameの同じUI actionは一回だけ実行する。

同時表示しないcommand paletteとgridは、開く直前のfocusを一件だけ共有して保持し、Escapeによる取消時に復帰を要求する。両者を切り替える場合は元の復帰先を引き継ぐ。grid内のbutton focus中もEscape一回で閉じ、menu・palette・modalの入力を優先する。command実行や外部dropで閉じる場合は破棄し、新しい操作先へ古いfocusを戻さない。復帰先widgetの有効性・生存は既存eguiのfocus登録で判定する。これは二つのcommand overlay取消の契約であり、filmstripや全modal間のfocus stackではない。

未保存確認も、現在tabの編集に対して開く時だけ直前のwidget IDとtab IDを保持する。同じ確認への再要求、復帰描画前の再確認、Save As取消からの復帰では上書きしない。確認を取り消し、eguiの前passのmodal制限も解除された描画で同じtabへ一回だけfocus復帰を要求する。待機中は次の描画を要求し、通常復帰後に再要求しない。確認のため別tabへ切り替える場合は元tabのfocusを流用しない。確定した離脱・media loadでは破棄し、背景操作の遮断と編集保持は変更しない。

paletteの外観はtitle bar直下の中央に置く最大600 logical pxの暗いpanelとする。windowのtitle見出しは表示せず、検索欄を全幅にし、command名と現在shortcutを同じ一行の左右へ配置する。shortcutは行幅の半分までとし、選択/hover行だけ背景を付ける。長い内容は省略とwindow幅内の全文tooltip、候補が多い場合はwindow内scrollで扱う。検索・有効条件・dispatch・IME所有権は既存のままとし、recent rankingや設定buttonは追加しない。

shortcut prefixは一続きのkey入力だけに有効とし、1秒の期限切れ、Escape、focus喪失、mouse press、別command、file drop・離脱確認で解除する。Escapeはprefix取消をoverlay/fullscreen解除より先に扱う。prefix開始時刻と案内の時刻を共有して通知の所有を識別し、取消ではその案内だけを消して再描画する。後から出た別通知を消さず、正常な複数key shortcutは従来どおり一回dispatchする。

入力列が一致しなくなった場合は旧prefixを解除し、最後のkeyを単独で再判定する。新しいprefixならそのkeyから続き待ちと案内・1秒の期限を開始し直す。同じprefixの押し直しも同様とする。単一commandの再判定は維持し、未割当なら待ちを残さない。

shortcut設定の生成と読込は往復可能にする。`+` keyはmodifier区切りと曖昧にならない`Plus`として保存し、旧版が出力した`+`・`Ctrl++`等も同じkeyとして受け付ける。既存の利用者設定を移行のために上書きしない。

status barの再生・waveform timeline・reading mode・fullscreen解除buttonは、tooltipとaccessibility labelのshortcutを現在のbindingsから取得する。prefixは全sequenceを表示し、未割当なら操作名だけにする。既定bindingとclick dispatchは変更しない。

文字入力ではない通常controlのfocus中も、有効な現在bindingに一致するcommand/prefixはegui-winitのfocus全体のkey消費より先に処理する。Ctrl/Alt/Windows keyなしのSpace・矢印・Home/End・EscapeとTabはUI操作へ残し、進行中prefixの続きは既存shortcut処理へ渡す。menu・palette・grid・filmstrip・modal・TextEditは横取りせず、seek/trim/selectionの値key所有権は既存判定を維持する。合成されたfocus復帰keyからこの経路を実行しない。

egui-winitは修飾付きTabも消費するため、Tab eventのうち現在の有効bindingまたはprefixに一致するものを先に共有shortcut処理へ渡す。通常のbutton focus中も修飾付きTabは使えるが、menu・palette・grid・modalではUI入力を優先する。未割当のTabと通常controlのTab/Shift+Tab focus移動は維持する。filmstrip固有の項目移動はCtrl/Alt/Windows keyなしのTab/Shift+Tabだけとし、Ctrl+Tab等を奪わない。固定aliasや新commandは追加しない。

画像の標準Left/Rightは画像専用Previous image / Next image commandへ解決する。通常表示では共有Shell snapshotの画像順を一枚ずつ、reading modeでは重複しない見開き単位で移動し、既存のdirty/export guardを通す。動画・音声では従来の5秒Seekを維持し、Ctrl+左右の同種移動も一枚単位のまま変更しない。commandはmenu・palette・shortcuts.confから利用でき、設定で変更したキーとは別の固定aliasを設けない。新しい既定bindingと既存commandのcustom bindingが同じキーの場合は、従来のregistry順による既存command優先を維持する。

I04／I06追加binding契約（2026-09-10）: Previous imageにPageUp／Backspace／A、Next imageにPageDown／Space／D、reading配置切替にL、reading順反転にVを追加する。既存の主キー／prefix優先とcontext判定を使い、画像のSpaceから再生操作は呼ばない。各方向1～10枚を画像専用の個別commandとして登録し、menu／palette／grid／設定で同じdispatchを使う。Ctrl+1～0は先へ1～10枚、Shift併用は手前、Ctrl+Space／Ctrl+Backspaceは5枚。共有Shell snapshotの画像ファイル数で数え、reading中も同じ枚数の対象画像を含む見開きへ移る。jumpは端でclampし、同じpathはguard／reloadを始めない。通常移動の循環、既存Ctrl+左右の一枚移動は維持し、草案のCtrl+左右＝5枚だけは採用済み契約を優先する。別snapshot取得・ソート・編集操作は追加しない。

新生成設定はv4。v4以前の単独・未変更のLeft／Right／R／Hだけ画像／reading追加bindingを補い、custom／複数指定とv4の明示単独指定は保持する。既存J/K/L・comma速度の限定移行も保持し、fileは自動書換えしない。未宣言の新jump bindingが画像で有効になる宣言済みcommandのキー／prefixと重なる場合、その暗黙bindingだけ除く。明示jumpの競合は従来の主キーexact／prefix規則に従う。Ctrl+Shift+上段数字はShift後の記号に対応する有効なcustomキー／prefixがなければ物理数字を使う。非画像・Alt／Windows併用は変換しない。入力欄と既存modal／menu／focus入力保護を通し、PageUp／PageDown／Backspaceは通常widgetの入力を先取りしない。

Readingの分割はShell snapshotから画像だけを取り出した順序の先頭を基準とする。表示枚数は既存の2～10、先頭ページ枚数は1～表示枚数。初期値は両方2で1–2／3–4となり、先頭1なら1／2–3／4–5となる。表示枚数の増加時は、先頭が満杯なら同じ枚数へ追従し、短い先頭は維持する。減少時は先頭を新しい上限まで制限する。現在画像を含む見開きを表示し、設定変更・表示反転でactive pathや編集対象を暗黙に先頭へ変更しない。隣ページへの移動はその先頭画像へ、末尾／先頭からは循環する。全画像が同じ見開きに収まる場合は移動・再読込・未保存確認を行わない。Home/Endは従来どおり端点の画像を選び、その画像を含む見開きを表示する。

本画像のloaderはactive pathを先に要求する既存の完了契約を保持し、他のページをShell順で要求する。描画時にactive画像を本来の位置へ戻し、破損画像のerrorもその位置に残す。seek hoverは同じ分割範囲の低解像度previewを既存workerで連結し、hoverだけではactive pathを変えない。表示枚数はCtrl+[／Ctrl+]、先頭枚数はCtrl+Shift+Left／Rightの共有commandから変更する。限界値での再操作は再読込しない。statusに両方の枚数を表示する。

読書ボタンはclickでmode切替、primary dragで読書modeをpreviewする。最初のdragの主軸をreleaseまで固定し、上下は表示枚数、左右は先頭枚数へ対応する。開始時のUI密度で相対mouse移動を換算し、24 logical unitsで一段階、端での過剰移動は蓄積しない。runtimeのUI-thread専用PinnedCursorが所有windowを保持し、winitのWindows cursor lockを押下位置へ限定する。appは所有windowのfocus中・gesture中だけraw MouseMotionを受け、native handleやglobal hookを持たない。lock取得失敗は状態を変更せずstatusへ表示する。releaseで確定し、Escape、focus喪失、window size/DPI変更、離脱command、overlay/modal、描画不能時にはlockを先に解放して設定とmodeを開始前へ戻す。media pathや編集履歴は変えない。開始時にbutton/value widgetのfocusを外し、drag後の左右を閲覧へ戻す。drag中のkeyboard／wheelはEscape以外を入力しない。UIAでは現在modeを持つToggle buttonとして公開し、Toggle要求はdragを取り消した後で共有commandを実行する。設定枚数は未decode／error時にもstatusへ残す。

IMEのpreedit中、および確定/取消などIME eventを含むframeでは、paletteの上下・Enter・Escapeのkey eventを消費し、IME eventだけをTextEditへ渡す。確定用Enterをcommand実行やTextEditのfocus解除、取消用Escapeをpalette closeへ二重使用しない。入力欄の固定idにfocusがない時だけ描画前に要求し、eguiの上下focus移動による確定文字の取りこぼしを防ぐ。固定版eguiのrequest_focusはIME中断も要求するため、focus保持中は再要求しない。composition状態はpalette resetで解除し、通常の操作は次の独立key入力から再開する。OSのIME状態・keyboard layoutや設定は書き換えない。

### H1 video viewport

動画のdisplay matrixはruntimeで読み、90度単位の回転・反転をownedな四隅の順序へ変換する。stream metadataを初期値とし、frame metadataがあれば優先する。appはsource orientationを編集履歴より先にUV・寸法へ適用し、SARも軸交換に合わせる。source orientation自体は編集でもdirty状態でもない。software/hardware共通で既存UV shaderを使い、hardware frameをCPUへ移さない。平行移動はaspect-fitで相殺するが、任意角度・scale・shear・射影など表現できない行列は明示的なdecode errorにする。画像の既存EXIF適用は変更しない。

FFmpeg exportとthumbnailは既定autorotateがfilterより先にmetadataを適用するため、手動の二重回転を追加しない。編集cropはorientation適用後のpixel座標とする。回転metadata付きfixtureでpreview・保存後の寸法/四隅/再openとUndoを検証する。

固定OpenH264はvflip由来の負のstrideを受け取るとencodeに失敗するため、そのsoftware encoderだけは最後にFFmpeg copy filterで通常のframe bufferへ整える。export worker内の一frame copyであり、再生のD3D11VA経路・CPU transfer契約は変更しない。

動画・音声がFaultedになった場合、理由は期限付きstatusとは別に中央へ保持し、別mediaのload/closeで解除する。非対応matrixなどの理由が通知期限後に消えて黒画面だけにならないようにする。

動画frameを選んでから同frameのUI layoutを確定し、bar・timelineを除いた中央領域にsample aspect ratio込みでaspect-fitする。appは画面上の同じ矩形をselectionと表示に使い、runtimeへphysical pixelのdestination rectと編集UVを渡す。software shaderのviewportと直接表示時のVideo Processorのdestination/output target rectを一致させ、余白はclear色で残す。decode frameのnative ownershipと単一device・1回Presentは維持する。

H1のlive visual editでは、画像と同じ履歴順のcrop・90度回転・反転を動画にも適用する。appは編集後の寸法・sample aspect ratioでaspect-fitし、selectionをその編集後画像に対する正規化矩形として扱う。runtimeへdestination rectとsource UVの四隅だけを渡し、undo/redo・Seek・tab復帰にも現在の履歴を使う。表示変更ではdecode sessionや再生位置を再構築しない。

software動画は既存RGBA textureをUV付きshaderで表示する。hardware動画のUVがidentityでない時だけ、Video Processorの色変換結果をsource寸法のRGBA texture一枚へ出し、同じshaderでback bufferへ合成する。textureはrenderer所有・寸法変更時だけ再確保し、全処理を同じdevice内に保つ。CPU readback/reuploadや別adapterは使わず、未編集時は従来の直接Video Processor表示を維持する。追加GPUメモリはRGBA画素数×4 byte（4Kで約32 MiB、driver領域別）とし、rotation/mirrorのdriver固有能力へ依存しない。HDR変換の能力判定・typed errorは従来どおり適用する。trimのlive範囲再生と動画zoomはこのscenarioに含めない。

software描画はUIから引き継ぐscissor・blend・depth stateを解除する。表示frame数は新frameの描画成功時だけ加算し、UIだけの再描画では加算しない。UIのrepaint deadlineは動画・画像・folderの待機期限と統合し、停止中のlayout・tooltip・animation要求も処理する。

RedrawRequestedはそのevent内で描画し、egui-winitのrepaint応答を次frameの無条件要求へ変換しない。静止したUIは入力・worker完了・必要なdeadlineでのみ描画する。gridのopacity transitionもeguiのanimation期限に従い、表示中という理由だけで連続描画しない。status通知とshortcut prefixの失効も待機期限へ含め、mediaやwatcherがないWelcome画面でも時間どおり処理する。

音声だけ、または動画frameを待たない音声末尾の再生中は、位置表示へ20 ms後のUI repaintを要求する。audio eventのpoll自体は次の描画を無条件予約しない。pause後はこの周期描画を止める。WASAPIとdecodeのthread・clock契約は変更しない。

動画・音声の相対Seek commandはPlaying/PausedだけでなくEndedでも受け付け、既存の絶対Seekと同じ停止preview経路を使う。終了後に勝手に再生を開始せず、Spaceで明示的に再開する。trim外のsource previewからPlayした場合は従来どおりtrim開始へ戻る。Loading/Faultedおよびsession不在ではSeekしない。

appの絶対/相対Seekは負の時刻を0へ、取得済みの非zero durationを超えた時刻をsource末尾へ制限し、末尾へのSeekでは停止する。位置表示も同じ既知durationを上限にする。末尾で停止中のPlayは先頭またはtrim開始から再開する。長さが未取得・不明・0なら上限を推測せず、trim開始/終了をsource全体のSeek制限には使わない。

parallel video decodeは対象時刻より前の最後のowned frameを一枚だけ保持し、対象以降のframeが出れば破棄する。trim上限のないsource previewで対象以降のframeがないままEOFへ達した場合だけ、その最後のframeをVideoFinishedより先に渡す。PTSは書き換えず、appの初期clockはSeek先より前へ戻さない。hardware/software共通で、追加のCPU transfer・device・workerは使わない。bounded trimの選別、取消とconsumer closeの契約は維持する。

再生側へ渡るframeでSeek先より前のPTSを持つのはこの末尾previewだけである。current frameがない時のその一枚はlate-frame dropから除外し、音声だけが続く区間でも表示を保つ。同期すべき動画frameではないためA/V drift sampleにも加えない。対象時刻以降の通常frameの遅延drop・計測は変更しない。

### H1 live volume

動画面と動画/音声status barの独立したvolume表示では、修飾keyなしの縦wheelで音量を調整する。Line/Pageの1単位またはPointの50 logical pxで10 percentage pointsとし、同一frameのraw eventを合算して0～2倍の既存SetVolume編集へ一度だけ渡す。音声playlist／scrollbar・filmstrip・tab上のscrollは奪わない。音声timeline等のlist外操作は新しいHUD／list契約に従う。focus喪失、button保持、modal/menu/palette/grid/filmstrip中は受け付けない。smooth scrollの余韻では編集しない。

音量のwheel判定はframe最後のhoverではなく、raw event列の各PointerMoved/PointerGoneを追った時点の座標で行う。先頭wheelのため前frame末尾の位置を保持し、再layout passでは同じ開始位置を使う。status/videoの有効な領域を集め、その座標の最前面layerに属するwheelだけを一回合算する。button押下やfocus喪失を含むframeでは音量操作を取り消す。

音声playlistも各event時点の領域/layerでwheelを選別する。専用のegui InputStateへ対象eventだけを渡して既存のLine/Point/Page・修飾key・touch phase・smoothingを再利用し、得られたdeltaを同じScrollAreaのoffsetへ適用する。共有smooth scrollはこの一覧では使わず、scrollbar/touch dragは維持する。余韻はpointerが離れても元の一覧に留まり、load・現在行の再配置・modal/overlay・focus喪失・button操作で失効する。全他ScrollAreaへの一括変更は行わない。

runtimeのwinit message hookはbuttonに加えWM_MOUSEWHEEL/WM_MOUSEHWHEELの座標も先行してCursorMovedへ渡す。[wheel lParamのsigned screen座標](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel)を対象windowのclient座標へ変換し、wheel本体より前に同じUI thread上でWM_MOUSEMOVEを同期dispatchする。最新のOS cursor位置ではなく各queued messageの位置を使い、元のwheel delta・順序は変更しない。client buttonとwheel以外のmessageは補正しない。

Toggle muteは現在volumeが非zeroなら0にし、0ならactive tabの適用済みedit historyを逆順に見て直前の非zero volumeへ戻す。見つからない場合だけ既定の1倍を使う。undoより先のredo履歴や別tabの値を参照せず、復元も通常のSetVolume編集としてundo/redo・live反映・exportへ接続する。

動画・音声のvolumeはedit historyの現在値をlive playbackとexportで共有する。runtimeはWASAPIへ渡す直前のstereo f32 sampleへgainを適用し、decode済みqueueは元の値を保持する。変更時は5 msのrampで不連続を抑え、mute後は正確なzero sampleにする。master endpointや他applicationの音量は変更しない。初期gainはpipeline開始前に設定し、Seek・endpoint復旧・tab再open・undo/redoにも現在値を反映する。trimは別項のH1 live trim range契約に従う。

### H1 live rate

rateは0.25～4倍のedit値を再生・exportで共有する。音声は固定FFmpegのin-process `atempo`を使い、各段を0.5～2倍に保ってピッチを維持する。WASAPI workerはstereo f32 chunkを逐次filterし、出力queueの上限とvolumeの直前適用を維持する。1倍はfilterを通さない。EOFではfilterもdrainする。

速度変更は現在のsource位置でSeekと同じgeneration更新・全queue再構築を行う。pipeline内のrateは不変とし、pause・volumeを保持する。WASAPI経過時間とvideo-only時計をrate倍してsource時刻へ変換し、frame deadlineはrateで割る。Seek、timeline、trim端点は常にsource時刻であり、rateで短縮されたexport時刻と混ぜない。

### H1 image loading

通常画像のFitはvideo／readingと同じmedia viewport全体を使い、追加の8px余白を設けない。縦横比の違いによるletterboxは保持する。Coverは既定Shift+Cの画像専用commandとしてmenu／palette／custom bindingへ公開し、viewportを覆う二軸比率の大きい方を使う。Fit同様resizeに追従し、panを中央へ戻すが選択・crop preview・編集・sourceは変更しない。寸法は回転／crop preview後、倍率はphysical pixel基準とする。readingは見開き全体のFitを維持するためCoverを無効化する。手動zoomへ移れば既存のCustom倍率・上限を使う。

Fitは通常画像・reading pageとも表示領域に入る比率をそのまま使い、2%などの縮小下限を課さない。手動zoomの下限は既存2%と長辺1 physical pixel相当の小さい方、上限は既存64倍を維持する。大きい画像のFitからzoomを始めても2%へ飛ばず、Custom倍率はwindow resizeで変わらない。これは表示倍率の変更で、decode寸法・texture上限・source pixelは変更しない。

画像のActual/100%はsourceの1 pixelを画面の1 physical pixelへ対応させ、Customの倍率も同じ基準にする。appは現在のegui pixels-per-pointでviewportをphysical寸法へ変換してcoreのscale/zoomへ渡し、描画時にlogical寸法へ戻す。Fitは現在のmedia領域、keyboard/menuのzoomは直近表示viewportと編集・crop preview後の寸法を使い、固定window寸法や未編集source寸法を使わない。panとpointer補正はlogical座標のままとし、selection・履歴・source fileは変えない。OS DPI設定の変更や新しいUI scale設定は追加しない。

Ctrl+wheelの画像zoomはeguiの単位・指数倍率の換算を維持し、scroll値の符号をframeごとの固定倍率へ置き換えない。2026-09-12 I07で平滑化済みzoom_deltaからraw eventごとの即時反映へ変更した。画像surfaceがpointer入力を受ける時だけ適用し、palette・grid・保存確認中やmenu popupでは背景画像をzoomしない。倍率と各eventのcursor基点pan補正は同じframeの描画へ反映する。回帰testは処理済みscroll値を書き換えず、実際のwheel eventから即時倍率・pointer位置と入力遮断を検証する。

固定egui-directx11 0.13.0は頂点・clipにcontextのzoom factorを別途掛ける。FullOutputのpixels-per-pointには既にzoomが含まれるため、runtime adapterは渡す値からzoomを一度除き二重拡大を防ぐ。appの入力・media座標・font生成は完全なpixels-per-pointを使い続ける。補正は固定rendererの契約に閉じ込め、依存更新時は実pixel寸法・clipとpointer hit位置を再検証する。

画像decodeとreading pageの取得はruntimeの単一foreground workerで行う。要求は最新1件だけを保持し、新しい要求・media切替・closeでgenerationを更新する。workerは各ページのdecode直後に公開し、結果mailboxには現在generationの未受取の連続したページだけをまとめる。各chunkに開始indexと要求全体の件数を付け、appは現在generationかつ次の連続indexだけをtexture化する。最初のchunkでactive画像を表示し、後続chunkでは隣ページだけを追加してactive画像のanimation・編集・textureを再初期化しない。未読込ページはShell順の位置を確保してLoading表示とし、寸法未判明時はactive画像の比率（なければ正方形）で仮配置する。後続の実寸取得時には再配置し、全ページ完了後に先読みを開始する。要求全体の512 MiB上限はchunk境界でリセットしない。workerはframe間と結果公開前にgenerationを確認する。新しい要求は待機中の古い要求と未受取結果を置換し、UI threadでworkerの終了を待たない。codec内部の単一frame decodeは即座に中断できない場合があるが、workerはwindowやGPU objectを参照しない。

H1の静止画decode cacheは最大10件・RGBA合計256 MiBで保持する。reading最大10枚の先読みを保持できるよう件数だけを8から拡張し、byte上限は増やさない。path・file size・更新時刻をworker側で照合し、不一致/metadata取得不能なら旧entryを除去する。decode前後のmetadataが一致し、要求が有効な成功結果だけをcacheし、animation・上限超過画像・失敗は保持しない。ownedなDecodedImageはArcでappと共有して画素列を複製せず、要求全体の512 MiB判定はcache hitにも適用する。cacheはwindowのworker寿命まで保持し、古いentryから解放する。このdecode cache自体はGPU textureを所有しない。decoder/texture/allocatorを含むprocess全体の上限ではなく、同じsize/更新時刻を維持する外部改変まで検出する保証もない。

I03の先読みはforeground画像workerとは別のlatest-only worker一つで行い、上記cacheを短いmutex区間で共有する。UI threadはcodec呼出しを待たず、foreground workerだけが採用した同一pathの完了を待てる。表示完了とShell snapshot取得後、現在の移動方向の隣画像一件（readingでは隣見開き全体）を選ぶ。foreground要求に含まれる実行中の一枚だけは引き継ぎ、それ以外の古い先読み・media切替・closeは取り消す。generationとfile stampを公開直前にも確認する。batch合計のRGBA上限は256 MiB、成功した静止画だけをcacheへ入れ、GPU upload・画質変更・現在画像や編集の変更は行わない。codec内部で取消が遅れる場合は最大一件のbackground decodeがforegroundと重なる。前後数十枚やanimationの先読み、初回段階表示全般を完了したという契約ではない。

cache lockは要求／結果mailboxと分離する。大きな画素列のevictionや解放中もUIの要求・取消を待たせない。先読みはPNG/WebPのanimation headerを見てskipし、GIF/AVIFも対象外とする。対象静止画ではforegroundと同じorientation／RGBA変換を利用する。採用しない先読みの取消tokenは要求更新のmailbox lock内で無効化し、workerはcache登録直前にもtokenを確認する。先読み完了はappの表示完了通知へ流さず、foregroundによるfile stamp再確認を経て初めて利用する。

続くH1の再訪表示では、app側も最大8件・RGBA相当256 MiBの静止画textureを直近利用順に保持する。workerが返したDecodedImageのArc identityが一致する場合だけ既存TextureHandleを再利用し、pathだけで古い画像を選ばない。これによりcache hitの画素変換とGPU再uploadを省く。decode cacheと同じ画素を共有するが、GPU resource量と各cacheの所有範囲は別であり、二つ合わせたprocess全体の上限ではない。animation・上限超過・失敗は保持しない。graphics復旧開始時にはtexture cacheを消し、現在画像/reading pageだけを既存の再upload経路で復元する。single device・native handle非公開・編集UV表示は維持する。

初回texture化・animation更新・復旧のRGBA変換では、行内のalphaがすべて255のときだけpremultipliedと同じbyteとして扱う。透明/半透明を含む行には固定eguiのunmultiplied変換を維持し、独自の丸めや色変換を追加しない。全画像の事前走査は避け、画素順・寸法・source RGBA・描画thread・upload経路を変えない。

animationのdeadlineに大きな遅れがある場合、最初の一周で実frame delayの合計を求め、経過した完全な周回を整数時間の剰余で省略する。残り一周以内だけを進め、通常更新を含めて現在frameと次のdeadlineの位相を保つ。frame番号が変わらなければtexture再upload/画像由来のredrawを要求しない。遅延時間に比例する全周回の反復、frame delayの丸め、OS時計/設定変更は行わない。

一回の画像・reading要求で保持するRGBA frame列は合計512 MiBまでとし、animationを逐次収集しながら上限を確認する。decoderのscratch、GPU texture、表示切替時の旧画像は別であり、process全体の512 MiB上限を意味しない。超過時は画質を落としたりanimationを途中で切ったりせず明示errorにする。readingの先頭は現在画像を再利用し、失敗した後続pageには位置を保ったerrorを表示する。読み込み中もtab操作とwindow操作ができ、loading/errorを画面へ表示する。

textureの一辺の上限はrendererが実際のD3D feature levelから返す。appはegui contextとwinit inputの両方へ起動時に設定し、texture登録前にも寸法を確認してpanicを防ぐ。外部から開くpathはruntimeのShell互換canonical pathへ統一し、相対pathでもsnapshotの現在項目と一致させる。

I03の原寸読込中表示は、元画像のorientation適用後の寸法を持つ共有memory previewだけを利用する。寸法はforeground decodeから登録し、metadata keyと一緒に失効／evictionする。寸法未知の既存disk thumbnailを原寸画像と取り違えない。appは原寸decodeとは独立したlatest-only worker一つで要求中のpathだけを照合し、世代付きRGBAを受け取る。miss時に追加の原寸decodeや補助processを起動せず、UI threadでfile I/Oを行わない。previewは別の一時texture群とし、通常画像では元寸法に基づくzoom／crop／rotateを描画に適用するがselection操作や編集・export用の画素には渡さない。readingではShell順の未読込位置にだけ置く。原寸成功／失敗、media切替、要求更新、closeで破棄し、遅れて届いたpreviewは既に読み終えたページを置換しない。graphics復旧でも破棄し、その世代の残存preview eventを使わない。全原寸が揃った時だけ従来の完了状態へ進む。未訪問／未cache画像の最初の黒い待機を解消したとは扱わない。

### H1 preview stream selection

動画thumbnail・filmstripと音声waveformは、再生と同じFFmpeg best-stream選択を使う。先頭streamの固定指定やCLIの自動選択には依存しない。native probeで選んだindexを子processへ明示し、既存のTS Seek準備と取消確認を維持する。誤った旧previewを残さないよう各cache keyをv3へ更新する。手動のstream選択UIは追加しない。

動画thumbnail/filmstripでFFmpegが正常終了しても画像が空の場合だけ、同じpreview worker内で選択video streamをSeekして最終frameのPTSを調べ、その時刻で一度だけ再生成する。時刻探索はpacket/frameごとに取消を確認し、RGBA変換やframe列の保持、新しいworkerは追加しない。CLIのmicrosecond丸めで最後のframeを除外しないよう再生成位置は1 microsecondだけ手前に置く。回転・SAR・縮小・paddingは同じFFmpeg filter経路を使う。失敗exitや静止画にはこのfallbackを適用せず、取消後はcacheへ保存しない。成功済みcache keyは維持する。空previewでは追加decodeが必要であり、長いGOP・遅いstorageの所要時間を一定に保証するものではない。

### H1 export lifecycle

動画・音声exportはtrimの有無によらず再生と同じFFmpeg best-streamを明示指定する。CLIの画素数/channel数による自動選択へ戻さず、選んだ映像・音声だけへ編集を適用する。既存のcopyts/start_at_zeroはtrim時だけとし、trimなしの時刻処理は変更しない。静止画のexport経路と保存先の保護は維持する。

Open file/folderとSave Asは専用STAでnative dialogを表示し、UIはthreadをjoinせず結果eventを受ける。本体windowをownerに指定して通常のmodal入力制限を保ち、workerがwindowの共有所有権を保持してnative handleの寿命を保証する（[IModalWindow::Show](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-imodalwindow-show)）。同時pickerは1件、picker中も描画・再生を継続し、選択前の保留Open Folderは失効させる。Save As結果は開始時のtab/pathと照合し、Cancel・失敗では書き出さずdirty guardを復元する。native dialogを閉じるまで本体の終了操作は受け付けない。

owner handleはUI threadで取得し、COM objectとSTA cleanupはworker内へ閉じ込める。復帰時はruntimeが現在のclient座標を読み取り、appがeguiのpointer位置を更新する。native dialogがcursor eventを消費しても、pointerを動かさずに次のbuttonをclickできるようにする。

Saveはruntime所有の単一background export jobへimmutableなsource・target・operation snapshotを渡す。appは進捗と完了eventだけを受け取り、再生とUI event loopを継続する。追加exportは現在jobの完了またはcancelまで開始しない。export中の追加編集は保持し、完了時はexportしたoperation列に対応する履歴位置だけをsavedにする。対象tabのclose・detach・folder内移動とprocess終了は、job終了まで保留する。dirty guardからのexportは成功時だけ元の操作を再評価し、cancel・失敗時は編集とguardを保持する。

FFmpegはtargetと同じfilesystemの専用一時directoryへ出力する。成功・非空output・cancel未要求を確認してからrenameでtargetを置換し、失敗・cancelでは既存targetを変更しない。runtimeはFFmpegの進捗pipeとdiagnostic pipeをdrainし、cancel時には子processを終了・回収して一時outputを片付ける。hardware fallbackも同じ一時output内で行う。sourceと同一pathの拒否は維持する。

U10のtab hoverはtab名の直下へ低解像度previewとpathを表示し、hover自体ではactivate／Seek／編集／focus変更を行わない。画像・音声はfilmstripと同じPreviewCache、動画はseek hoverと同じ240px・20区間中央のthumbnail keyを使う。現在tabの動画は現在位置、非active tabは最後に描画したpath／位置／durationの記録から区間を選ぶ。未観測の動画は0秒を使う。この記録はpreview専用で、U07の再生位置復元・background session保持ではない。専用latest-only worker一つが取得し、世代・tab ID・path・区間が一致する結果だけを一時textureへ変換する。hover離脱、drag／button押下、menu／modal／別overlay、fullscreen、media切替、close、graphics復旧で取消／表示破棄する。失敗はNo previewとpathを示し、同じhover中の毎frame再生成は行わない。tooltipの通常delay前からhover対象だけを取得するが、全tabの先行生成や原寸／編集済みframeの生成は追加しない。

### M7 advanced presentation and interaction

U07のlist表示状態は、eguiの共通scroll IDに位置を任せず、保持するPlaylistへ縦offset、画像／再生tabのfilmstrip viewへ横offsetと現在pathを持たせる。各drawの実際のclamp済みoffsetを保存する。tab切替ではplaylistのwheel残量・filmstripの未確定focus要求を取消し、確定した位置を復元する。filmstripのpreview取消／texture破棄はview初期化と分け、同じShell項目一覧のrefreshとgraphics復旧では位置を保持する。項目の並び変更や明示的なoverlay再openでは従来の現在項目へのrecenterを使う。timeline panelはTabId別のegui IDで高さを保持し、close時にそのPanelStateだけを除く。window縮小時の既存size clampと未保存guardは維持し、keyboard／UIA全focusの復帰監査は残す。

H1 U07の画像tabは、現在表示しているtab IDをTabSetの変更済みactive IDと区別し、別tabへの切替時に読み込み済みImagePresentation・元編集Arc・view・読書設定／ページ・folder snapshot・bar開閉・状態／一時statusを移す。同じpathのtab復帰ではRGBAとtexture handleを再利用し、ファイルを再openしない。animationのframe／deadlineもpresentationとともに保持し、非active画像は描画／animation uploadしない。未完了loadは復帰時に再要求、未完了resizeは保持した元Arcから新世代で再処理し、旧結果を適用しない。復帰直後のShell refreshで要求ページが同じなら再loadせず、隣接pathが変われば更新する。snapshotはtab close／同tabのpath変更で破棄し、既存の有界cacheとは別に開いたtabの画像をpinするためprocess全体のmemory capではない。graphics復旧はwindowのdevice世代を進め、非active画像は復帰時にCPU画素からtextureを再uploadする。全focus状態の保持は引き続きU07の残件とし、動画／音声sessionの保持契約は上記M3のRetainedPlaybackTabに従う。

H1 U06ではTabSetの選択状態をWelcomeまたはMedia(TabId)に限定し、空の選択状態をなくす。media tabのslice／active()はmediaだけを返し、Welcomeは専用の安定したtab IDでUIへ公開する。初回Openで置換し、最後のmedia closeで戻す。単独Welcomeのcloseはno-opとする。最近開いたfileは最大40件のcanonical path参照だけをAPPDATA/towavue/recent-files.txtへ保存し、編集／session backupを含めない。履歴は専用workerで読み書きし、process間file lockの中で直近操作を既存履歴へmergeして同directoryの一時fileからreplaceする。不正／読取不能な既存履歴は上書きせず警告し、現在windowの一覧は利用可能に保つ。終了は保留recordの保存とworker終了を待つ。Welcomeのrecent thumbnail gridは可視項目だけ既存filmstripのPreviewLoader／低解像度cacheを共用し、画面でmetadata列挙や原寸decodeを行わない。

H1 U09のtab context menuは指したtab IDを保持し、表示だけではactiveを変更しない。close／other／left／right／allは選択時のtab ID一覧を固定し、既存の未保存Save／Discard／Cancelとexport中の保護を通す。各dirty tabを個別確認し、Cancelでは以後のcloseを止め、既に承認したclose／保存は戻さない。共通CommandIdをmenu／palette／shortcutからもdispatchし、これらの入口はactive tabを基準にする。閉じたtabはwindow内の最大32件のpath参照だけを保持し、Ctrl+Shift+Tで末尾から明示新規tabとして開く。編集／再生状態のbackupや永続化はしない。detachはclose履歴に含めない。path copyはOS clipboardへの文字出力、Explorer表示はruntimeのSTAでShell PIDLを解決して選択表示し、mediaを外部実行しない。

preview cacheはruntimeがFFmpeg / FFprobeの子processとdisk I/Oを所有し、appへowned RGBA画像とdurationだけを返す。cache keyは正規化path、file size、更新時刻、preview種別と寸法から作り、`%LOCALAPPDATA%\towavue\preview-cache`を64 MiB以内へ古い順に削減する。waveform、duration、hover thumbnailは専用workerで生成し、path付きeventをappへ返すため、古いtabの結果を現在のtabへ適用せずUI threadもblockしない。

I03/U10ではPreviewCacheのclone間で、decode済み低解像度RGBAを最大64件・16 MiBまで共有する。worker側で同じmetadata付きkeyを再計算し、memory hitならdisk PNGの再読込・再decode・補助process起動を省く。異なるsource／更新／preview種別／時刻／寸法は共用しない。取消をhit前後にも確認し、生成中はcache mutexを保持しない。window内だけのcacheであり、process全体のメモリ上限ではない。原寸画像のforeground公開後にも、同じgenerationとfile stampが有効なら最初のorientation適用済みframeから240×160以内のnearest previewを登録する。原寸画素列のcopyやdiskへのencodeは行わず、filmstripと画像seek previewが同じkeyを利用する。原寸表示・編集・保存の画質は変更しない。未訪問画像の初回生成、tab hover／recent UI、連打中の段階表示と動画sheet先行生成は別の残件とする。

disk cacheのdirectory作成・保存／削減は補助処理とする。作成不能でアプリ初期化を停止させず、保存先の使用中やI/O失敗はdiagnosticへ出して生成・decode済み画像をそのまま返す。次の生成時にdirectory作成と保存を再試行する。メディア生成／decode失敗と取消は従来どおり失敗として返す。権限・共有状態が削減を妨げる場合の64 MiB達成は保証せず、保存済みメディアや原本のerror処理へこの方針を広げない。LOCALAPPDATA自体の欠落を含む設定・環境全般のfallbackではない。

disk保存を試みた生成結果は、保存成功時だけmemory cacheにも登録する。保存失敗後の再要求をmemory hitで遮断せず、従来の再試行を保つ。原寸画像から直接作るpreviewは最初からmemory専用で、disk永続化を要求しない。

H1ではduration・waveform・hover thumbnailごとにruntime所有の常設workerを1本だけ使い、実行中1件＋最新の待機1件へ制限する。新しい要求は未開始の旧要求を置き換え、media load/最後のtab closeでは待機を消す。3種類は互いに待たせず、別のfilmstrip workerは従来どおり独立する。window closeは未開始要求を破棄してworkerへ終了を伝え、window/GPUを所有しない実行中previewをjoinしない。これは個別decoderのメモリ上限ではなく、windowあたりの同時処理件数の上限である。

preview取消では要求単位のtokenに実行中のowned Childを登録する。要求置換・clear・worker dropはtokenを失効し、その子processだけをkillする。spawn/登録と取消を同じ短いlockで直列化し、取消済み要求から子processを後発させない。worker側でstdout/stderrを並行排出して終了を回収し、次の要求は新しいtokenを使う。[Rust Childの寿命契約](https://doc.rust-lang.org/std/process/struct.Child.html)に従い、handleのdropだけに終了を任せない。filmstripも同じ取消を使う。cache/TS Seek準備は処理境界で失効を確認するが、実行中のfilesystem I/Oやnative FFmpeg probeを強制中断する保証はない。UIはprocessの完了やreader threadをjoinしない。

waveformはFFmpegからmono S16 PCMを逐次受け取り、runtimeで平均絶対振幅を集計して中央揃えの白いbarへ描く。全frameを終端まで保持するshowwavespicを使わない。64 KiB入力bufferと最大width×1024個のu64和を保持し、満杯になったら隣接binを併合して時間粒度を倍にする。終端の実sample数で従来と同じ列範囲を求め、集計binの部分重なりだけ平均値で近似する。640列の和は最大5 MiBで、音声の長さには比例しない。短い素材・silence・pulse・rampで従来平均振幅との誤差を検証し、duration metadata欠落には依存しない。PNG寸法・白色・透明背景・cache上限を維持する。子process診断も末尾16 KiBへ限定する。FFmpeg自身のdecoder/demuxer作業領域を含むprocess全体の厳密なメモリ上限ではない。

grid menuは既存のcommand registryだけをdispatchし、画像・動画・音声ごとの16 commandを`%APPDATA%\towavue\grid.conf`に保持する。cell順は物理keyの`1234/qwer/asdf/zxcv`と固定してclickとkey入力を一致させる。表示・非表示には短いopacity transitionだけを使い、media操作の意味を持つanimationは追加しない。

H1の起動時はshortcut／grid設定の読込・parse・初回保存に失敗しても、その設定だけをmemory内の既定値へ戻してwindowを開く。失敗したpathと理由、既定値使用、原本を修正してReloadできることを一回のnative OK警告で知らせる。既存fileを修復・上書きせず、片方の有効な設定は保持する。明示Reloadは従来どおり失敗した設定の現在値を保持し、起動時fallbackを再適用しない。APPDATA自体の欠落やgraphics／worker初期化の失敗まで隠す規則ではない。

grid入力はwinit PhysicalKeyのDigit1～4とKeyQ/W/E/R/A/S/D/F/Z/X/C/Vへ対応させ、logical文字やIME確定文字を位置として使わない。Shift/Capsによる文字変化は位置を変えず、Ctrl/Alt/Super付きは通常shortcut側へ渡す。gridの対象keyはeguiのfocus処理より先に扱うが、palette・modal中は横取りしない。paletteを開いたらgridを閉じ、入力欄へ集中させる。keyboard layout・OS IME設定は変更しない。

gridの4列・4行は表示領域から求めた同じcell寸法に固定し、長いcommand名で列を拡張しない。keyと名称はcell内の行数で折返し・省略し、無効項目もhoverで全文を示す。高さが小さいcellではfontを11pxにしてkeyと名称の2行を保つ。設定pathも幅制限付き省略と全文tooltipを使う。通常cellの110×52 logical pxを上限とし、小さいwindowやUI拡大時は縮める。clickもkeyと同様に一回実行して閉じ、fade-out中のcell入力は無効にする。command配置・shortcut・有効条件は変更しない。

window外へdropしたtabは、appが同じexecutableへ現在pathを引数として渡して別processを起動し、起動成功後だけ元tabを閉じる。edit historyをprocess間で暗黙移送せず、dirty tabは既存guardを通す。別windowへの再結合を行うprocess間protocolは設けない。

hardware exportはruntimeがFFmpegのMedia Foundation H.264 encoderへ`hw_encoding=1`を要求し、成功時だけhardware利用として返す。encoderまたはcontainerが非対応なら、同じrequestをM6 software codecで再実行する。appは設定値ではなく`ExportOutcome`の実結果を表示する。

hardware frameはFFmpegのPQ / HLG transfer metadataをruntime内で保持する。rendererは`ID3D11VideoProcessorEnumerator1::CheckVideoProcessorFormatConversion`でsource color spaceからSDR swap-chain color spaceへの変換を確認し、対応時だけ`ID3D11VideoContext1`へ入力・出力color spaceを設定する。未対応時はtyped errorとし、metadataを無視した表示やHDR対応の宣言をしない。10-bit HDR pass-throughは、HDR displayと10-bit UI合成を含む別のarchitecture decisionなしには有効化しない。

## 5. スレッド・同期契約

- demux、video decode、audio decode、WASAPI outputを役割ごとのworkerに分ける。
- packet queueはboundedかつblockingとし、packetを便宜的にdropしない。
- audio sampleは通常再生中にdropしない。decoded video frameだけをpresentation時刻に基づいてdropできる。
- 音声が存在して再生中なら`IAudioClock`をmasterとし、running中にdevice clockが停止した区間だけ単調時計を下限にする。動画のみ、priming中、audio drain後は単調時計とanchorを使う。
- Seekはgeneration更新、demux停止、全queue破棄、`avformat_seek_file`、decoder flush、目標時刻までのdecode/discard、audio/video primingを一つのtransactionとして扱う。
- すべてのworker結果へgenerationを付け、不一致のpacket、frame、eventを破棄する。
- device removal、audio endpoint変更、decode failureはtyped session errorとして扱い、復旧可能な場合だけpipelineを再構築する。

## 6. Explorerのフォルダ並び順

### 意味

「Explorerの並び順」は自然名前順ではない。対象フォルダに対してWindows Explorerが現在適用または保存しているSort Byの列、方向、複数列条件を指す。Name、Date modified、Date created、Size、Typeと、Shell拡張が提供する列を同じ契約で扱う。

### 取得優先順位

1. mediaを開いた時点で、同じfolder PIDLを表示しているExplorer windowを`IShellWindows`から探す。
2. 複数ある場合はforeground window、次に直近active windowを選ぶ。
3. そのwindowのactive Shell viewを`IServiceProvider`、`IShellBrowser`経由で取得し、`IFolderView2`へqueryする。
4. 一致するwindowがなければ、専用STA Shell worker上で非表示`IExplorerBrowser`を対象folderへnavigateする。独自property bag名を設定せず、Explorerと同じ保存済みfolder view/default templateの解決を利用する。読み取りによって設定を書き換えないよう`EBO_NOPERSISTVIEWSTATE`を使う。
5. `GetSortColumnCount`と`GetSortColumns`でsort metadataを取得し、Shell viewが返すitem順を正とする。

registryのExplorer Bagsを直接解析しない。これは非公開の保存形式へ依存し、現在適用中だが未保存のviewとも一致しないためである。

### FolderSnapshot

`FolderOrderProvider`は専用STA worker上で次の情報を持つimmutable `FolderSnapshot`を返す。

- canonical folder PIDLとfilesystem path
- Shell item identityとfilesystem pathを持つordered media items
- `PROPERTYKEY`とascending/descendingを持つsort columns
- source: `LiveExplorerView`、`PersistedShellView`、`Fallback`
- capture generationとtimestamp

Shell viewの列挙順を取得してから、対応mediaだけをfilterする。filmstrip、全種移動、同種移動、audio playlistは同じsnapshotを共有し、個別に再sortしない。

Open Folderで対応mediaが見つからなかった場合はstatusだけを更新し、表示中mediaのsnapshot・tab・編集を保持する。別folderのsnapshotは実際のmedia load時にだけ現在のnavigationへ反映する。

H1ではUIからのsnapshot取得を非同期要求へ変更する。専用STAは最新1件の待機要求と完了結果を持ち、appはgenerationと現在pathで古い結果を拒否する。取得中もmedia表示・tab操作を継続し、同folderなら既存snapshotを保持する。Open Folder要求をwatcherの背景refreshで上書きせず、media切替・最後のtab closeでは古い要求を失効させる。終了時は結果公開を止め、実行中のShell API完了をUIでjoinしない。COMの解放とSTA終了は所有worker上で行う。

Shell STAはsnapshot完了後もWindows messageを処理する。通常の条件変数だけで休止すると、Shell/COMが残したSTA所属の補助windowへの呼出しまで止めるため、所有するauto-reset eventと[MsgWaitForMultipleObjectsEx](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-msgwaitformultipleobjectsex)で要求・終了・messageを同時に待つ。queueのdispatchはmailbox lock外で行い、eventはproviderとworkerの共有所有権がなくなってから閉じる。新しい周期pollやUI側の待機は追加せず、最新要求・世代失効・Shell並び順の契約は維持する。

フォルダ変更はoverlapped `ReadDirectoryChangesW`で検知し、150 msのdebounce後に新しいsnapshotを作る。現在項目はShell identity、次にcanonical pathで再対応付けし、位置番号だけで保持しない。ExplorerのSort By変更はmedia load時とfilmstripを開く時の再取得へ反映する。

Shell viewの作成・列挙に失敗してもmedia open自体は失敗させない。その場合だけWindows自然名前昇順へfallbackし、診断ログと一時status messageで縮退を明示する。

### 検証条件

M4ではName、Date modified、Date created、Size、Typeの昇順・降順、同値、複数列sort、Explorerが開いている場合と閉じている場合、sort変更後の再読込を自動またはfixture付きintegration testで確認する。

## 7. ライセンスと配布

2026-09-09のowner指定により、現在のgoalは公開行為を含めず、未公開評価版の安定性・速度・操作感と草案の見た目の仕上げを継続する。署名・release公開・配布開始はownerの別指示まで実行しない。Windows 10の互換性目標は維持するがowner環境へOSを導入せず、合理的な仮想環境検証が難しければ実機確認を省略して未検証と記録する。Windows Server上のCI成功をWindows 10実機確認へ置き換えない。これらを理由に無関係なH1改善を停止しない。

評価Setupには、そのbuildで使用したinstaller source／入力manifest／原ライセンス／再build手順を`licenses/INSTALLER-SOURCES.zip`として同梱する。既存のbuild-source一覧を再利用し、ZIP内の`SOURCES.json`、installed inventory、guideのsize／hashを対応させる。本体・native sourceの別companionをinstaller変更のために旧版から再定義せず、外部binary／toolchain入力は固定情報により別途照合する。ZIP自体も明示配置／削除一覧に含める。sourceからのSetup再構築は本体／native再buildや、実導入・公開完了の証明ではない。

2026-09-07、ownerの指定により、初回配布はmonapadと同様のインストーラーexeとする。参照した[monapadの配布設定](https://github.com/sheetau/monapad/blob/1c765729dd1386077a3caffc67d45ed4a89506e0/package.json)はNSIS、oneClick=false、インストール先変更可である。towavueもインストール先を選べるSetup.exeを目標とし、インストール不要の単一実行file化は要求しない。Rust/native構成は維持し、Electronやmonapadの自動更新・file関連付けをこの指定から追加しない。

必要なDLL・ffmpeg.exe・ffprobe.exeはインストール先へ配置し、利用者による開発用FFMPEG_DIR/PATHの設定を必要としない構成を計画する。これは配布方式の決定であり、同梱物の検証・再配布条件の確認・installer実装・clean-machine検証の完了ではない。署名・公開・課金は別途扱う。

H1のinstaller安全性は、まずNSIS 3.12のUnicode／zlib、通常user権限のfixture専用Setup.exeで検証する。Welcome・インストール先選択・確認付き削除を持ち、既存の非空directory、drive root、UNC、reparse point経由の配置を拒否する。削除は完成時の配置markerを照合して同梱fileの明示一覧だけを対象とし、directoryは空の場合だけ除去する。再帰削除、再起動時削除、registry／shortcut／関連付け／共有runtimeの変更はこの段階に入れない。fixtureはアプリ・FFmpeg・VC packageを含まず、実アプリ導入／更新の完成や配布採用とは分ける。将来の本体Setupでも利用者のmedia／設定は所有対象にしない。

H1の資料への入口はHelp menu／command paletteの`Show licenses and sources`とする。実行中exeの隣にある`licenses/START-HERE.html`をExplorerで選択表示し、HTMLやarchive自体は自動実行しない。欠落時は期待pathを表示し、cwd・FFMPEG_DIR・PATH・開発treeや推測した公開URLへfallbackしない。Windows runtimeの専用STA workerがShell操作を所有し、appへは成功path／errorだけを返す。appは重複要求を抑え、再生・編集状態を変えない。この入口は資料の存在・release適合性・配布採用の承認ではなく、installer側の配置と最終source同時提供は別途必要。

H1の補助process探索はruntime内へ統一する。本体exeと同じdirectoryにffmpeg.exe／ffprobe.exeのいずれかが存在すれば、両方ともそのdirectoryを使う。不完全な配置を開発用helperで埋め合わせない。同梱helperが両方ともない開発配置だけ、非空のFFMPEG_DIR/binを使う。そこにも必要なexeがなければ期待pathを含むerrorにし、PATH上の別版を暗黙に起動しない。processへ渡すpathは絶対pathとし、作業directoryやPATHを変更しない。これは既存preview／保存の探索修正であり、同梱物の採用・installer作成・DLL検索規則の変更ではない。

本体入りSetupの最初のlocal評価版はfixtureと同じpath／明示削除の処理を使い、95個の本体・runtime fileとnotice資料を空の専用directoryへ配置する。通常user権限・対話形式のみで、既存版の上書き、registration／shortcut、関連付け、自動起動はこのsliceには含めない。source archiveは固定hashの別companionとし、installed guideにはその名前・hash・未公開状態を明記する。元のnotice／header／build recordは保持し、archiveを省いたinstalled treeには独自のfile inventoryを付け、companion全体のinventoryと混同しない。VC判定は既存のRegistry64 readerを同梱Windows PowerShell wrapperから再利用する。必要時だけuserの確認後に元packageのfull UIを起動し、条件同意の代行やquiet installをしない。post-checkと実exit codeを確認し、3010は再起動要求として保持するが再起動・本体起動は実行しない。共有VC runtimeは削除対象にしない。これはbuildの評価段階であり、supported Windowsでの実導入・更新・削除と公開判断は別gateである。

local評価Setupの次のsliceでは、現在userのHKCU Registry64に専用のuninstall登録を作り、同userのProgramsへ起動用shortcutを1個置く。全user、desktop、関連付け、自動更新には拡張しない。既存key／shortcutがあれば配置前に停止し、完成したpayloadのmarkerを書いた後で登録する。削除前に登録のpayload id／install pathを検証し、payload削除成功後に登録を除く。変更されたshortcutは記録SHAとの不一致により保持し、未知のregistry value／subkeyも除去しない。欠落した登録からshortcut所有権を推測しない。helperはNSISのprivate temporary directoryで同期実行し、COMをそのprocess内で解放する。試験は実アプリ一覧ではない専用GUID keyとscratch内shortcutへ限定する。導入更新のgateは引き続き別途必要。

既存版更新は、旧uninstallerを先に実行する方式にはしない。最初に読み取り専用で、旧markerのpath／inventory hash、新payloadの固定inventory hash、両方の明示file一覧と実内容を照合し、追加／置換／削除対象を確定する。記録済みfileの変更・欠落、使用中file、所有外fileとの衝突、曖昧な名前やreparse pointは停止条件とし、利用者の差し替えDLLを自動復元しない。現在のmarkerはuninstaller自体の原本hashを持たないため、旧uninstallerは実行せず現在bytesの退避対象としてのみ扱う。判定は観測snapshotであり、更新許可やatomicityの証明ではない。実更新には直前再検証、同volumeの退避と失敗時復旧、登録切替、対応OSの実lifecycle検証が別途必要であり、それまではSetupの既存folder拒否を維持する。

更新のfile処理は、専用の同volume sibling directoryへ新旧bytesと不変journalを準備してから実施する。callerは返されたjournal digestをApply前に別途保持する。元のfileへ新bytesを直接書き込まず、既存fileの共有制限付きhandleを保持したまま名前を退避し、検証済み別fileを配置する。open済み対象への通常の置換はWindowsで拒否されるため、この二段階の間の欠落も復旧対象とし、payloadと登録の整合後に本体exe、その後にuninstallerを公開する。復旧は新旧の既知bytes、退避fileと配置receiptを照合し、第三の変更や配置完了記録後の欠落は上書きせず停止する。receipt直前の中断を含む全namespaceのatomicityや同userによる競合変更の防御は保証しない。削除対象と復旧に使った資料は保持し、directoryの再帰削除は行わない。file処理の成功だけでは登録を含む更新完了とはしない。登録切替・digestの永続化と再開UI・cleanup・実Setup接続・対象OSの中断／復旧・NTFS metadataの完全な維持を別gateとして残し、最初は試験配置でのみ実行する。

fileのRollbackは、検証した退避原本の名前を元へ戻す方式とし、復旧用の大きなcopyを新規作成しない。原本が欠落・変更・使用中なら配置の変更前に停止し、独立backupからの自動再作成で曖昧さを隠さない。これにより復旧時のfile identityとmetadataを維持するが、filesystem metadata用の空き領域まで不要になるという保証や、電源断に対するatomicityの保証はしない。変更のないfileは照合と共有制限付きhandleを維持する一方、不要な新旧copyは作らない。本体exeは中断時の混在起動を防ぐため同一bytesでも退避・最終配置の対象とする。

同じdirectoryへの更新では、既存uninstall登録の所有IDとEstimatedSizeだけを切り替える。旧・新の値と型をcallerが退避し、helperはそのどちらかに一致する中間状態だけを許可する。容量値を先、所有IDを最後に書き、逆方向も同じ処理で復旧する。install path、起動command、shortcutとその記録hash、未知のregistry dataには触れない。既存key欠落、別path、不明な値／型は停止条件とする。二値のatomic更新ではなく、fileと合わせた復旧は下記journal契約に従い、再開UI・実Setupへの接続は別gateである。

登録付き更新では、Prepareが旧登録の型付き値を読み、file plan由来の新ID／容量と同じ不変journalへ記録する。Apply／Rollbackとも登録状態を配置変更前に検証し、file群が目的の状態になってから、本体exeの最終配置直前に登録を切り替える。file変更を伴う登録失敗時はexeを退避したまま保持し、同じjournal／digestから旧版への復旧を再試行する。新規配置が途中まで進んだ状態のApply再実行は許可せず、復旧後に新しいtransactionを準備する。digestの独立した永続化、uninstallerを含む実Setupの競合防止と復旧導線、cleanup、対象OSでのlifecycle検証が揃うまでは内部helperと試験配置に限定する。

登録付きの内部entry pointは、不変journalの完成後、配置変更前に既存HKCU Registry64 keyの`TowavuePendingUpdate`へdirectory／digestを一つのREG_SZとして保存し、flushする。未完了記録があれば新規更新を拒否し、別processは指定したinstall path／登録key／shortcutとjournalの結合を確認してRollbackする。成功後だけ同一記録を削除し、失敗・不明な型／内容／別配置の記録は保持する。新しい登録helperの削除確認／削除はpending値の存在だけで拒否する。旧uninstallerの退避と公開順序、復旧UIへの接続は引き続きgateとし、Setupの既存folder拒否を維持する。

新Setupと登録付きentry pointの排他単位はuser SID／登録keyとする。別directoryへの新規導入でも一つの登録先を奪い合うため、directoryだけでは分けない。共通helperがglobal named mutexの名前を生成し、最初にobjectを作ったprocessだけがhandleを保持して処理する。既存objectならhandleを閉じて拒否し、wait／thread所有は使わない。NSISのsection workerとGUI終了処理が別threadでも解放可能にし、正常完了・終了時にhandleを閉じる。導入はprerequisite／登録確認から配置と登録完了まで、削除は所有確認前からpayload・登録・marker削除まで保持する。旧生成物へ遡って効くものではなく、更新childへの所有の受け渡しと旧uninstaller保護、実導入済み製品の中断復旧は未完了である。

file更新／Rollbackの公開順序は二つのexeを対象とする。退避先を両方とも検証してからUninstall.exeを先、本体を次に退避し、payloadと登録を揃え、本体、uninstallerの順に公開する。両exeは同一bytesでも退避対象とし、最初の退避直後は整合した元の本体だけが残り、混在中はどちらの設置pathからも新規起動させない。新journalはschema 3で旧readerによる再開を拒否し、新readerはschema 1／2の本体だけ退避した中断も復旧する。Rollback時に元exeがまだ設置pathにある場合は旧hashを照合して原本を退避し、同じobjectを戻す。すでに起動済み・自己copy済みの旧uninstallerを停止する保証ではなく、legacy processの確認は引き続き必要である。

2026-09-09、上記の段階的な既存folder拒否をlocal評価Setupの登録済み同directory更新に限って解除する。単なる登録発見は更新許可ではなく、確認後に新payload／新uninstallerを配置外へstageし、既存file変更前に親のleaseを解放してchildが通常取得・全体再検証する。競合中なら停止し、先に完了した別操作は現在状態として再検証する。排他の迂回flagや旧uninstaller実行は導入しない。pending記録は新規Applyでなく確認付きRollbackへ接続し、旧版復旧後にSetupを再実行して更新する。復旧時はVC導入や新payload stagingを行わず、失敗時の資料と利用者変更を保持する。helperはNSIS pluginのSystem.dllをframework参照と誤認しない専用cwdから起動し、build時に原本／staged source hashを照合する。実app branchの生成payload・専用GUID登録による試験と、実アプリ／VC・自己copy削除・対応OS・progress／復旧UIの認定は分け、後者のgateは残す。

更新／復旧の段階別診断はhelperの返却objectと分けたverbose streamへ出し、NSISは処理終了後の短いstack応答でなく実行中のdetails logへ接続する。推測の進捗率／残り時間は出さない。完了画面は新規導入・更新・旧版復旧を区別し、復旧では新版未導入とSetup再実行を明示する。3010の再起動要求は文面と終了codeへ保持し、再起動自体は実行しない。NSISが渡す新uninstallerの一時source pathはlocal／reparse検査後にDOS別名を展開するが、inventory内のfile名別名拒否は維持する。表示値・実短縮pathの試験を、実画面のclipping／focus／screen readerやVC実動作の確認と同一視しない。

本体はMIT OR Apache-2.0。配布向けFFmpegはGPL/nonfree componentsとそれに反する推移依存を除いた9.0.1のDLLを動的リンクする。配布時には対応するFFmpeg source、build configuration、変更差分、著作権・LGPL表示、第三者license一覧を同じreleaseから取得可能にする。

固定開発buildは`--enable-version3`を含み、license表示はLGPL 3以降である。本体の直接依存6 DLLに加え、ffmpeg.exe/ffprobe.exeのためavdevice DLLも必要になる。現在の同梱候補とsource・第三者表示・VC runtimeの未完了事項は[DISTRIBUTION.md](DISTRIBUTION.md)へ記録する。開発archiveにLICENSE.txtがあることだけでは配布承認としない。

FFmpeg binaryやsource archiveは、再現可能なbuild・配布工程を定義するmilestoneまでGitへ入れない。

2026-09-08、固定開発buildにはChromaprint経由のGPL FFTW静的リンクがあることを確認した。このbinaryを上記LGPL構成の配布候補から除外し、開発参照用として保持する。[FFMPEG_REBUILD.md](FFMPEG_REBUILD.md)に、機能を削らず固定sourceのKissFFT backendを選ぶ修正案と再build gateを記録した。FFmpegの自己申告licenseだけで承認せず、新binaryの実link入力と全体動作を確認する。本体のlicense変更は行わない。

同日、ownerはZVBIの未使用の番組制御・放送時刻APIを含めない限定buildの検証を許可した。FFmpegのTeletext字幕デコードは維持し、codecの無効化、成功を装うstub、元license表記の変更は行わない。これは検証の許可であり、汎用ZVBIとの全API互換や配布採用の決定ではない。対象source/headerの除外、字幕の実出力比較、新headerでのFFmpeg全体再buildと最終候補の品質gateを満たすまで既存DLLを置き換えない。根拠と検証範囲は[NATIVE_RUNTIME_AUDIT.md](NATIVE_RUNTIME_AUDIT.md)へ記録する。
