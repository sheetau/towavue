# towavue

towavueは、画像・動画・音声を一つの軽快なWindowsアプリで閲覧・再生し、基本的な非破壊編集まで行うことを目標とするプロジェクトです。メディアを主役にした最小UIと、D3D11を中心とするGPU常駐の表示経路を両立させます。

## 現在の状態

動画サムネイルシートの生成では、16コマごとの入力・補助デコーダー・縮小処理を共用します。通常経路ではコマごとのFFmpeg起動とPNG往復をなくし、対応できない入力は従来経路へ戻します。測定用1080p長GOP素材の16コマ取得は、Releaseで約1.33秒から約0.76秒へ短縮しました。再生デコーダーやGPU表示経路は変更していません。全素材・端末での速度やピーク負荷を保証するものではありません。

動画の細いシークバーを横にドラッグすると、一時停止してメイン映像に低解像度プレビューを表示します。バー上の小さなサムネイルと時刻もドラッグ中に表示し続けます。離した時に一度だけSeekし、元が再生中なら再開します（終端は停止）。Escape・フォーカス喪失ではSeekせず取り消します。切り抜き・回転・反転・リサイズと表示倍率をプレビューにも反映しますが、粗い時刻サンプルなので正確なフレーム確認は離した後に行います。

動画のSeek・タブホバーは低解像度のサムネイルシートを共用します。短い動画は約20区間、長い動画は約5秒間隔で、16コマずつ準備します。再生位置の周辺を先読みし、遠い位置はホバー時に優先生成します。準備中は単枚プレビュー、準備後は同じテクスチャ内の表示範囲だけを切り替えます。生成素材の可視ウィンドウでホバー・Seek・タブ表示とドラッグの確定／取消を確認しました。長GOPの負荷、混在DPI、全素材の表示品質・性能は未検証です。

隣の静止画を先読みした結果から、縮小プレビューも共有キャッシュへ登録します。表示前でもfilmstripなどで使え、縮小キャッシュだけが消えた場合は保持中の原寸から再生成します。先読みする枚数・原寸予算は増やさず、先読みだけで現在の画面を変更しません。

初めて開くアニメーション画像でも、最初のフレームを復号できた時点で240×160以内の縮小プレビューを表示できます。GIF／APNG／animated WebP／AVIFの原寸デコーダーを共用し、全フレームの読込中も通常表示・読書モードの元寸法を保ちます。原寸が先に完了した場合は直接原寸を表示します。静止画の最初の復号待ちやcold-storageの遅延、可視UIでの改善幅は未解消・未検証です。

サムネイルのメモリキャッシュをウィンドウ間で共有し、同じサムネイルや再生時間の重複生成を抑えます。低解像度画素はホスト全体で64件／16 MiB、再生時間は成功値64件まで保持します。全素材での表示時間の改善幅や、アニメーション画像の先読みは未検証・未完です。

同じユーザー・ログオンセッション・実行ファイルからの通常起動は、既存のホストへ集約します。新しく開くウィンドウ同士もGPU deviceを共有し、タブを結合できます。ファイル／フォルダの起動と、引数なしのWelcomeに対応します。受付を確認できない場合は重複起動せず診断します。更新前から独立して動いているプロセスのタブを自動回収する処理はありません。

filmstripのサムネイルを外へドラッグすると、同じプロセス・GPU deviceの新しいウィンドウで元ファイルを開きます。元タブの編集・保存先・再生状態は変えません。ウィンドウの初期化やファイル受付に失敗した場合はfilmstripを残し、受付後の読み込みエラーは新ウィンドウ側に表示します。

画像・音声・動画タブを外へドラッグすると、同じプロセスの新しいウィンドウへ表示・再生状態と未保存編集を移します。アニメーションの現在フレーム、読みかけのページとプレビュー、処理中の画像編集も引き継ぎます。取得済み画像は再読込せず、不足ページだけ元のメモリ予算内で読み込みます。停止位置・選択・プレイリスト設定などを保持し、移動先の準備に失敗した場合は元タブを残します。元の最後のタブを移した後はWelcome画面になります。同じホストの既存ウィンドウへは、タブバーの挿入線を目印にドラッグ結合できます。空のWelcomeにも挿入できます。可視ウィンドウの実入力・重なり・混在DPIは未検証で、別々に起動したプロセス間の結合は未対応です。

共有GPUの障害時は、対象ウィンドウの再生を止め、同じ新device上の描画先をすべて作成してから復帰します。途中で作成に失敗した場合は、対象の編集と停止位置を残します。Retryはそのウィンドウだけを復旧し、すでに動いている他のウィンドウやCancelしたウィンドウを再起動しません。復旧時に停止中の表示が1フレーム進む問題も修正し、時計位置と表示フレームの時刻を別々に保持します。

通常起動をウィンドウ管理ホストへ移し、通知の宛先・待機時刻・終了判定をウィンドウ単位に分離しました。非表示の2ウィンドウで、片方の未保存確認を取消／破棄しても、もう片方の動画表示・停止位置を維持することを確認しています。移動した再生sessionの通知は、元ウィンドウを閉じた後も現在の所有先へ配送します。

同じD3D11 deviceに複数の描画先を作るruntime基盤を使っています。非表示の所有ウィンドウ2つで、同じ動画sessionの交互描画・リサイズ・描画先の破棄／再作成と次フレームへの進行を確認しています。filmstripからの新ウィンドウも同じhostを使います。

filmstripのサムネイルをウィンドウ外へドラッグして離すと、その元ファイルを新しいウィンドウで開けます。元タブと未保存編集は残し、新しいウィンドウへ編集結果をコピーする操作ではありません。既存プレビューを追従表示し、ウィンドウ内へのdrop・Escape・フォーカス喪失などで取消します。起動要求に失敗した場合はfilmstripを残して理由を表示します。

タブをドラッグすると、掴んだ位置を保ってタブ本体が追従し、挿入先の隣接タブがすぐに場所を空けます。順序の確定は離した時だけで、再生中のタブや編集状態は切り替えません。タブバー端に保持すると横スクロールでき、Escape・フォーカス喪失などで取消します。ウィンドウ間の結合・状態移送はまだ未実装で、外への分離は従来の保存確認付き・パスだけの別プロセス起動です。

映像デコードを休止した非アクティブ動画の最後の表示フレームは、同じGPU上の独立した1枚として保持します。デコーダーのテクスチャ配列全体を残さず、タブ復帰直後の表示・停止位置・背景音声を維持します。H.264の小画像と1080pで24枚から1枚への保持対象削減と画素一致を確認しました。終端不明で背景デコードを続ける場合は対象外です。これはプロセス全体のVRAM上限や復帰時のデコード待ち解消を意味しません。

タブへ戻ると、最後にフォーカスしていたメディア操作部品も復元します。再生・読書・リピート／シャッフル、Seek・選択辺・時間範囲／音量、プレイリスト／filmstrip項目を対象とし、復帰だけでは再生や編集は変わりません。非表示・無効・削除済みの操作部品は現在タブへ戻し、読込待ちでも新しい操作を優先します。同じタブのファイル切替・再読込・closeでは記憶を破棄します。

タブ名やタブの閉じるボタンへフォーカスを移し、Shift+F10／メニューキーでもタブのコンテキストメニューを開けます。対象はフォーカス先のタブで、開くだけでは現在のメディアを切り替えません。UIAのShowContextMenuにも対応し、Escapeは呼出元へ戻ります。未保存確認のCancel後も戻り、対象を閉じた場合は残るアクティブタブ、全タブを閉じた場合はWelcomeへフォーカスを戻します。

