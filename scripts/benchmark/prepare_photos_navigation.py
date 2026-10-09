"""Copy navigation fixtures in their original Shell order for Photos comparison."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

from photos_navigation import validate_order
from visual import sha


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--navigation", required=True, type=Path)
    parser.add_argument("--interaction", required=True, type=Path)
    args = parser.parse_args()
    destination = args.root.resolve()
    destination.mkdir()  # Never overwrite a prepared corpus or another folder.
    repo = Path(__file__).resolve().parents[2]
    env = os.environ.copy()
    env["PATH"] = str(next((repo / "vendor/ffmpeg/autobuild-2026-08-31-13-27").glob("*/bin"))) + os.pathsep + env["PATH"]
    probe = repo / "target/release/examples/folder-order-dump.exe"
    groups = []
    for corpus in (args.navigation.resolve(), args.interaction.resolve()):
        manifest = json.loads((corpus / "manifest.json").read_text(encoding="utf-8"))
        for group in manifest["groups"]:
            if group["id"] not in ("jpeg1080", "jpeg24mp", "png4k", "large1000", "mixed"):
                continue
            folder = (corpus / group["files"][0]["path"]).parent
            result = subprocess.run([str(probe), str(folder)], capture_output=True, check=True,
                                    env=env, creationflags=subprocess.CREATE_NO_WINDOW)
            paths = [Path(line) for line in result.stdout.decode("utf-8").splitlines()]
            entries = {str((corpus / file["path"]).resolve()).casefold(): file for file in group["files"]}
            if {str(p).casefold() for p in paths} != set(entries):
                raise RuntimeError("Source Shell membership differs from its manifest")
            target = destination / group["id"]
            target.mkdir()
            files = []
            for index, source in enumerate(paths):
                original = entries[str(source).casefold()]
                if sha(source) != original["sha256"]:
                    raise RuntimeError("Source fixture hash mismatch")
                output = target / (f"{index:04d}" + source.suffix)
                # Independent files are required: Photos did not enumerate the
                # neighbors of hardlinked calibration fixtures correctly.
                shutil.copyfile(source, output)
                if sha(output) != original["sha256"]:
                    raise RuntimeError("Copied fixture hash mismatch")
                files.append({"path": str(output.relative_to(destination)), "identity": original["identity"],
                              "sha256": original["sha256"], "source": str(source), "source_name": source.name})
            subprocess.run(["powershell", "-NoProfile", "-NonInteractive", "-File",
                            str(Path(__file__).with_name("photos_fixture_order.ps1")),
                            "-FixtureRoot", str(destination), "-Folder", str(target)],
                           check=True, timeout=20, creationflags=subprocess.CREATE_NO_WINDOW)
            check = subprocess.run([str(probe), str(target)], capture_output=True, check=True,
                                   env=env, creationflags=subprocess.CREATE_NO_WINDOW)
            validate_order([Path(file["path"]).name for file in files],
                           [Path(line).name for line in check.stdout.decode("utf-8").splitlines()])
            groups.append({"id": group["id"], "original_shell": result.stderr.decode("utf-8").strip(), "files": files})
            print(group["id"], len(files), "verified copies", flush=True)
    (destination / "manifest.json").write_text(json.dumps({"groups": groups,
        "note": "Independent byte-identical copies named by original Shell position; only new fixture folders use name-ascending Shell order."}, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
