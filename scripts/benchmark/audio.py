"""CLI launch to a matching 997 Hz tone delivered by WASAPI loopback.

This observes software loopback delivery, not acoustic onset. No microphone is
opened and no audio recording is written. Only tone scores and timestamps remain.
"""
import argparse
import json
import os
from pathlib import Path
import random
import subprocess
import time
import warnings

import numpy as np
import soundcard as sc

from visual import U, prepare_apps, sha, windows


def tone_score(data, rate=48000):
    if len(data) < 240:
        return 0., 0.
    mono = np.mean(data, axis=1).astype(float)
    mono -= mono.mean()
    energy = float(np.mean(mono * mono))
    if energy < 1e-12:
        return 0., 0.
    t = np.arange(len(mono)) / rate
    # Least-squares quadrature handles non-integral periods and packet lengths.
    basis = np.column_stack((np.sin(2 * np.pi * 997 * t), np.cos(2 * np.pi * 997 * t)))
    fit = basis @ np.linalg.lstsq(basis, mono, rcond=None)[0]
    fraction = max(0., 1. - float(np.mean((mono - fit) ** 2)) / energy)
    return float(np.sqrt(energy)), fraction


def matches(rms, fraction):
    return rms > .0002 and fraction > .8


def packet(recorder):
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        data = recorder.record(numframes=None)
    return data, [str(warning.message) for warning in caught]


def audio_apps(root, repo):
    apps = prepare_apps(root, repo)
    del apps["irfanview"]
    apps["mpc-hc"]["after"] = ["/new", "/play", "/volume", "20"]
    apps["vlc"]["args"] = [value for value in apps["vlc"]["args"] if value != "--gain=0"] + ["--gain=0.2"]
    return apps