左上のロゴを右上へドラッグするとFile、右下でEdit、左下でViewを開けます。8 logical px以上動かして離す操作で、ドラッグ中は対応する矢印を強調します。元の位置へ戻す／左上方向／Escape／フォーカス喪失では取消です。方向操作では離した位置の脇にメニューを開き、通常クリック・キーボードによるメニュー操作も従来どおり使えます。開くだけでは編集や保存は実行しません。

保存中はツールバー下の境界線に進捗を表示します。音声・動画は編集後の出力時間に基づく推定で、ノーマライズ時は解析と書き出しを半分ずつ表示します。静止画や長さ不明の素材は短い白線が移動し、取消要求後は止まります。完了／取消／失敗で元の境界へ戻り、画像移動やサムネイル読込では表示しません。取消ボタンと保存状態の詳細は従来どおり利用できます。

JPEG も File menu「Metadata export options」から XMP の Title／Artist／Comment／Copyright を指定できます。言語別の既存値・作者順を表示し、Keepは全値を保持、Setは一つの値へ置換、Removeはその項目の全値を削除します。JPEG形式で保存してください。設定未使用／全項目Keepの通常保存でも対象4項目を保持します。EXIF／IPTC／JPEGコメントとの同期・他6項目・Extended XMPは未対応で、元の未知／技術XMPはコピーしません。

動画・音声とPNG画像では、同じ「Metadata export options」からタイトル・アーティストなど10項目をKeep／Set／Removeで指定できます。既存値を非同期で表示し、Apply後のSave／Export asに反映します（動画は音声のみ出力にも反映）。設定は現在のファイル・タブ内だけで保持し、再読込・別ファイル／次曲・タブを閉じると解除します。再生・表示画素・編集履歴は変更せず、Cancel／Escapeは未適用の入力を破棄します。PNGはPNG形式で保存してください。全項目Keepや設定未使用の通常PNG保存でも対象の文字情報を保持します。PNGのEXIF／XMPは編集対象外で、PNG／JPEG以外の画像形式は制約を表示しApplyできません。画像metadataの読取待ち／失敗時もApply不可です。非対応の出力形式・破損／過大な文字情報では、既存の保存先を変更しません。

動画・音声のFile menu「Audio export options」から、ノーマライズとKeep／Mono／Stereoを設定できます。Apply後のSave／Export as／音声のみ出力に反映し、再生音や編集履歴は変更しません。設定はタブ内の現在のファイルだけで保持し、再読込・別ファイルや次曲への移動・タブを閉じると既定のOff／Keepへ戻ります。Cancel／Escapeは設定を変更しません。ノーマライズは編集・channel変換後のsample peakを−1 dBFSへ合わせる二段階処理で、LUFS／true-peak補正や動的音量追従ではありません。全体音量の変更を打ち消す場合がありますが、曲中の強弱と無音は保持します。

動画のFile menu「Export audio only」から、音声だけを別ファイルへ書き出せます。既定は`元の名前-audio.wav`で、WAV／FLAC／MP3／M4A／AAC／Ogg Opus／Opusを選べます。時間範囲・区間削除／伸縮・音量・速度は反映し、映像のcrop／回転／resizeは含めません。再encodeであり圧縮音声の無変換抽出ではありません。元動画の編集履歴・保存済み状態・通常のSave先は変わらず、未保存編集の確認も残ります。音声なし・空の出力・キャンセルでは既存の保存先を保持します。

音声でも `,`／`.` で10ms戻る／進む操作が使えます。移動すると一時停止し、削除・伸縮後の時間軸にも対応します。View menuの「Step audio backward／forward (10 ms)」からも使え、先頭・終端で止まります。これは音声の微小Seekで、PCMの1sample移動や圧縮形式ごとのフレーム境界移動ではありません。

動画はtimeline表示中に`Ctrl+R`、またはEdit menuの「Resize / resample video」からリサイズできます。右下のdialogで幅・高さとNearest／Bilinear／Bicubic／Lanczosを指定し、映像面にGPUプレビューを表示します。元の表示比率を既定で保持し、連動する辺は2px単位へ丸めます。出力は偶数16～16384px・正方形ピクセルで、寸法・GPU予算を超える指定は適用できません。Applyは一件の編集、Cancel／Escapeと同寸法・ピクセル比不変の場合は非編集です。crop／回転・Undo/Redo・保存へ接続済みで、再生位置や再生／停止状態は変更しません。通常window・全素材品質／持続性能の最終確認は未完です。

動画のtimeline表示中は`Ctrl＋ホイール`でカーソル位置を基点にズームし、右ドラッグで表示位置を移動できます。`+／-`で拡大縮小、`Ctrl+H`で100%、`Shift+W`でFit、`Shift+C`でCoverへ戻せます。View menuからも使え、倍率はステータス欄に表示します。100%は表示向きの1画素行を画面の1物理pxへ合わせ、横方向は元のピクセル比を保持します。表示だけの変更で、保存画素・編集履歴・選択を変えません。timelineを閉じたり全画面・タブ切替をしても倍率と移動位置を保持します。画素を変更するCtrl+Rのresizeとは別の表示操作です。

動画もtimeline表示中にEdit menuの「Free rotate video」、または`Ctrl+Shift+R`から自由回転できます。右下のdialogで角度を入力すると映像面にGPUプレビューを表示し、Applyで一件だけ確定します。映像上の`Alt＋左ドラッグ`でも左右の移動で回転でき、全体をviewportへFitしてプレビューします。Altを押したままマウスを離すと確定、Alt先離し／Escape／フォーカス喪失では取消です。Cancel／0度では履歴・選択を変えず、再生位置や再生／停止状態も操作しません。ピクセル比を保って正方形ピクセルへ変換し、黒い余白を付けます。切り抜き・再回転・Undo/Redo・保存へ接続済みですが、通常window／大きな素材の最終品質・性能確認は未完です。

画像はEdit menuの「Free rotate image」、または`Ctrl+Shift+R`から自由回転できます。数値とスライダーで-180～180度を0.1度単位に調整し、配置プレビューを見てApplyで確定します。Cancel／Escape／角度0では編集を増やしません。画像上で`Alt＋左ドラッグ`すると左右の移動で直接回転をプレビューできます。Altを押したままマウスを離すと確定、Altを先に離す／Escape／フォーカス喪失では取消です。1 logical pxにつき0.5度、対象は編集後の画像全体です。透明な余白と編集順序を保持し、確定後の表示用画素とPNG保存の一致を確認しました。プレビューは配置の近似で、確定時に全フレームを再サンプリングします。通常ウィンドウでの最終操作・外観確認は未完です。R／Lによる90度回転は従来どおり使えます。

Edit menuから1:1／4:3／3:4／3:2／2:3／16:9／9:16の選択範囲を作れます。`Ctrl+K`の後に`1～7`でも同じ順に選べます。編集後の画像の中央へ配置し、作成だけでは元ファイルや編集履歴を変えません。動画はtimeline表示中に使え、画面上のピクセル比を考慮して2px単位へ丸めます。時間範囲の選択は解除し、Ctrl+Yでvisual cropできます。既存の辺ドラッグとShiftによる比率保持も使えます。読書中と原寸読込・resample待機中は利用できません。

