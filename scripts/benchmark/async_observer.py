"""Calibrated conditional asynchronous window normalizer. Owns no windows, processes, or native handles."""
import ctypes as C
import time
from visual import U, W, geometry
from interaction import CaptureUnavailable

U.GetWindowLongPtrW.argtypes = [W.HWND, C.c_int]
U.GetWindowLongPtrW.restype = C.c_ssize_t

def ready(hwnd):
    client, _, origin = geometry(hwnd)
    return ((client.right, client.bottom) == (800, 650) and origin.x >= 0 and origin.y >= 0
            and bool(U.GetWindowLongPtrW(hwnd, -20) & 8))

class Normalizer:
    def __init__(self):
        self.hwnd = None
        self.pending = False
        self.requests = []

    def step(self, session):
        session.check()
        if self.hwnd != session.hwnd:
            self.hwnd, self.pending = session.hwnd, False
        if ready(self.hwnd):
            # A successful async return is not readiness. Only observed actual
            # geometry/topmost clears the single outstanding request.
            self.pending = False
            return True
        if self.pending:
            return False
        if len(self.requests) >= 16:
            raise RuntimeError('Fixture repeatedly invalidated normalization')
        client, outer, origin = geometry(self.hwnd)
        resize = (client.right, client.bottom) != (800, 650) or origin.x < 0 or origin.y < 0
        begin = time.perf_counter_ns()
        if resize:
            ok = U.SetWindowPos(self.hwnd, -1, 70, 70,
                                800 + outer.right - outer.left - client.right,
                                650 + outer.bottom - outer.top - client.bottom,
                                0x10 | 0x40 | 0x4000)
        else:
            ok = U.SetWindowPos(self.hwnd, -1, 0, 0, 0, 0, 1 | 2 | 0x10 | 0x4000)
        end = time.perf_counter_ns()
        self.requests.append(dict(hwnd=self.hwnd, begin_ns=begin, end_ns=end, resize=resize, success=bool(ok)))
        if not ok:
            raise C.WinError(C.get_last_error())
        self.pending = True
        return False

    def capture(self, session):
        session.check()
        if not ready(session.hwnd):
            raise CaptureUnavailable('Geometry/topmost normalization pending')
        return session.capture(desktop_roi=True)
