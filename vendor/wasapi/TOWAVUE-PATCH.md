# towavue wasapi patch

Base: crates.io `wasapi` 0.24.0, archive SHA256
`ee144c215d8d31e94c1a599fbaf540ca82a7017e4f1786816b179f334fac751d`.
The published source, manifests, lockfile and MIT license are retained; only
Cargo's local `.cargo-ok` marker is omitted.

The only API change implements the standard Windows `AsRawHandle` trait for
the owning event `Handle`. It borrows the existing handle without changing its
creation, wait behavior or destructor. No new unsafe operation is introduced.
towavue duplicates this borrowed event into an independently owned handle in
the Windows runtime before publishing it to the playback control thread.

This lets queued Pause/Shutdown controls wake an event-driven Shared WASAPI
owner immediately. The library does not change scheduling, PCM or client setup.
Runtime event lifetime/race tests and explicit native playback controls verify
the use site. Rebase this minimal interface extension before upgrading wasapi.