動画・音声でJ／Lは5秒戻る／進む、Kは再生／一時停止です。左右矢印・Spaceも使えます。動画のtimeline表示中はLを反時計回りの回転へ渡し、Seekは右矢印を使います。画像のR/Lは変更していません。カスタム主キーとそのprefixを追加キーより優先します。

動画の範囲選択・回転・反転・切り抜きは、timelineを表示している時だけ操作できます。T／View menuの「Toggle video editing timeline」で開閉します。timelineを閉じたり全画面にしても、編集結果と確定済みの選択は保持します。選択枠は編集中だけ表示し、途中のドラッグと辺の数値focusは解除します。音量・速度・Seek・再生停止・保存とUndo/Redoは視聴中も操作できます。動画では `,`／`.` で前／次の実フレームへ移動し、一時停止します。速度変更は `Ctrl+,`／`Ctrl+.` です。削除・伸縮した時間軸でも移動でき、連続入力は最大32操作まで順番に処理します。Seekや別の操作・タブ切替で取消します。旧設定の標準速度キーだけを移行し、変更済みのキーは保持します。動画の視聴面、または動画・音声の再生ボタンを動かさず400ms長押しすると、押している間だけ2倍速になります。離すと元の速度へ戻り、一時停止中から始めた場合は再び停止します。短い再生ボタンのクリックは従来どおりです。通常ウィンドウでの最終操作確認は未完です。

動画の下のSeekバーを上へドラッグするとtimelineが開きます。横へ動かし始めた場合は通常のSeekを続け、開閉はT／View menu／command paletteでも操作できます。音声timelineは全画面でも常時表示します。timeline上の横ドラッグで時間範囲を選択し、Deleteで選択部分を除去して前後をつなぎ、Ctrl+Yで選択部分だけを残せます。再生ヘッドのドラッグ・クリックはSeek、Ctrl+Aは時間全選択、I／Oは選択の開始／終了です。選択自体は編集ではなく、削除・切り抜きはUndo/Redo可能で元ファイルを変更しません。音量線の縦ドラッグで選択範囲（未選択なら全体）の音量を0～200%へ変更し、Alt＋選択範囲内の横ドラッグで長さを伸縮できます。Tabで音量・選択長へfocusして左右／Home／Endでも調整できます。Shift＋Spaceで選択範囲を再生し、Spaceで一時停止・再開、Escapeで通常範囲へ戻れます。選択末尾では次曲へ進まず停止し、選択外へのSeekや時間編集でも範囲再生を解除します。画像・動画・時間選択の枠は背景を反転する1物理pxの線へ揃え、選択外の暗幕と見た目のつまみを除きました。辺のドラッグ・数値操作は維持しています。通常ウィンドウと混在DPIを含む操作全体の仕上げは改善途中です。

**M7: Advanced presentation and interactionまで完了しています。** M6までの閲覧・再生・非破壊編集基盤に、非同期waveform／thumbnail cache、timeline、メディア別grid menu、window外tab detach、hardware encode優先とfallback、HDR color-space能力判定を追加しました。source fileは直接変更せず、フォルダー内の移動順は同じフォルダーを開いているExplorerの実際のSort By状態を優先し、Explorerが閉じている場合もShell viewが解決した保存状態またはfolder templateを利用します。

TSなど開始PTSが0でない素材も、表示・Seek・trim/exportはメディア開始からの時刻へ揃えます。Matroskaの長さ表示と、TSのGOP途中Seek・thumbnailで映像が出ない問題をH1で修正しました。長いGOPのSeek中も再Seek・tab close・終了要求を処理境界で確認しますが、遅いstorageや進行中のFFmpeg callによる待ち時間をなくすものではありません。

開発版では、preview cacheのフォルダー作成・保存に失敗しても、生成できたthumbnailやwaveformをそのまま利用します。保存先が使えるようになれば再試行します。メディア自体の生成・decode失敗や取消を成功扱いにするものではありません。

小さなプレビューの画素はwindow内で最大64件・16 MiBまで共用します。読み込んだ原寸画像からも240×160以内の低解像度プレビューを作り、filmstripと画像seekのサムネイルで使います。元画像の寸法を持つプレビューが残っていれば、通常画像や見開きの原寸読込中にも先に表示します。原寸の拡大率・切り抜き・回転に合わせて描きますが、プレビューを編集・保存用の画像には使いません。最近のファイル一覧も同じ低解像度cacheを使います。未訪問／未cache画像の待機と先行サムネイル生成はまだ改善途中です。

タブ名へマウスを置くと、その下にプレビューとフルパスを表示します。画像・音声はfilmstrip、動画はseekと同じ低解像度プレビューを共用します。動画の表示位置は現在位置に近い区間で、別タブでは最後に表示していた位置を使います。hoverだけではタブや再生位置・編集を変更しません。

読み込み済みの画像タブへ戻る時は、原寸画像を再読込せず、拡大率・位置・選択範囲・読書設定とページを復元します。未完了の読み込み／resize処理は復帰時に再開し、閉じたタブの状態は保持しません。開いている画像はタブを閉じるまで保持するため、メモリ使用量はタブ数に応じて増えます。

動画・音声もタブ別の再生session・時計・拡大率／選択・timeline開閉を保持します。別タブで画像を見ながら音声を再生でき、複数の動画／音声の時計も独立して進みます。終端が分かる非表示動画は映像decodeを停止し、戻る時は開いたままの入力を使って、その時点から映像だけを再開します。音声はタブ復帰で再開始しません。長さ不明の動画は時計に合わせて映像queueを処理し、実際の終端を確認します。playlistと表示中filmstripのscroll位置、timelineの高さもタブごとに保持します。復帰直後は最後に表示した映像を再表示し、現在位置の映像が届いたら置き換えます。全focus状態の保持、新しい映像への置換待機の改善は引き続き残件です。各sessionのqueueは有界ですが、開くタブ数に応じて使用resourceは増えます。

タブの右クリックから、そのタブ・他のタブ・左右・全タブを閉じる、パスをコピー、Explorerで選択表示できます。一括クローズでも未保存タブごとに確認し、Cancelで残りの処理を止めます。Ctrl+Shift+Tで閉じたタブを再表示できますが、保持するのはwindow内の直近32件のパスだけです。破棄した編集や再生状態は戻しません。同じ操作はFile menuとcommand paletteからも利用できます。

音声は曲末で次曲へ進みます。Repeat offは末尾で停止、allは一巡、oneは同じ曲を繰り返します。音声のCtrl+Rで切り替え、下バーとView menu／command paletteからも操作できます。シャッフルは現在曲から始まる重複のない再生順を作り、Ctrl+Left／Rightもその順を使います。モードはタブ別で、画像を表示している間も背景音声の自動送りを続けます。別曲への自動移動で未保存編集や進行中のexportを失わないよう、その場合は曲末で止めます。手動移動では従来の保存確認を使います。gapless再生や再起動後の再生モード保存は保証していません。

