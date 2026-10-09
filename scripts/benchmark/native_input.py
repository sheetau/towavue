"""Owned native seek gesture for external benchmarks."""
import ctypes as C
from visual import U, W, geometry

class MouseInput(C.Structure):
    _fields_=[('dx',W.LONG),('dy',W.LONG),('data',W.DWORD),('flags',W.DWORD),
              ('time',W.DWORD),('extra',C.c_size_t)]

class InputData(C.Union):
    _fields_=[('mouse',MouseInput)]

class Input(C.Structure):
    _fields_=[('kind',W.DWORD),('data',InputData)]

assert C.sizeof(Input)==40

def cursor():
    U.GetCursorPos.argtypes=[C.POINTER(W.POINT)]
    point=W.POINT()
    if not U.GetCursorPos(C.byref(point)):raise C.WinError(C.get_last_error())
    return point.x,point.y

def click(session,x,y):
    session.check()
    U.GetAsyncKeyState.argtypes=[C.c_int];U.GetAsyncKeyState.restype=C.c_short
    if U.GetForegroundWindow()!=session.hwnd or any(U.GetAsyncKeyState(key)&0x8000 for key in (1,2,4,0x10,0x11,0x12,0x5b,0x5c)):
        raise RuntimeError('Native input requires owned foreground and released physical input')
    client,outer,origin=geometry(session.hwnd)
    if (client.right,client.bottom)!=(800,650):raise RuntimeError('Native input client changed')
    target=(origin.x+int(x),origin.y+int(y))
    neutral=(origin.x+client.right//2,origin.y+50)
    for point in (target,neutral):
        if U.GetAncestor(U.WindowFromPoint(W.POINT(*point)),2)!=session.hwnd:
            raise RuntimeError('Native input point is not owned')
    U.GetSystemMetrics.argtypes=[C.c_int];U.GetSystemMetrics.restype=C.c_int
    left,top,width,height=[U.GetSystemMetrics(n) for n in (76,77,78,79)]
    if width<=0 or height<=0:raise RuntimeError('Invalid virtual screen')
    def move(point):
        px,py=point
        if not(left<=px<left+width and top<=py<top+height):raise RuntimeError('Native input point off screen')
        # Aim at the center of the pixel's absolute-coordinate interval.
        ax=((px-left)*65536+32768)//width
        ay=((py-top)*65536+32768)//height
        if (ax*width//65536+left,ay*height//65536+top)!=point:
            raise RuntimeError('Native absolute input cannot preserve target pixel')
        return Input(0,InputData(mouse=MouseInput(ax,ay,0,0x8000|0x4000|0x2000|1,0,0)))
    events=(Input*4)(move(target),Input(0,InputData(mouse=MouseInput(0,0,0,2,0,0))),
        Input(0,InputData(mouse=MouseInput(0,0,0,4,0,0))),move(neutral))
    previous=cursor()
    U.SendInput.argtypes=[W.UINT,C.POINTER(Input),C.c_int];U.SendInput.restype=W.UINT
    delivered=U.SendInput(4,events,C.sizeof(Input))
    session.native_input=dict(previous=previous,target=target,neutral=neutral,delivered=delivered,
        virtual_screen=[left,top,width,height],transport='SendInput move/down/up/neutral, no coalescing')
    if delivered!=4:
        if delivered==2:
            release=(Input*1)(Input(0,InputData(mouse=MouseInput(0,0,0,4,0,0))))
            if U.SendInput(1,release,C.sizeof(Input))!=1:
                U.mouse_event.argtypes=[W.DWORD,W.DWORD,W.DWORD,W.DWORD,C.c_size_t]
                U.mouse_event(4,0,0,0,0)
        restore(session)
        raise RuntimeError('Native input was not completely delivered')

def restore(session):
    record=getattr(session,'native_input',None)
    if record is None or record.get('restore_checked'):return
    record['restore_checked']=True
    current=cursor()
    if current in (tuple(record['neutral']),tuple(record['target'])):
        U.SetCursorPos.argtypes=[C.c_int,C.c_int]
        record['restored']=bool(U.SetCursorPos(*record['previous']))
    else:
        record['restored']=False
        record['cursor_changed']=current
