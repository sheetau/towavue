"""Owned-window primitives for external interaction benchmarks on Windows.

These helpers use native messages, not physical keyboard latency. Commands only
target windows whose PID belongs to the retained Popen handle.
"""
import ctypes as C
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import subprocess
import time

from PIL import ImageGrab
from visual import U, windows, geometry, normalize, detect

U.GetMenu.argtypes = [W.HWND]
U.GetMenu.restype = W.HMENU
U.GetSubMenu.argtypes = [W.HMENU, C.c_int]
U.GetSubMenu.restype = W.HMENU
U.GetMenuItemCount.argtypes = [W.HMENU]
U.GetMenuStringW.argtypes = [W.HMENU, W.UINT, W.LPWSTR, C.c_int, W.UINT]
U.GetMenuItemID.argtypes = [W.HMENU, C.c_int]
U.GetMenuItemID.restype = W.UINT
U.GetMenuState.argtypes = [W.HMENU, W.UINT, W.UINT]
U.MapVirtualKeyW.argtypes = [W.UINT, W.UINT]
U.SetForegroundWindow.argtypes = [W.HWND]
U.GetForegroundWindow.restype = W.HWND
U.EnumChildWindows.argtypes = [W.HWND, C.c_void_p, W.LPARAM]
U.GetClassNameW.argtypes = [W.HWND, W.LPWSTR, C.c_int]
U.GetDlgCtrlID.argtypes = [W.HWND]
U.SendMessageW.argtypes = [W.HWND, W.UINT, W.WPARAM, W.LPARAM]
U.SendMessageW.restype = W.LPARAM
U.IsWindow.argtypes = [W.HWND]


U.AttachThreadInput.argtypes=[W.DWORD,W.DWORD,W.BOOL]
U.GetKeyboardState.argtypes=[C.POINTER(C.c_ubyte)]
U.SetKeyboardState.argtypes=[C.POINTER(C.c_ubyte)]


def children(hwnd):
    found=[]
    @C.WINFUNCTYPE(W.BOOL,W.HWND,W.LPARAM)
    def visit(child,unused):
        text,kind=C.create_unicode_buffer(1024),C.create_unicode_buffer(256)
        U.GetWindowTextW(child,text,len(text));U.GetClassNameW(child,kind,len(kind))
        found.append({'hwnd':child,'class':kind.value,'text':text.value,'id':U.GetDlgCtrlID(child)})
        return True
    U.EnumChildWindows(hwnd,visit,0)
    return found


def menu_items(menu, prefix=""):
    items = []
    for index in range(U.GetMenuItemCount(menu)):
        label = C.create_unicode_buffer(512)
        U.GetMenuStringW(menu, index, label, len(label), 0x400)
        path = prefix + label.value
        child = U.GetSubMenu(menu, index)
        if child:
            items.extend(menu_items(child, path + "/"))
        elif label.value:
            items.append({"text": path, "id": U.GetMenuItemID(menu, index),
                          "state": U.GetMenuState(menu, index, 0x400)})
    return items


class CaptureUnavailable(RuntimeError):
    """Owned client pixels cannot yet be safely observed."""


