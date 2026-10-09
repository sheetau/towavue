"""Documented MPC-HC WM_COPYDATA and VLC RC controls for owned sessions."""
import ctypes as C
from ctypes import wintypes as W
import threading
import time
import math
from interaction import U, geometry
import native_input


def prepare_seek_input(session):
    """Establish real foreground ownership before timed input, or reject setup."""
    session.check()
    U.GetForegroundWindow.restype=W.HWND
    U.SetForegroundWindow(session.hwnd)
    if U.GetForegroundWindow()==session.hwnd:
        return dict(foreground=True,activation='SetForegroundWindow')
    # Posted mouse messages do not activate a window. The app deliberately
    # cancels seek gestures without focus; topmost visibility is insufficient.
    # Activate only the disposable fixture's verified caption, outside timing.
    client,outer,origin=geometry(session.hwnd)
    U.GetDpiForWindow.argtypes=[W.HWND];U.GetDpiForWindow.restype=W.UINT
    dpi=U.GetDpiForWindow(session.hwnd)
    if not dpi:raise RuntimeError('Cannot read fixture window DPI')
    x=(outer.left+outer.right)//2;y=outer.top+round(15*dpi/96)
    if U.GetAncestor(U.WindowFromPoint(W.POINT(x,y)),2)!=session.hwnd:
        raise RuntimeError('Owned activation caption is occluded')
    if U.SendMessageW(session.hwnd,0x84,0,((y&0xffff)<<16)|(x&0xffff))!=2:
        raise RuntimeError('Owned activation point is not a caption')
    U.GetAsyncKeyState.argtypes=[C.c_int];U.GetAsyncKeyState.restype=C.c_short
    if any(U.GetAsyncKeyState(key)&0x8000 for key in (1,2,4,0x10,0x11,0x12,0x5b,0x5c)):
        raise RuntimeError('Input button or modifier is already held during setup')
    class MouseInput(C.Structure):
        _fields_=[('dx',W.LONG),('dy',W.LONG),('data',W.DWORD),('flags',W.DWORD),
                  ('time',W.DWORD),('extra',C.c_size_t)]
    class InputData(C.Union):
        _fields_=[('mouse',MouseInput)]
    class Input(C.Structure):
        _fields_=[('kind',W.DWORD),('data',InputData)]
    U.SendInput.argtypes=[W.UINT,C.POINTER(Input),C.c_int];U.SendInput.restype=W.UINT
    U.GetCursorPos.argtypes=[C.POINTER(W.POINT)];U.SetCursorPos.argtypes=[C.c_int,C.c_int]
    previous=W.POINT()
    if not U.GetCursorPos(C.byref(previous)):raise C.WinError(C.get_last_error())
    try:
        if not U.SetCursorPos(x,y):raise C.WinError(C.get_last_error())
        if U.GetAncestor(U.WindowFromPoint(W.POINT(x,y)),2)!=session.hwnd:
            raise RuntimeError('Owned activation caption became occluded')
        events=(Input*2)(Input(0,InputData(mouse=MouseInput(0,0,0,2,0,0))),
                         Input(0,InputData(mouse=MouseInput(0,0,0,4,0,0))))
        delivered=U.SendInput(2,events,C.sizeof(Input))
        if delivered!=2:
            if delivered==1:
                # Do not leave our injected press held after partial delivery.
                U.mouse_event.argtypes=[W.DWORD,W.DWORD,W.DWORD,W.DWORD,C.c_size_t]
                U.mouse_event.restype=None
                U.mouse_event(4,0,0,0,0)
            raise RuntimeError('Owned caption activation was not delivered')
        time.sleep(.05)
    finally:
        current=W.POINT()
        if U.GetCursorPos(C.byref(current)) and (current.x,current.y)==(x,y):
            U.SetCursorPos(previous.x,previous.y)
    if U.GetForegroundWindow()!=session.hwnd:
        raise RuntimeError('Native foreground activation was not granted')
    return dict(foreground=True,activation='owned-caption-click-before-timing')


class CopyData(C.Structure):
    _fields_=[('data',C.c_size_t),('length',W.DWORD),('pointer',C.c_void_p)]


