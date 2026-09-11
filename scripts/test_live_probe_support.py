import json
import unittest

from live_probe_support import PeerEvents, SpeechEvidence, final_usage_report, pcm16_mono


class EvidenceTests(unittest.TestCase):
    def test_silence_and_impulses_never_qualify_at_any_media_rate(self):
        for rate in (8000, 16000, 24000, 48000):
            for amplitude in (0, 1000, 32767, -32768):
                speech = SpeechEvidence()
                frame = [amplitude] + [0] * (rate // 50 - 1)
                for _ in range(20):
                    speech.add(frame, rate)
                self.assertFalse(speech.qualified)
            speech = SpeechEvidence()
            samples = [1000] * (rate // 5)
            for offset in range(0, len(samples), 17):
                speech.add(samples[offset:offset + 17], rate)
            self.assertTrue(speech.qualified)
            self.assertEqual(speech.report()["voiced_ms"], 200)

    def test_stereo_planar_and_padding_do_not_double_media_duration(self):
        rate, frames = 48000, 4800
        sample = (-32768).to_bytes(2, "little", signed=True)
        padding = sample * 4800
        for planes, channels, planar in [
            ([sample * frames + padding], 1, False),
            ([sample * frames * 2 + padding], 2, False),
            ([sample * frames + padding, sample * frames + padding], 2, True),
        ]:
            mono = pcm16_mono(planes, frames, channels, planar)
            self.assertEqual(len(mono), frames)
            self.assertEqual(mono[0], -32768)
            speech = SpeechEvidence()
            speech.add(mono, rate)
            self.assertFalse(speech.qualified)
            self.assertEqual(speech.report()["voiced_ms"], 100)
            speech.add(mono, rate)
            self.assertTrue(speech.qualified)
            self.assertEqual(speech.report()["voiced_ms"], 200)

    def test_only_exact_correlated_permission_denial_is_expected(self):
        error = {"type": "invalid_request_error", "code": "event_not_allowed",
                 "client_event_id": "browser-restricted", "message": "synthetic denial"}
        events = PeerEvents(True)
        events.observe(json.dumps({"type": "error", "event_id": "e", "error": error}))
        self.assertTrue(events.permission_denied)
        events.check()
        for field in ("type", "code", "client_event_id"):
            changed = dict(error, **{field: "unrelated"})
            events = PeerEvents(True)
            events.observe(json.dumps({"type": "error", "event_id": "e", "error": changed}))
            with self.assertRaises(RuntimeError):
                events.check()
        events = PeerEvents(False)
        events.observe(json.dumps({"type": "error", "event_id": "e", "error": error}))
        with self.assertRaises(RuntimeError):
            events.check()
        for missing in ("message", "type", "code", "client_event_id"):
            changed = dict(error)
            del changed[missing]
            events = PeerEvents(True)
            events.observe(json.dumps({"type": "error", "event_id": "e", "error": changed}))
            with self.assertRaises(RuntimeError):
                events.check()

    def test_late_errors_remain_failures_after_valid_closed_usage(self):
        closed = json.dumps({"type": "session.closed", "event_id": "closed", "reason": "close_requested",
                             "session": {"id": "s", "model": "gpt-live-1",
                                         "status": "active", "expires_at": 1},
                             "usage": {"seconds": 1}})
        for invalid in ('{', '{"type":"error"}', '{"type":"session.closed"}',
                        '{"type":"session.output_transcript.delta","delta":"x"}',
                        '{"type":"unknown","type":"unknown"}', '{"type":"unknown","x":NaN}',
                        '{"type":"error","error":{"type":"invalid_request_error","code":null}}',
                        '{"type":"transport.failed","event_id":"failed"}'):
            for before in (True, False):
                events = PeerEvents(True)
                for message in ((invalid, closed) if before else (closed, invalid)):
                    events.observe(message)
                self.assertTrue(events.closed)
                with self.assertRaises(RuntimeError):
                    events.check()

    def test_permission_exception_validates_optional_fields_before_whitelisting(self):
        error = {"type": "invalid_request_error", "code": "event_not_allowed",
                 "message": "synthetic denial", "client_event_id": "browser-restricted"}
        for invalid in (None, 123, {}, [], True):
            events = PeerEvents(True)
            events.observe(json.dumps({"type": "error", "event_id": "e",
                                       "client_event_id": invalid, "error": error}))
            self.assertFalse(events.permission_denied)
            with self.assertRaises(RuntimeError):
                events.check()
        for invalid in ({}, [], 123, True):
            events = PeerEvents(True)
            events.observe(json.dumps({"type": "error", "event_id": "e",
                                       "error": dict(error, param=invalid)}))
            self.assertFalse(events.permission_denied)
            with self.assertRaises(RuntimeError):
                events.check()
        for valid in (None, "session.client"):
            events = PeerEvents(True)
            events.observe(json.dumps({"type": "error", "event_id": "e", "client_event_id": "outer",
                                       "future": {"retained": True},
                                       "error": dict(error, param=valid, future={"any": [None, 1]})}))
            self.assertTrue(events.permission_denied)
            events.check()
        events = PeerEvents(True)
        events.observe(json.dumps({"type": "error", "event_id": "e",
                                   "client_event_id": "browser-restricted",
                                   "error": dict(error, client_event_id="unrelated")}))
        with self.assertRaises(RuntimeError):
            events.check()

    def test_safe_final_usage_report_never_relays_unrelated_content(self):
        report = final_usage_report(b'private text\n{"final_usage_confirmed":true,"final_seconds":2,"sdp":"private"}')
        self.assertEqual(report, {"rust_final_usage_confirmed": True, "final_seconds": 2})


if __name__ == "__main__":
    unittest.main()
