"""Open a large folder and move between owned sibling image folders."""
import argparse
import json
import os
from pathlib import Path
import random
import shutil
import subprocess
import time
from interaction import Session
from visual import prepare_apps


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root',type=Path,required=True);p.add_argument('--corpus',type=Path,required=True)
    p.add_argument('--repeats',type=int,default=30)
    args=p.parse_args();root=args.root.resolve();root.mkdir(parents=True,exist_ok=True)
    if any(root.iterdir()):raise SystemExit('Use an empty output directory')
    repo=Path(__file__).resolve().parents[2];corpus=args.corpus.resolve();apps=prepare_apps(root,repo)
    groups=json.loads((corpus/'manifest.json').read_text(encoding='utf-8'))['groups']
    probe=repo/'target/release/examples/folder-order-dump.exe'
    env=os.environ.copy();env['PATH']=str(next((repo/'vendor/ffmpeg/autobuild-2026-08-31-13-27').glob('*/bin')))+os.pathsep+env['PATH']
    first={}
    for group in groups:
        folder=(corpus/group['files'][0]['path']).parent
        result=subprocess.run([str(probe),str(folder)],env=env,capture_output=True,check=True,creationflags=subprocess.CREATE_NO_WINDOW)
        path=Path(result.stdout.decode('utf-8').splitlines()[0])
        first[group['id']]=next(f for f in group['files'] if (corpus/f['path'])==path)
    if len({file['identity'] for file in first.values()}) != len(first):
        raise RuntimeError('Folder targets must have distinct visual identities; regenerate the corpus')
    hierarchy=root/'fixture-parent'
    child=hierarchy/'child'
    child.mkdir(parents=True)
    base=next(g for g in groups if g['id']=='jpeg1080')['files']
    shutil.copy2(corpus/base[0]['path'],hierarchy/'parent.jpg')
    shutil.copy2(corpus/base[1]['path'],child/'child.jpg')
    rng=random.Random(104064)
    (root/'run.json').write_text(json.dumps({'scope':'towavue Open folder and explicit sibling file Open in both apps',
        'not_equivalent':'IrfanView exposes file Open; no direct parent/child folder command was found in its native menus',
        'first_in_shell':first,'repeats':args.repeats,'seed':104064},indent=2),encoding='utf-8')
    with (root/'samples.jsonl').open('w',encoding='utf-8') as out:
        for block in range(-1,args.repeats):
            for name in rng.sample(['towavue','irfanview'],2):
                s=Session(name,apps[name],corpus/first['jpeg1080']['path'],root/f'b{block+1:02d}-{name}',800/1920*100)
                try:
                    s.ready(first['jpeg1080']['identity']);time.sleep(.5)
                    if name=='irfanview':s.command_id(1091)
                    for target in ('large1000','png4k','jpeg24mp','jpeg1080'):
                        file=corpus/first[target]['path']
                        submit=s.prepare_open(file.parent if name=='towavue' else file,folder=name=='towavue')
                        start=time.perf_counter_ns();submit()
                        row=s.observe(first[target]['identity'],start,retain=f'{target}.png' if block==0 else None)
                        row.update(app=name,case=target,scenario='open_folder' if name=='towavue' else 'open_file_in_other_folder',
                                   block=block,phase='warmup' if block<0 else 'measurement')
                        out.write(json.dumps(row)+'\n');out.flush();print(block,name,target,row['status'],row['latency_upper_ms'],flush=True)
                        time.sleep(.2)
                except Exception as error:
                    out.write(json.dumps({'app':name,'block':block,'status':'setup_error','detail':str(error)})+'\n');out.flush();print(str(error),flush=True)
                finally:s.close()
            s=Session('towavue',apps['towavue'],child/'child.jpg',root/f'b{block+1:02d}-hierarchy')
            try:
                s.ready(base[1]['identity']);time.sleep(.5)
                for direction,identity,key in [('parent',base[0]['identity'],0x26),('child',base[1]['identity'],0x28)]:
                    start=time.perf_counter_ns();s.chord(0x11,0x12,key)
                    row=s.observe(identity,start,retain=direction+'.png' if block==0 else None)
                    row.update(app='towavue',case='single_child_hierarchy',scenario=direction,
                               block=block,phase='warmup' if block<0 else 'measurement')
                    out.write(json.dumps(row)+'\n');out.flush();print(block,'towavue',direction,row['status'],row['latency_upper_ms'],flush=True)
                    time.sleep(.2)
            finally:s.close()


if __name__=='__main__':main()