- 対応予定OS: Windows 10 22H2以降（現時点の実機確認はWindows 11。Windows 10は未検証）
- 対応予定アーキテクチャ: x86-64
- Rust: 1.98.0 / Edition 2024 / MSVC ABI
- ライセンス: MIT OR Apache-2.0
- 配布予定: インストール先を選べるWindows用Setup.exe。現時点では未提供で、下記は開発版のビルド手順
- installerの[安全性試験用Setup](docs/INSTALLER_FIXTURE.md)を実装しました。試験文書だけを配置・削除するfixtureで、本体のインストーラーではありません。
- [本体入りのlocal評価用Setup](docs/LOCAL_SETUP.md)も組み立て可能です。Windows 11の基準機で実導入・更新・起動／保存・通常アンインストールを確認しました。Windows 10やVC未導入環境などの確認と公開判断は残り、配布版はまだありません。
- 次の工程: H1 human evaluation and UX stabilization。実際の利用flowを観察し、小さな検証可能な単位でUI/UXと機能の不一致を直す

ownerの2026-09-09の指定により、現在は見た目・操作感・安定性のブラッシュアップを優先し、公開配布は行いません。Windows 10をowner環境へ導入せず、仮想環境で合理的に確認できなければ実機確認は省略し、未検証として記録します。公開作業やWindows 10実機の用意を、現在の改善作業の停止理由にはしません。

新しい機能・UI改善goalの対象と進捗は[UX_IMPLEMENTATION_PLAN](docs/UX_IMPLEMENTATION_PLAN.md)で管理します。通常画像のFitから余分な8pxの余白を除き、Shift+C／View menu／command paletteへCover表示を追加しました。Coverは縦横比を保って表示領域を覆い、画面外の部分は表示しませんが、画像の切り抜き編集や保存は行いません。Shift+WでFitへ戻せます。reading modeでは見開き全体のFitを維持します。

上部のロゴ・タブと下部の内容を上下中央へ揃え、タブ名は一定の左余白で表示します。基本バー・画像余白は黒、active tab／境界は#181818、hoverは#4C4C4Cへ統一しました。timelineを開くと下側の境界はtimeline上へ移り、statusとの間には線を残しません。

開発版の角丸・境界・右上のwindow controlsはWindows標準の描画を使い、独自のタイトルバーを追加せずtab barと並べます。空白部分のdrag／double-click、端のresizeもWindowsへ渡します。支援技術からのwindow操作と未保存確認、画像・動画の描画復旧をWindows 11基準機で確認しました。混在DPI・物理keyboardの追加確認は残り、Snap候補は基準機の設定で無効なため表示未検証です。OS設定は変更していません。

UI本文には数字を等幅にしたFigtree、一般の操作iconにはCodiconを同梱しました。日本語はWindowsのYu Gothic UIを優先し、ない場合はMeiryo等へfallbackします。OSへのfontインストールや設定変更は行いません。[fontの出典・ライセンス・再生成方法](crates/towavue-app/assets/fonts/README.md)も保存しています。

固定開発FFmpegには、LGPLという自己表示だけでは扱えないGPL推移依存が見つかりました。このbinaryは配布候補から外し、機能を保つ[再buildと検証](docs/FFMPEG_REBUILD.md)を進めます。現行の開発用fileや本体のライセンスは変更していません。

preview／保存用の補助exeは本体と同じフォルダーを優先します。ffmpeg.exe／ffprobe.exeが片方でもあれば同じ配置だけを使い、不足を別のFFmpegで補いません。両方ともない開発配置では下記のFFMPEG_DIR/binを使います。PATH上の別版への自動切替は行わず、欠落時は必要なpathをエラーに表示します。この処理の検証は、Setup.exeの完成や同梱物の配布承認ではありません。

ライセンスとソースの案内は、左上のmenu → Help → Show licenses and sources、またはcommand paletteで`licenses`を検索して辿れます。exe隣の`licenses/START-HERE.html`をExplorerで選択表示します。開発版で資料が未配置なら期待する場所を表示し、別版の資料へ自動で切り替えません。資料の同梱・最終公開はまだ完了していません。

複数の映像・音声streamを含む素材では、thumbnail・filmstrip・waveformと保存も再生と同じstreamを自動選択します。手動でstreamを切り替えるUIはありません。

音声playlistはShell順の番号付き一覧です。行全体をclickして選曲でき、現在曲を明るく表示します。長い名前は一行に省略し、hoverで全文を確認できます。多数の曲も可視行だけを描画します。各曲の長さの列はまだありません。

同じフォルダーの別音声をOpenしてtabを再利用する時は、前の曲の保存済み編集と保存先を引き継ぎません。保存物を開き直してもtrim・音量・速度を二重に適用しません。未保存編集があるtabは別tabを開いて保護します。

playlistの行へfocusした時は上下で前後、Home/Endで先頭/末尾、PageUp/PageDownで一画面分を移動できます。画面外の行は必要な分だけscrollして表示します。移動だけでは曲を変えず、Enter/Spaceで選曲し、未保存編集があれば確認します。

filmstrip・palette・grid・menuや確認画面を開いている間は、背景のplaylistを操作せず、hoverの説明も表示しません。閉じると通常の一覧操作へ戻ります。filmstrip自身の選曲は引き続き利用できます。

filmstripの上にpalette・grid・menuがある時は、背後のfilmstripも操作しません。Escapeでは上のpalette／gridを先に閉じ、filmstripの操作へ戻ります。

playlistの可視行とfilmstripの可視項目はUI Automationから選択できます。フォルダー更新で位置が変わっても操作対象は同じパスを保持し、名前とフルパス・現在項目の説明を公開します。filmstripはfocus中にも枠と名前を表示します。画面外の全項目を支援技術だけで辿る操作の検証は未完了です。

filmstripのTab/Shift+Tab移動では、切替後の現在項目へfocusも移ります。未保存確認でCancelした場合は元の項目へ戻り、項目にfocusがあってもEscape一回でfilmstripを閉じられます。

filmstripを開くと現在項目へfocusします。同じmediaのまま閉じると呼出元の操作部へ戻り、別mediaへ移った場合は現在tabへ戻ります。fullscreenでは終了操作部を復帰先にします。

曲を開く・選曲する・tabへ戻ると、playlistの現在行が画面外なら必要な分だけscrollします。同じ曲の再生中は手動scrollした位置を保持します。

Mキーの消音解除は、100%へ固定で戻さず、そのtabの直前の非zero音量を復元します。音量は既存の編集履歴に含まれ、Undo/Redoと保存にも反映されます。

動画面、または動画/音声のstatus barの音量表示上ではwheelで音量を調整できます。音声playlist上では一覧scrollを優先します。Ctrl/Shiftなどの修飾key、drag中、menuや確認画面の表示中は音量を変更しません。

動画・音声は再生終了後も左右矢印で5秒Seekでき、移動先で停止します。Spaceで再生を再開できます。

長さが取得できた素材のSeekは先頭から末尾までに制限し、動画の末尾では最後のframeを表示します。trim外の元映像を確認する操作は引き続き利用できます。

## 文書

- [ARCHITECTURE.md](docs/ARCHITECTURE.md): 技術選定、境界、データフロー、不変条件
- [ROADMAP.md](docs/ROADMAP.md): 段階的な実装順序と各ゲート
- [DEVELOPMENT.md](docs/DEVELOPMENT.md): 開発版の試用方法、手動確認matrix、変更内容ごとの編集先
- [KNOWN_GAPS.md](docs/KNOWN_GAPS.md): 現時点の制約、UI草案との差、次に検証する順序
- [DISTRIBUTION.md](docs/DISTRIBUTION.md): インストーラー配布の同梱候補・対応ソース・未完了の確認事項
- [AGENTS.md](AGENTS.md): 実装者・エージェントが常に守るルール
- [SESSION_LOG.md](SESSION_LOG.md): セッションをまたぐ事実ベースの進捗記録

