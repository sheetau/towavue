//! Bounded, non-stopping diagnostic records for owner-operated navigation bursts.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

pub const BURST_CAPACITY: usize = 65_536;

/// UI records use media generation, loader records use mailbox generation.
#[derive(Clone, Copy)]
#[repr(u64)]
pub enum BurstEvent {
    Key = 1,
    Navigate = 2,
    Queued = 3,
    QueueFull = 4,
    Dequeued = 5,
    OriginalAccepted = 6,
    TexturePrepared = 7,
    OriginalSubmitted = 8,
    FrameSubmitted = 9,
    CompletionReceived = 10,
    TargetSelected = 11,
    QueueCancelled = 12,
    QueueRestored = 13,
    SequenceSettled = 14,
    FrameStarted = 15,
    UiPrepared = 16,
    UiRendered = 17,
    PresentStarted = 18,
    OriginalGeometry = 19,
    LoaderRequested = 20,
    PrefetchStarted = 21,
    PrefetchReturned = 22,
    ForegroundStarted = 23,
    ForegroundReturned = 24,
    OriginalCached = 25,
    CacheHit = 26,
    OriginalPublished = 27,
    WaitStarted = 28,
    WaitFinished = 29,
    FolderRequested = 30,
    FolderCompleted = 31,
    FolderApplied = 32,
}

/// Eight u64 words: committed sequence, QPC tick, kind, generation, source ID, a/b/c.
/// Each reserved record is written once; publish its sequence last. A read-only
/// observer must reject uncommitted slots and verify the sequence around its read.
#[repr(C)]
pub struct BurstRecord {
    words: [AtomicU64; 8],
}

impl BurstRecord {
    const fn new() -> Self {
        Self {
            words: [const { AtomicU64::new(0) }; 8],
        }
    }

    fn publish(&self, sequence: u64, values: [u64; 7]) {
        for (word, value) in self.words[1..].iter().zip(values) {
            word.store(value, Ordering::Relaxed);
        }
        self.words[0].store(sequence, Ordering::Release);
    }
}

// SAFETY: uniquely named diagnostic-only symbols contain no pointers or pixels.
// Reservation may precede publication; NEXT is not a count of committed records.
#[unsafe(no_mangle)]
pub static TOWAVUE_BURST_NEXT: AtomicU64 = AtomicU64::new(0);
#[unsafe(no_mangle)]
pub static TOWAVUE_BURST_CAPACITY: AtomicU64 = AtomicU64::new(BURST_CAPACITY as u64);
#[unsafe(no_mangle)]
pub static TOWAVUE_BURST_RECORDS: [BurstRecord; BURST_CAPACITY] =
    [const { BurstRecord::new() }; BURST_CAPACITY];

pub fn burst_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("TOWAVUE_BURST_TRACE").is_some())
}

/// Stable FNV-1a over Windows UTF-16LE, normalizing separators and ASCII case.
/// The observer checks selected source IDs for collisions; no path is exported.
pub fn burst_source_id(path: &Path) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for unit in path.as_os_str().encode_wide() {
        let unit = match unit {
            47 => 92,
            65..=90 => unit + 32,
            _ => unit,
        };
        for byte in unit.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

pub fn record_burst(event: BurstEvent, generation: u64, path: Option<&Path>, payload: [u64; 3]) {
    if !burst_enabled() {
        return;
    }
    let position = TOWAVUE_BURST_NEXT.fetch_add(1, Ordering::Relaxed);
    if position >= TOWAVUE_BURST_CAPACITY.load(Ordering::Relaxed) {
        return;
    }
    let Some(record) = TOWAVUE_BURST_RECORDS.get(position as usize) else {
        // Never wrap/overwrite. NEXT > capacity explicitly invalidates a full trace.
        return;
    };
    let mut tick = 0;
    // QPC writes one local scalar; no handle or pointer escapes the call.
    let timed = unsafe { windows::Win32::System::Performance::QueryPerformanceCounter(&mut tick) };
    record.publish(
        position + 1,
        [
            if timed.is_ok() { tick as u64 } else { 0 },
            event as u64,
            generation,
            path.map(burst_source_id).unwrap_or(0),
            payload[0],
            payload[1],
            payload[2],
        ],
    );
}

#[test]
fn burst_record_layout_publication_and_private_source_identity_are_explicit() {
    assert_eq!(std::mem::size_of::<BurstRecord>(), 64);
    assert_eq!(std::mem::align_of::<BurstRecord>(), 8);
    assert_eq!(
        TOWAVUE_BURST_CAPACITY.load(Ordering::Relaxed),
        BURST_CAPACITY as u64
    );
    let record = BurstRecord::new();
    assert_eq!(record.words[0].load(Ordering::Acquire), 0);
    record.publish(7, [1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(record.words[0].load(Ordering::Acquire), 7);
    assert_eq!(
        record
            .words
            .each_ref()
            .map(|word| word.load(Ordering::Relaxed)),
        [7, 1, 2, 3, 4, 5, 6, 7]
    );
    assert_eq!(
        burst_source_id(Path::new("C:/Owned/A.png")),
        burst_source_id(Path::new(r"c:\owned\a.png"))
    );
    assert_ne!(
        burst_source_id(Path::new("a.png")),
        burst_source_id(Path::new("b.png"))
    );
}
