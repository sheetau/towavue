# Updating the public benchmarks

[BENCHMARKS.md](../BENCHMARKS.md) is the maintained public report. The repository
README embeds its eleven-workload overview. Both use the same reviewed
[data.json](data.json); the report, calculation details and figures are generated
by [publish.py](../../scripts/benchmark/publish.py).

## After a measurement

1. Audit the source run: command/endpoint compatibility, file identity, owned
   window, failures, warmup exclusion, measured blocks, app versions and executable
   hashes. A new cohort must not silently pool exploratory or incompatible runs.
2. Add a study ID with its source-summary SHA-256, build, blocks and categories.
   Replace only remeasured records, keyed by `category`, `case`, `scenario`,
   `metric`, `app`. Preserve all other comparator records and their attribution.
   Set the publication date and integration commit; revise environment/build
   notes when necessary. Do not label older values as newly measured.
3. Publish portable numeric summaries only. Do not include machine paths,
   personal filenames, profiles, raw captures or generated media. File open
   uses direct-entry operations; Windows-dialog measurements and comparisons
   containing only towavue are outside the public dataset.
4. If replacing the six-condition paired study, update `open_confidence` from
   the new independent cohort, including paired gaps, capture bounds and the
   multiple-comparison method. Update the corresponding method/claim prose in
   the generator. Do not reuse an old confidence interval with new medians.
5. Run the following commands using the pinned benchmark environment from the
   [harness guide](../../scripts/benchmark/README.md):

```powershell
target/benchmark/venv/Scripts/python.exe -m unittest discover -s scripts/benchmark -p test_publish.py -v
target/benchmark/venv/Scripts/python.exe scripts/benchmark/publish.py
target/benchmark/venv/Scripts/python.exe scripts/benchmark/publish.py --check
```

6. Inspect the overview and changed detail charts. Verify local links and
   `git diff --check`, then review data and document diffs. Adjust the README's
   date and speed claim to match the new results; its prose is not generated.
   The generator's method prose also needs review when the protocol changes.

The generator needs no private ledger, raw capture or application installation.
It writes PNG/SVG figures, `overview.json` with the exact matched conditions and
per-fixture contributions, and the complete benchmark Markdown. `--check`
regenerates in a temporary directory and compares every generated artifact.
Use the pinned Python/packages for byte-identical figure validation. New files
use UTF-8 without BOM and LF line endings.

## Overview aggregation

Each workload selects one endpoint and only case/action pairs shared by all
listed apps. Compute a median across actions for each fixture, then a median
across those fixture values. Trial counts do not weight the score. Startup,
seek and file-open fixtures have one selected condition; navigation can have
multiple actions. This prevents fixtures with more recorded actions from
dominating. All original cells remain in the detailed report, including
supplemental endpoints that do not enter the overview.

The overview has no pooled p95, combined-workflow duration or aggregate
significance claim. Photos activation has a different process boundary and is
explicitly labeled. File-open and process-startup results are separate.

The chart palette is stable: towavue teal, IrfanView orange, MPC-HC purple, VLC
rose and Photos blue. Completion charts use 95% opacity for the fastest observed
value and 23% for other apps, with "Shorter bars are faster" below. Playback
cadence has a 30 fps reference and no winner: smaller intervals are not inherently
better. Capture bounds and statistical confidence intervals are distinct.

Historical private reports and source runs remain archived for investigation;
they are not a second maintained publication. Their evidence hashes remain in
the public study inventory data.
