import unittest
import numpy as np
from prepare import pattern
from seek import frame_number, tone_match


class SeekDetectorTests(unittest.TestCase):
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
