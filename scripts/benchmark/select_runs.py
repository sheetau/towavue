"""Derive a reproducible visual dataset from complete final image/video batches.

Original runs are kept intact. Selection is by media kind, never by speed or
success. The final image batch records invisible-window bounds; the final video
batch clears resume state. Both corrections apply to every app in the batch.
"""
import argparse
import hashlib
import json
from pathlib import Path


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--images", type=Path, required=True)
    parser.add_argument("--videos", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    if (args.output / "samples.jsonl").exists():
        raise SystemExit("Use a new selection directory")
    sources, selected = [], []
    corpus_hash = None
    for root, kinds in ((args.images, ["image", "animation"]), (args.videos, ["video"])):
        metadata = json.loads((root / "run.json").read_text(encoding="utf-8"))
        if corpus_hash is not None and corpus_hash != metadata["corpus_sha256"]:
            raise ValueError("Different corpus manifests")
        corpus_hash = metadata["corpus_sha256"]
        source = root / "samples.jsonl"
        sources.append(dict(root=str(root.resolve()), kinds=kinds, samples_sha256=digest(source),
                            run_sha256=digest(root / "run.json")))
        for line in source.read_text(encoding="utf-8").splitlines():
            row = json.loads(line)
            if row["kind"] in kinds:
                row["source_run"] = str(root.resolve())
                selected.append(row)
    (args.output / "samples.jsonl").write_text("".join(json.dumps(row) + "\n" for row in selected), encoding="utf-8")
    (args.output / "run.json").write_text(json.dumps(dict(
        derived=True, sources=sources, corpus_sha256=corpus_hash,
        reason="All video pairs rerun with resume disabled/reset. All image pairs rerun with explicit invisible-window timestamps for narrower observation bounds. Original pilot batches retained separately.",
        selection="By media kind only, never by observed latency or success; all warmups and failures retained.",
        limitation="Images and videos were measured in separate randomized schedules; compare apps within a fixture."), indent=2) + "\n", encoding="utf-8")
    print(f"Selected {len(selected)} rows")


if __name__ == "__main__":
    main()