class MpcReceiver:
    """A dedicated notification window pumps synchronous MPC API replies."""
    def __init__(self):
        self.hwnd=None;self.messages=[];self.ready=threading.Event()
        self.thread=threading.Thread(target=self._run,daemon=True);self.thread.start()
        if not self.ready.wait(5) or not self.hwnd:raise RuntimeError('MPC receiver did not start')

    def _run(self):
        callback=C.WINFUNCTYPE(W.LPARAM,W.HWND,W.UINT,W.WPARAM,W.LPARAM)
        U.DefWindowProcW.argtypes=[W.HWND,W.UINT,W.WPARAM,W.LPARAM];U.DefWindowProcW.restype=W.LPARAM
        @callback
        def proc(hwnd,msg,wp,lp):
            if msg==0x4a:
                data=C.cast(lp,C.POINTER(CopyData)).contents
                if data.length<=65536 and data.pointer:
                    self.messages.append((time.perf_counter_ns(),int(wp),data.data,C.wstring_at(data.pointer,data.length//2).rstrip('\0')))
                return 1
            if msg==0x10:U.DestroyWindow(hwnd);return 0
            if msg==2:U.PostQuitMessage(0);return 0
            return U.DefWindowProcW(hwnd,msg,wp,lp)
        class WindowClass(C.Structure):
            _fields_=[('style',W.UINT),('proc',callback),('class_extra',C.c_int),('window_extra',C.c_int),
                      ('instance',W.HINSTANCE),('icon',W.HICON),('cursor',W.HANDLE),('background',W.HBRUSH),
                      ('menu',W.LPCWSTR),('name',W.LPCWSTR)]
        K=C.WinDLL('kernel32');K.GetModuleHandleW.restype=W.HMODULE
        instance=K.GetModuleHandleW(None);name=f'TowavueBenchmarkReceiver{id(self)}'
        wc=WindowClass(0,proc,0,0,instance,None,None,None,None,name)
        U.RegisterClassW.argtypes=[C.POINTER(WindowClass)]
        U.CreateWindowExW.argtypes=[W.DWORD,W.LPCWSTR,W.LPCWSTR,W.DWORD,C.c_int,C.c_int,C.c_int,C.c_int,W.HWND,W.HMENU,W.HINSTANCE,C.c_void_p]
        U.CreateWindowExW.restype=W.HWND;U.DestroyWindow.argtypes=[W.HWND]
        U.RegisterClassW(C.byref(wc))
        self.hwnd=U.CreateWindowExW(0,name,name,0,0,0,0,0,None,None,instance,None)
        self.ready.set()
        msg=W.MSG()
        while U.GetMessageW(C.byref(msg),None,0,0)>0:
            U.TranslateMessage(C.byref(msg));U.DispatchMessageW(C.byref(msg))

    def send(self,session,command,text=''):
        session.check()
        buffer=C.create_unicode_buffer(text)
        data=CopyData(command,C.sizeof(buffer),C.cast(buffer,C.c_void_p).value)
        return U.SendMessageW(session.hwnd,0x4a,self.hwnd,C.addressof(data))

    def close(self):
        U.PostMessageW(self.hwnd,0x10,0,0);self.thread.join(5)


def vlc(session,command):
    session.check()
    session.rc_socket.sendall((command+'\n').encode('utf-8'))


def seek_target_seconds(percent):
    # Stay inside the tone interval and halfway between 30 fps frame boundaries.
    # Native pixel rounding must not add a whole frame or wait for the next tone.
    return 12*percent/100 + 1/60


def towavue_seek_point(width,height,dpi,target_seconds,duration_seconds):
    radius=5*dpi/96
    travel=width-2*radius
    if not (travel>0 and math.isfinite(duration_seconds) and 0<target_seconds<duration_seconds):
        raise ValueError('Invalid native seek geometry or duration')
    bound=duration_seconds/(2*travel)
    if bound>=1/60:
        raise ValueError('Seekbar is too narrow for a common 30 fps target frame')
    x=round(radius+travel*target_seconds/duration_seconds)
    actual=(x-radius)/travel*duration_seconds
    if math.ceil(actual*30)!=math.ceil(target_seconds*30):
        raise ValueError('Native pixel rounding crosses the common target frame')
    return x,round(height-30*dpi/96),actual,bound


def seek(session,percent,receiver=None,duration_seconds=12.0):
    seconds=seek_target_seconds(percent)
    details=dict(requested_seconds=seconds)
    if session.name=='towavue':
        client,_,_=geometry(session.hwnd)
        U.GetDpiForWindow.argtypes=[W.HWND];U.GetDpiForWindow.restype=W.UINT
        dpi=U.GetDpiForWindow(session.hwnd)
        if not dpi:raise RuntimeError('Cannot read fixture window DPI')
        x,y,actual,bound=towavue_seek_point(client.right,client.bottom,dpi,seconds,duration_seconds)
        details.update(click_x=x,click_y=y,dpi=dpi,estimated_seconds=actual,pixel_rounding_bound_seconds=bound,duration_seconds=duration_seconds)
        native_input.click(session,x,y)
    elif session.name=='mpc-hc':receiver.send(session,0xA0002000,f'{seconds:.6f}')
    elif session.name=='vlc':vlc(session,f'seek {seconds:.6f}')
    return details
