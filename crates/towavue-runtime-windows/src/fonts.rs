use std::path::{Path, PathBuf};

/// Owned font bytes prepared without any window, graphics device or UI context.
pub struct UiFonts {
    pub japanese: Option<(Vec<u8>, u32)>,
    pub symbols: Option<Vec<u8>>,
    pub fallbacks: Vec<UiFontFallback>,
}

impl UiFonts {
    pub fn read() -> Self {
        Self {
            japanese: japanese_ui_font(),
            symbols: ui_symbol_font(),
            fallbacks: ui_font_fallbacks(),
        }
    }
}

/// One startup read, overlapped with native window/device initialization.
/// Join on consumption or early initialization failure; only owned bytes cross
/// the worker boundary. Font selection and missing-font fallbacks are unchanged.
pub struct UiFontPreparation(Option<std::thread::JoinHandle<UiFonts>>);

impl UiFontPreparation {
    pub fn start() -> std::io::Result<Self> {
        std::thread::Builder::new()
            .name("towavue-fonts".into())
            .spawn(UiFonts::read)
            .map(|worker| Self(Some(worker)))
    }

    pub fn finish(mut self) -> UiFonts {
        match self.0.take().expect("owned font worker").join() {
            Ok(fonts) => fonts,
            Err(_) => {
                crate::diagnostic!("Font preparation failed; retrying on the window thread");
                UiFonts::read()
            }
        }
    }
}

impl Drop for UiFontPreparation {
    fn drop(&mut self) {
        if let Some(worker) = self.0.take() {
            let _ = worker.join();
        }
    }
}

/// Detached installed font bytes. No native font handle survives the read.
pub struct UiFontFallback {
    pub name: &'static str,
    pub data: Vec<u8>,
    pub index: u32,
}

/// Installed regular faces for Windows' recommended non-Latin UI families.
/// Missing optional language packs are skipped; nothing is installed or bundled.
pub fn ui_font_fallbacks() -> Vec<UiFontFallback> {
    let Some(directory) = std::env::var_os("WINDIR") else {
        return Vec::new();
    };
    read_fallbacks(&PathBuf::from(directory).join("Fonts"))
}

fn read_fallbacks(directory: &Path) -> Vec<UiFontFallback> {
    // https://learn.microsoft.com/windows/apps/design/signature-experiences/typography#fonts-for-non-latin-languages
    // Keep only regular faces. Nirmala ships as separate TTFs or a collection;
    // index zero of the collection is the regular UI face.
    let candidates = [
        ("windows-segoe-ui", &[("segoeui.ttf", 0)][..]),
        ("windows-malgun", &[("malgun.ttf", 0)][..]),
        ("windows-yahei", &[("msyh.ttc", 1), ("msyh.ttf", 0)][..]),
        ("windows-jhenghei", &[("msjh.ttc", 1), ("msjh.ttf", 0)][..]),
        (
            "windows-nirmala",
            &[("Nirmala.ttf", 0), ("Nirmala.ttc", 0)][..],
        ),
        ("windows-leelawadee", &[("LeelawUI.ttf", 0)][..]),
        ("windows-myanmar", &[("mmrtext.ttf", 0)][..]),
        ("windows-ebrima", &[("ebrima.ttf", 0)][..]),
        ("windows-gadugi", &[("gadugi.ttf", 0)][..]),
        ("windows-himalaya", &[("himalaya.ttf", 0)][..]),
        ("windows-phagspa", &[("phagspa.ttf", 0)][..]),
    ];
    read_font_lanes(|lane| {
        candidates
            .iter()
            .copied()
            .enumerate()
            .filter(|(ordinal, _)| ordinal % 3 == lane)
            .filter_map(|(ordinal, (name, files))| {
                files.iter().find_map(|(file, index)| {
                    std::fs::read(directory.join(file))
                        .ok()
                        .filter(|data| !data.is_empty())
                        .map(|data| {
                            (
                                ordinal,
                                UiFontFallback {
                                    name,
                                    data,
                                    index: *index,
                                },
                            )
                        })
                })
            })
            .collect::<Vec<_>>()
    })
}

