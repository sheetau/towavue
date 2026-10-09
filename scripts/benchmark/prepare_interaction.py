"""Group existing visual fixtures and generate identifiable short/long audio."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
from visual import sha


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--ffmpeg', type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if any(root.iterdir()):
        raise SystemExit('Use an empty output directory')
    source = json.loads((args.corpus / 'manifest.json').read_text(encoding='utf-8'))
    groups, commands = [], []
    for group, kinds in [('mixed', ('image', 'animation')), ('video', ('video',))]:
        folder = root / group
        folder.mkdir()
        files = []
        for entry in source['files']:
            if entry['kind'] not in kinds:
                continue
            path = args.corpus / entry['path']
            shutil.copy2(path, folder / path.name)
            files.append(dict(entry, path=str(Path(group) / path.name)))
        groups.append(dict(id=group, files=files))
    for seconds in (12, 180):
        group = f'audio{seconds}'
        folder = root / group
        folder.mkdir()
        files = []
        for index, (extension, codec) in enumerate([
                ('wav', 'pcm_s16le'), ('flac', 'flac'), ('mp3', 'libmp3lame'),
                ('m4a', 'aac'), ('opus', 'libopus')]):
            frequency = 701 + index * 179
            path = folder / f'{index}-{extension}.{extension}'
            command = [str(args.ffmpeg.resolve()), '-v', 'error', '-f', 'lavfi', '-i',
                       f'aevalsrc=0.08*sin(2*PI*{frequency}*t)|0.08*sin(2*PI*{frequency}*t):s=48000',
                       '-t', str(seconds), '-c:a', codec,
                       *([] if index < 2 else ['-b:a', '192k']), str(path)]
            result = subprocess.run(command, capture_output=True, creationflags=subprocess.CREATE_NO_WINDOW)
            if result.returncode:
                raise RuntimeError(result.stderr.decode('utf-8', errors='replace'))
            commands.append(command)
            files.append(dict(id=f'{group}-{extension}', path=str(path.relative_to(root)),
                              kind='audio', frequency=frequency, seconds=seconds,
                              bytes=path.stat().st_size, sha256=sha(path)))
        groups.append(dict(id=group, files=files))
    (root / 'manifest.json').write_text(json.dumps(dict(
        groups=groups, files=[file for group in groups for file in group['files']],
        commands=commands, generator_sha256=sha(Path(__file__)),
        source_manifest_sha256=sha(args.corpus / 'manifest.json'),
        ffmpeg_sha256=sha(args.ffmpeg)), indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