`concepts/`はUI草案を含むローカル参照資料であり、Gitには含めません。設計上の決定事項は上記の追跡対象文書へ転記します。

## ビルドと実行

fileを指定せず起動するとWelcome tabのSTART欄からOpen File / Open Folderを選べます。現在のshortcutも表示し、幅が狭い場合はhoverで確認できます。Explorerからのdropでも開けます。最後のmedia tabを閉じるとWelcomeへ戻り、唯一のWelcomeを閉じても空のtab状態にはなりません。RECENTには直近40件のfileを新しい順で表示し、画像・動画のthumbnailと音声waveformから開けます。履歴は`%APPDATA%\towavue\recent-files.txt`のpath参照のみで、未保存編集のbackupやsession復元ではありません。壊れた履歴は上書きせず警告します。

Visual StudioのDesktop development with C++ workload、Windows SDK、LLVMを導入します。Developer PowerShellで、checksum固定済みのFFmpeg 9.0.1開発ファイルを準備してから実行します。ダウンロード先と生成fixtureはGit対象外です。

```powershell
$ffmpegDir = .\scripts\setup-ffmpeg.ps1
$env:FFMPEG_DIR = $ffmpegDir
$env:LIBCLANG_PATH = Join-Path $env:ProgramFiles 'LLVM\bin'
$env:PATH = "$(Join-Path $ffmpegDir 'bin');$env:PATH"

.\scripts\generate-m1-fixtures.ps1
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run -p towavue-app -- path\to\media.mp4
```

標準shortcutは画像でLeft／PageUp／Backspace／Aが前、Right／PageDown／Space／Dが次です。動画・音声ではSpaceでpause/resume（再生終了後は先頭から再開）、左右矢印で5秒Seekを維持します。Home/Endはfolderの最初/最後の画像、Ctrl+左右は同種media一枚、Alt+左右は全種media、Fはfilmstrip、Ctrl+Shift+Pはcommand paletteです。画像はShell順で移動し、reading modeの通常移動キーは重複しない見開き単位です。

画像の`Ctrl+1～0`は1～10枚先（0＝10）、Shift併用は手前へジャンプします。`Ctrl+Space`／`Ctrl+Backspace`は5枚先／手前です。画像ファイルだけを数え、読書中も見開き数ではなく枚数で数えます。通常の前後移動は循環しますが、数字／5枚ジャンプは端で止まります。Home/Endと同様、同じ端点なら再読み込みせず、移動先が変わる場合は未保存編集を確認します。各枚数の操作はImage jump menuとpaletteでも選べます。

設定は初回起動時に`%APPDATA%\towavue\shortcuts.conf`へ生成され、`Ctrl+K Ctrl+S`のようなprefixも指定できます。画像用キーは`previous_image`／`next_image`／`first_image`／`last_image`と`jump_images_forward_1`～`10`／`jump_images_backward_1`～`10`で変更できます。v4以前の未変更の画像・読書キーには追加キーを補いますが、変更済み設定は保持し、ファイルを自動書換えしません。既存のcustomキー／prefixと競合する暗黙のジャンプキーは追加しません。Ctrl+Shift+数字は上段数字キーを認識し、明示した記号キーの設定があればそちらを優先します。menuまたはReload shortcutで再読込します。

Reading mode（B）は初期状態で1–2、3–4…と連結表示します。Ctrl+[／Ctrl+]で表示枚数（2～10）、Ctrl+Shift+左右で先頭ページの枚数（1～表示枚数）を調整できます。先頭1枚なら1、2–3、4–5…となり、途中の画像を開くとその画像を含む見開きを表示します。R／Lで縦横配置を切替え、H／Vで並び順を反転します（画像の編集ではありません）。設定変更で編集中の画像は切り替わりません。statusに設定枚数を表示し、seek hoverも同じ見開きをpreviewします。読書ボタンの上下dragでも表示枚数、左右dragで先頭枚数を調整できます。drag中はカーソルを押下位置へ固定し、離すと確定、Escapeやfocus喪失で開始前の設定・modeへ戻ります。clickは従来どおりmode切替です。

未保存編集のある画像では読書モードを開始できません。保存またはUndoで未保存状態を解消すると開始できます。読書中は回転・反転・crop・Undo/Redoを無効にし、有効中の本アイコンは同じ輪郭の塗りつぶし表示になります。別の未保存画像tabへ戻る場合は、編集を残したまま通常表示へ戻します。

画像ではCtrl+C／Edit menuのCopy image or selectionで、編集後の原寸画像または選択範囲をクリップボードへコピーできます。回転・反転・cropと透過色を保持し、表示倍率や低解像度previewはコピーに使いません。アニメ画像は押した時点のframe、読書中は現在の画像fileだけを対象にします。文字入力中は通常の文字コピーを優先します。元file・編集・選択を変更せず、完了または失敗をstatusに表示します。

Ctrl+R／Edit menuのResize / resample imageで幅・高さ、縦横比固定、Nearest／Bilinear／Bicubic／Lanczosを指定できます。Applyは一つの非破壊編集として追加し、Cancel／Escapeは履歴を変えません。原画像を保持して非同期処理し、Undo／Redoと保存・コピーへ反映します。処理中は待機表示となり、失敗時はUndoで復帰できます。各辺1～16384 pixel、処理結果はanimation全frame合計512 MiB以内（graphics deviceのtexture上限も適用）です。読書中の編集は無効です。

ドット絵の拡大時はView menuのToggle image interpolation、またはcommand paletteで`nearest`を検索して、Smooth／Nearest表示を切り替えられます。これは画面表示だけの設定で、編集・保存・コピーの画素は変えません。読書表示とanimationにも適用し、現在の方式はstatusに表示します。window内のファイル移動では保持しますが、再起動時はSmoothです。初期shortcutは割り当てず、`toggle_image_interpolation`に好みのキーを設定できます。読み込み中の低解像度previewやタブ等の小さなpreviewは引き続きSmoothです。

prefixの続きは1秒以内に入力します。Escape、click、別commandやwindowへのfocus移動で待ちを解除し、途中から戻ったkeyを前のprefixへつなげません。

複数キーを指定する新形式では、設定fileに `# towavue shortcuts v2` の行を追加し、例えば `seek_forward = Right | L`、`toggle_pause = Space | K` と書きます。最初が主キーで、以降は追加キーです。`seek_forward = Right` だけにすれば追加キーを外せます。prefixも `Right | Ctrl+K L` のように指定でき、縦棒のキー自体は `Pipe` と書きます。旧形式では標準のLeft／Right／Spaceを変更していない3コマンドにだけJ/K/Lを補い、変更済みのキーは保持します。既存fileを自動書換えせず、旧形式の縦棒キーもそのまま読み込みます。

開発版では、起動時にshortcut／grid設定を読めない場合、その設定だけ既定値で継続してpathと理由を警告します。既存fileは書き換えません。修正後はFile → Reload keyboard shortcutsで再読み込みでき、再読み込みの失敗時は現在の設定を保持します。隔離した記述ミスの設定で、通常releaseの警告全文・解除後の操作・修正後Reloadを実画面で確認しました。全エラー種別・screen reader・混在DPIの確認ではありません。

Ctrl+Tab／Ctrl+Shift+Tabでtabを前後に切り替えます。filmstripを開いている場合も、通常のTab／Shift+Tabによる項目移動と区別します。menu・palette・確認画面の入力を優先し、custom設定へ変更した後に元の既定キーを固定aliasとして残しません。

