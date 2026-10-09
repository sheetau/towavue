"""Generate multiple images and large-folder fixtures without owner media."""
import argparse
import json
from pathlib import Path
from prepare import pattern, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if any(root.iterdir()):
        raise SystemExit('Use an empty corpus directory')
    records = []
    strata = [('jpeg1080',1920,1080,'jpg',{'quality':90}),
              ('jpeg24mp',6000,4000,'jpg',{'quality':95}),
              ('png4k',3840,2160,'png',{})]
    for group_index,(group,w,h,ext,options) in enumerate(strata):
        folder=root/group;folder.mkdir()
        entries=[]
        for i in range(16):
            identity=101+group_index*100+i
            path=folder/f'{i:04d}.{ext}'
            pattern(w,h,identity).save(path,**options)
            entries.append({'path':str(path.relative_to(root)), 'identity':identity,
                            'sha256':digest(path),'bytes':path.stat().st_size})
        records.append({'id':group,'width':w,'height':h,'files':entries})
        print(group,flush=True)
    large=root/'large1000';large.mkdir()
    entries=[]
    for i in range(1000):
        path=large/f'{i:04d}.jpg'
        pattern(640,360,1001+i).save(path,quality=90)
        entries.append({'path':str(path.relative_to(root)),'identity':1001+i,
                        'sha256':digest(path),'bytes':path.stat().st_size})
    records.append({'id':'large1000','width':640,'height':360,'files':entries,
                    'note':'1000 individually identifiable generated JPEGs'})
    (root/'manifest.json').write_text(json.dumps({'generator_sha256':digest(Path(__file__)),
        'groups':records},indent=2)+'\n',encoding='utf-8')


if __name__=='__main__':
    main()
