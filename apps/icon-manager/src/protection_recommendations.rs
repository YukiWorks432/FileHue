//! 調査済みの登録へ、開発者向けの方針を一度だけ反映する。
use super::*;

fn recommended_rules(
    mut rules: IconProtectionRules,
    sources: &[ExistingIcon],
) -> IconProtectionRules {
    let code: Vec<_> = sources
        .iter()
        .filter(|row| {
            !row.always_protected
                && !row.needs_review
                && row.provider_id.is_some()
                && row.provider_location.as_ref().is_some_and(|location| {
                    std::path::Path::new(&location.path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.eq_ignore_ascii_case("Code.exe"))
                })
        })
        .cloned()
        .collect();
    let providers: std::collections::BTreeSet<_> = code
        .iter()
        .filter_map(|row| row.provider_id.as_deref())
        .collect();
    for provider in providers {
        rules.set_provider(&code, provider, Some(false));
    }
    for row in sources
        .iter()
        .filter(|row| !row.always_protected && !row.needs_review)
    {
        if code
            .iter()
            .any(|candidate| candidate.registration.extension == row.registration.extension)
        {
            rules.set_extension(row, None);
        } else if file_icons_core::recommendations::development_extension(
            &row.registration.extension,
        ) {
            rules.set_extension(row, Some(false));
        }
    }
    rules
}

impl Manager {
    pub(super) fn apply_recommended_protection(&mut self, cx: &mut Context<Self>) {
        if self.operation.busy()
            || self.protection.loading
            || !self.valid
            || self.protection.error.is_some()
        {
            return;
        }
        self.pin_visible_protection_rows();
        let rules = recommended_rules(self.protection.rules.clone(), &self.protection.sources);
        let changes = super::changes::compare(
            &self.protection.sources,
            self.protection.draft,
            &self.protection.rules,
            true,
            &rules,
        );
        let (released, protected, choices) = changes.counts();
        self.protection.recommendation_changes = Some(changes);
        let unchanged = self.protection.draft && rules == self.protection.rules;
        let review = self
            .protection
            .sources
            .iter()
            .filter(|row| row.needs_review)
            .count();
        self.protection.draft = true;
        self.protection.rules = rules;
        self.refresh_protection_policy(cx);
        self.protection.recommendation_notice = Some(if unchanged {
            self.text(
                "おすすめ設定になっています。",
                "Recommended settings are already in place.",
            )
            .into()
        } else if self.settings.language == "en" {
            format!(
                "Recommended: {released} unprotected, {protected} protected, {choices} choices changed, {review} require review. Save to confirm."
            )
        } else {
            format!(
                "おすすめ設定: 保護解除 {released}・保護追加 {protected}・指定のみ変更 {choices}・要確認 {review}。保存で確定します。"
            )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ExistingIcon, IconExclusion, IconProtectionRules, ProtectionReason, recommended_rules,
    };
    fn row(extension: &str, executable: &str) -> ExistingIcon {
        ExistingIcon {
            registration: IconExclusion {
                extension: extension.into(),
                class: "Shared.Class".into(),
                source: "Shared.Class\\DefaultIcon".into(),
                reason: ProtectionReason::DefaultIcon,
            },
            provider_id: Some(executable.into()),
            provider_name: Some("VS Code".into()),
            provider_inferred: true,
            open_app: None,
            fingerprint: extension.into(),
            reference: String::new(),
            location: None,
            provider_location: Some(file_icons_windows::IconLocation {
                path: format!("C:\\Apps\\{executable}"),
                index: 0,
            }),
            protect: true,
            needs_review: false,
            always_protected: false,
        }
    }
    #[test]
    fn developer_policy_overrides_code_but_preserves_media_and_review() {
        let mut sources = vec![
            row("custom", "Code.exe"),
            row("py", "Other.exe"),
            row("mts", "Player.exe"),
            row("m2ts", "Player.exe"),
            row("pdf", "Other.exe"),
            row("rs", "Code.exe"),
            row("exe", "Code.exe"),
        ];
        sources[5].needs_review = true;
        sources[6].always_protected = true;
        let mut original = IconProtectionRules::default();
        original.set_extension(&sources[0], Some(true));
        original.set_extension(&sources[4], Some(true));
        original.set_extension(&sources[5], Some(false));
        original.extensions.get_mut("rs").unwrap().fingerprint = "old-registration".into();
        let next = recommended_rules(original.clone(), &sources);
        for (index, expected) in [
            (0, false),
            (1, false),
            (2, false),
            (3, true),
            (4, true),
            (5, true),
            (6, true),
        ] {
            let mut source = sources[index].clone();
            source.update_protection(&next, true);
            assert_eq!(
                source.protect, expected,
                "{}",
                source.registration.extension
            );
        }
        assert_eq!(next.extensions.get("pdf"), original.extensions.get("pdf"));
        assert!(!next.extensions.contains_key("custom"));
        assert_eq!(recommended_rules(next.clone(), &sources), next);
    }
}
