import unittest

import numpy as np

from audio import matches, tone_score


class ToneTests(unittest.TestCase):
    def test_tone_survives_phase_and_packet_length(self):
        for frames in (240, 480, 960, 2048):
            tone = .01 * np.sin(2 * np.pi * 997 * np.arange(frames) / 48000 + .7)
            self.assertTrue(matches(*tone_score(np.column_stack((tone, tone)))))

    def test_silence_noise_and_wrong_frequency_do_not_match(self):
        self.assertFalse(matches(*tone_score(np.zeros((480, 2)))))
        noise = np.random.default_rng(10405).normal(0, .02, (960, 2))
        self.assertFalse(matches(*tone_score(noise)))
        tone = .01 * np.sin(2 * np.pi * 440 * np.arange(960) / 48000)
        self.assertFalse(matches(*tone_score(np.column_stack((tone, tone)))))

    def test_manifest_frequency_identifies_another_file(self):
        for frequency in (701, 880, 1059, 1238, 1417):
            tone = .01 * np.sin(2 * np.pi * frequency * np.arange(960) / 48000)
            data = np.column_stack((tone, tone))
            self.assertTrue(matches(*tone_score(data, frequency=frequency)))
            self.assertFalse(matches(*tone_score(data, frequency=frequency + 179)))


if __name__ == "__main__":
    unittest.main()