def trial(name, app, case, corpus, root, identifier, timeout, recorder):
    path = corpus / case["path"]
    with path.open("rb") as file:
        while file.read(1024 * 1024):
            pass
    env = os.environ.copy()
    for key in ("APPDATA", "LOCALAPPDATA"):
        directory = root / "profiles" / name / key
        directory.mkdir(parents=True, exist_ok=True)
        env[key] = str(directory)
    if name == "towavue":
        settings = Path(env["APPDATA"]) / "towavue"
        settings.mkdir(exist_ok=True)
        (settings / "video-resume.txt").unlink(missing_ok=True)
        (settings / "playback-volume.conf").write_text("towavue playback volume v1\n1\n0.2\n0.2\n", encoding="utf-8")
        candidates = list((Path(__file__).resolve().parents[2] / "vendor/ffmpeg/autobuild-2026-08-31-13-27").glob("*/bin"))
        env["FFMPEG_DIR"] = str(candidates[0].parent)
        env["PATH"] = str(candidates[0]) + os.pathsep + env["PATH"]
    command = [str(app["exe"]), *app["args"], str(path), *app["after"]]
    row = dict(trial_id=identifier, app=name, case=case["id"], kind="audio",
               scenario="warm_filesystem_new_process_loopback", endpoint="loopback_detector_completion",
               status="timeout", latency_lower_ms="", latency_upper_ms="", command=command)
    # Reject contaminated trials, rather than attributing an already playing tone.
    baseline_until = time.perf_counter() + .25
    contaminated = False
    while time.perf_counter() < baseline_until:
        data, _ = packet(recorder)
        contaminated |= matches(*tone_score(data))
    if contaminated:
        row["status"] = "baseline_contaminated"
        return row
    samples, process = [], None
    with (root / "logs" / (identifier + ".log")).open("wb") as log:
        start = time.perf_counter_ns()
        try:
            process = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT,
                                       creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            row.update(start_ns=start, launch_return_ns=time.perf_counter_ns(), pid=process.pid)
            streak = 0
            while (time.perf_counter_ns() - start) / 1e9 < timeout:
                begin = time.perf_counter_ns()
                data, issues = packet(recorder)
                received = time.perf_counter_ns()
                rms, fraction = tone_score(data)
                completed = time.perf_counter_ns()
                match = matches(rms, fraction)
                samples.append(dict(begin_ns=begin, received_ns=received, completed_ns=completed,
                                    frames=len(data), rms=rms, tone_fraction=fraction, match=match, warnings=issues))
                streak = streak + 1 if match else 0
                if streak == 2:
                    latency = (completed - start) / 1e6
                    row.update(status="ok", latency_lower_ms=latency, latency_upper_ms=latency,
                               first_matching_packet_ms=(samples[-2]["received_ns"] - start) / 1e6)
                    break
                if process.poll() is not None:
                    row["status"] = "process_exited"
                    break
        finally:
            if process is not None:
                for owned in windows(process.pid):
                    U.PostMessageW(owned, 0x0010, 0, 0)
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.terminate()
                    process.wait(timeout=5)
                    row["forced_cleanup"] = True
    row["samples"] = samples
    row["packet_warning_count"] = sum(len(sample["warnings"]) for sample in samples)
    row["packet_median_frames"] = float(np.median([sample["frames"] for sample in samples])) if samples else None
    # Drain the final packet from this app before testing the next baseline.
    until = time.perf_counter() + .15
    while time.perf_counter() < until:
        packet(recorder)
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=30)
    parser.add_argument("--warmup-blocks", type=int, default=1)
    parser.add_argument("--timeout", type=float, default=8)
    args = parser.parse_args()
    root, corpus = args.root.resolve(), args.corpus.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if (root / "samples.jsonl").exists():
        raise SystemExit("Use a new output directory")
    (root / "logs").mkdir(exist_ok=True)
    apps = audio_apps(root, Path(__file__).resolve().parents[2])
    cases = [case for case in json.loads((corpus / "manifest.json").read_text(encoding="utf-8"))["files"] if case["kind"] == "audio"]
    speaker = sc.default_speaker()
    loopback = sc.get_microphone(id=speaker.id, include_loopback=True)
    if not loopback.isloopback:
        raise RuntimeError("Refusing a non-loopback recording device")
    metadata = dict(seed=10405, repeats=args.repeats, warmup_blocks=args.warmup_blocks,
                    device_name=speaker.name, device_id=speaker.id, samplerate=48000, channels=2,
                    requested_block_frames=480, soundcard_version="0.4.6",
                    script_sha256=sha(Path(__file__)), helper_sha256=sha(Path(__file__).with_name("visual.py")),
                    corpus_sha256=sha(corpus / "manifest.json"),
                    apps={name: dict(exe=str(app["exe"]), sha256=sha(app["exe"]), args=app["args"], after=app["after"]) for name, app in apps.items()},
                    endpoint="Second consecutive loopback packet matching the 997 Hz fixture tone, after detector completes",
                    limitations=["Software receipt/detection, not acoustic onset or native WASAPI sample timestamps", "Latency bounds are equal for this measured callback endpoint; they do not bound true sound onset", "WASAPI buffering and detector confirmation are included", "Master endpoint volume is unchanged; app volume configured to 20 percent", "No microphone and no retained audio", "Synthetic warm-filesystem new-process pilot"])
    (root / "run.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    rng = random.Random(10405)
    pairs = [(name, case) for name in apps for case in cases]
    with loopback.recorder(samplerate=48000, channels=2, blocksize=480) as recorder, (root / "samples.jsonl").open("w", encoding="utf-8") as output:
        index = 0
        for block in range(-args.warmup_blocks, args.repeats):
            schedule = pairs.copy()
            rng.shuffle(schedule)
            for name, case in schedule:
                identifier = f"{index:04d}-{name}-{case['id']}"
                row = trial(name, apps[name], case, corpus, root, identifier, args.timeout, recorder)
                row.update(block=block, phase="warmup" if block < 0 else "measurement")
                output.write(json.dumps(row) + "\n")
                output.flush()
                index += 1
                print(f"{index}/{len(pairs) * (args.repeats + args.warmup_blocks)} {name} {case['id']} {row['status']} {row['latency_upper_ms']}", flush=True)


if __name__ == "__main__":
    main()
