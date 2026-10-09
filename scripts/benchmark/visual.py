"""Local command-launch benchmark; screen-visible content, never Present-only timing.

Only processes created by this harness are resized or closed. Owner sessions and
preferences are not used. Timing includes process creation and window normalization.
"""
import argparse
import ctypes as C
from ctypes import wintypes as W
import hashlib
import json
import os
from pathlib import Path
import random
import shutil
import subprocess
import time

import numpy as np
from PIL import ImageGrab
from scipy import ndimage

U = C.WinDLL("user32", use_last_error=True)
U.SetProcessDPIAware()
CALLBACK = C.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
U.EnumWindows.argtypes = [CALLBACK, W.LPARAM]
U.GetWindowThreadProcessId.argtypes = [W.HWND, C.POINTER(W.DWORD)]
U.IsWindowVisible.argtypes = [W.HWND]
U.GetClientRect.argtypes = [W.HWND, C.POINTER(W.RECT)]
U.GetWindowRect.argtypes = [W.HWND, C.POINTER(W.RECT)]
U.ClientToScreen.argtypes = [W.HWND, C.POINTER(W.POINT)]
U.SetWindowPos.argtypes = [W.HWND, W.HWND, C.c_int, C.c_int, C.c_int, C.c_int, W.UINT]
U.PostMessageW.argtypes = [W.HWND, W.UINT, W.WPARAM, W.LPARAM]
U.GetWindowTextW.argtypes = [W.HWND, W.LPWSTR, C.c_int]
U.WindowFromPoint.argtypes = [W.POINT]
U.WindowFromPoint.restype = W.HWND
U.GetAncestor.argtypes = [W.HWND, W.UINT]
U.GetAncestor.restype = W.HWND


def windows(pid):
    found = []
    @CALLBACK
    def visit(hwnd, unused):
        owner = W.DWORD()
        U.GetWindowThreadProcessId(hwnd, C.byref(owner))
        if owner.value == pid and U.IsWindowVisible(hwnd):
            rect = W.RECT()
            U.GetClientRect(hwnd, C.byref(rect))
            if rect.right > 200 and rect.bottom > 100:
                found.append((rect.right * rect.bottom, hwnd))
        return True
    U.EnumWindows(visit, 0)
    return [handle for _, handle in sorted(found, reverse=True)]


def geometry(hwnd):
    client, outer, origin = W.RECT(), W.RECT(), W.POINT()
    U.GetClientRect(hwnd, C.byref(client))
    U.GetWindowRect(hwnd, C.byref(outer))
    U.ClientToScreen(hwnd, C.byref(origin))
    return client, outer, origin


def normalize(hwnd):
    client, outer, origin = geometry(hwnd)
    if client.right != 800 or client.bottom != 650 or origin.x < 0 or origin.y < 0:
        return bool(U.SetWindowPos(hwnd, -1, 70, 70,
            800 + outer.right - outer.left - client.right,
            650 + outer.bottom - outer.top - client.bottom, 0x0010 | 0x0040))
    # Foreground activation can be denied to a command-launched process. Keep
    # only this disposable fixture topmost without taking keyboard focus. The
    # retained process is closed after the trial; no owner window is changed.
    U.SetWindowPos(hwnd, -1, 0, 0, 0, 0, 0x0001 | 0x0002 | 0x0010)
    return False


def detect(image, identity):
    rgb = np.asarray(image)[::2, ::2, :3]
    mask = (rgb[:, :, 0] < 65) & (rgb[:, :, 1] > 170) & (rgb[:, :, 2] > 45) & (rgb[:, :, 2] < 155)
    labels, count = ndimage.label(mask)
    if not count:
        return None
    sizes = np.bincount(labels.ravel())
    sizes[0] = 0
    expected = np.random.default_rng(831 + identity).integers(0, 2, (4, 6))
    for label in np.argsort(sizes)[-5:]:
        if sizes[label] < 100:
            continue
        yy, xx = np.nonzero(labels == label)
        x, y, width, height = xx.min(), yy.min(), np.ptp(xx) + 1, np.ptp(yy) + 1
        if width < 70 or height < 45 or not 1.2 < width / height < 1.8:
            continue
        matches = 0
        for row in range(4):
            for col in range(6):
                px = int(x + (col + 0.75) * width / 6.25)
                py = int(y + (row + 0.75) * height / 4.25)
                r, g, b = rgb[py, px].astype(int)
                bit = int(r > g)
                matches += b > 120 and abs(r - g) > 55 and bit == expected[row, col]
        if matches == 24:
            return [int(v * 2) for v in (x, y, width, height)]
    return None


