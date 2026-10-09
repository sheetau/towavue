import unittest
import numpy as np
from prepare import pattern
from seek import frame_number, tone_match
from playback_control import seek_target_seconds, towavue_seek_point
import math


class SeekDetectorTests(unittest.TestCase):
    def test_native_seek_and_api_targets_select_the_same_frame_and_tone(self):
        for dpi in (96,120,144,192):
            for duration in (12.0,12.008,12.0065):
                for percent in (70,20,80,30,60,10):
                    target=seek_target_seconds(percent)
                    x,y,actual,bound=towavue_seek_point(800,650,dpi,target,duration)
                    self.assertLessEqual(abs(actual-target),bound+1e-9)
                    self.assertEqual(math.ceil(actual*30),math.ceil(target*30))
                    self.assertEqual(int(actual/1.2),percent//10)
                    self.assertEqual(y,round(650-30*dpi/96))
                    self.assertGreater(x,0);self.assertLess(x,800)

    def test_historical_full_width_click_would_wait_for_a_later_tone(self):
        target=1.2
        historical=(80-5)/790*12
        self.assertLess(historical,target-.06)
        _,_,actual,_=towavue_seek_point(800,650,96,seek_target_seconds(10),12)
        self.assertGreater(actual,target)
        with self.assertRaises(ValueError):
            towavue_seek_point(100,650,96,seek_target_seconds(10),12)

    def test_frame_identity_is_not_old_static_identity(self):
        for identity in (1,101,1001):
            self.assertIsNone(frame_number(pattern(800,450,identity)))
        for index in (0,36,72,108,216,252,288,359):
            self.assertEqual(frame_number(pattern(800,450,3000+index))[0],index)

    def test_audio_rejects_previous_position_and_silence(self):
        t=np.arange(480)/48000
        for segment in range(10):
            frequency=600+137*segment
            packet=np.repeat((.01*np.sin(2*np.pi*frequency*t))[:,None],2,axis=1)
            rms,fraction=tone_match(packet,frequency)
            self.assertGreater(rms,.0002);self.assertGreater(fraction,.99)
            for other in range(10):
                if other!=segment:self.assertLess(tone_match(packet,600+137*other)[1],.8)
        self.assertEqual(tone_match(np.zeros((480,2)),600),(0.,0.))


if __name__=='__main__':unittest.main()
