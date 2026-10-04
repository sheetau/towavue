"""Already-running video/audio Open and playlist/folder next/previous."""
import argparse
import json
import os
from pathlib import Path
import random
import shutil
import socket
import subprocess
import time

import soundcard as sc
from audio import packet
from interaction import Session
from playback_control import MpcReceiver, vlc
from seek import tone_match
from visual import prepare_apps, sha, normalize


def audio_observe(recorder, frequency, start, timeout=5, session=None):
    samples, streak = [], 0
    while (time.perf_counter_ns() - start) / 1e9 < timeout:
        closed = session.dialog_closed() if session else True
        begin = time.perf_counter_ns()
        data, warnings = packet(recorder)
        received = time.perf_counter_ns()
        rms, fraction = tone_match(data, frequency)
        end = time.perf_counter_ns()
        match = closed and rms > .0002 and fraction > .8
        samples.append(dict(begin_ns=begin, received_ns=received, end_ns=end,
                            rms=rms, fraction=fraction, warnings=warnings))
        streak = streak + 1 if match else 0
        if streak == 2:
            row = dict(status='ok', audio_upper_ms=(end-start)/1e6, audio_samples=samples)
            if session: session.finish_dialog(row, start, target_upper=end)
            return row
    row = dict(status='timeout', audio_samples=samples)
    if session: session.finish_dialog(row, start)
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--repeats', type=int, default=30)
    parser.add_argument('--groups', default='video,audio12,audio180')
    parser.add_argument('--open-fresh', action='store_true',
                        help='Open each unvisited target once from a separate seed file; no next/previous')
    args = parser.parse_args()
    root, corpus = args.root.resolve(), args.corpus.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if any(root.iterdir()):
        raise SystemExit('Use an empty output directory')
    repo = Path(__file__).resolve().parents[2]
    apps = prepare_apps(root, repo)
    apps['mpc-hc']['after'] = ['/new', '/play', '/volume', '20']
    apps['vlc']['args'] = [v for v in apps['vlc']['args'] if v != '--gain=0'] + [
        '--gain=0.2', '--extraintf=rc', '--rc-quiet']
    groups = [g for g in json.loads((corpus / 'manifest.json').read_text(encoding='utf-8'))['groups']
              if g['id'] in args.groups.split(',')]
    env = os.environ.copy()
    env['PATH'] = str(next((repo / 'vendor/ffmpeg/autobuild-2026-08-31-13-27').glob('*/bin'))) + os.pathsep + env['PATH']
    for group in groups:
        folder = (corpus / group['files'][0]['path']).parent
        probe = subprocess.run([str(repo / 'target/release/examples/folder-order-dump.exe'), str(folder)],
                               env=env, capture_output=True, check=True, creationflags=subprocess.CREATE_NO_WINDOW)
        entries = {str(corpus / f['path']).casefold(): f for f in group['files']}
        paths = probe.stdout.decode('utf-8').splitlines()
        if {p.casefold() for p in paths} != set(entries):
            raise RuntimeError('Shell membership mismatch')
        group['files'] = [entries[p.casefold()] for p in paths]
    rng = random.Random(104068)
    pairs = [(name, group) for name in ('towavue', 'mpc-hc', 'vlc') for group in groups]
    (root / 'run.json').write_text(json.dumps(dict(
        repeats=args.repeats, seed=104068, corpus_sha256=sha(corpus / 'manifest.json'),
        open_fresh=args.open_fresh,
        resume_policy=('Targets are unvisited in each fresh-profile session; initial playback uses a separate seed path'
                       if args.open_fresh else
                       'towavue history starts empty but can restore positions within the session; MPC/VLC resume is disabled'),
        order=groups, input='towavue Alt+arrows and native Open dialog; MPC file-skip menu/WM_COPYDATA Open; VLC RC next/prev/add',
        limitations=['Folder navigation and explicit playlists differ', 'Open input transport differs',
                     'Video endpoint is recognizable content; audio endpoint is software loopback',
                     'Volume knobs are 20%, not acoustic level calibration']), indent=2), encoding='utf-8')
    receiver = MpcReceiver()
    speaker = sc.default_speaker()
    loopback = sc.get_microphone(id=str(speaker.id), include_loopback=True)
    if not loopback.isloopback:
        raise RuntimeError('Loopback device required')
    try:
        with loopback.recorder(samplerate=48000, channels=2, blocksize=480) as recorder, \
                (root / 'samples.jsonl').open('w', encoding='utf-8') as out:
            for block in range(-1, args.repeats):
                rng.shuffle(pairs)
                for name, group in pairs:
                    files = group['files']
                    app = dict(apps[name])
                    if name == 'mpc-hc':
                        app['after'] = [*app['after'], '/slave', str(receiver.hwnd)]
                    if name == 'vlc':
                        with socket.socket() as reservation:
                            reservation.bind(('127.0.0.1', 0))
                            port = reservation.getsockname()[1]
                        app['args'] = [*app['args'], f'--rc-host=127.0.0.1:{port}']
                    launch = corpus / files[0]['path']
                    if name == 'mpc-hc':
                        launch = root / (group['id'] + '.mpcpl')
                        listing = ['MPCPLAYLIST']
                        for index, file in enumerate(files, 1):
                            listing += [f'{index},type,0', f'{index},filename,{corpus / file["path"]}']
                        launch.write_text('\n'.join(listing) + '\n', encoding='utf-8-sig')
                    if name == 'vlc':
                        launch = root / (group['id'] + '.m3u8')
                        launch.write_text('#EXTM3U\n' + '\n'.join(str(corpus / file['path']) for file in files) + '\n', encoding='utf-8')
                    if args.open_fresh:
                        seed = root / 'seed' / group['id'] / Path(files[0]['path']).name
                        seed.parent.mkdir(parents=True, exist_ok=True)
                        if not seed.exists():
                            shutil.copy2(corpus / files[0]['path'], seed)
                        launch = seed
                    session = Session(name, app, launch,
                                      root / f'b{block+1:02d}-{name}-{group["id"]}', volume=.2)
                    try:
                        session.ready(None)
                        time.sleep(.5)
                        normalize(session.hwnd)
                        time.sleep(.2)
                        if name == 'vlc':
                            session.rc_socket = socket.create_connection(('127.0.0.1', port), timeout=3)
                        def observe(file, start):
                            if file['kind'] == 'video':
                                return session.observe(file['identity'], start)
                            return audio_observe(recorder, file['frequency'], start, session=session)
                        initial = observe(files[0], time.perf_counter_ns())
                        if initial['status'] != 'ok':
                            raise RuntimeError('Initial content not verified')
                        time.sleep(.3)
                        actions = [('next', index) for index in range(1, len(files))]
                        actions += [('previous', index) for index in range(len(files)-2, -1, -1)]
                        actions += [('open', index) for index in (4, 1, 3, 0, 2)]
                        if args.open_fresh:
                            actions = [action for action in actions if action[0] == 'open']
                        for step, (operation, index) in enumerate(actions):
                            file = files[index]
                            if operation == 'open' and name == 'towavue':
                                action = session.prepare_open(corpus / file['path'])
                            elif operation == 'open' and name == 'mpc-hc':
                                action = lambda: receiver.send(session, 0xA0000000, str(corpus / file['path']))
                            elif operation == 'open':
                                action = lambda: vlc(session, 'add ' + (corpus / file['path']).as_uri())
                            elif name == 'towavue':
                                action = lambda: session.chord(0x12, 0x27 if operation == 'next' else 0x25)
                            elif name == 'mpc-hc':
                                action = lambda: session.command_id(920 if operation == 'next' else 919)
                            else:
                                action = lambda: vlc(session, 'next' if operation == 'next' else 'prev')
                            until = time.monotonic() + .06
                            while time.monotonic() < until:
                                packet(recorder)
                            start = time.perf_counter_ns()
                            action()
                            dispatch = time.perf_counter_ns()
                            row = observe(file, start)
                            row.update(app=name, case=file['id'], group=group['id'], kind=file['kind'],
                                       scenario=operation, block=block, step=step, start_ns=start,
                                       dispatch_return_ns=dispatch, phase='warmup' if block < 0 else 'measurement')
                            out.write(json.dumps(row) + '\n'); out.flush()
                            print(block, name, group['id'], operation, row['status'], flush=True)
                            if row['status'] != 'ok':
                                break
                            time.sleep(.15)
                    except Exception as error:
                        out.write(json.dumps(dict(app=name, case=group['id'], block=block,
                                                  status='setup_error', detail=str(error))) + '\n')
                        out.flush(); print(name, str(error), flush=True)
                    finally:
                        if hasattr(session, 'rc_socket'):
                            session.rc_socket.close()
                        session.close()
    finally:
        receiver.close()


if __name__ == '__main__':
    main()
