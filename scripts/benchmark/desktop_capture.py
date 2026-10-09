"""Desktop pixels for a guarded rectangle on the primary screen."""
import ctypes as C
from ctypes import wintypes as W
from PIL import Image

# Retain the GIL, as the pinned Pillow native screen-grab implementation does.
G=C.PyDLL('gdi32',use_last_error=True)
U=C.PyDLL('user32',use_last_error=True)
H=C.c_void_p;I=C.c_int
def bind(lib,name,result,args):
    f=getattr(lib,name);f.restype=result;f.argtypes=args;return f
create_dc=bind(G,'CreateDCA',H,[C.c_char_p,H,H,H])
compatible_dc=bind(G,'CreateCompatibleDC',H,[H])
compatible_bitmap=bind(G,'CreateCompatibleBitmap',H,[H,I,I])
select=bind(G,'SelectObject',H,[H,H])
bitblt=bind(G,'BitBlt',I,[H,I,I,I,I,H,I,I,W.DWORD])
get_bits=bind(G,'GetDIBits',I,[H,H,W.UINT,W.UINT,H,H,W.UINT])
delete_object=bind(G,'DeleteObject',I,[H])
delete_dc=bind(G,'DeleteDC',I,[H])
device_caps=bind(G,'GetDeviceCaps',I,[H,I])
set_dpi=bind(U,'SetThreadDpiAwarenessContext',H,[H])

class Core(C.Structure):
    _fields_=[('size',W.DWORD),('width',W.WORD),('height',W.WORD),('planes',W.WORD),('bits',W.WORD)]
assert C.sizeof(Core)==12
INVALID=C.c_void_p(-1).value

def grab(bbox):
    left,top,right,bottom=bbox
    width,height=right-left,bottom-top
    if not (0<=left<right and 0<=top<bottom and 0<width<=65535 and 0<height<=65535):
        raise ValueError('Invalid desktop rectangle')
    screen=memory=bitmap=old=None
    try:
        screen=create_dc(b'DISPLAY',None,None,None)
        if not screen:raise C.WinError(C.get_last_error())
        memory=compatible_dc(screen)
        if not memory:raise C.WinError(C.get_last_error())
        previous=set_dpi(C.c_void_p(-3))
        if not previous:raise C.WinError(C.get_last_error())
        try:screen_width,screen_height=device_caps(screen,8),device_caps(screen,10)
        finally:
            if not set_dpi(previous):raise C.WinError(C.get_last_error())
        # The original observer normalizes into the primary screen. Refuse an
        # out-of-primary rectangle instead of silently capturing another output.
        if right>screen_width or bottom>screen_height:raise ValueError('Capture rectangle exceeds primary screen')
        bitmap=compatible_bitmap(screen,width,height)
        if not bitmap:raise C.WinError(C.get_last_error())
        old=select(memory,bitmap)
        if not old or old==INVALID:raise C.WinError(C.get_last_error())
        if not bitblt(memory,0,0,width,height,screen,left,top,0x00CC0020):
            raise C.WinError(C.get_last_error())
        # GetDIBits requires the bitmap to be unselected. Retain the memory DC
        # and bitmap owners until extraction completes; restore on failures too.
        restored=select(memory,old)
        if not restored or restored==INVALID:raise C.WinError(C.get_last_error())
        old=None
        stride=(width*3+3)&-4
        buffer=C.create_string_buffer(stride*height)
        header=Core(12,width,height,1,24)
        rows=get_bits(memory,bitmap,0,height,buffer,C.byref(header),0)
        if rows!=height:raise RuntimeError('Incomplete desktop readback')
        picture=Image.frombytes('RGB',(width,height),buffer.raw,'raw','BGR',stride,-1)
        return picture
    finally:
        # Attempt every release even if one fails. Destroying the memory DC
        # first also removes its selection before retiring our owned bitmap.
        errors=[]
        if old and old!=INVALID:
            restored=select(memory,old)
            if not restored or restored==INVALID:errors.append('restore bitmap')
        if memory and not delete_dc(memory):errors.append('delete memory DC')
        if bitmap and not delete_object(bitmap):errors.append('delete bitmap')
        if screen and not delete_dc(screen):errors.append('delete screen DC')
        if errors:raise RuntimeError('Capture cleanup failed: '+', '.join(errors))