設定ファイルでは`+` keyを`Plus`（例: `Ctrl+Plus`）と書けます。旧版が生成した`+`や`Ctrl++`も読み込めるため、既存設定の書き直しは不要です。

保存確認のEscapeは編集を保持してCancelします。Tab／Shift+Tabでbuttonを選び、Enter／Spaceで実行できます。export失敗ではエラー通知だけを閉じ、保留中の保存確認は残ります。背景クリックで保存・破棄・確認解除は行いません。

現在tabの未保存確認をCancelすると、確認前の操作部へfocusも戻します。選択辺の矢印調整を続けられ、保存先選択を取り消してから確認をCancelした場合も同じです。別のmediaへ移った場合は古いfocusを戻しません。

保存確認やエラー通知中は、UI Automationからの背景操作も拒否します。paletteなどのoverlayは状態を保って一時的に隠し、Cancel後に通常操作へ戻ります。

Cancel exportは保留中の自動終了・移動を止めます。保存の確定前なら既存出力と未保存編集を保持します。取消より先に保存が完了した場合は、その保存済み出力を残します。

graphicsの再作成に失敗した場合はWindows標準のRetry/Cancelを表示します。Cancelは編集を保持し、Alt+F4で終了を要求すると、描画なしでもYes（現在fileをExport）／No（終了時は全未保存編集を破棄）／Cancel（保持）の確認から保存できます。保存失敗もnative通知で案内し、編集は残します。export中はwindow titleへ進捗を表示します。

H1ではtitle/tab barを黒基調の単一barへまとめ、左端logoにmenu、右端にwindow操作、下部に再生操作と省略path・状態情報を配置しました。上部の空白をdragして移動、double-clickで最大化・復元、window端をdragしてresizeできます。長い名前はhoverで全文を確認できます。

tabはbar内でdragして並べ替えられます。挿入線の位置で離すと順序だけを変更し、表示中のmedia・未保存編集は保持します。Escapeまたはbar外・window内へのdropで取り消します。window外へのdropは既存の別windowへ移す操作になり、未保存時は確認します。

tabが横幅に収まらない場合、開く・切り替える・並べ替える・window幅を変えると、現在tabが見える位置まで必要な分だけscrollします。通常の再描画では手動scroll位置を保持します。

日本語の名前はWindowsにある日本語fontを補助fontとして表示します。fontの同梱・downloadは行いません。日本語fontがない環境では欠字が残り、terminalへ診断を出します。paletteはIME変換・確定とcommand実行のEnterを分離します。毎frameのfocus再要求による変換取消を修正し、Windows日本語IMEの候補表示・上下選択・確定・Escape取消を実windowで確認しました。物理keyboard・他IME・混在DPIを含む入力matrixは未完了です。

logo menuはFile / Edit / Viewに分かれています。FileにOpen・Export・tab close・shortcut再読込、EditにUndo・crop・回転・trim・音量/速度、Viewに再生・移動・zoom・reading・各overlayをまとめています。現在のcustom shortcutを右側に表示し、使えない項目は無効表示、縦に収まらない場合はscrollできます。

logoへTabでfocusしてEnter／Spaceでmenuを開けます。menu内は上下またはTab／Shift+Tabで有効項目を移動し、右でsubmenu、左で親へ戻ります。Enter／Spaceで選択、Escapeでmenu全体を閉じます。再open時は先頭項目へ戻り、keyboardで選んだ項目はscroll内に表示します。

Escapeでメニューを取り消すとlogoへfocusが戻り、Enter／Spaceですぐ開き直せます。メニューからコマンドを選んだ場合もlogoを戻り先として引き継ぎ、コマンド自身のfocus先があればそちらを優先します。背景クリックでは復帰を行いません。

command paletteはtitle bar直下の暗いpanelへまとめ、全幅の検索欄と右揃えのshortcutを表示します。小さいwindowや長いprefixは省略・全文tooltipとscrollで扱い、検索・上下選択・Enter実行・Escape取消は従来どおりです。

パレットをEscapeで取り消すと、開く直前の操作部へfocusを戻します。画像・動画の選択辺を調整していた場合も、その辺の矢印操作を続けられます。コマンドを実行した場合は、その操作のfocus先を優先します。

gridもボタンfocus中にEscape一回で閉じ、開く前の操作部へ戻ります。gridとpaletteを切り替えても元の復帰先を保持します。gridの上に開いたlogoメニューは、最初のEscapeでメニューだけを閉じます。

メニューから開いたパレットの取消で、消えたメニュー項目へfocusを戻して終了する問題を修正しました。UI Automationへ渡すfocus先も、その画面に存在することを確認します。

logoやtabなどのボタンにfocusがある時も、R・Ctrl+Zなど現在のショートカットを使えます。Spaceや矢印・TabなどのUI操作、検索欄と開いたメニュー・確認画面の入力は優先します。

検索欄のCtrl+C／Ctrl+X／Ctrl+VはWindowsのテキストクリップボードを使い、外部アプリと文字列をやり取りできます。画像や選択範囲をクリップボードへコピーする機能ではありません。

Windows UI AutomationへUI情報と操作を接続し、Welcome・メニュー・command paletteの操作に加え、再生位置・trim端点・フォルダー内の画像位置・選択範囲の四辺を値として変更できます。スクリーンリーダーでの全画面操作や、全体の読み順・focusの横断確認は未完了です。

画像・動画はCtrl+AまたはEditのSelect whole mediaで全体を選択できます。Tabで四辺のfocusを移し、矢印で画像1 pixel・動画2 pixelずつ調整します。Home/Endで軸の端へ指定でき、逆転・零長は拒否します。選択だけでは未保存編集にならず、Ctrl+Yでcropを確定しCtrl+Zで戻せます。reading表示は対象外で、検索欄のCtrl+Aは文字の全選択に使います。画像端の既存ハンドルも、操作surface内なら画像外側の部分から掴めます。

拡大した画像では、TabやUI Automationで選んだハンドルが見える位置まで必要な分だけpanします。倍率・選択範囲は変えず、同じ辺への明示的な再Focusや値変更でも追従します。通常の再描画では手動panを引き戻しません。

タブの閉じるボタンはUI Automationへ対象ファイル名とフルパスの説明を公開します。タブの操作対象は並べ替えや隣のタブを閉じても変わらず、未保存編集には通常の確認画面が開きます。

F11またはViewのToggle fullscreenで、現在monitorのborderless fullscreenへ切り替えられます。通常のbar・timelineは隠れ、画像・動画・readingを広く表示します。pointerを下端へ移すと、映像のサイズを変えずにstatus／seek barとfullscreen解除buttonが現れます。操作部から離れると隠れますが、Seekのdrag中はreleaseまで保持します。Escapeは開いたoverlayを先に閉じ、次に通常windowへ戻ります。paletteやfilmstrip、保存確認はfullscreenでも利用でき、Tまたはstatus barのtimeline buttonで通常windowへ戻ってtimelineを表示します。復帰時は元の位置・size・最大化状態を保ちます。Enterは割り当てていません。

全画面では、他の操作に使われていないTab／Shift+Tabでも操作部を表示し、最初はfullscreen解除buttonへfocusします。その後はTabで移動し、解除buttonのEnterで通常windowへ戻れます。キーボード操作中はpointerを動かしても表示を維持し、映像面のclick・windowのfocus喪失・確認画面などで解除します。Enter自体をfullscreen切替shortcutへ割り当てる変更ではありません。

