//! Private update cache. A process-wide host owns policy; a sharing-locked file
//! serializes cache transitions with an external installer helper. No operation
//! recursively removes a tree, follows a reparse point or changes app settings.
use super::SignedUpdate;
use crate::Cancellation;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path, PathBuf, Prefix},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use towavue_core::release::ReleaseVersion;
use windows::{Win32::Storage::FileSystem::*, core::PCWSTR};

const STATE: &str = "state.txt";
const MANIFEST: &str = "manifest.txt";
const SIGNATURE: &str = "manifest.sig";
const SETUP: &str = "setup.exe";
static SERIAL: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdatePhase {
    Ready,
    NextLaunch,
    Installing,
    Failed,
}

impl UpdatePhase {
    fn text(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NextLaunch => "next-launch",
            Self::Installing => "installing",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct State {
    stage: String,
    phase: UpdatePhase,
}

impl State {
    fn bytes(&self) -> Vec<u8> {
        format!(
            "towavue-update-state-v1\n{}\n{}\n",
            self.stage,
            self.phase.text()
        )
        .into_bytes()
    }
    fn parse(bytes: &[u8]) -> io::Result<Self> {
        let text = std::str::from_utf8(bytes).map_err(io::Error::other)?;
        let fields: Vec<_> = text.split('\n').collect();
        let ["towavue-update-state-v1", stage, phase, ""] = fields.as_slice() else {
            return Err(invalid("Invalid update cache state"));
        };
        if !valid_stage(stage) {
            return Err(invalid("Invalid update stage identity"));
        }
        let phase = match *phase {
            "ready" => UpdatePhase::Ready,
            "next-launch" => UpdatePhase::NextLaunch,
            "installing" => UpdatePhase::Installing,
            "failed" => UpdatePhase::Failed,
            _ => return Err(invalid("Unknown update cache phase")),
        };
        Ok(Self {
            stage: (*stage).into(),
            phase,
        })
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}
fn valid_stage(name: &str) -> bool {
    name.strip_prefix("stage-").is_some_and(|part| {
        part.len() == 56
            && part
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
pub(super) fn stage_name() -> io::Result<String> {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    Ok(format!(
        "stage-{:08x}{time:032x}{:016x}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ))
}

// Retain each directory handle without delete sharing, closing the interval
// between path inspection and opening a child. Read/write sharing remains so
// ordinary directory content operations can proceed under the cache mutex.
struct Directories {
    _held: Vec<File>,
}
impl Directories {
    fn lock(path: &Path, create: bool) -> io::Result<Self> {
        if !path.is_absolute()
            || !matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
            || path
                .components()
                .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
        {
            return Err(invalid(
                "Update cache requires a normalized local absolute path",
            ));
        }
        let mut held = Vec::new();
        for part in path
            .ancestors()
            .filter(|p| p.is_absolute())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if create && !part.exists() {
                fs::create_dir(part)?;
            }
            let directory = OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES.0)
                .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
                .open(part)?;
            let metadata = directory.metadata()?;
            if !metadata.is_dir()
                || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            {
                return Err(invalid(
                    "Update cache must not traverse links or reparse points",
                ));
            }
            held.push(directory);
        }
        Ok(Self { _held: held })
    }
}

pub(super) fn read_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(invalid("Update cache entry is not a regular file"));
    }
    Ok(file)
}

fn bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    read_file(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("Update cache entry exceeds its size limit"));
    }
    Ok(bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[derive(Clone, Debug)]
pub struct UpdateStore {
    root: PathBuf,
    #[cfg(test)]
    test_key: Option<Vec<u8>>,
}

#[derive(Debug)]
pub enum StartupUpdate {
    None,
    Cached(CachedUpdate),
    /// Another helper still owns installation. A new primary host must not
    /// open media/runtime files while that transaction is in progress.
    Installing,
}

pub struct CachedUpdate {
    state: State,
    update: SignedUpdate,
    directory: PathBuf,
    // Payload and directory names remain protected while a verified cache entry
    // is in use. Defer/start operations must not trust a historical hash alone.
    _directories: Directories,
    _setup: File,
}

impl std::fmt::Debug for CachedUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedUpdate")
            .field("version", &self.version())
            .field("phase", &self.phase())
            .finish_non_exhaustive()
    }
}

impl CachedUpdate {
    pub fn version(&self) -> ReleaseVersion {
        self.update.manifest().version
    }
    pub fn phase(&self) -> UpdatePhase {
        self.state.phase
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

impl UpdateStore {
    /// Does not touch disk. Methods performing I/O belong on the update worker.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            #[cfg(test)]
            test_key: None,
        }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn lock(&self) -> io::Result<(Directories, File)> {
        let directories = Directories::lock(&self.root, true)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(self.root.join("cache.lock"))?;
        if !lock.metadata()?.is_file()
            || lock.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        {
            return Err(invalid("Invalid update cache lock"));
        }
        Ok((directories, lock))
    }

    fn state(&self) -> io::Result<Option<State>> {
        match bounded(&self.root.join(STATE), 256) {
            Ok(bytes) => State::parse(&bytes).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn publish(&self, state: &State) -> io::Result<()> {
        // Called only under cache.lock. Never truncate the old decision: a crash
        // exposes either the complete old state or the complete new state.
        let temporary = self.root.join(format!("{}.state", stage_name()?));
        write_new(&temporary, &state.bytes())?;
        let source: Vec<_> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<_> = self
            .root
            .join(STATE)
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: the cache parent lease prevents path redirection; both paths
        // are owned, NUL-terminated local names valid for this synchronous call.
        let result = unsafe {
            MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(target.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(io::Error::other);
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }

    fn authenticate(&self, bytes: &[u8], signature: &[u8]) -> io::Result<SignedUpdate> {
        #[cfg(test)]
        if let Some(key) = &self.test_key {
            let manifest =
                towavue_core::release::ReleaseManifest::parse(bytes).map_err(io::Error::other)?;
            return SignedUpdate::authenticate_with_key(manifest, bytes, signature, key);
        }
        SignedUpdate::authenticate(bytes, signature)
    }

    fn verify(&self, state: State) -> io::Result<CachedUpdate> {
        let directory = self.root.join(&state.stage);
        let directories = Directories::lock(&directory, false)?;
        let bytes = bounded(&directory.join(MANIFEST), 256)?;
        let signature = bounded(&directory.join(SIGNATURE), 512)?;
        let update = self.authenticate(&bytes, &signature)?;
        let mut setup = read_file(&directory.join(SETUP))?;
        update.verify_file(&mut setup)?;
        Ok(CachedUpdate {
            state,
            update,
            directory,
            _directories: directories,
            _setup: setup,
        })
    }

    pub fn load(&self) -> io::Result<Option<CachedUpdate>> {
        let _lock = self.lock()?;
        self.state()?.map(|state| self.verify(state)).transpose()
    }

    /// Call only for a fresh primary process, after launch forwarding has been
    /// ruled out. An interrupted helper is a failure, never an automatic retry.
    pub fn startup(&self, installed: ReleaseVersion) -> io::Result<StartupUpdate> {
        let _lock = self.lock()?;
        let helper = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(self.root.join("handoff.lock"));
        let _helper = match helper {
            Ok(file) => {
                let metadata = file.metadata()?;
                if !metadata.is_file()
                    || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
                {
                    return Err(invalid("Invalid installer handoff lock"));
                }
                file
            }
            Err(error) if error.raw_os_error() == Some(32) => return Ok(StartupUpdate::Installing),
            Err(error) => return Err(error),
        };
        let Some(state) = self.state()? else {
            return Ok(StartupUpdate::None);
        };
        let mut selected = self.verify(state)?;
        if selected.version() <= installed {
            fs::remove_file(self.root.join(STATE))?;
            Self::cleanup(selected);
            return Ok(StartupUpdate::None);
        }
        if selected.phase() == UpdatePhase::Installing {
            selected.state.phase = UpdatePhase::Failed;
            self.publish(&selected.state)?;
        }
        Ok(StartupUpdate::Cached(selected))
    }

    fn cleanup(selected: CachedUpdate) {
        let CachedUpdate {
            directory,
            _directories,
            _setup,
            ..
        } = selected;
        drop(_setup);
        // Only known cache names in this authenticated generation. Extra files
        // prevent directory removal and are preserved. Other readers' sharing
        // locks also win; cleanup is optional, never a reason to force deletion.
        for name in [SETUP, MANIFEST, SIGNATURE] {
            let _ = fs::remove_file(directory.join(name));
        }
        if let Ok(entries) = fs::read_dir(&directory) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                let owned = [".ps1", ".ready", ".error"]
                    .iter()
                    .any(|suffix| name.strip_suffix(suffix).is_some_and(valid_stage));
                if owned {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        drop(_directories);
        let _ = fs::remove_dir(directory);
    }

    pub fn download(
        &self,
        update: SignedUpdate,
        cancel: &Cancellation,
    ) -> io::Result<CachedUpdate> {
        self.prepare(update.clone(), |file| update.download(file, cancel))
    }

    fn prepare(
        &self,
        update: SignedUpdate,
        write: impl FnOnce(&mut File) -> io::Result<()>,
    ) -> io::Result<CachedUpdate> {
        // Do not hold the cache mutex during a potentially slow network transfer.
        let root_lease = Directories::lock(&self.root, true)?;
        let state = State {
            stage: stage_name()?,
            phase: UpdatePhase::Ready,
        };
        let directory = self.root.join(&state.stage);
        fs::create_dir(&directory)?;
        let result = (|| {
            let _stage_lease = Directories::lock(&directory, false)?;
            let mut setup = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .share_mode(0)
                .open(directory.join(SETUP))?;
            write(&mut setup)?;
            setup.sync_all()?;
            update.verify_file(&mut setup)?;
            let (bytes, signature) = update.signed_metadata();
            write_new(&directory.join(MANIFEST), bytes)?;
            write_new(&directory.join(SIGNATURE), signature)?;
            drop(setup);
            let verified = self.verify(state.clone())?;
            let _lock = self.lock()?;
            let retired = if let Some(previous) = self.state()? {
                let previous = self.verify(previous)?;
                if previous.phase() != UpdatePhase::Ready
                    || previous.version() >= verified.version()
                {
                    return Err(invalid(
                        "An update is already scheduled or a newer update is cached",
                    ));
                }
                Some(previous)
            } else {
                None
            };
            self.publish(&state)?;
            if let Some(retired) = retired {
                Self::cleanup(retired);
            }
            Ok(verified)
        })();
        if result.is_err() {
            // This function created the unique directory. Delete only its three
            // known names while holding its parent; never sweep other cache data.
            if let Ok(stage_lease) = Directories::lock(&directory, false) {
                for name in [SETUP, MANIFEST, SIGNATURE] {
                    let _ = fs::remove_file(directory.join(name));
                }
                drop(stage_lease);
                let _ = fs::remove_dir(&directory);
            }
        }
        drop(root_lease);
        result
    }

    /// State transitions compare the selected generation under the cache mutex.
    /// A stale notification cannot replace another update or an installer claim.
    pub fn transition(&self, selected: &mut CachedUpdate, phase: UpdatePhase) -> io::Result<()> {
        let _lock = self.lock()?;
        if selected.directory.parent() != Some(self.root.as_path())
            || self.state()?.as_ref() != Some(&selected.state)
        {
            return Err(invalid(
                "The selected update changed; check for updates again",
            ));
        }
        let allowed = matches!(
            (selected.phase(), phase),
            (
                UpdatePhase::Ready,
                UpdatePhase::NextLaunch | UpdatePhase::Installing
            ) | (UpdatePhase::NextLaunch, UpdatePhase::Installing)
                | (UpdatePhase::Installing, UpdatePhase::Failed)
                | (UpdatePhase::Failed, UpdatePhase::Ready)
        );
        if !allowed {
            return Err(invalid("Invalid update state transition"));
        }
        let next = State {
            stage: selected.state.stage.clone(),
            phase,
        };
        self.publish(&next)?;
        selected.state = next;
        Ok(())
    }
}