def sha(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def prepare_apps(root, repo):
    provenance = root / "provenance"
    provenance.mkdir(exist_ok=True)
    for script in Path(__file__).parent.glob("*.py"):
        shutil.copy2(script, provenance / script.name)
    profiles = root / "profiles"
    profiles.mkdir(exist_ok=True)
    irfan = profiles / "irfanview"
    irfan.mkdir(exist_ok=True)
    vlc = profiles / "vlc"
    vlc.mkdir(exist_ok=True)
    portable = root / "tools" / "MPC-HC64"
    portable.mkdir(parents=True, exist_ok=True)
    installed = Path(os.environ["ProgramFiles(x86)"]) / "K-Lite Codec Pack" / "MPC-HC64"
    for file in installed.iterdir():
        if file.is_file() and file.suffix.lower() in (".exe", ".dll"):
            destination = portable / file.name
            if not destination.exists():
                shutil.copy2(file, destination)
    # The K-Lite build loads its bundled decoders from this subdirectory.
    # Copying only top-level DLLs can silently select different system filters.
    for directory in ("LAVFilters64", "MPCVR"):
        component = installed / directory
        if component.is_dir():
            shutil.copytree(component, portable / directory, dirs_exist_ok=True)
    ini = portable / "mpc-hc64.ini"
    if not ini.exists():
        # Presence of an INI next to the executable selects MPC's portable store.
        ini.write_text("[Settings]\nUpdaterAutoCheck=0\nRememberFilePos=0\nRememberPosForAudioFiles=0\nAutoZoom=0\nRememberWindowSize=1\n", encoding="utf-8")
    apps = {
        "towavue": {"exe": repo / "target/release/towavue.exe", "kind": ["image", "animation", "video"],
                    "args": ["--"], "after": []},
        "irfanview": {"exe": Path(os.environ["ProgramFiles"]) / "IrfanView/i_view64.exe", "kind": ["image", "animation"],
                      "args": [], "after": [f"/ini={irfan}", "/display=(70,70,816,690,{zoom},0,0)"]},
        "mpc-hc": {"exe": portable / "mpc-hc64.exe", "kind": ["video"],
                   "args": [], "after": ["/new", "/play", "/volume", "0", "/fixedsize", "816,690,70,70"]},
        "vlc": {"exe": Path(os.environ["ProgramFiles"]) / "VideoLAN/VLC/vlc.exe", "kind": ["video"],
                "args": ["--no-one-instance", f"--config={vlc / 'vlcrc'}", "--no-qt-privacy-ask", "--no-qt-video-autoresize", "--no-video-title-show", "--qt-continue=0", "--gain=0"], "after": []},
    }
    # Record the shared inputs even for runners with scenario-specific metadata.
    # Per-run overrides (including baseline executables) remain in run.json.
    inventory = {name: {'exe': str(app['exe']), 'sha256': sha(app['exe'])}
                 for name, app in apps.items()}
    (root / 'prepared-apps.json').write_text(json.dumps(inventory, indent=2)+'\n', encoding='utf-8')
    return apps



def trial(app_name, app, case, corpus, root, trial_id, timeout, retain):
    path = corpus / case["path"]
    # Explicitly warm input bytes; never label this cold-disk startup.
    with path.open("rb") as file:
        while file.read(1024 * 1024):
            pass
    env = os.environ.copy()
    profile = root / "profiles" / app_name
    for name in ("APPDATA", "LOCALAPPDATA"):
        directory = profile / name
        directory.mkdir(parents=True, exist_ok=True)
        env[name] = str(directory)
    if app_name == "towavue":
        settings = Path(env["APPDATA"]) / "towavue"
        settings.mkdir(exist_ok=True)
        # This private fixture profile has no live process between trials.
        # Clear only its resume file, so repeated startup always begins at zero.
        (settings / "video-resume.txt").unlink(missing_ok=True)
        (settings / "window-placement.conf").write_text("towavue window placement v1\n70\n70\n886\n728\n96\nnormal\n", encoding="utf-8")
        (settings / "playback-volume.conf").write_text("towavue playback volume v1\n1\n0\n0.5\n", encoding="utf-8")
        # The normal uninstrumented Release requires its development DLL directory.
        candidates = list((Path(__file__).resolve().parents[2] / "vendor/ffmpeg/autobuild-2026-08-31-13-27").glob("*/bin"))
        env["FFMPEG_DIR"] = str(candidates[0].parent)
        env["PATH"] = str(candidates[0]) + os.pathsep + env["PATH"]
    width = next(stream.get("width", 1920) for stream in case["probe"]["streams"] if stream["codec_type"] == "video")
    if case["identity"] == 7:
        width = 1920
    after = [arg.replace("{zoom}", f"{800 / width * 100:.6f}") for arg in app["after"]]
    command = [str(app["exe"]), *app["args"], str(path), *after]
    samples, captures, normalization = [], [], 0
    visibility_samples = []
    last_absent = time.perf_counter_ns()
    row = {"trial_id": trial_id, "app": app_name, "case": case["id"], "kind": case["kind"],
           "scenario": "warm_filesystem_new_process_normalized_client", "endpoint": "recognizable_content",
           "status": "timeout", "latency_lower_ms": "", "latency_upper_ms": "", "detail": ""}
    log = (root / "logs" / f"{trial_id}.log").open("wb")
    start = time.perf_counter_ns()
    process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT,
                               creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    launch_return = time.perf_counter_ns()
    hwnd = None
    latest_image = None
    try:
        while (time.perf_counter_ns() - start) / 1e9 < timeout:
            visibility_begin = time.perf_counter_ns()
            available = windows(process.pid)
            visibility_samples.append({"begin_ns": visibility_begin, "end_ns": time.perf_counter_ns(), "visible": bool(available)})
            if not available:
                last_absent = visibility_begin
                if process.poll() is not None:
                    row["status"], row["detail"] = "process_exited", str(process.returncode)
                    break
                time.sleep(0.005)
                continue
            hwnd = available[0]
            normalization += normalize(hwnd)
            client, _, origin = geometry(hwnd)
            unobscured = True
            for x, y in ((8, 8), (client.right-8, 8), (8, client.bottom-8),
                         (client.right-8, client.bottom-8), (client.right//2, client.bottom//2)):
                owner = W.DWORD()
                U.GetWindowThreadProcessId(U.WindowFromPoint(W.POINT(origin.x+x, origin.y+y)), C.byref(owner))
                unobscured &= owner.value == process.pid
            if not unobscured:
                visibility_samples[-1]['owned_client_unobscured'] = False
                time.sleep(.003)
                continue
            begin = time.perf_counter_ns()
            latest_image = ImageGrab.grab(bbox=(origin.x, origin.y, origin.x + client.right, origin.y + client.bottom))
            end = time.perf_counter_ns()
            captures.append((end - begin) / 1e6)
            marker = detect(latest_image, case["identity"])
            samples.append({"begin_ns": begin, "end_ns": end, "match": marker is not None})
            if marker:
                point = W.POINT(origin.x + marker[0] + marker[2] // 2, origin.y + marker[1] + marker[3] // 2)
                visible_owner = U.GetAncestor(U.WindowFromPoint(point), 2)
                if visible_owner != hwnd:
                    row["status"], row["detail"] = "occluded", "Marker was not in the owned top-level window"
                    break
                if client.right != 800 or client.bottom != 650:
                    continue
                row.update(status="ok", latency_lower_ms=max(0, (last_absent - start) / 1e6),
                           latency_upper_ms=(end - start) / 1e6, marker=marker)
                break
            last_absent = begin
        row.update(pid=process.pid, start_ns=start, launch_return_ns=launch_return,
                   capture_count=len(captures), capture_median_ms=float(np.median(captures)) if captures else None,
                   normalization_count=normalization, samples=samples, visibility_samples=visibility_samples, command=command)
        if latest_image is not None and (retain or row["status"] != "ok"):
            screenshot = root / "evidence" / f"{trial_id}.png"
            latest_image.save(screenshot)
            row["screenshot"] = str(screenshot.relative_to(root))
    finally:
        for owned in windows(process.pid):
            U.PostMessageW(owned, 0x0010, 0, 0)
        try:
            process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            # Popen's process handle identifies exactly this fixture process.
            process.terminate()
            process.wait(timeout=5)
            row["forced_cleanup"] = True
        log.close()
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=30)
    parser.add_argument("--warmup-blocks", type=int, default=1)
    parser.add_argument("--timeout", type=float, default=8)
    parser.add_argument("--apps", default="towavue,irfanview,mpc-hc,vlc")
    parser.add_argument("--cases", default="")
    parser.add_argument("--seed", type=int, default=10404)
    args = parser.parse_args()
    root, corpus = args.root.resolve(), args.corpus.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if any(root.iterdir()):
        raise SystemExit("Use an empty output directory; existing content is never reused")
    for child in ("logs", "evidence"):
        (root / child).mkdir(exist_ok=True)
    repo = Path(__file__).resolve().parents[2]
    apps = prepare_apps(root, repo)
    apps = {name: app for name, app in apps.items() if name in args.apps.split(",")}
    manifest = json.loads((corpus / "manifest.json").read_text(encoding="utf-8"))
    cases = [case for case in manifest["files"] if not args.cases or case["id"] in args.cases.split(",")]
    pairs = [(name, case) for name, app in apps.items() for case in cases if case["kind"] in app["kind"]]
    if not pairs:
        parser.error("No eligible application/file pairs")
    schedule = []
    rng = random.Random(args.seed)
    if args.repeats < 1 or args.warmup_blocks < 0:
        parser.error("repeats must be positive and warmup-blocks nonnegative")
    for block in range(-args.warmup_blocks, args.repeats):
        block_pairs = pairs.copy()
        rng.shuffle(block_pairs)
        schedule.extend((block, name, case) for name, case in block_pairs)
    metadata = {"seed": args.seed, "repeats": args.repeats, "warmup_blocks": args.warmup_blocks, "timeout_s": args.timeout,
                "resume_policy": "towavue fixture history cleared per trial; MPC file/audio resume disabled; VLC qt-continue=0",
                "corpus_sha256": sha(corpus / "manifest.json"), "script_sha256": sha(Path(__file__)),
                "apps": {name: {"exe": str(app["exe"]), "sha256": sha(app["exe"]), "args": app["args"], "after": app["after"]} for name, app in apps.items()},
                "endpoint": "Desktop capture of all 24 target cells in an owned, unobscured 800x650 client",
                "limitations": ["Synthetic pilot, not representative real photographs or cinema", "No cold-disk claim", "Process-launch plus normalization cost is included", "Client size is fixed; inner media viewport and quality can differ", "Recognizable content is not full-resolution quality or photon arrival", "Decoder backend not inferred from a picture", "No audio arrival measurement in this visual runner"]}
    (root / "run.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    with (root / "samples.jsonl").open("w", encoding="utf-8") as output:
        for index, (block, name, case) in enumerate(schedule):
            identifier = f"{index:04d}-{name}-{case['id']}"
            row = trial(name, apps[name], case, corpus, root, identifier, args.timeout, block == 0)
            row["block"] = block
            row["phase"] = "warmup" if block < 0 else "measurement"
            output.write(json.dumps(row) + "\n")
            output.flush()
            print(f"{index+1}/{len(schedule)} {name} {case['id']} {row['status']} {row['latency_upper_ms']}", flush=True)
            time.sleep(0.15)


if __name__ == "__main__":
    main()