class Session:
    def __init__(self, name, app, path, root, zoom=100, volume=0):
        self.name, self.root = name, root
        root.mkdir(parents=True, exist_ok=True)
        env = os.environ.copy()
        for key in ("APPDATA", "LOCALAPPDATA"):
            directory = root / key
            directory.mkdir(exist_ok=True)
            env[key] = str(directory)
        if name == "towavue":
            settings = Path(env["APPDATA"]) / "towavue"
            settings.mkdir(exist_ok=True)
            (settings / "video-resume.txt").unlink(missing_ok=True)
            (settings / "window-placement.conf").write_text(
                "towavue window placement v1\n70\n70\n886\n728\n96\nnormal\n", encoding="utf-8")
            (settings / "playback-volume.conf").write_text(
                f"towavue playback volume v1\n1\n{volume}\n{volume or 0.5}\n", encoding="utf-8")
            bins = list((Path(__file__).resolve().parents[2] / "vendor/ffmpeg/autobuild-2026-08-31-13-27").glob("*/bin"))
            env["PATH"] = str(bins[0]) + os.pathsep + env["PATH"]
        self.command = [str(app["exe"]), *app["args"], str(path),
                        *[arg.replace("{zoom}", str(zoom)) for arg in app["after"]]]
        self.log = (root / "process.log").open("wb")
        self.started = time.perf_counter_ns()
        self.process = subprocess.Popen(self.command, env=env, stdin=subprocess.PIPE, stdout=self.log, stderr=self.log,
                                        creationflags=subprocess.CREATE_NO_WINDOW)
        self.hwnd = None
        self.pending_dialog = None

    def ready(self, identity, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            available = windows(self.process.pid)
            if available:
                self.hwnd = available[0]
                U.SetForegroundWindow(self.hwnd)
                normalize(self.hwnd)
                try:
                    if identity is None or detect(self.capture(), identity):
                        return
                except CaptureUnavailable:
                    # Initial window animation may expose the background. Keep
                    # waiting without capturing it; the deadline still applies.
                    pass
            if self.process.poll() is not None:
                raise RuntimeError(f"{self.name} exited: {self.process.returncode}")
            time.sleep(.01)
        if self.hwnd:
            self.capture().save(self.root / "ready-timeout.png")
        raise TimeoutError(f"{self.name}: no target content")

    def check(self):
        owner = W.DWORD()
        U.GetWindowThreadProcessId(self.hwnd, C.byref(owner))
        if self.process.poll() is not None or owner.value != self.process.pid:
            raise RuntimeError("Owned window is no longer valid")

    def capture(self):
        self.check()
        client, _, origin = geometry(self.hwnd)
        if (client.right, client.bottom) != (800, 650):
            raise CaptureUnavailable(f'Fixture client changed size: {client.right}x{client.bottom}')
        # Do not retain another application's pixels if it covers this fixture.
        for x, y in ((8, 8), (client.right-8, 8), (8, client.bottom-8),
                     (client.right-8, client.bottom-8), (client.right//2, client.bottom//2)):
            window = U.WindowFromPoint(W.POINT(origin.x+x, origin.y+y))
            owner = W.DWORD()
            U.GetWindowThreadProcessId(window, C.byref(owner))
            if owner.value != self.process.pid:
                raise CaptureUnavailable('Owned capture region is occluded')
        image = ImageGrab.grab(bbox=(origin.x, origin.y, origin.x + client.right, origin.y + client.bottom))
        return image

    def key(self, vk):
        self.check()
        scan = U.MapVirtualKeyW(vk, 0)
        param = 1 | (scan << 16) | ((1 << 24) if vk in (0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28) else 0)
        if not U.PostMessageW(self.hwnd, 0x100, vk, param):
            raise C.WinError(C.get_last_error())
        U.PostMessageW(self.hwnd, 0x101, vk, param | 0xC0000000)

    def command_id(self, command):
        self.check()
        U.PostMessageW(self.hwnd, 0x111, command, 0)

    def click(self,x,y):
        self.check()
        point=(int(y)<<16)|int(x)
        U.PostMessageW(self.hwnd,0x200,0,point)
        U.PostMessageW(self.hwnd,0x201,1,point)
        U.PostMessageW(self.hwnd,0x202,0,point)

    def chord(self, *keys):
        self.check()
        # Share only the owned fixture's input queue, never the foreground owner's.
        # Synchronous messages observe modifier state before it is restored.
        kernel=C.WinDLL('kernel32');current=kernel.GetCurrentThreadId()
        target=U.GetWindowThreadProcessId(self.hwnd,None)
        if not U.AttachThreadInput(current,target,True):raise C.WinError(C.get_last_error())
        saved=(C.c_ubyte*256)();U.GetKeyboardState(saved)
        state=(C.c_ubyte*256)(*saved)
        try:
            for key in keys:
                state[key]=0x80;U.SetKeyboardState(state)
                U.SendMessageW(self.hwnd,0x100,key,1|(U.MapVirtualKeyW(key,0)<<16))
            for key in reversed(keys):
                state[key]=0;U.SetKeyboardState(state)
                U.SendMessageW(self.hwnd,0x101,key,1|(U.MapVirtualKeyW(key,0)<<16)|0xC0000000)
        finally:
            U.SetKeyboardState(saved);U.AttachThreadInput(current,target,False)

    def prepare_open(self, path, folder=False):
        """Fill the native dialog; return a callback submitting the owned dialog.

        Timing begins at dialog submission, excluding typing and path selection.
        """
        if self.name=='irfanview':
            self.command_id(1126)
        else:
            self.chord(*([0x11,0x10,0x4f] if folder else [0x11,0x4f]))
        deadline=time.monotonic()+5
        while time.monotonic()<deadline:
            dialogs=[]
            for hwnd in windows(self.process.pid):
                kind=C.create_unicode_buffer(256)
                U.GetClassNameW(hwnd,kind,len(kind))
                if kind.value=='#32770': dialogs.append(hwnd)
            if len(dialogs)==1:
                dialog=dialogs[0]
                controls=children(dialog)
                edits=[c for c in controls if c['class']=='Edit' and c['id']==1148]
                if not edits:
                    edits=[c for c in controls if c['class']=='Edit' and c['id']==1152]
                if len(edits)==1:
                    buffer=C.create_unicode_buffer(str(path))
                    U.SendMessageW(edits[0]['hwnd'],0xC,0,C.cast(buffer,C.c_void_p).value)
                    if folder:
                        # A typed directory first navigates the native picker. The
                        # measured second confirmation selects that current folder.
                        U.PostMessageW(dialog,0x111,1,0)
                        settled=False
                        until=time.monotonic()+5
                        while time.monotonic()<until:
                            current_edits=[c for c in children(dialog) if c['class']=='Edit']
                            if any(c['id']==1152 and c['text'] in ('', Path(path).name) for c in current_edits):
                                settled=True;break
                            time.sleep(.02)
                        if not settled:raise RuntimeError('Folder picker did not navigate to the requested fixture: '+json.dumps(current_edits))
                    def submit():
                        self.check()
                        owner=W.DWORD();U.GetWindowThreadProcessId(dialog,C.byref(owner))
                        if owner.value!=self.process.pid: raise RuntimeError('Dialog ownership lost')
                        now = time.perf_counter_ns()
                        self.pending_dialog = dict(hwnd=dialog, last_visible_ns=now, first_absent_ns=None,
                                                   last_exists_ns=now, first_destroyed_ns=None)
                        U.PostMessageW(dialog,0x111,1,0)
                    return submit
            time.sleep(.02)
        raise RuntimeError('Native file dialog not found: '+json.dumps(controls if dialogs else []))

    def dialog_closed(self):
        """Bracket native HWND visibility; this does not measure DWM fade completion."""
        state = self.pending_dialog
        if state is None:
            return True
        before = time.perf_counter_ns()
        visible = U.IsWindowVisible(state['hwnd'])
        exists = U.IsWindow(state['hwnd'])
        after = time.perf_counter_ns()
        if state['first_absent_ns'] is None and visible:
            state['last_visible_ns'] = before
        elif state['first_absent_ns'] is None:
            state['first_absent_ns'] = after
        if state['first_destroyed_ns'] is None and exists:
            state['last_exists_ns'] = before
        elif state['first_destroyed_ns'] is None:
            state['first_destroyed_ns'] = after
        return not visible

    def finish_dialog(self, row, start, target_lower=None, target_upper=None):
        state, self.pending_dialog = self.pending_dialog, None
        if state is None:
            return
        row['dialog_visibility'] = state
        if state['first_absent_ns'] is None:
            return
        row['dialog_close_lower_ms'] = max(0, (state['last_visible_ns']-start)/1e6)
        row['dialog_close_upper_ms'] = (state['first_absent_ns']-start)/1e6
        if target_upper is not None:
            row['post_dialog_upper_ms'] = max(0, (target_upper-state['last_visible_ns'])/1e6)
            row['post_dialog_observed_ms'] = max(0, (target_upper-state['first_absent_ns'])/1e6)
        if target_lower is not None:
            row['post_dialog_lower_ms'] = max(0, (target_lower-state['first_absent_ns'])/1e6)
        if state['first_destroyed_ns'] is not None:
            row['dialog_destroy_lower_ms'] = max(0, (state['last_exists_ns']-start)/1e6)
            row['dialog_destroy_upper_ms'] = (state['first_destroyed_ns']-start)/1e6
            if target_upper is not None:
                row['post_destroy_upper_ms'] = max(0, (target_upper-state['last_exists_ns'])/1e6)
            if target_lower is not None:
                row['post_destroy_lower_ms'] = max(0, (target_lower-state['first_destroyed_ns'])/1e6)

    def observe(self, identity, start, timeout=5, retain=None):
        last_absent = start
        samples = []
        row = {"status": "timeout", "latency_lower_ms": "", "latency_upper_ms": ""}
        while (time.perf_counter_ns() - start) / 1e9 < timeout:
            closed = self.dialog_closed()
            begin = time.perf_counter_ns()
            image = self.capture()
            end = time.perf_counter_ns()
            marker = detect(image, identity) if closed else None
            samples.append({"begin_ns": begin, "end_ns": end, "match": bool(marker)})
            if marker:
                _, _, origin = geometry(self.hwnd)
                point = W.POINT(origin.x + marker[0] + marker[2] // 2, origin.y + marker[1] + marker[3] // 2)
                if U.GetAncestor(U.WindowFromPoint(point), 2) != self.hwnd:
                    row["status"] = "occluded"
                    break
                row.update(status="ok", latency_lower_ms=max(0, (last_absent-start)/1e6),
                           latency_upper_ms=(end-start)/1e6, marker=marker)
                break
            last_absent = begin
        if retain or row["status"] != "ok":
            image.save(self.root / (retain or f"failure-{start}.png"))
        row.update(samples=samples, start_ns=start, pid=self.process.pid)
        self.finish_dialog(row, start, last_absent if row['status']=='ok' else None,
                           end if row['status']=='ok' else None)
        return row

    def close(self):
        for hwnd in windows(self.process.pid):
            U.PostMessageW(hwnd, 0x10, 0, 0)
        forced = False
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            self.process.wait(timeout=5)
            forced = True
        self.log.close()
        return forced