fullscreenの画像・動画・readingでは、操作が2秒ないとcursorも隠れます。pointer移動・click・wheel・key入力で戻り、button保持中や操作overlay・保存確認・読み込み中は表示を維持します。音声playlistとWelcomeでは隠しません。

timelineを閉じているときはstatus上端の細いbarで動画・音声の位置を変更できます。画像では同じfolderの画像順へ移動します。dragは離した時に一回だけ確定し、再生終了後のSeekは一時停止状態になります。command paletteは検索後に上下keyで候補を選び、Enterで実行、Escapeで閉じられます。

timelineと細いbarのSeek／folder移動は左buttonのclick・dragで行います。右・中・追加buttonのdragでは位置を変更しません。

シーク部にfocusがある時は、左右キーで5秒（画像では1枚）ずつ、Home／Endで先頭／末尾へ移動します。Tabで別の部品へ移り、回転・再生・Undoなど他の操作は現在のshortcut設定を使います。画像の未保存確認と、動画・音声のsource時刻基準は変わりません。

Seekの確定にはbuttonを離した位置を使い、その後のcursor移動を混ぜません。Escape・focus喪失・別commandで取消し、押し直すまで再開しません。fullscreen中の最初のEscapeもSeekの取消だけを行います。

動画のhover thumbnailを取得できない区間では「Thumbnail unavailable」と表示し、同じfileを開いている間はその区間を繰り返し取得しません。再openで再試行できます。thumbnailの失敗だけで再生・Seek・保存を無効にはしません。

duration・波形・hover thumbnailの生成は、それぞれ実行中1件と最新の待機1件に制限します。file切替・closeでは不要なFFmpeg/FFprobeを停止し、filmstripも表示範囲の変更・closeで旧処理を取り消します。ただし、実行中のfilesystem I/Oやnative probeによる待ち時間をなくす保証ではありません。

波形は音声を逐次読み取り、画面幅に応じた一定量の集計データから描きます。2時間音声の通常release試験では、生成用processのピーク使用量が約1.36 GBから約42 MBへ下がり、本体との合計も約191 MBでした。素材・codecを問わない総メモリ上限ではありません。

動画はbar・timelineを除いた領域へ縦横比を保って表示し、非正方形pixelのsample aspect ratioも反映します。hardware/softwareとも同じ表示矩形を使い、crop selectionも映像に合わせます。

動画の回転metadataも、90度単位の回転・反転として自動適用します。その向きを基準にcropや手動回転を行い、保存後も同じ向きになります。任意角度や変形を含む非対応のdisplay matrixは無視せず、理由を画面へ表示します。再生失敗の理由は、一時通知が消えた後も別mediaを開くまで残ります。

動画のR/Lによる90度回転、H/Vによる反転、selectionとCtrl+Yによるcropも、現在の再生画面へ操作順に反映します。Undo/Redoとtab復帰でも編集結果を表示し、回転後の縦横比を保ちます。hardware decodeの編集表示も同じGPU内で処理し、CPUへ映像を戻しません。

cropは確定したpixel矩形をpreviewと保存で共有します。画像は1 pixel、動画は偶数pixel単位へ選択を合わせ、確定時に出力寸法を表示します。動画は既定encoderの制約で16×16未満を確定せず、選択を残して案内します。画像の1×1 cropも保存でき、寸法が変わらない全領域cropでは未保存編集を増やしません。

Gはメディア種別ごとの4×4 grid menuを開き、`1234/qwer/asdf/zxcv`またはclickで選択します。配置は`%APPDATA%\towavue\grid.conf`で変更できます。動画・音声ではTでwaveform timelineを表示し、動画上のhover thumbnailとclick/drag Seekを利用できます。previewはUI thread外で生成され、path・size・更新時刻をkeyにした最大64 MiBのcacheを`%LOCALAPPDATA%\towavue\preview-cache`へ保存します。

timelineは上端をdragして高さを変えられます。高さの変更ではSeekや編集は行いません。windowを縮めると映像領域を残す高さへ制限し、狭い幅ではtrim表示の説明部分を省いて端点時刻を優先します。

trimの開始は上側、終了は下側のgripを横dragして調整できます。drag中は候補を表示するだけで、離した時に一回だけ編集し、Undoで戻せます。Escapeやfocus喪失で取り消します。逆転・零長の候補は赤く示し、離しても元の範囲を保持します。I/Oでの指定も引き続き使えます。

trim端点はUI Automationへ開始・終了の秒数として公開します。focus中は左右で1秒ずつ、Home／Endでsourceの先頭／末尾を指定できます。逆転・零長は拒否し、変更は既存のUndo／Redoで戻せます。確認画面中は操作できません。frame単位へのsnapではありません。

trim端点もbuttonを離した位置で確定します。同frameの後続cursor移動は端点へ混ぜず、focus離脱・復帰やEscapeがreleaseと同じframeに届いても編集を取り消します。

短いdragでも押したgripを保持し、背景のSeekへ切り替わらないようにしました。gripの単なるclickは編集せず、Seekは押下からreleaseまでが同frameに届く場合も扱います。

gridは小さいwindowでも4列を保ち、長い名前は省略・hoverで全文表示します。clickとkeyはどちらも一回実行して閉じます。

gridのkeyはQWERTYの`1234/qwer/asdf/zxcv`に相当する物理位置です。Shiftで位置は変わらず、Ctrl/Alt/Windows key付きは通常shortcutとして扱います。paletteを開くとgridは閉じます。

画像ではCtrl+wheelまたは+/-でzoom、Ctrl+Hでactual size、Shift+Wでfit、右dragでpanします。左dragでselectionを作り、辺dragでresize、Shift付き作成で正方形、Shift付きresizeで比率を保持します。選択範囲clickまたはCtrl+Shift+Yはcrop previewです。Bでreading mode、Rで縦横切替、Hで表示順反転、Ctrl+[ / Ctrl+]で表示数を2～10枚に変更できます。

Shift付きの選択は画像端で片側だけが潰れないよう、比率を保って拡大を止めます。辺のresizeでは反対側と直交方向の中心を保ち、drag開始時の比率を使います。確定時の整数pixel（動画は偶数pixel）への丸めは従来どおりです。

reading modeは横並びなら高さ、縦並びなら幅を揃え、ページ間の隙間なしで全体を中央へ収めます。各画像の縦横比は保ち、シークバーの見開きpreviewも同じ並べ方を使います。読めないページは場所を残してエラーを表示します。左右キーは重複しない見開き単位、Ctrl+左右は一枚ずつ移動します。

選択範囲の始点と辺resizeの判定はbuttonを押した位置を使い、release位置まで反映してからpixelへ丸めます。移動eventが少ないdragで始点がずれたり選択が消えたりする処理を修正しました。

選択・panは押下からreleaseまでが同じ描画frameに届いても処理し、release後のhover移動を終点に混ぜません。固定event列の回帰testで確認しており、物理入力・混在DPIの横断確認は継続中です。

Windowsでreleaseが最後のcursor移動通知より先に届く場合も、button messageの座標を先に反映します。移動直後のreleaseが古い位置でclick扱いとなる例を修正し、通常releaseで高速の辺resizeと通常clickを確認しました。