// Owned buffers cross these scoped workers. All workers join on success or
// unwind; ordinal sorting preserves family precedence despite completion order.
// The caller handles one lane, with at most two additional 256 KiB stacks.
fn read_font_lanes<T: Send>(read_lane: impl Fn(usize) -> Vec<(usize, T)> + Sync) -> Vec<T> {
    std::thread::scope(|scope| {
        let read_lane = &read_lane;
        let spawn = |lane| {
            #[cfg(test)]
            if parallel_tests::spawn_fails(lane) {
                return Err(std::io::Error::other("Injected font worker spawn failure"));
            }
            std::thread::Builder::new()
                .name(format!("font-read-{lane}"))
                .stack_size(256 * 1024)
                .spawn_scoped(scope, move || read_lane(lane))
        };
        let first = spawn(0);
        let second = spawn(1);
        let mut fonts = read_lane(2);
        for (lane, worker) in [(0, first), (1, second)] {
            match worker {
                Ok(worker) => fonts.extend(
                    worker
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
                ),
                Err(_) => fonts.extend(read_lane(lane)),
            }
        }
        fonts.sort_unstable_by_key(|(ordinal, _)| *ordinal);
        fonts.into_iter().map(|(_, font)| font).collect()
    })
}

#[cfg(test)]
#[path = "fonts/parallel_tests.rs"]
mod parallel_tests;

/// Reads one installed Japanese font; no font files are bundled or modified.
pub fn japanese_ui_font() -> Option<(Vec<u8>, u32)> {
    let directory = PathBuf::from(std::env::var_os("WINDIR")?).join("Fonts");
    [("YuGothM.ttc", 1), ("meiryo.ttc", 0), ("msgothic.ttc", 0)]
        .into_iter()
        .find_map(|(name, index)| {
            std::fs::read(directory.join(name))
                .ok()
                .map(|data| (data, index))
        })
}

/// Reads the installed Windows symbol face without copying or modifying it.
pub fn ui_symbol_font() -> Option<Vec<u8>> {
    let directory = PathBuf::from(std::env::var_os("WINDIR")?).join("Fonts");
    std::fs::read(directory.join("seguisym.ttf")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_preserves_installed_bytes_face_indices_and_fallback_order() {
        let prepared = UiFontPreparation::start().expect("font worker").finish();
        let serial = UiFonts::read();
        assert_eq!(prepared.japanese, serial.japanese);
        assert_eq!(prepared.symbols, serial.symbols);
        assert_eq!(prepared.fallbacks.len(), serial.fallbacks.len());
        for (prepared, serial) in prepared.fallbacks.iter().zip(&serial.fallbacks) {
            assert_eq!(prepared.name, serial.name);
            assert_eq!(prepared.index, serial.index);
            assert_eq!(prepared.data, serial.data);
        }
    }

    #[test]
    fn abandoned_preparation_joins_its_worker() {
        let (finished, completion) = std::sync::mpsc::channel();
        let preparation = UiFontPreparation(Some(std::thread::spawn(move || {
            finished.send(()).expect("completion receiver");
            UiFonts {
                japanese: None,
                symbols: None,
                fallbacks: Vec::new(),
            }
        })));
        drop(preparation);
        completion
            .try_recv()
            .expect("worker completed before drop returned");
    }

    #[test]
    fn installed_fallback_candidates_skip_missing_empty_and_unreadable_files() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "towavue-font-fixture-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&root).expect("owned font fixture");
        assert!(read_fallbacks(&root).is_empty());
        std::fs::write(root.join("malgun.ttf"), b"korean fixture").expect("fixture");
        std::fs::write(root.join("Nirmala.ttf"), []).expect("empty legacy candidate");
        std::fs::write(root.join("Nirmala.ttc"), b"collection fixture").expect("fixture");
        std::fs::create_dir(root.join("segoeui.ttf")).expect("unreadable file candidate");
        let fonts = read_fallbacks(&root);
        assert_eq!(
            fonts.iter().map(|font| font.name).collect::<Vec<_>>(),
            ["windows-malgun", "windows-nirmala"]
        );
        assert_eq!(fonts[1].data, b"collection fixture");
        assert_eq!(fonts[1].index, 0);
        std::fs::write(root.join("Nirmala.ttf"), b"regular fixture")
            .expect("legacy regular candidate");
        let fonts = read_fallbacks(&root);
        assert_eq!(fonts.len(), 2, "only one source per family");
        assert_eq!(fonts[1].data, b"regular fixture");
        std::fs::remove_file(root.join("malgun.ttf")).expect("remove fixture");
        std::fs::remove_file(root.join("Nirmala.ttf")).expect("remove fixture");
        std::fs::remove_file(root.join("Nirmala.ttc")).expect("remove fixture");
        std::fs::remove_dir(root.join("segoeui.ttf")).expect("remove fixture directory");
        std::fs::remove_dir(root).expect("remove owned root");
    }
}
