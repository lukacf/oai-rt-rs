#!/usr/bin/env python3
"""Real-peer smoke for the Rust live_webrtc_smoke example.

Requires aiortc and av. Only silent synthetic input is sent. Raw media,
SDP, credentials, and transcripts are never persisted or printed.
"""

import argparse
import array
import asyncio
import json
import time
import sys
from fractions import Fraction

from aiortc import AudioStreamTrack, RTCConfiguration, RTCPeerConnection, RTCSessionDescription
from av import AudioFrame


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


async def probe(binary, restrict_browser, fork):
    peer = RTCPeerConnection(RTCConfiguration(iceServers=[]))
    peer.addTrack(Silence())
    channel = peer.createDataChannel("oai-events")
    opened = asyncio.Event()
    closed = asyncio.Event()
    voiced = asyncio.Event()
    transcript_ready = asyncio.Event()
    permission_rejected = asyncio.Event()
    state = {"frames": 0, "voiced_frames": 0, "peak": 0, "transcript": ""}
    tasks = []
    process = None

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
        event = json.loads(message)
        if event["type"] == "session.output_transcript.delta":
            state["transcript"] += event["delta"]
            if "The connection test is ready" in state["transcript"]:
                transcript_ready.set()
        elif event["type"] == "session.closed":
            closed.set()
        elif event["type"] == "error":
            error = event["error"]
            if error.get("code") == "event_not_allowed" and error.get("client_event_id") == "browser-restricted":
                permission_rejected.set()

    async def receive_audio(track):
        while True:
            frame = await track.recv()
            if frame.format.name not in ("s16", "s16p"):
                raise RuntimeError("Expected decoded PCM16 audio from the peer")
            channels_per_plane = 1 if frame.format.is_planar else len(frame.layout.channels)
            samples = array.array("h")
            for plane in frame.planes:
                samples.frombytes(bytes(plane)[:frame.samples * channels_per_plane * 2])
            if sys.byteorder != "little":
                samples.byteswap()
            peak = max(abs(sample) for sample in samples)
            state["peak"] = max(state["peak"], peak)
            state["frames"] += 1
            if peak >= 500:
                state["voiced_frames"] += 1
            if state["voiced_frames"] >= 10:
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
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        process.stdin.write((json.dumps({"sdp": peer.localDescription.sdp}) + "\n").encode())
        await process.stdin.drain()
        answer_line = await asyncio.wait_for(process.stdout.readline(), 35)
        if not answer_line:
            raise RuntimeError("Rust signaling failed before returning the SDP answer")
        answer = json.loads(answer_line)
        await peer.setRemoteDescription(RTCSessionDescription(sdp=answer["sdp"], type="answer"))
        await asyncio.wait_for(opened.wait(), 15)
        process.stdin.write(b'{"ready":true}\n')
        await process.stdin.drain()
        await asyncio.wait_for(asyncio.gather(voiced.wait(), transcript_ready.wait()), 25)
        if restrict_browser:
            await asyncio.wait_for(permission_rejected.wait(), 5)
        process.stdin.write(b'{"media_verified":true}\n')
        await process.stdin.drain()
        process.stdin.close()
        await asyncio.wait_for(closed.wait(), 15)
        result_line = await asyncio.wait_for(process.stdout.readline(), 15)
        result = json.loads(result_line)
        return_code = await asyncio.wait_for(process.wait(), 5)
        if return_code:
            raise RuntimeError("Rust signaling/control assertions failed")
        if result.get("sideband_acks") != 2 or not result.get("closed"):
            raise RuntimeError("Rust sideband did not confirm context ACKs and final usage")
        result.update(
            media_frames=state["frames"],
            voiced_frames=state["voiced_frames"],
            peak=state["peak"],
            datachannel_transcript_matched=True,
            datachannel_closed=True,
            browser_restriction_verified=restrict_browser and permission_rejected.is_set(),
        )
        print(json.dumps(result, sort_keys=True))
    finally:
        await peer.close()
        for task in tasks:
            task.cancel()
        if tasks:
            await asyncio.gather(*tasks, return_exceptions=True)
        if process is not None and process.returncode is None:
            process.terminate()
            try:
                await asyncio.wait_for(process.wait(), 3)
            except asyncio.TimeoutError:
                process.kill()
                await process.wait()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, help="Built Rust live_webrtc_smoke example")
    parser.add_argument("--restrict-browser", action="store_true")
    parser.add_argument("--fork", action="store_true")
    args = parser.parse_args()
    asyncio.run(probe(args.binary, args.restrict_browser, args.fork))
