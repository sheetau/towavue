import types
import unittest
from unittest.mock import Mock, patch

import native_input as native


class NativeInputTests(unittest.TestCase):
    def setUp(self):
        self.position=(1000,900)
        self.desktop=(-1920,-200,3840,1440)
        self.deliveries=[4]
        self.sent=[]
        self.api=types.SimpleNamespace(
            GetForegroundWindow=Mock(return_value=123),
            GetAsyncKeyState=Mock(return_value=0),
            GetAncestor=Mock(return_value=123),
            WindowFromPoint=Mock(return_value=123),
            GetSystemMetrics=Mock(side_effect=lambda n:self.desktop[n-76]),
            GetCursorPos=Mock(side_effect=self.get_cursor),
            SetCursorPos=Mock(side_effect=self.set_cursor),
            SendInput=Mock(side_effect=self.send),
            mouse_event=Mock(),
        )
        self.session=types.SimpleNamespace(hwnd=123,check=Mock())
        geometry=(types.SimpleNamespace(right=800,bottom=650),None,types.SimpleNamespace(x=78,y=121))
        self.patches=[patch.object(native,'U',self.api),patch.object(native,'geometry',return_value=geometry)]
        for replacement in self.patches:replacement.start()
        self.addCleanup(lambda:[replacement.stop() for replacement in reversed(self.patches)])

    def get_cursor(self,point):
        point._obj.x,point._obj.y=self.position
        return 1

    def set_cursor(self,x,y):
        self.position=(x,y)
        return 1

    def send(self,count,events,size):
        self.assertEqual(size,40)
        inserted=self.deliveries.pop(0)
        self.sent.append([(events[i].data.mouse.dx,events[i].data.mouse.dy,events[i].data.mouse.flags) for i in range(count)])
        for x,y,flags in self.sent[-1][:inserted]:
            if flags&1:
                left,top,width,height=self.desktop
                self.position=(x*width//65536+left,y*height//65536+top)
        return inserted

    def test_foreign_focus_held_input_and_foreign_point_refuse_injection(self):
        for condition in ('focus','held','point'):
            with self.subTest(condition=condition):
                self.api.GetForegroundWindow.return_value=456 if condition=='focus' else 123
                self.api.GetAsyncKeyState.return_value=0x8000 if condition=='held' else 0
                self.api.GetAncestor.return_value=456 if condition=='point' else 123
                with self.assertRaises(RuntimeError):native.click(self.session,164,620)
                self.api.SendInput.assert_not_called()
                self.api.SetCursorPos.assert_not_called()

    def test_native_batch_preserves_target_pixel_and_restores_only_once(self):
        native.click(self.session,164,620)
        self.assertEqual([r[2] for r in self.sent[0]],[0xe001,2,4,0xe001])
        self.assertEqual(self.session.native_input['target'],(242,741))
        self.assertEqual(self.position,(478,171))
        native.restore(self.session)
        self.assertEqual(self.position,(1000,900))
        self.assertTrue(self.session.native_input['restored'])
        self.position=(478,171)  # A later user movement must not be undone by cleanup.
        native.restore(self.session)
        self.assertEqual(self.position,(478,171))
        self.assertEqual(self.api.SetCursorPos.call_count,1)

    def test_user_movement_after_dispatch_is_retained_and_invalidates_restore(self):
        native.click(self.session,164,620)
        self.position=(1500,700)
        native.restore(self.session)
        self.assertFalse(self.session.native_input['restored'])
        self.assertEqual(self.session.native_input['cursor_changed'],(1500,700))
        self.api.SetCursorPos.assert_not_called()

    def test_partial_native_press_is_released_and_failure_is_retained(self):
        for release_count in (1,0):
            with self.subTest(release_count=release_count):
                self.deliveries=[2,release_count]
                self.sent=[]
                self.position=(1000,900)
                with self.assertRaises(RuntimeError):native.click(self.session,164,620)
                self.assertEqual(self.sent[-1],[(0,0,4)])
                self.assertEqual(self.position,(1000,900))
                self.assertEqual(self.session.native_input['delivered'],2)
                if release_count==0:self.api.mouse_event.assert_called_once_with(4,0,0,0,0)


if __name__=='__main__':unittest.main()
