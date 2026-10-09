"""Photos navigation via owned native keyboard input, without Computer Use.

Preparation inspects the filmstrip through Windows UI Automation. Timing includes
only native input dispatch and the existing desktop-recognition observer. The
manifest's file sequence must agree with both Photos and the Shell probe.
"""
import argparse
import ctypes as C
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import random
import subprocess
import time
from urllib.parse import quote

from async_observer import Normalizer
from interaction import CaptureUnavailable, Session
from native_input import MouseInput, cursor
from visual import U, geometry, sha, windows

K = C.WinDLL("kernel32", use_last_error=True)


class ProcessEntry(C.Structure):
    _fields_ = [("size", W.DWORD), ("usage", W.DWORD), ("pid", W.DWORD),
                ("heap", C.c_size_t), ("module", W.DWORD), ("threads", W.DWORD),
                ("parent", W.DWORD), ("priority", W.LONG), ("flags", W.DWORD),
                ("exe", W.WCHAR * 260)]


K.CreateToolhelp32Snapshot.argtypes = [W.DWORD, W.DWORD]
K.CreateToolhelp32Snapshot.restype = W.HANDLE
K.Process32FirstW.argtypes = [W.HANDLE, C.POINTER(ProcessEntry)]
K.Process32NextW.argtypes = [W.HANDLE, C.POINTER(ProcessEntry)]
K.OpenProcess.argtypes = [W.DWORD, W.BOOL, W.DWORD]
K.OpenProcess.restype = W.HANDLE
K.QueryFullProcessImageNameW.argtypes = [W.HANDLE, W.DWORD, W.LPWSTR, C.POINTER(W.DWORD)]
K.GetExitCodeProcess.argtypes = [W.HANDLE, C.POINTER(W.DWORD)]
K.WaitForSingleObject.argtypes = [W.HANDLE, W.DWORD]
K.CloseHandle.argtypes = [W.HANDLE]


class Keyboard(C.Structure):
    _fields_ = [("vk", W.WORD), ("scan", W.WORD), ("flags", W.DWORD),
                ("time", W.DWORD), ("extra", C.c_size_t)]


class InputData(C.Union):
    _fields_ = [("keyboard", Keyboard), ("mouse", MouseInput)]


class Input(C.Structure):
    _fields_ = [("kind", W.DWORD), ("data", InputData)]


assert C.sizeof(Input) == 40
U.SendInput.argtypes = [W.UINT, C.POINTER(Input), C.c_int]
U.SendInput.restype = W.UINT
U.GetAsyncKeyState.argtypes = [C.c_int]
U.GetAsyncKeyState.restype = C.c_short


def title(hwnd):
    text = C.create_unicode_buffer(2048)
    U.GetWindowTextW(hwnd, text, len(text))
    return text.value


def processes(package):
    snapshot = K.CreateToolhelp32Snapshot(2, 0)
    if snapshot == C.c_void_p(-1).value:
        raise C.WinError(C.get_last_error())
    result = {}
    try:
        entry = ProcessEntry()
        entry.size = C.sizeof(entry)
        more = K.Process32FirstW(snapshot, C.byref(entry))
        while more:
            if entry.exe.casefold() == "photos.exe":
                handle = K.OpenProcess(0x1000, False, entry.pid)
                if not handle:
                    raise C.WinError(C.get_last_error())
                try:
                    path = C.create_unicode_buffer(32768)
                    length = W.DWORD(len(path))
                    if not K.QueryFullProcessImageNameW(handle, 0, path, C.byref(length)):
                        error = C.get_last_error()
                        code = W.DWORD()
                        if K.GetExitCodeProcess(handle, C.byref(code)) and code.value != 259:
                            more = K.Process32NextW(snapshot, C.byref(entry))
                            continue
                        raise C.WinError(error)
                    if Path(path.value) != package / "Photos.exe":
                        raise RuntimeError("Unexpected Photos executable")
                    result[entry.pid] = entry.parent
                finally:
                    K.CloseHandle(handle)
            more = K.Process32NextW(snapshot, C.byref(entry))
    finally:
        K.CloseHandle(snapshot)
    return result


