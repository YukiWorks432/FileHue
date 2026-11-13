/// 表示時に言語を選び直し、操作結果も言語切り替えに追従させる。
pub enum Status {
    Idle,
    Working,
    Cancelled,
    Complete,
    Error(Failure, String),
}

pub enum Failure {
    IncompleteRecovery,
    RegistrationConflict,
    OpenSettings,
    RecoveryAvailable,
    UnsupportedSettings(bool),
    Operation,
    Download,
    Import,
    PreviousPackRestored,
    PackCleanupPending,
    ShellCleanupPending,
    ReloadSettings,
    ReadTheme,
    ReadRights,
    SelectFolder,
    SaveLanguage,
}

impl Status {
    pub fn render(&self, english: bool) -> String {
        let (ja, en, detail) = match self {
            Self::Idle => ("", "", None),
            Self::Cancelled => (
                "キャンセルしました。適用状態は変わりません。",
                "Cancelled. Applied icons are unchanged.",
                None,
            ),
            Self::Working => ("処理中…", "Working…", None),
            Self::Complete => ("完了しました。", "Completed.", None),
            Self::Error(kind, detail) => {
                let (ja, en) = match kind {
                    Failure::IncompleteRecovery => (
                        "未完了の登録復旧があります。「競合を確認」から内容を再確認して続けてください。",
                        "Registration recovery is incomplete. Open Review conflicts to review and resume.",
                    ),
                    Failure::RegistrationConflict => (
                        "外部アプリによってアイコン設定が変更されています。「競合を確認」から復旧できます。",
                        "Icon settings have changed outside this app. Open Review conflicts to recover.",
                    ),
                    Failure::OpenSettings => (
                        "設定を開けません。ほかの管理画面を閉じるか、設定ファイルを確認して再起動してください。",
                        "Cannot open settings. Close other manager windows or check settings and restart.",
                    ),
                    Failure::RecoveryAvailable => (
                        "設定を読めません。保存済みの記録から登録を解除できます。壊れた設定は退避して初期化します。",
                        "Cannot read settings. Restore registration from the saved recovery record. Damaged settings will be backed up and reset.",
                    ),
                    Failure::UnsupportedSettings(true) => (
                        "設定形式に対応していません。引き継ぐ場合は対応する版で確認してください。登録解除時は設定を退避して初期化します。",
                        "This settings format is unsupported. Use a compatible version to retain settings. Restoring registration backs up and resets them.",
                    ),
                    Failure::UnsupportedSettings(false) => (
                        "設定形式に対応していません。元の設定を保持しています。対応する版で確認してください。",
                        "This settings format is unsupported. The original settings are preserved. Use a compatible version to review them.",
                    ),
                    Failure::Download => (
                        "パックの取得に失敗しました。対象の出典と詳細を確認して再試行してください。",
                        "Could not download this pack. Check its source and details, then retry.",
                    ),
                    Failure::Import => (
                        "テーマの取り込みに失敗しました。選択を保持しています。詳細を確認して再試行してください。",
                        "Could not import this theme. Your selection is retained. Check the details and retry.",
                    ),
                    Failure::Operation => (
                        "処理に失敗しました。復元状態と詳細を確認してください。",
                        "Operation failed. Check recovery state and details.",
                    ),
                    Failure::PreviousPackRestored => (
                        "切り替えに失敗したため、以前のアイコンパックへ戻しました。詳細を確認してください。",
                        "Could not switch packs. The previous icon pack has been restored. Check the details.",
                    ),
                    Failure::PackCleanupPending => (
                        "パックの削除要求を保存しましたが、一部の素材が残っています。使用中のファイルや詳細を確認し、削除を再試行してください。",
                        "The pack removal request was saved, but some files remain. Check files in use and the details, then retry cleanup.",
                    ),
                    Failure::ShellCleanupPending => (
                        "アプリ用ファイルの整理を完了できませんでした。先にWindows標準に戻し、使用中のアプリや内容を確認して再試行してください。",
                        "Could not finish cleaning up app files. Restore Windows defaults first, check applications using the files and the details, then retry.",
                    ),
                    Failure::ReloadSettings => (
                        "設定を読めません。上書きを防ぐため操作を停止しました。",
                        "Cannot read settings. Changes have been disabled to protect recovery data.",
                    ),
                    Failure::ReadRights => (
                        "出典と利用条件を読めません。",
                        "Cannot read source and licenses.",
                    ),
                    Failure::ReadTheme => ("テーマを読めません。", "Cannot read theme."),
                    Failure::SelectFolder => ("選択に失敗しました。", "Selection failed."),
                    Failure::SaveLanguage => (
                        "表示言語を保存できません。",
                        "Cannot save display language.",
                    ),
                };
                (ja, en, Some(detail))
            }
        };
        let message = if english { en } else { ja };
        match detail {
            Some(detail) => format!("{message}\n{detail}"),
            None => message.into(),
        }
    }
}
