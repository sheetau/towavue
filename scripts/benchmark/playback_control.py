"""Documented MPC-HC WM_COPYDATA and VLC RC controls for owned sessions."""
import ctypes as C
from ctypes import wintypes as W
import threading
import time
from interaction import U, geometry


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


def seek(session,percent,receiver=None):
    if session.name=='towavue':
        client,_,_=geometry(session.hwnd)
        session.click(round(client.right*percent/100),client.bottom-30)
    elif session.name=='mpc-hc':receiver.send(session,0xA0002000,f'{12*percent/100:.3f}')
    elif session.name=='vlc':vlc(session,f'seek {12*percent/100:.3f}')
