"""Native seek request to correct video frame / position-specific loopback tone."""
import argparse
import json
from pathlib import Path
import random
import socket
import threading
import time
import math
import numpy as np
import soundcard as sc
from scipy import ndimage
from audio import packet
from interaction import Session, U, geometry
from playback_control import MpcReceiver, prepare_seek_input, seek, seek_target_seconds
from visual import prepare_apps, sha, W, normalize
import native_input


FRAME_BITS={tuple(np.random.default_rng(831+3000+i).integers(0,2,(4,6)).ravel()):i for i in range(360)}
assert len(FRAME_BITS)==360


def frame_number(image):
    rgb=np.asarray(image)[::2,::2,:3]
    mask=(rgb[:,:,0]<65)&(rgb[:,:,1]>170)&(rgb[:,:,2]>45)&(rgb[:,:,2]<155)
    labels,count=ndimage.label(mask)
    if not count:return None
    sizes=np.bincount(labels.ravel());sizes[0]=0
    for label in np.argsort(sizes)[-5:]:
        if sizes[label]<100:continue
        yy,xx=np.nonzero(labels==label)
        x,y,w,h=xx.min(),yy.min(),np.ptp(xx)+1,np.ptp(yy)+1
        if w<70 or h<45 or not 1.2<w/h<1.8:continue
        bits=[]
        for row in range(4):
            for col in range(6):
                r,g,b=rgb[int(y+(row+.75)*h/4.25),int(x+(col+.75)*w/6.25)].astype(int)
                if b<=120 or abs(r-g)<=55:break
                bits.append(int(r>g))
        found=FRAME_BITS.get(tuple(bits))
        if found is not None:return found,[int(v*2) for v in (x,y,w,h)]
    return None


def tone_match(data,frequency):
    if len(data)<240:return 0.,0.
    mono=np.mean(data,axis=1).astype(float);mono-=mono.mean()
    energy=float(np.mean(mono*mono))
    if energy<1e-12:return 0.,0.
    t=np.arange(len(mono))/48000
    basis=np.column_stack((np.sin(2*np.pi*frequency*t),np.cos(2*np.pi*frequency*t)))
    fit=basis@np.linalg.lstsq(basis,mono,rcond=None)[0]
    return float(np.sqrt(energy)),max(0.,1-float(np.mean((mono-fit)**2))/energy)