class RetainedProcess:
    def __init__(self, pid):
        self.pid = pid
        self.handle = K.OpenProcess(0x1000 | 0x100000, False, pid)
        if not self.handle:
            raise C.WinError(C.get_last_error())

    def poll(self):
        code = W.DWORD()
        if not K.GetExitCodeProcess(self.handle, C.byref(code)):
            raise C.WinError(C.get_last_error())
        return None if code.value == 259 else code.value

    def release(self):
        K.CloseHandle(self.handle)


def validate_order(expected, observed, allow_virtualized=False):
    complete = expected == observed
    prefix = (allow_virtualized and len(expected) > 40 and 9 <= len(observed) < len(expected)
              and expected[:len(observed)] == observed)
    if len(set(expected)) != len(expected) or not (complete or prefix):
        raise RuntimeError("Photos filmstrip order or membership differs from the frozen fixture order")


def native_key(session, vk):
    session.check()
    if U.GetForegroundWindow() != session.hwnd:
        raise RuntimeError("Native keyboard requires the owned foreground window")
    if any(U.GetAsyncKeyState(key) & 0x8000 for key in range(1, 256)):
        raise RuntimeError("Physical input is held")
    events = (Input * 2)(Input(1, InputData(keyboard=Keyboard(vk, 0, 1, 0, 0))),
                        Input(1, InputData(keyboard=Keyboard(vk, 0, 3, 0, 0))))
    sent = U.SendInput(2, events, C.sizeof(Input))
    if sent != 2:
        if sent == 1:
            release = (Input * 1)(events[1])
            if U.SendInput(1, release, C.sizeof(Input)) != 1:
                U.keybd_event.argtypes = [W.BYTE, W.BYTE, W.DWORD, C.c_size_t]
                U.keybd_event(vk, 0, 3, 0)
        raise RuntimeError("Incomplete native keyboard dispatch")


def input_preflight(session, expected_name):
    session.check()
    if title(session.hwnd) != expected_name:
        raise RuntimeError("The preceding Photos fixture changed")
    client, _, origin = geometry(session.hwnd)
    if (client.right, client.bottom) != (800, 650):
        raise RuntimeError("Photos client geometry changed")
    for x, y in ((8, 8), (792, 8), (8, 642), (792, 642), (400, 325)):
        hwnd = U.WindowFromPoint(W.POINT(origin.x + x, origin.y + y))
        if U.GetAncestor(hwnd, 2) != session.hwnd:
            raise RuntimeError("Photos input region is occluded")


