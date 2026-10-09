"""Warm next/previous, distant selection and native Open benchmarks.

Each block uses a fresh app with an isolated profile. Navigation starts after a
one-second idle period, then waits 100 ms after each observed target. This is a
paced consumer workload, not an unbounded key-repeat throughput test.
"""
import argparse
import json
import os
from pathlib import Path
import random
import subprocess
import time

from interaction import Session, menu_items, U
from visual import prepare_apps, sha


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root',type=Path,required=True)
    p.add_argument('--corpus',type=Path,required=True)
    p.add_argument('--repeats',type=int,default=30)
    p.add_argument('--groups',default='')
    p.add_argument('--scenarios',default='')
    args=p.parse_args()
    root,corpus=args.root.resolve(),args.corpus.resolve()
    root.mkdir(parents=True,exist_ok=True)
    if any(root.iterdir()):raise SystemExit('Use an empty output directory')
    apps=prepare_apps(root,Path(__file__).resolve().parents[2])
    # Explicit display fitting and resampling, not the destructive /resample CLI.
    ini=root/'profiles/irfanview/i_view64.ini'
    ini.write_text('[Viewing]\nFitWindowOption=2\nUseResample=1\nZoomResample=1\n',encoding='utf-8')
    apps['irfanview']['after']=[f'/ini={ini.parent}']
    groups=json.loads((corpus/'manifest.json').read_text(encoding='utf-8'))['groups']
    if args.groups:groups=[g for g in groups if g['id'] in args.groups.split(',')]
    probe=Path(__file__).resolve().parents[2]/'target/release/examples/folder-order-dump.exe'
    probe_env=os.environ.copy()
    bins=list((Path(__file__).resolve().parents[2]/'vendor/ffmpeg/autobuild-2026-08-31-13-27').glob('*/bin'))
    probe_env['PATH']=str(bins[0])+os.pathsep+probe_env['PATH']
    for g in groups:
        source=corpus/g['files'][0]['path']
        result=subprocess.run([str(probe),str(source.parent)],capture_output=True,check=True,env=probe_env,
                              creationflags=subprocess.CREATE_NO_WINDOW)
        paths=result.stdout.decode('utf-8').splitlines()
        entries={str((corpus/f['path']).resolve()).casefold():f for f in g['files']}
        if set(p.casefold() for p in paths)!=set(entries):raise RuntimeError('Shell membership mismatch')
        g['files']=[entries[p.casefold()] for p in paths]
        (root/(g['id']+'-shell-order.txt')).write_text(result.stderr.decode('utf-8')+'\n'+'\n'.join(paths),encoding='utf-8')
    rng=random.Random(104061)
    pairs=[(name,g) for name in ('towavue','irfanview') for g in groups]
    metadata={'seed':104061,'repeats':args.repeats,'warmup_blocks':1,
              'open_target_seed':'104061:<block>:<group>; identical target sequence in both apps',
              'corpus_sha256':sha(corpus/'manifest.json'),
              'apps':{n:{'exe':str(apps[n]['exe']),'sha256':sha(apps[n]['exe'])} for n in ('towavue','irfanview')},
              'settings':'towavue default HQ/Fit; IrfanView FitWindowOption=2 UseResample=1',
              'endpoint':'correct 24-cell identity in desktop capture; not full-resolution completion',
              'input':'posted native key for arrows; native menu commands for Irfan first/last; dialog OK for Open',
              'cache':'OS warm; fresh process per group/block; 1 s initial idle, 100 ms between observations',
              'scenario_filter':args.scenarios,
              'limitations':['Different resampling filters, not certified pixel equivalence',
                             'Native message timing excludes physical keyboard transport',
                             'Multiple steps within one session are correlated; block is resampling unit']}
    (root/'run.json').write_text(json.dumps(metadata,indent=2)+'\n',encoding='utf-8')
    with (root/'samples.jsonl').open('w',encoding='utf-8') as out:
        for block in range(-1,args.repeats):
            schedule=pairs.copy();rng.shuffle(schedule)
            for name,g in schedule:
                folder=root/f'b{block+1:02d}-{name}-{g["id"]}'
                files=g['files']
                app=dict(apps[name])
                if name=='irfanview':
                    listing=root/(g['id']+'-filelist.txt')
                    listing.write_text('\n'.join(str(corpus/f['path']) for f in files),encoding='utf-8-sig')
                    app['after']=[*app['after'],f'/filelist={listing}']
                s=Session(name,app,corpus/files[0]['path'],folder)
                try:
                    s.ready(files[0]['identity']);time.sleep(1)
                    if name=='irfanview':
                        s.command_id(1164)
                        s.command_id(1086)
                        s.ready(files[0]['identity'])
                        time.sleep(.1)
                        U.SendMessageW(s.hwnd,0x116,U.GetMenu(s.hwnd),0)
                        menus=menu_items(U.GetMenu(s.hwnd))
                        (folder/'menus.json').write_text(json.dumps(menus,indent=2),encoding='utf-8')
                        if not all(next(m for m in menus if m['id']==i)['state']&8 for i in (28,1091)):
                            raise RuntimeError('Irfan fit/resampling configuration was not applied')
                    actions=[]
                    count=min(8,len(files)-1)
                    for index in range(1,count+1):actions.append(('next',index,lambda:s.key(0x27)))
                    for index in range(count-1,-1,-1):actions.append(('previous',index,lambda:s.key(0x25)))
                    actions.extend([
                        ('distant_last',len(files)-1,lambda:s.command_id(1112) if name=='irfanview' else s.key(0x23)),
                        ('distant_first',0,lambda:s.command_id(1086) if name=='irfanview' else s.key(0x24))])
                    targets=random.Random(f'104061:{block}:{g["id"]}').sample(range(1,len(files)),2)
                    for index in targets:actions.append(('random_open',index,None))
                    if args.scenarios:actions=[a for a in actions if a[0] in args.scenarios.split(',')]
                    failures=0
                    for step,(operation,index,action) in enumerate(actions):
                        if action is None:action=s.prepare_open(corpus/files[index]['path'])
                        start=time.perf_counter_ns();action()
                        row=s.observe(files[index]['identity'],start,retain=f'{step:02d}.png' if block==0 else None)
                        row.update(app=name,case=g['id'],kind='image',scenario=operation,step=step,
                                   phase='warmup' if block<0 else 'measurement',block=block,
                                   target_index=index,target_identity=files[index]['identity'])
                        out.write(json.dumps(row)+'\n');out.flush()
                        if row['status']!='ok':failures+=1
                        time.sleep(.1)
                    print(block,name,g['id'],'failures',failures,flush=True)
                except Exception as error:
                    out.write(json.dumps({'app':name,'case':g['id'],'block':block,
                        'phase':'warmup' if block<0 else 'measurement','status':'setup_error','detail':str(error)})+'\n');out.flush()
                    print(block,name,g['id'],type(error).__name__,str(error),flush=True)
                finally:
                    forced=s.close()
                    (folder/'cleanup.json').write_text(json.dumps({'forced':forced}),encoding='utf-8')


if __name__=='__main__':main()
