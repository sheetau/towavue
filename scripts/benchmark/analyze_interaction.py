"""Scenario summaries with block bootstrap, retaining unsuccessful attempts."""
import argparse
from collections import defaultdict
import csv
import json
from pathlib import Path
import numpy as np


def summarize(rows,value,lower=None):
    valid=[r for r in rows if isinstance(r.get(value),(int,float))]
    result={'attempts':len(rows),'successful':len(valid)}
    if not valid:return result
    values=np.array([r[value] for r in valid]);ordered=np.sort(values)
    result.update(median_ms=float(np.median(values)),p95_ms=float(ordered[int(np.ceil(.95*len(values)))-1]))
    if lower and all(isinstance(r.get(lower),(int,float)) for r in valid):
        lower_values=np.array([r[lower] for r in valid])
        result['median_lower_ms']=float(np.median(lower_values))
        result['p95_lower_ms']=float(np.sort(lower_values)[int(np.ceil(.95*len(valid)))-1])
        result['median_interval_width_ms']=float(np.median(values-lower_values))
    blocks=defaultdict(list)
    for r in valid:blocks[r.get('block',0)].append(r[value])
    grouped=list(blocks.values());rng=np.random.default_rng(104066);estimates=[]
    for _ in range(2000):
        indices=rng.integers(0,len(grouped),len(grouped))
        estimates.append(np.median(np.concatenate([grouped[i] for i in indices])))
    result.update(blocks=len(grouped),median_ci_low_ms=float(np.percentile(estimates,2.5)),median_ci_high_ms=float(np.percentile(estimates,97.5)))
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('root',type=Path);args=p.parse_args()
    rows=[json.loads(s) for s in (args.root/'samples.jsonl').read_text(encoding='utf-8').splitlines()]
    grouped=defaultdict(list);errors=[]
    for row in rows:
        if row.get('phase')=='warmup' or row.get('block',0)<0:continue
        if row.get('status')=='setup_error':errors.append(row);continue
        grouped[(row['app'],row['case'],row.get('scenario','seek' if 'target_percent' in row else 'startup'))].append(row)
    summaries=[]
    for (app,case,scenario),items in grouped.items():
        metrics=[('latency','latency_upper_ms','latency_lower_ms'),('video','video_upper_ms','video_lower_ms'),
                 ('audio','audio_upper_ms',None),('recognizable','recognizable_upper_ms','recognizable_lower_ms'),
                 ('converged','converged_upper_ms',None),
                 ('dialog_close','dialog_close_upper_ms','dialog_close_lower_ms'),
                 ('post_dialog','post_dialog_upper_ms','post_dialog_lower_ms'),
                 ('dialog_destroy','dialog_destroy_upper_ms','dialog_destroy_lower_ms'),
                 ('post_destroy','post_destroy_upper_ms','post_destroy_lower_ms'),
                 ('waveform_prepare','native_ms',None),
                 ('waveform_memory_hit','memory_hit_ms',None),
                 ('waveform_disk_hit','disk_hit_ms',None),
                 ('waveform_detail','detail_ms',None),
                 ('per_trial_median_frame_interval','observed_interval_median_ms',None),
                 ('per_trial_p95_frame_interval','observed_interval_p95_ms',None)]
        for metric,value,lower in metrics:
            expected = any(value in r for r in items)
            expected |= metric == 'video' and any(r.get('video_status') not in (None, 'not_applicable') for r in items)
            expected |= metric == 'audio' and any('audio_status' in r or r.get('kind') == 'audio' for r in items)
            expected |= metric == 'latency' and any('latency_upper_ms' in r for r in items)
            if not expected:continue
            summaries.append(dict(app=app,case=case,scenario=scenario,metric=metric,**summarize(items,value,lower)))
    output={'summaries':summaries,'setup_errors':errors,'warmup_rows':sum(r.get('block',0)<0 for r in rows),
            'ci':'2000 percentile bootstrap resamples of whole process/session blocks; successful observations only',
            'limits':'Capture interval is separate from block-sampling CI. Setup failures retained separately, never counted as successful operations.'}
    (args.root/'summary.json').write_text(json.dumps(output,indent=2)+'\n',encoding='utf-8')
    keys=list(dict.fromkeys(k for r in summaries for k in r))
    with (args.root/'summary.csv').open('w',newline='',encoding='utf-8') as file:
        writer=csv.DictWriter(file,fieldnames=keys);writer.writeheader();writer.writerows(summaries)
    print(json.dumps({'groups':len(summaries),'setup_errors':len(errors)}))


if __name__=='__main__':main()
