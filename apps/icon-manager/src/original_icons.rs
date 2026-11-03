//! 見本を一時領域へ生成し、抽出用子プロセスを期限付きで回収する。
use anyhow::Result;
use file_icons_windows::{ExistingIcon, IconLocation};
use std::{
    collections::BTreeMap,
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub(super) struct Previews {
    pub paths: BTreeMap<String, PathBuf>,
    pub provider_paths: BTreeMap<String, PathBuf>,
    _directory: tempfile::TempDir,
}

pub(super) fn generate(rows: &[ExistingIcon]) -> Result<Previews> {
    let directory = tempfile::tempdir()?;
    let mut unique = BTreeMap::new();
    let mut keys = BTreeMap::new();
    // ソフトの見出しを優先し、種類別の画像と同じ資源は一度だけ抽出する。
    for (key, location) in rows
        .iter()
        .map(|row| {
            (
                format!("p:{}", row.group_id()),
                row.provider_location.as_ref(),
            )
        })
        .chain(rows.iter().map(|row| {
            (
                format!("f:{}", row.registration.extension),
                row.location.as_ref(),
            )
        }))
    {
        if let Some(location) = location {
            let resource = serde_json::to_string(location)?;
            if unique.len() < 128 || unique.contains_key(&resource) {
                let next = unique.len();
                let index = *unique.entry(resource).or_insert(next);
                let candidates = keys.entry(key).or_insert_with(Vec::new);
                if !candidates.contains(&index) {
                    candidates.push(index);
                }
            }
        }
    }
    let mut locations = vec![
        IconLocation {
            path: String::new(),
            index: 0
        };
        unique.len()
    ];
    for (key, index) in unique {
        locations[index] = serde_json::from_str(&key)?;
    }
    let request = directory.path().join("request.json");
    std::fs::write(&request, serde_json::to_vec(&locations)?)?;
    use std::os::windows::process::CommandExt;
    let mut child = PreviewProcess(
        Command::new(worker_executable()?)
            .arg("--original-icon-worker")
            .arg(&request)
            .arg(directory.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()?,
    );
    wait_for_preview(&mut child.0, Duration::from_secs(5))?;
    let paths = keys
        .into_iter()
        .filter_map(|(extension, candidates)| {
            candidates
                .into_iter()
                .map(|index| directory.path().join(format!("{index}.png")))
                .find(|path| path.is_file() && path.metadata().is_ok_and(|m| m.len() <= 65536))
                .map(|path| (extension, path))
        })
        .collect();
    let paths: BTreeMap<String, PathBuf> = paths;
    let select = |prefix: &str| {
        paths
            .iter()
            .filter_map(|(key, path)| {
                key.strip_prefix(prefix)
                    .map(|key| (key.to_owned(), path.clone()))
            })
            .collect()
    };
    Ok(Previews {
        paths: select("f:"),
        provider_paths: select("p:"),
        _directory: directory,
    })
}

struct PreviewProcess(Child);
impl Drop for PreviewProcess {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
    }
}

fn wait_for_preview(child: &mut Child, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let cleanup = Instant::now() + Duration::from_secs(1);
            while child.try_wait()?.is_none() {
                anyhow::ensure!(Instant::now() < cleanup, "preview worker did not terminate");
                std::thread::sleep(Duration::from_millis(10));
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

pub(super) fn worker(request: PathBuf, directory: PathBuf) -> Result<()> {
    let mut data = Vec::new();
    std::fs::File::open(request)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    anyhow::ensure!(data.len() <= 1024 * 1024, "preview request too large");
    let locations: Vec<IconLocation> = serde_json::from_slice(&data)?;
    anyhow::ensure!(locations.len() <= 128, "too many preview resources");
    for (index, location) in locations.iter().enumerate() {
        let _ = file_icons_windows::native_preview::write_original_icon(
            location,
            &directory.join(format!("{index}.png")),
        );
    }
    Ok(())
}

fn worker_executable() -> Result<PathBuf> {
    #[cfg(test)]
    {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/debug/file-icons-manager.exe"))
    }
    #[cfg(not(test))]
    {
        Ok(std::env::current_exe()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_preview_keeps_a_working_resource_among_stale_registrations() {
        for paths in [
            ["missing-wfi-package-resource.dll", "shell32.dll"],
            ["shell32.dll", "missing-wfi-package-resource.dll"],
        ] {
            let rows = paths.into_iter().enumerate().map(|(index, path)| {
                serde_json::from_value::<ExistingIcon>(serde_json::json!({
                    "registration": { "extension": format!("sample-{index}"), "class": "Example.File", "source": "Example.File", "reason": "default_icon" },
                    "provider_id": "same-application", "provider_name": "Example", "provider_inferred": false,
                    "open_app": null, "fingerprint": "observed", "reference": path, "location": null,
                    "provider_location": { "path": path, "index": 0 }, "protect": true,
                    "needs_review": false, "always_protected": false
                })).unwrap()
            }).collect::<Vec<_>>();
            let images = generate(&rows).unwrap();
            let path = images
                .provider_paths
                .get("same-application")
                .expect("他の登録の資源が欠けていても代表画像を残すこと");
            let encoded = std::fs::read(path).unwrap();
            assert!(encoded.starts_with(b"\x89PNG\r\n\x1a\n"));
            assert_eq!(&encoded[16..24], &[0, 0, 0, 32, 0, 0, 0, 32]);
        }
    }

    #[test]
    fn oversized_requests_do_not_generate_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("request.json");
        std::fs::write(&path, vec![b' '; 1024 * 1024 + 1]).unwrap();
        assert!(worker(path.clone(), root.path().into()).is_err());
        std::fs::write(
            &path,
            serde_json::to_vec(&vec![
                IconLocation {
                    path: "shell32.dll".into(),
                    index: 2
                };
                129
            ])
            .unwrap(),
        )
        .unwrap();
        assert!(worker(path, root.path().into()).is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[test]
    fn stalled_preview_worker_is_terminated_within_deadline() {
        let mut child = PreviewProcess(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "original_icons::tests::sleeping_worker_probe",
                ])
                .env("WFI_PREVIEW_SLEEP_TEST", "1")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let started = Instant::now();
        wait_for_preview(&mut child.0, Duration::from_millis(100)).unwrap();
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(child.0.try_wait().unwrap().is_some());
    }
    #[test]
    #[ignore = "期限試験の親からのみ起動する"]
    fn sleeping_worker_probe() {
        assert_eq!(std::env::var("WFI_PREVIEW_SLEEP_TEST").as_deref(), Ok("1"));
        std::thread::sleep(Duration::from_secs(30));
    }
}