選択drag・panの途中でEscapeを押すと、開始前の範囲・位置へ戻ります。focus喪失、保存確認やoverlay、別commandでも進行中の操作を取り消し、buttonを押し直すまで再開しません。fullscreen中も最初のEscapeはdragの取消だけを行います。編集履歴やsource fileは変更しません。

押下直後、まだ描画されていない時点のEscapeも取消対象です。保留押下を次の描画へ持ち越さず、既存selectionやfullscreenは保持します。

画像の100%は画面の実pixel基準です。zoomは現在の表示領域とcrop・回転後の寸法を使うため、小さいwindowやcrop previewからの一段の拡大も現在の見た目を基準にします。

Ctrl+wheelはcursor位置を基点に拡大・縮小します。wheelがzoom倍率へ変換された後の入力を使うよう修正し、実windowでの動作を確認しました。palette・grid・保存確認中やmenu上のwheelでは背景画像を拡大しません。

大きい画像もFitでは2%未満まで縮小して全体を収めます。reading modeにも同じ計算を使い、手動zoomの10%未満は小数2桁で表示します。

通常画像表示ではCtrl+Yでcropを履歴へ追加し、R/Lで90度回転、H/Vで反転します。動画・音声ではI/Oでtrimの開始・終了、上下矢印でvolume、Mでmute、`,` / `.` / `/`でrateを変更・resetします。Ctrl+Z / Ctrl+Shift+Zはundo/redo、Ctrl+Shift+SはSave As、Ctrl+Sは直近export先への再Saveです。dirtyなmediaの移動・close・終了時はExport / Discard / Cancelを選択できます。同一source pathへのexportは拒否されます。

画像と動画はfileごとのtab、音声は同じfolderのplaylist tabとして開きます。filmstripはShell snapshotの全対応mediaをExplorer順で表示し、middle clickで明示的に新規tabを作れます。H1では中央のthumbnail列へ変更し、画像・動画preview、音声waveformとdurationを表示します。現在項目を中央へ寄せ、wheelで横scroll、Tab / Shift+Tabで移動できます。previewは可視項目だけを単一workerで読み込みます。フォルダー変更は`ReadDirectoryChangesW`で検知してdebounce後にsnapshotを更新します。M1のcodec fixtureはMP4/H.264/AAC、MKV/HEVC/AAC、WebM/VP9/Opusです。再生終了時のdiagnosticにはadapter LUID、hardware frame数、CPU transfer数、表示・drop frame数、Seek latency、A/V driftを記録します。

Shellのフォルダー情報はH1で非同期取得に変更しました。取得中はstatusにOpening folder / Loading orderを表示し、切替後の古い結果は適用しません。native Open file/folder・Save Asも専用threadで表示し、選択中の描画・再生を継続します。本体への入力は従来どおりmodal制限され、選択画面を閉じると復帰します。

Explorerから画像・動画・音声file、またはfolderをwindowへdropして開けます。複数fileにも対応し、画像・動画は新規tab、音声は同folderのplaylistへ入ります。folderはShell順の先頭mediaを開く方式です。未保存編集は元tabに保持し、確認dialogの最中はdropを受け付けません。sourceの移動やcopyは行いません。

tabをwindow外へdragしてdropすると同じmediaを別processのwindowへ移し、dirtyなtabには既存のExport / Discard / Cancel guardを適用します。動画exportはCtrl+Shift+EでMedia Foundation hardware encode優先を切り替えられ、利用不能ならsoftwareへfallbackし、実際の経路をstatusへ表示します。PQ/HLG sourceはD3D11 Video Processorの色空間変換能力を確認してからSDRへtone mapし、adapterが変換を保証しない場合は不正な色で表示せず明示的なerrorにします。

H1ではSaveをbackground化しました。書き出し中も再生・tab切替・追加編集ができ、進捗windowからcancelできます。成功時だけ出力先を置換し、失敗・cancelでは既存fileと編集を保持します。書き出し開始後に追加した編集は未保存のまま残ります。

volume・mute・rateは現在の再生にも反映します。rateはピッチを維持した0.25～4倍で、変更時は現在位置から再開します。undo/redo・Seek・tab復帰でも編集値を使い、他アプリやWindowsのmaster volumeは変更しません。timelineとSeekの時刻は元メディア基準です。

画像とreading pageのdecodeもbackground化し、切替後の古い結果は表示しません。保持するRGBA frame列は1要求合計512 MiBまでです（decoderの作業領域やGPUを含むprocess全体の上限ではありません）。上限超過・破損画像・GPUの寸法上限は画面へerrorを表示します。

静止画は最大8枚・256 MiBのdecode cacheをwindow内で共有し、戻る操作での再decodeを省きます。表示完了後は移動方向の隣画像（readingでは隣見開きの先頭）も別workerで一件先読みします。fileのサイズ・更新時刻が変われば読み直し、別の移動要求が来れば古い先読みを取り消します。GIF／AVIF／animationは先読みせず、容量を超える画像もcacheしません。画質は変えません。cacheや先読みには表示中以外のメモリ・CPUも使い、cacheの256 MiBはprocess全体の上限ではありません。見開きは全画像を待たず、読み終えたページから表示します。未読込位置にはLoadingを表示し、寸法が分かった時点で配置を調整します。初回起動や速い連打の待ち時間をなくしたものではありません。

同じdecode結果にはGPU textureも再利用し、再訪時の画素変換とuploadを省きます。texture側も最大8枚・RGBA相当256 MiBで保持し、graphics復旧時には破棄します。両cacheは画素を共有しますがGPU resourceは別に使うため、process全体を256 MiBに制限するものではありません。

静止画・一時停止・静止したmenuでは不要な連続描画を止め、入力・読み込み完了・animationの期限に応じて更新します。音声再生の位置表示も期限付きで更新し、音声出力のpollから無条件に再描画しません。

再生の映像・音声は独立した入力と上限付きqueueで供給します。音声の出力待ちが映像の初期表示や停止中Seekを塞がず、短い映像と長い音声の組合せでも映像を表示できます。音声producerはpipelineごとに一つだけ開始し、映像のhardware fallbackや通常のタブ復帰では再開始しません。device復旧時は全タブのsessionを停止し、同じ新deviceへ接続し直します。

I/Oでtrim端点を指定するとtimelineが開き、再生・保存するsource範囲をミリ秒付きで表示します。未指定の開始/終了は先頭/末尾です。逆転・零長・範囲外はその場で拒否し、同じ端点の再指定は履歴を増やしません。Playは範囲内を再生して終端で停止し、再Playで範囲の開始へ戻ります。範囲外へSeekすると一時停止のsource previewになり、I/Oで端点を選び直せます。その状態のPlayも範囲の開始へ戻ります。Undo/Redo・rate変更・tab復帰でも現在のtrimを使います。

trim保存は開始以上・終了未満のframe／sampleを選びます。短すぎて動画frameや音声sampleが残らない場合は保存を失敗にし、既存の出力先を保護します。動画の最後のframe長や圧縮音声のpaddingにより、出力containerのdurationは指定区間と同一とは限りません。低精度PTSの途中Seek後に残る音声sample位相差は監査中です。

## ライセンス

本リポジトリのコードは、利用者の選択により[MIT License](LICENSE-MIT)または[Apache License 2.0](LICENSE-APACHE)の下で利用できます。将来同梱するFFmpeg DLLとその他の第三者コンポーネントには、それぞれのライセンスが適用されます。
