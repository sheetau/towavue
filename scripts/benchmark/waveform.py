"""Internal waveform preparation and cache timing for the shared audio corpus."""
import argparse
import json
import os
from pathlib import Path
import random
import subprocess

from visual import sha


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--repeats', type=int, default=30)
    args = parser.parse_args()
    if args.repeats < 0:
        parser.error('repeats must be nonnegative')
    root, corpus = args.root.resolve(), args.corpus.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if any(root.iterdir()):
        raise SystemExit('Use an empty output directory')
    (root/'logs').mkdir()
    repo = Path(__file__).resolve().parents[2]
    exe = repo/'target/release/examples/waveform-fixture-cost.exe'
    bins = next((repo/'vendor/ffmpeg/autobuild-2026-08-31-13-27').glob('*/bin'))
    env = os.environ.copy()
    env['PATH'] = str(bins)+os.pathsep+env['PATH']
    files = [f for f in json.loads((corpus/'manifest.json').read_text(encoding='utf-8'))['files']
             if f['kind'] == 'audio']
    if not files:
        parser.error('No audio fixtures')
    (root/'run.json').write_text(json.dumps(dict(
        repeats=args.repeats, warmup_blocks=1, seed=104071,
        exe_sha256=sha(exe), corpus_sha256=sha(corpus/'manifest.json'),
        script_sha256=sha(Path(__file__)),
        boundary='Safe runtime API return, not screen display or playback onset',
        cache='Source bytes OS-warm; new disk/memory preview cache per trial, then memory and reopened disk hits',
        comparators='N/A: no equivalent waveform endpoint in the selected competitors',
        order='overview, memory hit, reopened disk hit, timeline detail'), indent=2)+'\n', encoding='utf-8')
    rng = random.Random(104071)
    reference = {}
    with (root/'samples.jsonl').open('w', encoding='utf-8') as output:
        for block in range(-1, args.repeats):
            rng.shuffle(files)
            for file in files:
                path = corpus/file['path']
                path.read_bytes()
                identifier = f'b{block+1:02d}-{file["id"]}'
                try:
                    result = subprocess.run([str(exe), str(path), str(root/'cache'/identifier), str(file['seconds'])],
                                            env=env, capture_output=True, creationflags=subprocess.CREATE_NO_WINDOW,
                                            timeout=120)
                    stdout, stderr, exit_code = result.stdout, result.stderr, result.returncode
                    status = 'process_error' if exit_code else 'ok'
                except subprocess.TimeoutExpired as error:
                    stdout, stderr, exit_code = error.stdout or b'', error.stderr or b'', None
                    status = 'timeout'
                text = stdout.decode('utf-8', errors='replace')
                (root/'logs'/f'{identifier}.log').write_text(
                    text+'\n'+stderr.decode('utf-8', errors='replace'), encoding='utf-8')
                row = dict(app='towavue', case=file['id'], block=block,
                           phase='warmup' if block < 0 else 'measurement',
                           scenario='internal_waveform', status=status, exit_code=exit_code)
                if status == 'ok':
                    row.update(json.loads(text.strip().splitlines()[-1]))
                    hashes = (row['preview_hash'], row['detail_hash'])
                    if file['id'] in reference and reference[file['id']] != hashes:
                        row['status'] = 'inconsistent_pixels_or_detail'
                    reference.setdefault(file['id'], hashes)
                output.write(json.dumps(row)+'\n'); output.flush()
                print(block, file['id'], row['status'], row.get('native_ms'), flush=True)
                if row['status'] != 'ok':
                    raise SystemExit('Waveform probe failed; inspect retained inputs/cache/logs')


if __name__ == '__main__':
    main()
