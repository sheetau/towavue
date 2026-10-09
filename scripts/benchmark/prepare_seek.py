"""Moving video with per-frame identity and position-specific audio tones."""
import argparse
import json
from pathlib import Path
import subprocess
import wave
import numpy as np
from PIL import ImageDraw
from prepare import pattern, digest


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root',type=Path,required=True)
    p.add_argument('--ffmpeg',type=Path,required=True)
    args=p.parse_args();root=args.root.resolve();root.mkdir(parents=True,exist_ok=True)
    if any(root.iterdir()):raise SystemExit('Use an empty corpus directory')
    ffmpeg=args.ffmpeg.resolve();commands=[];files=[]
    def run(arguments):
        cmd=[str(ffmpeg),'-hide_banner','-loglevel','error','-y',*map(str,arguments)]
        commands.append(cmd)
        result=subprocess.run(cmd,capture_output=True,creationflags=subprocess.CREATE_NO_WINDOW)
        if result.returncode:raise RuntimeError(result.stderr.decode('utf-8',errors='replace'))
    def add(path,kind):
        probe=subprocess.run([str(ffmpeg.with_name('ffprobe.exe')),'-v','error','-show_streams','-show_format','-of','json',str(path)],capture_output=True,check=True,creationflags=subprocess.CREATE_NO_WINDOW)
        files.append({'id':path.stem,'path':path.name,'kind':kind,'sha256':digest(path),'bytes':path.stat().st_size,'probe':json.loads(probe.stdout)})
        print(path.name,flush=True)
    # Every 10% of the 12-second clip has a different frequency. No microphone.
    rate=48000;t=np.arange(rate*12)/rate
    freq=600+137*np.minimum((t/1.2).astype(int),9)
    signal=.08*np.sin(2*np.pi*np.cumsum(freq)/rate)
    audio=root/'seek-pcm.wav'
    with wave.open(str(audio),'wb') as out:
        out.setparams((2,2,rate,0,'NONE','not compressed'))
        out.writeframes(np.repeat((signal*32767).astype('<i2')[:,None],2,axis=1).tobytes())
    add(audio,'audio')
    for extension,codec in [('flac','flac'),('mp3','libmp3lame'),('m4a','aac'),('opus','libopus')]:
        path=root/f'seek-{extension}.{extension}'
        run(['-i',audio,'-c:a',codec,*([] if codec=='flac' else ['-b:a','192k']),path]);add(path,'audio')
    master=root/'moving-master.mkv'
    cmd=[str(ffmpeg),'-hide_banner','-loglevel','error','-y','-f','rawvideo','-pixel_format','rgb24','-video_size','1920x1080','-framerate','30','-i','pipe:0','-i',str(audio),'-c:v','ffv1','-level','3','-threads','4','-c:a','pcm_s16le',str(master)]
    commands.append(cmd)
    with (root/'master.log').open('wb') as log:
        proc=subprocess.Popen(cmd,stdin=subprocess.PIPE,stdout=log,stderr=log,creationflags=subprocess.CREATE_NO_WINDOW)
        base=pattern(1920,1080,3000)
        for frame in range(360):
            image=base.copy();draw=ImageDraw.Draw(image)
            unit=135;left=555;top=270
            bits=np.random.default_rng(831+3000+frame).integers(0,2,(4,6))
            for row in range(4):
                for col in range(6):
                    color=(238,42,210) if bits[row,col] else (24,210,240)
                    draw.rectangle((left+col*unit,top+row*unit,left+(col+1)*unit-1,top+(row+1)*unit-1),fill=color)
            x=(frame*17)%1800
            draw.rectangle((x,900,x+100,1000),fill='yellow')
            proc.stdin.write(image.tobytes())
        proc.stdin.close()
        if proc.wait()!=0:raise RuntimeError('Master encode failed')
    variants=[('h264-1080.mp4','libopenh264',['-b:v','8M','-g','60'],'yuv420p',False),
              ('hevc-4k10.mp4','hevc_nvenc',['-preset','p4','-cq','24','-g','120'],'p010le',True),
              ('vp9-1080.webm','libvpx-vp9',['-deadline','realtime','-cpu-used','6','-b:v','6M','-g','120'],'yuv420p',False),
              ('av1-4k10.mp4','libsvtav1',['-preset','12','-crf','32','-g','120','-svtav1-params','lp=4'],'yuv420p10le',True),
              ('h264-long-gop.mp4','libopenh264',['-b:v','8M','-g','300'],'yuv420p',False)]
    for name,codec,options,pixel,upscale in variants:
        path=root/name
        run(['-i',master,*(['-vf','scale=3840:2160'] if upscale else []),'-c:v',codec,*options,'-pix_fmt',pixel,'-c:a','libopus' if name.endswith('webm') else 'aac','-b:a','128k','-threads','4',path]);add(path,'video')
    (root/'manifest.json').write_text(json.dumps({'generator_sha256':digest(Path(__file__)),'files':files,
        'commands':commands,'duration_seconds':12,'fps':30,'frame_identity_base':3000,
        'tone_frequency':'600 + 137 * floor(time / 1.2)',
        'limitations':['Synthetic texture, moving rectangle and changing marker; not natural cinema','4K content is upscaled from the common 1080p master']},indent=2)+'\n',encoding='utf-8')


if __name__=='__main__':main()
