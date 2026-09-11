#!/usr/bin/env python3
"""Real-peer smoke for the Rust live_webrtc_smoke example.

Requires aiortc and av. Only synthetic silence is sent. Raw media, SDP,
credentials, provider IDs and transcripts are never persisted or printed.
"""

import argparse
import asyncio
import json
import sys
import time
from fractions import Fraction

from aiortc import AudioStreamTrack, RTCConfiguration, RTCPeerConnection, RTCSessionDescription
from aiortc.mediastreams import MediaStreamError
from av import AudioFrame

from live_probe_support import PeerEvents, SpeechEvidence, final_usage_report, pcm16_mono, strict_json


class Silence(AudioStreamTrack):
    def __init__(self):
        super().__init__()
        self.samples_sent = 0
        self.started = time.monotonic()

    async def recv(self):
        await asyncio.sleep(max(0, self.started + self.samples_sent / 48000 - time.monotonic()))
        frame = AudioFrame(format="s16", layout="mono", samples=960)
        for plane in frame.planes:
            plane.update(bytes(plane.buffer_size))
        frame.sample_rate = 48000
        frame.time_base = Fraction(1, 48000)
        frame.pts = self.samples_sent
        self.samples_sent += 960
        return frame


def mono_samples(frame):
    if frame.format.name not in ("s16", "s16p"):
        raise ValueError("unsupported decoded audio format")
    return pcm16_mono([bytes(plane) for plane in frame.planes], frame.samples,
                      len(frame.layout.channels), frame.format.is_planar)


async def probe(binary, restrict_browser, fork):
    peer = RTCPeerConnection(RTCConfiguration(iceServers=[]))
    peer.addTrack(Silence())
    channel = peer.createDataChannel("oai-events")
    opened, closed, voiced = asyncio.Event(), asyncio.Event(), asyncio.Event()
    transcript_ready, permission_rejected, failed = asyncio.Event(), asyncio.Event(), asyncio.Event()
    evidence = PeerEvents(restrict_browser)
    speech = SpeechEvidence()
    state = {"frames": 0, "peak": 0, "closing": False}
    tasks = []
    process = None
    result = {}

    def fail(reason):
        evidence.fail(reason)
        failed.set()

    @channel.on("open")
    def on_open():
        opened.set()
        if restrict_browser:
            channel.send(json.dumps({
                "type": "session.commentary.append", "event_id": "browser-restricted",
                "content": "This browser command must be rejected.", "delegation_id": None,
            }))

    @channel.on("message")
    def on_message(message):
        evidence.observe(message)
        if evidence.first_error:
            failed.set()
        if evidence.closed:
            closed.set()
        if evidence.permission_denied:
            permission_rejected.set()
        if "The connection test is ready" in evidence.transcript:
            transcript_ready.set()

    async def receive_audio(track):
        while True:
            try:
                frame = await track.recv()
            except MediaStreamError:
                if not state["closing"] and not evidence.closed:
                    fail("media ended before close was requested")
                return
            try:
                samples = mono_samples(frame)
                speech.add(samples, frame.sample_rate)
            except (ValueError, TypeError):
                fail("malformed decoded audio")
                return
            state["peak"] = max(state["peak"], max((abs(sample) for sample in samples), default=0))
            state["frames"] += 1
            if speech.qualified:
                voiced.set()

    @peer.on("track")
    def on_track(track):
        if track.kind == "audio":
            tasks.append(asyncio.create_task(receive_audio(track)))

    try:
        offer = await peer.createOffer()
        await peer.setLocalDescription(offer)
        process = await asyncio.create_subprocess_exec(
            binary,
            *(["--restrict-browser"] if restrict_browser else []),
            *(["--fork"] if fork else []),
            stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        )
        process.stdin.write((json.dumps({"sdp": peer.localDescription.sdp}) + "\n").encode())
        await process.stdin.drain()
        answer_line = await asyncio.wait_for(process.stdout.readline(), 35)
        if not answer_line:
            raise RuntimeError("Rust signaling failed before returning the SDP answer")
        answer = strict_json(answer_line)
        await peer.setRemoteDescription(RTCSessionDescription(sdp=answer["sdp"], type="answer"))
        await asyncio.wait_for(opened.wait(), 15)
        process.stdin.write(b'{"ready":true}\n')
        await process.stdin.drain()
        requirements = [voiced.wait(), transcript_ready.wait()]
        if restrict_browser:
            requirements.append(permission_rejected.wait())
        media = asyncio.gather(*requirements)
        failure = asyncio.create_task(failed.wait())
        done, pending = await asyncio.wait([media, failure], timeout=25,
                                           return_when=asyncio.FIRST_COMPLETED)
        verified = media in done and evidence.first_error is None
        for future in pending:
            future.cancel()
        await asyncio.gather(media, failure, return_exceptions=True)
        if not verified:
            fail("media, transcript or exact permission evidence did not complete")
        state["closing"] = True
        process.stdin.write((json.dumps({"media_verified": verified}) + "\n").encode())
        await process.stdin.drain()
        process.stdin.close()
        await asyncio.wait_for(closed.wait(), 15)
        result_line = await asyncio.wait_for(process.stdout.readline(), 15)
        return_code = await asyncio.wait_for(process.wait(), 5)
        if return_code:
            fail("Rust signaling/control/final-drain assertions failed")
        if result_line:
            result = strict_json(result_line)
        if (not isinstance(result, dict) or result.get("sideband_acks") != 2
                or result.get("closed") is not True or result.get("created_matches_attached") is not True
                or fork and result.get("fork_new_id_verified") is not True):
            fail("Rust did not confirm exact ACKs, session identities and final usage")
    finally:
        state["closing"] = True
        await peer.close()
        for task in tasks:
            task.cancel()
        outcomes = await asyncio.gather(*tasks, return_exceptions=True)
        if any(isinstance(outcome, BaseException) and not isinstance(outcome, asyncio.CancelledError)
               for outcome in outcomes):
            fail("media receiver task failed")
        if process is not None:
            if process.returncode is None:
                process.terminate()
                try:
                    await asyncio.wait_for(process.wait(), 3)
                except asyncio.TimeoutError:
                    process.kill()
                    await process.wait()
            stderr = await process.stderr.read(65537)
            if len(stderr) > 65536:
                fail("Rust diagnostic byte budget exceeded")
            receipt = final_usage_report(stderr)
            if receipt:
                print(json.dumps(receipt, sort_keys=True), file=sys.stderr)
    evidence.check()
    result.update(media_frames=state["frames"], speech=speech.report(), peak=state["peak"],
                  datachannel_transcript_matched=True, datachannel_closed=evidence.closed,
                  browser_restriction_verified=restrict_browser and evidence.permission_denied)
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="Built Rust live_webrtc_smoke example")
    parser.add_argument("--restrict-browser", action="store_true")
    parser.add_argument("--fork", action="store_true")
    args = parser.parse_args()
    asyncio.run(probe(args.binary, args.restrict_browser, args.fork))
