"""Observe progressing frame identities on the desktop, not decoder drop counts."""
import argparse
import json
from pathlib import Path
import random
import time

import numpy as np

from interaction import Session
from seek import frame_number
from visual import prepare_apps, geometry, U, W, sha, normalize


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--repeats', type=int, default=10)
    args = parser.parse_args()
    root, corpus = args.root.resolve(), args.corpus.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if any(root.iterdir()):
        raise SystemExit('Use an empty output directory')
    apps = prepare_apps(root, Path(__file__).resolve().parents[2])
    files = [f for f in json.loads((corpus / 'manifest.json').read_text(encoding='utf-8'))['files']
             if f['kind'] == 'video']
    pairs = [(name, file) for name in ('towavue', 'mpc-hc', 'vlc') for file in files]
    rng = random.Random(104067)
    metadata = {
        'repeats': args.repeats, 'seed': 104067, 'seconds_per_trial': 5,
        'corpus_sha256': sha(corpus / 'manifest.json'),
        'endpoint': 'desktop observations of unique 30 fps source frame identities',
        'limitations': ['Capture can miss frames; source identity gaps are not decoder drops',
                       'Capture and classification impose CPU/GPU load',
                       'No physical scanout or acoustic continuity claim'],
    }
    (root / 'run.json').write_text(json.dumps(metadata, indent=2), encoding='utf-8')
    with (root / 'samples.jsonl').open('w', encoding='utf-8') as out:
        for block in range(-1, args.repeats):
            rng.shuffle(pairs)
            for name, file in pairs:
                session = Session(name, apps[name], corpus / file['path'],
                                  root / f'b{block+1:02d}-{name}-{file["id"]}')
                row = dict(app=name, case=file['id'], block=block, scenario='desktop_pacing',
                           phase='warmup' if block < 0 else 'measurement', status='timeout')
                samples = []
                try:
                    session.ready(None)
                    time.sleep(.5)
                    normalize(session.hwnd)
                    time.sleep(.2)
                    started = time.perf_counter_ns()
                    while (time.perf_counter_ns() - started) / 1e9 < 5:
                        begin = time.perf_counter_ns()
                        image = session.capture()
                        end = time.perf_counter_ns()
                        result = frame_number(image)
                        if result:
                            frame, marker = result
                            _, _, origin = geometry(session.hwnd)
                            point = W.POINT(origin.x + marker[0] + marker[2] // 2,
                                            origin.y + marker[1] + marker[3] // 2)
                            if U.GetAncestor(U.WindowFromPoint(point), 2) != session.hwnd:
                                row['status'] = 'occluded'
                                break
                            samples.append(dict(begin_ns=begin, end_ns=end, frame=frame))
                    else:
                        changes = [sample for index, sample in enumerate(samples)
                                   if index == 0 or sample['frame'] != samples[index-1]['frame']]
                        if len(changes) > 2:
                            intervals = np.diff([s['end_ns'] for s in changes]) / 1e6
                            row.update(status='ok', distinct_observations=len(changes),
                                       observed_interval_median_ms=float(np.median(intervals)),
                                       observed_interval_p95_ms=float(np.percentile(intervals, 95)),
                                       observed_interval_max_ms=float(max(intervals)),
                                       observed_gaps_over_100ms=int(sum(intervals > 100)),
                                       source_advance=changes[-1]['frame']-changes[0]['frame'],
                                       elapsed_ms=(changes[-1]['end_ns']-changes[0]['end_ns'])/1e6)
                    if block == 0 and samples:
                        image.save(session.root / 'last.png')
                except Exception as error:
                    row.update(status='setup_error', detail=str(error))
                finally:
                    row['forced_cleanup'] = session.close()
                row['samples'] = samples
                out.write(json.dumps(row) + '\n')
                out.flush()
                print(block, name, file['id'], row['status'],
                      row.get('observed_interval_p95_ms'), flush=True)


if __name__ == '__main__':
    main()
