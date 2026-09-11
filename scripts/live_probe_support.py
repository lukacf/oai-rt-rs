"""Pure evidence checks shared by the real-peer probe and deterministic tests."""

import array
import json
import math
import sys


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result


def reject_constant(_):
    raise ValueError("nonfinite JSON constant")


def strict_json(message):
    return json.loads(message, object_pairs_hook=unique_object, parse_constant=reject_constant)


def number(value):
    return type(value) in (int, float) and math.isfinite(value)


def pcm16_mono(raw_planes, frames, channels, planar):
    if type(frames) is not int or frames < 0 or type(channels) is not int or channels < 1:
        raise ValueError("invalid decoded audio dimensions")
    planes = []
    count = frames * (1 if planar else channels)
    for raw in raw_planes:
        samples = array.array("h")
        samples.frombytes(raw[:count * 2])
        if sys.byteorder != "little":
            samples.byteswap()
        if len(samples) != count:
            raise ValueError("truncated decoded audio plane")
        planes.append(samples)
    if planar:
        if len(planes) != channels:
            raise ValueError("wrong planar channel count")
        return [sum(plane[index] for plane in planes) // channels for index in range(frames)]
    if len(planes) != 1:
        raise ValueError("wrong interleaved plane count")
    return [sum(planes[0][index:index + channels]) // channels
            for index in range(0, len(planes[0]), channels)]


class SpeechEvidence:
    def __init__(self):
        self.rate = 0
        self.samples = self.active = self.energy = 0
        self.voiced_windows = self.run = self.longest_run = self.active_samples = 0

    def add(self, samples, rate):
        if type(rate) is not int or rate < 50 or rate % 50 or self.rate not in (0, rate):
            raise ValueError("invalid or changed decoded sample rate")
        self.rate = rate
        for sample in samples:
            if type(sample) is not int or not -32768 <= sample <= 32767:
                raise ValueError("invalid decoded PCM16 sample")
            self.samples += 1
            self.energy += sample * sample
            if abs(sample) >= 500:
                self.active += 1
                self.active_samples += 1
            if self.samples == rate // 50:
                if self.active * 10 >= self.samples and self.energy >= self.samples * 300 * 300:
                    self.voiced_windows += 1
                    self.run += 1
                    self.longest_run = max(self.longest_run, self.run)
                else:
                    self.run = 0
                self.samples = self.active = self.energy = 0

    @property
    def qualified(self):
        return self.voiced_windows >= 10 and self.longest_run >= 5

    def report(self):
        return dict(sample_rate=self.rate, voiced_ms=self.voiced_windows * 20,
                    continuous_voiced_ms=self.longest_run * 20, active_samples=self.active_samples)


class PeerEvents:
    def __init__(self, restricted):
        self.restricted = restricted
        self.permission_denied = False
        self.closed = False
        self.transcript = ""
        self.first_error = None

    def fail(self, reason):
        if self.first_error is None:
            self.first_error = reason

    def observe(self, message):
        try:
            if len(message) > 1024 * 1024:
                raise ValueError("probe event byte budget exceeded")
            event = strict_json(message)
            if not isinstance(event, dict) or not isinstance(event.get("type"), str):
                raise ValueError("missing event discriminator")
            kind = event["type"]
            if kind == "error":
                error = event.get("error")
                if not isinstance(error, dict):
                    raise ValueError("malformed provider error")
                expected = (self.restricted
                            and error.get("type") == "invalid_request_error"
                            and error.get("code") == "event_not_allowed"
                            and error.get("client_event_id") == "browser-restricted")
                if expected:
                    self.permission_denied = True
                else:
                    self.fail("unexpected data-channel provider error")
            elif kind == "session.output_transcript.delta":
                if (not isinstance(event.get("delta"), str)
                        or not all(number(event.get(field)) for field in ("start_ms", "end_ms"))):
                    raise ValueError("malformed transcript")
                if len(self.transcript) + len(event["delta"]) > 65536:
                    raise ValueError("probe transcript budget exceeded")
                self.transcript += event["delta"]
            elif kind == "session.closed":
                session = event.get("session", {})
                if (not isinstance(session, dict)
                        or not isinstance(session.get("id"), str)
                        or not isinstance(session.get("model"), str)
                        or session.get("status") != "active"
                        or not number(session.get("expires_at"))
                        or not isinstance(event.get("usage"), dict)
                        or not number(event["usage"].get("seconds"))
                        or event.get("reason") not in {
                            "close_requested", "expired", "content", "remote_hangup", "connection_lost"
                        }):
                    raise ValueError("malformed final usage")
                self.closed = True
            elif kind == "response.event":
                nested = event.get("event")
                if not isinstance(nested, dict) or not isinstance(nested.get("type"), str):
                    raise ValueError("malformed nested event")
        except (ValueError, TypeError, OverflowError):
            self.fail("malformed data-channel event")

    def check(self):
        if self.first_error is not None:
            raise RuntimeError(self.first_error)


def final_usage_report(stderr):
    report = None
    for line in stderr.splitlines():
        try:
            value = strict_json(line)
            if (isinstance(value, dict) and value.get("final_usage_confirmed") is True
                    and number(value.get("final_seconds"))):
                report = {"rust_final_usage_confirmed": True, "final_seconds": value["final_seconds"]}
        except (ValueError, TypeError, OverflowError):
            continue
    return report
