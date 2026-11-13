use towavue_core::CommandId;

// Search metadata, not query-specific dispatch. Keep phrases separate so a whole
// phrase outranks a bag of words. An exhaustive match makes new commands require
// a deliberate vocabulary decision; numeric siblings share their action words.
pub(super) fn phrases(command: CommandId) -> &'static str {
    use CommandId::*;
    match command {
        OpenFile => "load|import|読み込み|ファイルを開く",
        OpenFolder => "directory|browse folder|フォルダーを開く|ディレクトリ",
        OpenGallery => "recent files|library|history grid|履歴一覧|ギャラリー",
        ToggleReuseFolderTab => "reuse tab|same folder tab|タブ再利用",
        ReuseFolderTabOff => "separate tabs|new tabs|タブ再利用しない",
        ReuseFolderTabAll => "reuse any media|同じフォルダーで再利用",
        ReuseFolderTabSameKind => "reuse same type|同じ種類で再利用",
        ToggleTabIcons => "tab file type icons|タブアイコン|種類表示",
        GoToFile => "open file by name|jump to file|番号で移動|ファイル名で移動",
        OpenRecentFolder => "folder history|recent directories|最近のフォルダー",
        FocusSearch => "find in gallery|search history|検索欄|履歴検索",
        SearchCurrentFiles => "find files|filter files|ファイル検索|絞り込み",
        SearchCurrentFolders => "find folders|filter directories|フォルダー検索",
        ShowLicenses => "copyright|third party notices|ライセンス|著作権",
        CheckForUpdates => "upgrade|latest version|更新確認|アップデート",
        About => "version|app information|バージョン|アプリ情報",
        CloseTab => "dismiss tab|タブを閉じる",
        NextTab => "switch tab forward|次のタブ",
        PreviousTab => "switch tab backward|前のタブ",
        CloseOtherTabs => "keep current tab|他のタブを閉じる",
        CloseTabsLeft => "close preceding tabs|左のタブを閉じる",
        CloseTabsRight => "close following tabs|右のタブを閉じる",
        CloseAllTabs => "clear tabs|すべてのタブを閉じる",
        ReopenClosedTab => "restore tab|recover tab|閉じたタブを戻す",
        TogglePause => "play|pause|resume|stop|再生|停止|一時停止",
        SeekBackward => "rewind|skip back|skip backward|jump back|巻き戻し|戻る",
        SeekForward => "skip|skip ahead|skip forward|jump forward|fast forward|早送り|スキップ",
        SeekVideo0 => "restart|beginning|jump to start|最初に戻る|先頭",
        SeekVideo10 | SeekVideo20 | SeekVideo30 | SeekVideo40 | SeekVideo50 | SeekVideo60
        | SeekVideo70 | SeekVideo80 | SeekVideo90 => {
            "jump to percent|percentage position|割合で移動"
        }
        PreviousFolder => "previous directory|前のフォルダー",
        NextFolder => "next directory|次のフォルダー",
        ParentFolder => "up folder|parent directory|一つ上|親フォルダー",
        FirstChildFolder => "enter folder|child directory|子フォルダー",
        HistoryBack => "back history|previous location|履歴を戻る",
        HistoryForward => "forward history|next location|履歴を進む",
        PreviousMedia => "previous file|previous item|前のファイル",
        NextMedia => "next file|next item|skip file|次のファイル",
        PreviousSameKind => "previous same type|前の同じ種類",
        NextSameKind => "next same type|次の同じ種類",
        ToggleFilmstrip => "thumbnails|thumbnail strip|folder overview|サムネイル|一覧",
        ToggleCommandPalette => "commands|actions|コマンド検索|操作検索",
        OpenKeyboardSettings => {
            "keybindings|hotkeys|shortcut settings|キー割り当て|ショートカット設定"
        }
        ReloadShortcuts => "refresh keybindings|reload hotkeys|キー設定再読込",
        ZoomIn => "magnify|enlarge view|拡大|ズームイン",
        ZoomOut => "reduce view|shrink view|縮小|ズームアウト",
        ActualSize => "original size|100 percent|1:1|等倍|原寸",
        FitToWindow => "contain|fit whole image|全体表示|ウィンドウに合わせる",
        CoverWindow => "fill window|cover viewport|画面いっぱい",
        ResizeViewFit => "window resize behavior|resizing zoom|ウィンドウ変更時の表示",
        ResizeViewKeepArea => "retain visible region|表示範囲を維持",
        ResizeViewKeepScale => "retain zoom|倍率を維持",
        ClearSelection => "deselect|unselect|選択解除",
        SelectAll => "select entire|全選択|全体を選択",
        ZoomSelection => "magnify selection|選択範囲を拡大",
        ToggleReadingMode => "comic|manga|book|reader|漫画|読書|見開き",
        IncreaseReadingPages => "more pages|increase spread|表示ページを増やす",
        DecreaseReadingPages => "fewer pages|decrease spread|表示ページを減らす",
        ToggleReadingAxis => "vertical reading|horizontal reading|scroll direction|縦読み|横読み",
        ReverseReadingOrder => {
            "reading direction|right to left|left to right|右開き|左開き|読む順序"
        }
        ReloadFolderOrder => "refresh sort|rescan folder|並び順を再読込",
        ReadingLeft => "read left|左へ読む",
        ReadingRight => "read right|右へ読む",
        IncreaseReadingFirstPage => "offset spread forward|見開き開始を進める",
        DecreaseReadingFirstPage => "offset spread backward|見開き開始を戻す",
        Undo => "revert edit|undo change|取り消し|元に戻す",
        Redo => "repeat undone edit|やり直し",
        ApplyCrop => "cut image|trim picture|切り抜き|トリミング",
        DeleteTimeSelection => {
            "remove selected time|cut selection|erase segment|時間範囲削除|区間削除"
        }
        KeepTimeSelection => {
            "crop selection|trim selection|keep only selected time|切り抜き|トリミング|選択範囲だけ残す"
        }
        MuteAudioTrack => {
            "mute track|exclude track|remove audio|output track|トラックミュート|音声除外|音を消す"
        }
        PlayTimeSelection => "preview selection|play range|区間再生|選択範囲を再生",
        RotateClockwise => "turn right|rotate right|右回転|時計回り",
        RotateCounterclockwise => "turn left|rotate left|左回転|反時計回り",
        RotateFineClockwise => "fine rotate right|右微回転",
        RotateFineCounterclockwise => "fine rotate left|左微回転",
        FlipHorizontal => "mirror|mirror horizontal|reflect horizontal|左右反転|鏡像",
        FlipVertical => "mirror|mirror vertical|reflect vertical|上下反転|鏡像",
        SetTrimStart => "mark in|selection start|in point|範囲開始|始点",
        SetTrimEnd => "mark out|selection end|out point|範囲終了|終点",
        VolumeDown => "quieter|softer|lower volume|音量を下げる|小さく",
        VolumeUp => "louder|raise volume|音量を上げる|大きく",
        ToggleMute => "unmute|sound off|sound on|消音|ミュート解除",
        CycleVolumeStep => "volume increment|volume sensitivity|音量刻み|音量ステップ",
        VolumeStepTwo | VolumeStepFive | VolumeStepTen => "volume increment percent|音量刻み",
        AudioRepeatOff => "no repeat|stop looping audio|リピートなし",
        AudioRepeatAll => "loop playlist|repeat playlist|全曲リピート",
        AudioRepeatOne => "loop song|repeat track|一曲リピート",
        FolderNavigationStop => "folder end stop|no wrap navigation|フォルダー末尾で停止",
        FolderNavigationLoop => "wrap navigation|loop folder|フォルダー内を循環",
        RateDown => "speed|slow|slower|slow down|playback speed|減速|ゆっくり|再生速度",
        RateUp => "speed|fast|faster|speed up|playback speed|加速|倍速|速く|再生速度",
        ResetRate => "normal speed|reset speed|1x|等速|標準速度",
        EditSpeed => {
            "speed|fast|slow|change speed|duration|stretch time|再生速度|速度変更|長さ変更|倍速"
        }
        DeleteFile => "remove|erase|trash|削除|消去|ゴミ箱",
        RenameFile => "change name|filename|名前変更|リネーム",
        MoveFile => "relocate|move folder|ファイル移動|移動先",
        Save => "export|write|store|保存|書き出し",
        ExportAs => "export|save|save as|write|store|convert|保存|書き出し|変換",
        ExportAudio => "save audio|extract audio|rip audio|convert audio|音声抽出|音声保存",
        ExportFrame => {
            "save frame|snapshot|screenshot|capture frame|still image|フレーム保存|静止画|スクリーンショット"
        }
        AudioExportOptions => {
            "audio encoding|audio codec|bitrate|sample rate|channels|音声形式|ビットレート|サンプルレート"
        }
        MetadataExportOptions => "exif|tags|metadata privacy|位置情報|メタデータ|タグ",
        ToggleTimeline => "waveform|time editor|波形|タイムライン|時間編集",
        ToggleHardwareEncode => "gpu encoding|hardware acceleration|ハードウェアエンコード|gpu",
        ExportQualityHigh | ExportQualityBalanced | ExportQualitySmaller => {
            "compression|file size|image quality|video quality|圧縮|画質|ファイルサイズ"
        }
        ToggleFullscreen => "full screen|maximize view|全画面|フルスクリーン",
        PreviousImage => "previous picture|previous page|前の画像|前のページ",
        NextImage => "next picture|next page|次の画像|次のページ",
        FirstImage => "first picture|first page|先頭画像|最初のページ",
        LastImage => "last picture|last page|末尾画像|最後のページ",
        CopyFilePath => "copy location|clipboard path|パスコピー|場所コピー",
        RevealFile => "explorer|show in folder|file location|エクスプローラー|保存場所",
        CopyImage => "copy pixels|clipboard image|画像コピー",
        PasteImage => "clipboard|import clipboard|貼り付け|クリップボード",
        ResizeImage => "resample image|image dimensions|image resolution|画像サイズ|解像度変更",
        ResizeVideo => "resample video|video dimensions|video resolution|動画サイズ|動画解像度",
        ToggleImageInterpolation => {
            "nearest neighbor|pixelated|smooth zoom|補間|ドット表示|なめらか"
        }
        ToggleImageMinification => "downscale filter|shrink quality|縮小品質|縮小フィルター",
        CycleAudioRepeat => "loop audio|repeat song|repeat playlist|リピート|音声ループ",
        CycleAudioTrack => "soundtrack|audio language|dubbing|音声切替|吹き替え",
        LoadSubtitles => "captions|subtitle file|srt|ass|字幕読み込み",
        ToggleSubtitles => "show captions|hide captions|字幕表示|字幕非表示",
        ToggleVideoRepeat => "loop video|repeat movie|動画ループ|動画リピート",
        ToggleAudioShuffle => "random order|random playback|ランダム再生|シャッフル",
        PreviousVideoFrame => "frame back|step backward|previous frame|コマ戻し",
        NextVideoFrame => "frame advance|step forward|next frame|コマ送り",
        JumpImagesBackward1 | JumpImagesBackward2 | JumpImagesBackward3 | JumpImagesBackward4
        | JumpImagesBackward5 | JumpImagesBackward6 | JumpImagesBackward7 | JumpImagesBackward8
        | JumpImagesBackward9 | JumpImagesBackward10 => {
            "skip images backward|jump pages back|画像を飛ばして戻る"
        }
        JumpImagesForward1 | JumpImagesForward2 | JumpImagesForward3 | JumpImagesForward4
        | JumpImagesForward5 | JumpImagesForward6 | JumpImagesForward7 | JumpImagesForward8
        | JumpImagesForward9 | JumpImagesForward10 => {
            "skip images forward|jump pages ahead|画像を飛ばして進む"
        }
        SelectAspectSquare => "square selection|1:1|正方形",
        SelectAspectFourThree
        | SelectAspectThreeFour
        | SelectAspectThreeTwo
        | SelectAspectTwoThree
        | SelectAspectSixteenNine
        | SelectAspectNineSixteen => "aspect ratio selection|比率選択|アスペクト比",
        FreeRotateImage => "rotation angle|straighten image|傾き補正|画像回転角度",
        FreeRotateVideo => "rotation angle|straighten video|傾き補正|動画回転角度",
        StepAudioBackward => "audio step back|audio rewind|音声を戻す",
        StepAudioForward => "audio step ahead|audio skip forward|音声を進める",
    }
}
