import types
import unittest
from unittest.mock import Mock, patch

import photos_navigation as photos


class PhotosNavigationTests(unittest.TestCase):
    def test_missing_reordered_and_duplicate_membership_is_rejected(self):
        for expected, observed in ((["a", "b"], ["b", "a"]),
                                   (["a", "b"], ["a"]),
                                   (["a", "a"], ["a", "a"])):
            with self.subTest(expected=expected, observed=observed):
                with self.assertRaises(RuntimeError):
                    photos.validate_order(expected, observed)

    def test_virtualized_prefix_requires_explicit_scope_and_verified_neighbors(self):
        names = [f"{i:04d}.jpg" for i in range(1000)]
        with self.assertRaises(RuntimeError):
            photos.validate_order(names, names[:41])
        photos.validate_order(names, names[:41], allow_virtualized=True)
        for prefix in (names[:8], names[1:42], names[:10][::-1]):
            with self.assertRaises(RuntimeError):
                photos.validate_order(names, prefix, allow_virtualized=True)

    def test_foreign_focus_or_held_input_never_receives_keyboard_injection(self):
        session = types.SimpleNamespace(hwnd=123, check=Mock())
        for foreground, held in ((456, 0), (123, 0x8000)):
            api = types.SimpleNamespace(GetForegroundWindow=Mock(return_value=foreground),
                  GetAsyncKeyState=Mock(return_value=held), SendInput=Mock())
            with patch.object(photos, "U", api), self.assertRaises(RuntimeError):
                photos.native_key(session, 0x23)
            api.SendInput.assert_not_called()

    def test_partial_keyboard_press_is_released_and_trial_still_fails(self):
        session = types.SimpleNamespace(hwnd=123, check=Mock())
        sent = []

        def inject(count, events, size):
            self.assertEqual(size, 40)
            sent.append([(events[i].data.keyboard.vk, events[i].data.keyboard.flags) for i in range(count)])
            return 1

        api = types.SimpleNamespace(GetForegroundWindow=Mock(return_value=123),
              GetAsyncKeyState=Mock(return_value=0), SendInput=Mock(side_effect=inject), keybd_event=Mock())
        with patch.object(photos, "U", api), self.assertRaises(RuntimeError):
            photos.native_key(session, 0x23)
        self.assertEqual(sent, [[(0x23, 1), (0x23, 3)], [(0x23, 3)]])
        api.keybd_event.assert_not_called()


if __name__ == "__main__":
    unittest.main()
