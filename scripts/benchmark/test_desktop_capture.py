import contextlib
import ctypes as C
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import desktop_capture as capture
import interaction


@contextlib.contextmanager
def native_bindings(**overrides):
    functions = {
        'create_dc': Mock(return_value=100),
        'compatible_dc': Mock(return_value=200),
        'compatible_bitmap': Mock(return_value=300),
        'select': Mock(side_effect=[400, 300]),
        'bitblt': Mock(return_value=1),
        'get_bits': Mock(return_value=2),
        'delete_dc': Mock(return_value=1),
        'delete_object': Mock(return_value=1),
        'device_caps': Mock(side_effect=lambda dc, key: 1920 if key == 8 else 1080),
        'set_dpi': Mock(side_effect=[-2, -3]),
    }
    functions.update(overrides)
    with contextlib.ExitStack() as stack:
        for name, function in functions.items():
            stack.enter_context(patch.object(capture, name, function))
        yield functions


class DesktopCaptureTests(unittest.TestCase):
    def test_bgr_bottom_up_rows_preserve_rgb_and_ignore_stride_padding(self):
        def read(dc, bitmap, first, height, buffer, header, usage):
            self.assertEqual(height, 2)
            self.assertEqual(C.cast(header, C.POINTER(capture.Core)).contents.width, 3)
            data = bytes([60, 50, 40] * 3 + [255] * 3 + [30, 20, 10] * 3 + [255] * 3)
            C.memmove(buffer, data, len(data))
            return 2
        with native_bindings(get_bits=Mock(side_effect=read)) as api:
            image = capture.grab((70, 70, 73, 72))
            self.assertEqual(image.tobytes(), bytes([10, 20, 30] * 3 + [40, 50, 60] * 3))
            self.assertEqual(api['set_dpi'].call_args_list[-1].args, (-2,))
            api['delete_object'].assert_called_once_with(300)
            self.assertEqual([c.args for c in api['delete_dc'].call_args_list], [(200,), (100,)])

    def test_native_failures_release_only_acquired_objects(self):
        cases = [
            ('create_dc', 0, [], []),
            ('compatible_dc', 0, [100], []),
            ('set_dpi', 0, [200, 100], []),
            ('compatible_bitmap', 0, [200, 100], []),
            ('select', 0, [200, 100], [300]),
            ('bitblt', 0, [200, 100], [300]),
            ('get_bits', 1, [200, 100], [300]),
        ]
        for name, value, dcs, bitmaps in cases:
            with self.subTest(stage=name), native_bindings(**{name: Mock(return_value=value)}) as api:
                with self.assertRaises((OSError, RuntimeError)):
                    capture.grab((70, 70, 73, 72))
                self.assertEqual([c.args[0] for c in api['delete_dc'].call_args_list], dcs)
                self.assertEqual([c.args[0] for c in api['delete_object'].call_args_list], bitmaps)

    def test_cleanup_failure_does_not_skip_remaining_releases(self):
        with native_bindings(delete_dc=Mock(side_effect=[0, 1])) as api:
            with self.assertRaisesRegex(RuntimeError, 'delete memory DC'):
                capture.grab((70, 70, 73, 72))
            api['delete_object'].assert_called_once_with(300)
            self.assertEqual([c.args for c in api['delete_dc'].call_args_list], [(200,), (100,)])

    def test_invalid_or_outside_primary_rectangles_are_refused(self):
        with native_bindings() as api:
            with self.assertRaises(ValueError):
                capture.grab((-1, 0, 2, 2))
            api['create_dc'].assert_not_called()
        with native_bindings() as api:
            with self.assertRaisesRegex(ValueError, 'primary screen'):
                capture.grab((1919, 0, 1922, 2))
            api['compatible_bitmap'].assert_not_called()
            self.assertEqual(api['set_dpi'].call_args_list[-1].args, (-2,))
            self.assertEqual(api['delete_dc'].call_count, 2)


class OwnedCaptureTests(unittest.TestCase):
    def session(self):
        session = interaction.Session.__new__(interaction.Session)
        session.hwnd = 123
        session.process = SimpleNamespace(pid=9)
        session.check = Mock()
        return session

    @contextlib.contextmanager
    def environment(self, owner=9):
        def ownership(hwnd, output):
            output._obj.value = owner
            return 1
        with patch.object(interaction, 'geometry', return_value=(interaction.W.RECT(0, 0, 800, 650), None, interaction.W.POINT(70, 70))), \
             patch.object(interaction.U, 'WindowFromPoint', return_value=123), \
             patch.object(interaction.U, 'GetWindowThreadProcessId', side_effect=ownership), \
             patch.object(interaction, 'grab_desktop_roi', return_value='roi') as roi, \
             patch.object(interaction.ImageGrab, 'grab', return_value='pillow') as pillow:
            yield roi, pillow

    def test_other_runners_keep_the_original_capture_default(self):
        with self.environment() as (roi, pillow):
            self.assertEqual(self.session().capture(), 'pillow')
            roi.assert_not_called()
            pillow.assert_called_once_with(bbox=(70, 70, 870, 720))

    def test_startup_roi_receives_only_the_checked_rectangle(self):
        with self.environment() as (roi, pillow):
            self.assertEqual(self.session().capture(desktop_roi=True), 'roi')
            roi.assert_called_once_with((70, 70, 870, 720))
            pillow.assert_not_called()

    def test_occlusion_blocks_both_pixel_acquisition_paths(self):
        for mode in [False, True]:
            with self.subTest(roi=mode), self.environment(owner=10) as (roi, pillow):
                with self.assertRaises(interaction.CaptureUnavailable):
                    self.session().capture(desktop_roi=mode)
                roi.assert_not_called()
                pillow.assert_not_called()


if __name__ == '__main__':
    unittest.main()
