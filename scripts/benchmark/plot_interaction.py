"""Export workload-specific charts from interaction summaries."""
import argparse
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    args = parser.parse_args()
    rows = json.loads((args.root / 'summary.json').read_text(encoding='utf-8'))['summaries']
    pairs = sorted({(r['scenario'], r['metric']) for r in rows})
    colors = {'towavue': '#087f8c', 'irfanview': '#e17c35', 'mpc-hc': '#8055ad', 'vlc': '#cc486c'}
    for scenario, metric in pairs:
        subset = [r for r in rows if (r['scenario'], r['metric']) == (scenario, metric)]
        cases = sorted({r['case'] for r in subset})
        apps = sorted({r['app'] for r in subset})
        fig, ax = plt.subplots(figsize=(max(8, len(cases)*1.3), 5.5))
        width = .8/len(apps)
        for index, app in enumerate(apps):
            entries = [next((r for r in subset if r['app'] == app and r['case'] == case), {}) for case in cases]
            medians = [r.get('median_ms', np.nan) for r in entries]
            positions = np.arange(len(cases)) + (index-(len(apps)-1)/2)*width
            ax.bar(positions, medians, width, label=app, color=colors.get(app))
            for x, y, row in zip(positions, medians, entries):
                if np.isfinite(y):
                    label = f'{y:.0f}' if y >= 10 else (f'{y:.1f}' if y >= 1 else f'{y:.3f}')
                    ax.text(x, y, label, ha='center', va='bottom', fontsize=8)
                    low, high = row.get('median_ci_low_ms', y), row.get('median_ci_high_ms', y)
                    ax.plot([x, x], [low, high], color='black', linewidth=1)
        ax.set_xticks(np.arange(len(cases)), cases, rotation=25, ha='right')
        if scenario == 'desktop_pacing':
            ax.set_ylabel('Observed frame-change interval (ms)')
            ax.axhline(1000/30, color='gray', linestyle='--', linewidth=1, label='30 fps source interval')
        elif scenario == 'internal_waveform':
            ax.set_ylabel('Internal API return (ms; lower is faster)')
        else:
            ax.set_ylabel('Observed median completion (ms; lower is faster)')
        ax.set_title(f'{scenario.replace("_", " ")} — {metric}')
        ax.grid(axis='y', alpha=.2); ax.set_axisbelow(True); ax.legend()
        note = ('Internal runtime API · warm source bytes · process launch excluded · distinct cache stages'
                if scenario == 'internal_waveform' else
                'Synthetic fixtures · warm OS cache · native commands · successful trials only')
        fig.text(.01, .01, note+'\n'
                 'Lines: block bootstrap CI; desktop sampling bounds apply only to visual endpoints. See report for failures/settings.', fontsize=8)
        fig.subplots_adjust(bottom=.29, top=.88, left=.1, right=.98)
        for extension in ('png', 'svg'):
            fig.savefig(args.root / f'{scenario}-{metric}.{extension}', dpi=160)
        plt.close(fig)


if __name__ == '__main__':
    main()