def trial(s,percent,receiver,recorder,video,retain=None,duration_seconds=12.0):
    # Clear queued pre-seek loopback data; the target frequency differs from the old segment.
    end=time.monotonic()+.06
    while time.monotonic()<end:packet(recorder)
    shared={};stop=threading.Event()
    # Consume audio on the recorder's owning thread; capture video separately.
    start=time.perf_counter_ns()
    requested=seek(s,percent,receiver,duration_seconds)
    dispatch_return=time.perf_counter_ns()
    def video_observe():
        lower=start;rows=[]
        while not stop.is_set() and (time.perf_counter_ns()-start)/1e9<3:
            begin=time.perf_counter_ns()
            try:image=s.capture()
            except Exception as error:
                shared['video_error']=str(error);break
            end=time.perf_counter_ns()
            result=frame_number(image)
            frame=result[0] if result else None
            rows.append(dict(begin_ns=begin,end_ns=end,frame=frame))
            target=math.ceil(seek_target_seconds(percent)*30)
            if frame is not None and target<=frame<target+30:
                _,_,origin=geometry(s.hwnd);marker=result[1]
                point=W.POINT(origin.x+marker[0]+marker[2]//2,origin.y+marker[1]+marker[3]//2)
                if U.GetAncestor(U.WindowFromPoint(point),2)!=s.hwnd:
                    shared['video_error']='occluded';break
                shared.update(video_lower_ns=lower,video_end_ns=end,frame=frame)
                if retain:image.save(retain)
                break
            lower=begin
        shared['video_samples']=rows
    worker=threading.Thread(target=video_observe) if video else None
    if worker:worker.start()
    streak=0;rows=[]
    while (time.perf_counter_ns()-start)/1e9<3:
        begin=time.perf_counter_ns();data,issues=packet(recorder);received=time.perf_counter_ns()
        rms,fraction=tone_match(data,600+137*(percent//10));end=time.perf_counter_ns()
        match=rms>.0002 and fraction>.8
        rows.append(dict(begin_ns=begin,received_ns=received,end_ns=end,frames=len(data),rms=rms,fraction=fraction,match=match,warnings=issues))
        streak=streak+1 if match else 0
        if streak>=2 and 'audio_end_ns' not in shared:
            shared.update(audio_end_ns=end,audio_first_ns=rows[-2]['received_ns'])
        if 'audio_end_ns' in shared and (not video or 'video_end_ns' in shared or 'video_error' in shared):break
    stop.set()
    if worker:worker.join(4)
    shared['audio_samples']=rows
    row=dict(start_ns=start,dispatch_return_ns=dispatch_return,target_percent=percent,
             target_frame=math.ceil(seek_target_seconds(percent)*30),target_frequency=600+137*(percent//10),
             scenario='seek_aligned',request=requested,**shared)
    row['audio_status']='ok' if 'audio_end_ns' in shared else 'timeout'
    row['video_status']=('ok' if 'video_end_ns' in shared else shared.get('video_error','timeout')) if video else 'not_applicable'
    for endpoint in ('audio','video'):
        if endpoint+'_end_ns' in shared:row[endpoint+'_upper_ms']=(shared[endpoint+'_end_ns']-start)/1e6
    if 'video_lower_ns' in shared:row['video_lower_ms']=max(0,(shared['video_lower_ns']-start)/1e6)
    native_input.restore(s)
    if hasattr(s,'native_input'):
        row['native_input']=s.native_input
        if not s.native_input.get('restored'):
            row['status']='input_state_changed'
    return row


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root',type=Path,required=True);p.add_argument('--corpus',type=Path,required=True)
    p.add_argument('--repeats',type=int,default=10);p.add_argument('--cases',default='');p.add_argument('--apps',default='towavue,mpc-hc,vlc')
    p.add_argument('--towavue-baseline',type=Path)
    args=p.parse_args();root=args.root.resolve();root.mkdir(parents=True,exist_ok=True)
    if any(root.iterdir()):raise SystemExit('Use an empty output directory')
    corpus=args.corpus.resolve();apps=prepare_apps(root,Path(__file__).resolve().parents[2])
    names=args.apps.split(',')
    if args.towavue_baseline:
        apps['towavue-before']=dict(apps['towavue'],exe=args.towavue_baseline.resolve())
        names.append('towavue-before')
    apps['mpc-hc']['after']=['/new','/play','/volume','20']
    apps['vlc']['args']=[v for v in apps['vlc']['args'] if v!='--gain=0']+['--gain=0.2','--extraintf=rc','--rc-quiet']
    files=json.loads((corpus/'manifest.json').read_text(encoding='utf-8'))['files']
    if args.cases:files=[f for f in files if f['id'] in args.cases.split(',')]
    schedule=[(n,f) for n in names for f in files]
    receiver=MpcReceiver();rng=random.Random(104062)
    speaker=sc.default_speaker();loopback=sc.get_microphone(id=str(speaker.id),include_loopback=True)
    if not loopback.isloopback:raise RuntimeError('Not a loopback endpoint')
    metadata={'seed':104062,'blocks':args.repeats,'seeks_per_block':6,'duration_seconds':12,'corpus_sha256':sha(corpus/'manifest.json'),
              'apps':{name:{'exe':str(apps[name]['exe']),'sha256':sha(apps[name]['exe'])} for name in names},
              'settings':'isolated profiles; application volume knobs 20%; no acoustic level calibration; default video decoding',
              'endpoint':'first desktop observation at/after requested frame, within 1 s; two target-frequency loopback packets',
              'input_protocol':'owned-native-seek-v1',
              'input':'towavue guarded SendInput move/down/up/neutral with no coalescing and DPI-scaled compact travel (5 DIP endpoint inset); MPC-HC documented slave API; VLC RC on an ephemeral IPv4 loopback port',
              'target_policy':'12 s percentage plus 1/60 s; native click uses manifest container duration; common next 30 fps frame and interior tone interval',
              'preparation':'real foreground ownership is required before timing; an owned-caption activation click may be used and is recorded per session',
              'supersedes':'historical seek runner clicked full client width and could seek before the detector target; do not merge those results',
              'limitations':['No acoustic or physical-input timestamp','Frame-accurate first output is not guaranteed by desktop sampling',
                             'Audio confidence window and packet buffering included','Different command transports; dispatch duration retained',
                             'Native pixel rounding remains bounded and is recorded per request; target timestamps are not bit-identical']}
    (root/'run.json').write_text(json.dumps(metadata,indent=2)+'\n',encoding='utf-8')
    try:
        with (root/'samples.jsonl').open('w',encoding='utf-8') as out:
            for block in range(-1,args.repeats):
                rng.shuffle(schedule)
                for name,f in schedule:
                    folder=root/f'b{block+1:02d}-{name}-{f["id"]}'
                    app=dict(apps[name])
                    if name=='mpc-hc':app['after']=[*app['after'],'/slave',str(receiver.hwnd)]
                    if name=='vlc':
                        with socket.socket() as reservation:
                            reservation.bind(('127.0.0.1',0));port=reservation.getsockname()[1]
                        app['args']=[*app['args'],f'--rc-host=127.0.0.1:{port}']
                    s=Session('towavue' if name=='towavue-before' else name,app,corpus/f['path'],folder,volume=.2)
                    s.seek_kind=f['kind']
                    try:
                        s.ready(None);time.sleep(1);normalize(s.hwnd);time.sleep(.2)
                        preparation=prepare_seek_input(s)
                        if block==0:s.capture().save(folder/'initial.png')
                        if name=='vlc':
                            s.rc_socket=socket.create_connection(('127.0.0.1',port),timeout=3)
                        for step,percent in enumerate([70,20,80,30,60,10]):
                            source_format=f['probe']['format']
                            duration=float(source_format['duration'])
                            if 'matroska' in source_format.get('format_name','').split(','):
                                duration-=float(source_format.get('start_time',0))
                            row=trial(s,percent,receiver,loopback_recorder,f['kind']=='video',folder/f'{step}.png' if block==0 else None,duration)
                            row.update(app=name,case=f['id'],kind=f['kind'],block=block,step=step,phase='warmup' if block<0 else 'measurement',input_preparation=preparation)
                            out.write(json.dumps(row)+'\n');out.flush()
                            print(block,name,f['id'],percent,row['video_status'],row['audio_status'],flush=True)
                            if row.get('status','ok')!='ok' or row['audio_status']!='ok' or row['video_status'] not in ('ok','not_applicable'):
                                # Later targets could otherwise be reached by natural playback
                                # after an ignored input. Stop this sequence and retain failure.
                                break
                            time.sleep(.15)
                    except Exception as error:
                        out.write(json.dumps({'app':name,'case':f['id'],'block':block,'status':'setup_error','detail':str(error)})+'\n');out.flush();print(name,str(error),flush=True)
                    finally:
                        native_input.restore(s)
                        if hasattr(s,'rc_socket'):s.rc_socket.close()
                        forced=s.close()
                        (folder/'close.json').write_text(json.dumps(dict(forced_cleanup=forced,exit_code=s.process.returncode)),encoding='utf-8')
                        if forced or s.process.returncode!=0:
                            raise RuntimeError('Owned fixture did not exit normally')
    finally:receiver.close()


if __name__=='__main__':
    # WASAPI recorder is created and consumed on this main thread.
    speaker=sc.default_speaker();loopback=sc.get_microphone(id=str(speaker.id),include_loopback=True)
    if not loopback.isloopback:raise SystemExit('Loopback device required')
    with loopback.recorder(samplerate=48000,channels=2,blocksize=480) as loopback_recorder:main()
