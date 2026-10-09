"""New process to recognizable content and convergence to final viewport pixels.

Pixel convergence is defined against the same app's settled viewport, not a
decoder's full-resolution buffer. It cannot certify pixels outside the viewport.
"""
import argparse
import json
from pathlib import Path
import random
import time
import numpy as np
from interaction import Session, CaptureUnavailable, menu_items
from async_observer import Normalizer
from visual import prepare_apps,detect,windows,geometry,U,W,sha


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root',type=Path,required=True);p.add_argument('--corpus',type=Path,required=True)
    p.add_argument('--repeats',type=int,default=30)
    p.add_argument('--cases',default='')
    p.add_argument('--kinds',default='image',help='Comma-separated image/animation manifest kinds')
    p.add_argument('--towavue-baseline',type=Path)
    args=p.parse_args();root=args.root.resolve();root.mkdir(parents=True,exist_ok=True)
    if any(root.iterdir()):raise SystemExit('Use an empty output directory')
    repo=Path(__file__).resolve().parents[2];corpus=args.corpus.resolve();apps=prepare_apps(root,repo)
    ini=root/'profiles/irfanview/i_view64.ini'
    ini.write_text('[Viewing]\nFitWindowOption=2\nUseResample=1\nZoomResample=1\n',encoding='utf-8')
    apps['irfanview']['after']=[f'/ini={ini.parent}']
    names=['towavue','irfanview']
    if args.towavue_baseline:
        apps['towavue-before']=dict(apps['towavue'],exe=args.towavue_baseline.resolve())
        names.append('towavue-before')
    files=[f for f in json.loads((corpus/'manifest.json').read_text(encoding='utf-8'))['files'] if f['kind'] in args.kinds.split(',')]
    if args.cases:files=[f for f in files if f['id'] in args.cases.split(',')]
    rng=random.Random(104065)
    schedule=[(n,f) for n in names for f in files]
    (root/'run.json').write_text(json.dumps({'repeats':args.repeats,'seed':104065,
        'corpus_sha256':sha(corpus/'manifest.json'),'kinds':args.kinds,
        'normalization_protocol':'conditional-async-v1','capture_protocol':'primary-desktop-roi-v1',
        'settings':'towavue default HQ and Fit; Irfan Fit/Resample audited after timing via refreshed native menu state',
        'apps':{name:{'exe':str(apps[name]['exe']),'sha256':sha(apps[name]['exe'])} for name in names},
        'recognizable_endpoint':'all 24 target cells visible in owned 800x650 client',
        'convergence_endpoint':'first target capture with mean absolute RGB error <= 0.5 and <=1% pixels above max-channel error 8 against settled same-app viewport',
        'limitations':['Convergence of displayed pixels, not proof of full source-resolution decoding',
                       'No cross-app color/filter equivalence claim','Conditional async window normalization included from process creation; actual geometry/topmost verified before capture','OS cache warm','Animation excluded from convergence metric']},indent=2),encoding='utf-8')
    with (root/'samples.jsonl').open('w',encoding='utf-8') as out:
        for block in range(-1,args.repeats):
            rng.shuffle(schedule)
            for name,f in schedule:
                folder=root/f'b{block+1:02d}-{name}-{f["id"]}'
                # Warm file bytes before the measured process launch.
                (corpus/f['path']).read_bytes()
                s=Session('towavue' if name=='towavue-before' else name,apps[name],corpus/f['path'],folder)
                row={'app':name,'case':f['id'],'block':block,'phase':'warmup' if block<0 else 'measurement','status':'timeout'}
                captures=[];samples=[];first=None;absent=s.started;normalizations=0;slow_captures=0;normalizer=Normalizer()
                try:
                    deadline=time.monotonic()+8
                    while time.monotonic()<deadline:
                        available=windows(s.process.pid)
                        if not available:
                            absent=time.perf_counter_ns();time.sleep(.003);continue
                        s.hwnd=available[0]
                        normalize_begin=time.perf_counter_ns()
                        if not normalizer.step(s):
                            samples.append({'begin_ns':normalize_begin,'end_ns':time.perf_counter_ns(),'unavailable':'Geometry/topmost normalization pending'})
                            time.sleep(.003);continue
                        begin=time.perf_counter_ns()
                        try:image=normalizer.capture(s)
                        except CaptureUnavailable as error:
                            samples.append({'begin_ns':begin,'end_ns':time.perf_counter_ns(),'unavailable':str(error)})
                            time.sleep(.003);continue
                        end=time.perf_counter_ns()
                        capture_ms=(end-begin)/1e6
                        slow_captures=slow_captures+1 if capture_ms>80 else 0
                        if capture_ms>150 or slow_captures>=2:
                            samples.append({'begin_ns':begin,'end_ns':end,'capture_guard':True})
                            row.update(status='capture_unusable',capture_ms=capture_ms)
                            break
                        marker=detect(image,f['identity'])
                        samples.append({'begin_ns':begin,'end_ns':end,'match':bool(marker)})
                        if marker:
                            _,_,pt=geometry(s.hwnd)
                            point=W.POINT(pt.x+marker[0]+marker[2]//2,pt.y+marker[1]+marker[3]//2)
                            if U.GetAncestor(U.WindowFromPoint(point),2)!=s.hwnd:
                                row['status']='occluded';break
                            if first is None:
                                first=end;row.update(recognizable_lower_ms=max(0,(absent-s.started)/1e6),recognizable_upper_ms=(end-s.started)/1e6)
                            if f['kind']=='animation':
                                row.update(status='ok',convergence_status='not_applicable')
                                if block==0:image.save(folder/'recognizable.png')
                                break
                            # Exclude app chrome and transient edge overlays. Retain a central
                            # region containing texture outside the uniform identity cells.
                            pixels=np.array(image.convert('RGB').crop((10,80,790,570)))
                            captures.append((begin,end,pixels))
                            if end-first>700_000_000:
                                settled=pixels.astype(np.int16);scores=[]
                                for cb,ce,prior in captures:
                                    error=np.abs(prior.astype(np.int16)-settled)
                                    scores.append({'begin_ns':cb,'end_ns':ce,'mae':float(error.mean()),'bad_fraction':float(np.mean(error.max(axis=2)>8))})
                                # The final three observations must agree, or the reference is unstable.
                                stable=all(v['mae']<=.5 and v['bad_fraction']<=.01 for v in scores[-3:])
                                if stable:
                                    match=next(v for v in scores if v['mae']<=.5 and v['bad_fraction']<=.01)
                                    row.update(status='ok',converged_upper_ms=(match['end_ns']-s.started)/1e6,pixel_scores=scores)
                                    if block==0:image.save(folder/'settled.png')
                                    break
                        else:absent=begin
                    if row['status']=='ok' and name=='irfanview':
                        # Refresh lazy native check states after every timed endpoint.
                        # Reading an unopened menu without WM_INITMENU reports stale flags.
                        U.SendMessageW(s.hwnd,0x116,U.GetMenu(s.hwnd),0)
                        menus=menu_items(U.GetMenu(s.hwnd))
                        (folder/'menus-after-timing.json').write_text(json.dumps(menus,indent=2),encoding='utf-8')
                        row['fit_resample_checked']=all(next(m for m in menus if m['id']==i)['state']&8 for i in (28,1091))
                        if not row['fit_resample_checked']:row['status']='settings_mismatch'
                    row.update(start_ns=s.started,samples=samples,command=s.command,normalization_count=sum(r["resize"] for r in normalizer.requests),normalization_requests=normalizer.requests)
                except Exception as error:
                    row.update(status='setup_error',detail=str(error))
                finally:
                    row['forced_cleanup']=s.close()
                    row['exit_code']=s.process.returncode
                out.write(json.dumps(row)+'\n');out.flush()
                print(block,name,f['id'],row['status'],row.get('recognizable_upper_ms'),row.get('converged_upper_ms'),flush=True)
                if row['status']!='ok' or row['forced_cleanup'] or row['exit_code']!=0:
                    raise SystemExit('Entire run stopped: invalid observation or cleanup; retain partial data without performance claims')


if __name__=='__main__':main()