class PhotosSession(Session):
    def __init__(self, package, path, root):
        self.root, self.hwnd, self.pending_dialog = root, None, None
        self.process = None
        root.mkdir()
        self.before = processes(package)
        old_windows = {hwnd for pid in self.before for hwnd in windows(pid)}
        if old_windows:
            raise RuntimeError("An existing Photos viewer is open; preserve it")
        os.startfile("ms-photos:viewer?fileName=" + quote(str(path), safe=""))
        deadline = time.monotonic() + 12
        normalizer = Normalizer()
        try:
            while time.monotonic() < deadline:
                if self.hwnd is None:
                    matches = [(pid, hwnd) for pid in processes(package) for hwnd in windows(pid)
                               if hwnd not in old_windows and title(hwnd) == path.name]
                    if len(matches) > 1:
                        raise RuntimeError("Ambiguous Photos viewer ownership")
                    if matches:
                        pid, self.hwnd = matches[0]
                        self.process = RetainedProcess(pid)
                if self.hwnd and normalizer.step(self):
                    try:
                        self.capture()
                    except CaptureUnavailable:
                        pass
                    else:
                        self.normalization = normalizer.requests
                        return
                time.sleep(.02)
            raise RuntimeError("Photos viewer setup timed out")
        except BaseException:
            if self.process is not None:
                self.close()
            raise

    def prepare(self, files):
        result = subprocess.run([
            "powershell", "-NoProfile", "-NonInteractive", "-File",
            str(Path(__file__).with_name("photos_navigation_setup.ps1")),
            "-Window", str(self.hwnd), "-ProcessId", str(self.process.pid),
            "-FirstFile", Path(files[0]["path"]).name,
            "-ExpectedCount", str(len(files)),
            "-LastFile", Path(files[-1]["path"]).name,
        ], capture_output=True, timeout=35, creationflags=subprocess.CREATE_NO_WINDOW)
        if result.returncode:
            raise RuntimeError(result.stderr.decode("utf-8", errors="replace"))
        audit = json.loads(result.stdout.decode("utf-8-sig"))
        save(self.root / "filmstrip.json", audit)
        validate_order([Path(f["path"]).name for f in files], audit["files"], allow_virtualized=True)
        audit["virtualized_prefix"] = len(audit["files"]) < len(files)
        self.check()
        if U.GetForegroundWindow() != self.hwnd:
            raise RuntimeError("Photos filmstrip focus did not activate the owned viewer")
        return audit

    def close(self):
        # Only the identified viewer is closed. Spare/background processes and
        # any other window in a shared Photos process are never terminated.
        self.check()
        U.PostMessageW(self.hwnd, 0x10, 0, 0)
        deadline = time.monotonic() + 5
        while U.IsWindow(self.hwnd) and time.monotonic() < deadline:
            time.sleep(.02)
        closed = not U.IsWindow(self.hwnd)
        code = self.process.poll()
        self.process.release()
        if not closed:
            raise RuntimeError("Owned Photos viewer did not close")
        return {"viewer_closed": closed, "process_exit_code": code}


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=10)
    parser.add_argument("--warmup-blocks", type=int, default=1)
    parser.add_argument("--groups", default="")
    args = parser.parse_args()
    if args.repeats < 1 or args.warmup_blocks not in (0, 1):
        parser.error("Use positive repeats and zero or one warmup block")
    root, corpus = args.root.resolve(), args.corpus.resolve()
    root.mkdir()
    package_result = subprocess.run([
        "powershell", "-NoProfile", "-NonInteractive", "-Command",
        "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); Get-AppxPackage Microsoft.Windows.Photos | Select-Object Version,InstallLocation | ConvertTo-Json -Compress",
    ], check=True, capture_output=True, timeout=15, creationflags=subprocess.CREATE_NO_WINDOW)
    package_info = json.loads(package_result.stdout.decode("utf-8-sig"))
    package = Path(package_info["InstallLocation"])
    groups = json.loads((corpus / "manifest.json").read_text(encoding="utf-8"))["groups"]
    if args.groups:
        groups = [g for g in groups if g["id"] in args.groups.split(",")]
    if not groups:
        raise RuntimeError("No fixture groups")
    repo = Path(__file__).resolve().parents[2]
    env = os.environ.copy()
    env["PATH"] = str(next((repo / "vendor/ffmpeg/autobuild-2026-08-31-13-27").glob("*/bin"))) + os.pathsep + env["PATH"]
    for group in groups:
        for file in group["files"]:
            if sha(corpus / file["path"]) != file["sha256"]:
                raise RuntimeError("Fixture hash mismatch")
        probe = subprocess.run([str(repo / "target/release/examples/folder-order-dump.exe"),
                                str((corpus / group["files"][0]["path"]).parent)],
                               capture_output=True, check=True, env=env, creationflags=subprocess.CREATE_NO_WINDOW)
        actual = [Path(s).name for s in probe.stdout.decode("utf-8").splitlines()]
        validate_order([Path(f["path"]).name for f in group["files"]], actual)
        save(root / (group["id"] + "-shell-order.json"), {"files": actual, "source": probe.stderr.decode("utf-8")})
    seed = 104130
    save(root / "run.json", {"package": package_info, "exe_sha256": sha(package / "Photos.exe"),
         "manifest_sha256": sha(corpus / "manifest.json"), "runner_sha256": sha(Path(__file__)),
         "setup_sha256": sha(Path(__file__).with_name("photos_navigation_setup.ps1")),
         "repeats": args.repeats, "warmup_blocks": args.warmup_blocks, "seed": seed,
         "groups": [g["id"] for g in groups], "settings": "Installed Photos defaults, Fit, filmstrip visible and focused; no profile isolation",
         "protocol": "SendInput arrow/Home/End pairs; standard Session.observe desktop capture; 800x650 client; one-second initial idle and 100 ms between targets. Filmstrip shown/focused before timing; large-folder next/previous only, without pre-traversal.",
         "unavailable_conditions": [{"case": "large1000", "scenarios": ["distant_first", "distant_last"],
             "reason": "Calibration could not make a single boundary command reach the complete folder boundary; partial Photos lists must not be timed as full-folder jumps."}],
         "scope": "Fresh viewer sessions; natural spare process retained. Warm byte-identical copies renamed by original Shell position; verified Shell/filmstrip order. Not a paired contemporaneous comparison with historical apps."})
    rng = random.Random(seed)
    with (root / "samples.jsonl").open("x", encoding="utf-8") as output:
        for block in range(-args.warmup_blocks, args.repeats):
            schedule = list(groups)
            rng.shuffle(schedule)
            for group in schedule:
                files = group["files"]
                folder = root / f"b{block+1:02d}-{group['id']}"
                session = PhotosSession(package, corpus / files[0]["path"], folder)
                try:
                    audit = session.prepare(files)
                    row = session.observe(files[0]["identity"], time.perf_counter_ns(), timeout=10)
                    if row["status"] != "ok":
                        raise RuntimeError("Initial content did not match")
                    save(folder / "setup.json", {"uia": audit, "initial_processes": session.before,
                         "normalization": session.normalization, "initial_observation": row,
                         "pid": session.process.pid, "hwnd": session.hwnd})
                    time.sleep(1)
                    initial_cursor = cursor()
                    count = min(8, len(files) - 1)
                    actions = [("next", i, 0x27) for i in range(1, count + 1)]
                    actions += [("previous", i, 0x25) for i in range(count - 1, -1, -1)]
                    if audit["boundary_navigation_eligible"]:
                        actions += [("distant_last", len(files) - 1, 0x23), ("distant_first", 0, 0x24)]
                    previous_identity = files[0]["identity"]
                    previous_name = Path(files[0]["path"]).name
                    for step, (scenario, index, vk) in enumerate(actions):
                        if cursor() != initial_cursor:
                            raise RuntimeError("Cursor changed during the fixture session")
                        if previous_identity == files[index]["identity"]:
                            raise RuntimeError("Target identity must differ from the preceding image")
                        input_preflight(session, previous_name)
                        start = time.perf_counter_ns()
                        native_key(session, vk)
                        dispatch_end = time.perf_counter_ns()
                        row = session.observe(files[index]["identity"], start,
                                              retain=f"{step:02d}.png" if block == 0 else None)
                        row.update(app="photos", case=group["id"], kind="image", scenario=scenario,
                                   step=step, block=block, phase="warmup" if block < 0 else "measurement",
                                   target_index=index, target_identity=files[index]["identity"],
                                   dispatch_ms=(dispatch_end-start)/1e6, title=title(session.hwnd))
                        if U.GetForegroundWindow() != session.hwnd or cursor() != initial_cursor:
                            row["status"] = "input_interference"
                        if row["title"] != Path(files[index]["path"]).name:
                            row["status"] = "title_mismatch"
                        output.write(json.dumps(row) + "\n")
                        output.flush()
                        if row["status"] != "ok":
                            raise RuntimeError("Invalid observation; stop without retries")
                        previous_identity = files[index]["identity"]
                        previous_name = Path(files[index]["path"]).name
                        time.sleep(.1)
                    print(block, group["id"], len(actions), "valid navigation operations", flush=True)
                except BaseException as error:
                    output.write(json.dumps({"app": "photos", "case": group["id"], "block": block,
                        "phase": "warmup" if block < 0 else "measurement", "status": "session_error", "detail": str(error)}) + "\n")
                    output.flush()
                    raise
                finally:
                    save(folder / "cleanup.json", session.close())
    for group in groups:
        for file in group["files"]:
            if sha(corpus / file["path"]) != file["sha256"]:
                raise RuntimeError("Fixture changed during measurement")
    save(root / "completion.json", {"all_fixture_hashes_verified": True,
         "exe_sha256": sha(package / "Photos.exe"), "processes_after": processes(package)})


if __name__ == "__main__":
    main()
