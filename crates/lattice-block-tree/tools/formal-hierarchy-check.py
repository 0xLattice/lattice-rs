#!/usr/bin/env python3
"""Check real Rust reducer/capability observations with unchanged TraceHierarchy.
Fixtures provide trusted facts and verdicts, not authenticated network admission.
"""
import argparse
import copy
import hashlib
import importlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
PIN = 'a9f4c2879b6565ae11c17d35efe14475c5830978'

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('formal', type=Path, nargs='?', default=ROOT.parents[2] / 'lattice-formal')
    parser.add_argument('--output-dir', type=Path, default=ROOT.parents[1] / 'target/formal-hierarchy')
    args = parser.parse_args()
    formal = args.formal.resolve()
    if subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=formal, text=True).strip() != PIN:
        raise ValueError('formal revision mismatch')
    if subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=formal):
        raise ValueError('formal checkout has tracked modifications')
    if not os.environ.get('JAVA') and Path('/opt/homebrew/opt/openjdk/bin/java').exists():
        os.environ['JAVA'] = '/opt/homebrew/opt/openjdk/bin/java'
    sys.path.insert(0, str(formal / 'scripts'))
    checker = importlib.import_module('hierarchy_trace_check')
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    (output / 'report.json').unlink(missing_ok=True)
    report = {'formal_revision': PIN, 'scope': 'finite trusted-fact reducer event refinement plus real parent-anchor capabilities; supplied verdicts are not production transition/admission decisions', 'traces': [], 'negative_controls': []}
    sources = [Path(__file__).resolve(), ROOT / 'examples/hierarchy_trace.rs',
               ROOT / 'src/tree.rs', ROOT / 'src/continuity.rs', ROOT / 'src/facts.rs',
               ROOT.parents[1] / 'Cargo.lock',
               formal / 'specs/Hierarchy.tla', formal / 'specs/TraceHierarchy.tla',
               formal / 'scripts/hierarchy_trace_check.py', formal / 'scripts/trace_check.py']
    report['source_sha256'] = {str(p): sha(p) for p in sources}
    report['rustc'] = subprocess.check_output(['rustc', '--version'], text=True).strip()
    for mode in ['parent-first', 'child-first']:
        raw = subprocess.check_output(['cargo', 'run', '--locked', '--quiet', '--example', 'hierarchy_trace', '--', mode], cwd=ROOT)
        path = output / f'trace-{mode}.json'
        path.write_bytes(raw)
        trace = json.loads(raw)
        events = trace['events']
        # Non-vacuity: positive executions, attribution changes and actual refusals.
        if len(events) != 36:
            raise AssertionError('unexpected event coverage; review and update explicitly')
        for reason in ['continuity', 'rootExclusion', 'grindElsewhere']:
            assert any(e['reason'] == reason and not e['accepted'] for e in events), reason
        assert all(events[-1]['after'][ch]['validatedTip'] is not None for ch in ['P', 'C', 'G'])
        ok, detail = checker.check(path)
        print(detail, flush=True)
        if not ok:
            raise RuntimeError(detail)
        report['traces'].append({'mode': mode, 'events': len(events), 'sha256': sha(path)})
        if mode == 'child-first':
            ok, detail = checker.selftest(path)
            print(detail, flush=True)
            if not ok:
                raise RuntimeError(detail)
            report['negative_controls'].append({'upstream_selftest': detail})
            # Target the propagated non-carrier raise explicitly, not trace syntax.
            for ch, block in [('C', 'c1'), ('G', 'g0')]:
                mutated = copy.deepcopy(trace)
                index = next(i for i, e in enumerate(events) if e['kind'] == 'observe' and e['block'] == 'p4' and e['qty'] == 20)
                before = events[index-1]['after'][ch]['weights'][block]
                assert int(events[index]['after'][ch]['weights'][block]) > int(before)
                mutated['events'][index]['after'][ch]['weights'][block] = before
                bad = output / f'negative-missing-attribution-{ch}.json'
                bad.write_text(json.dumps(mutated))
                accepted, result = checker.check(bad)
                if accepted or 'WeightsMatch' not in result:
                    raise AssertionError(f'missing {ch} attribution was not rejected: {result}')
                print(f'negative missing {ch} recursive attribution: caught', flush=True)
                report['negative_controls'].append({'missing_attribution': ch, 'rejected_by': 'WeightsMatch'})
    report['passed'] = True
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(f'Report: {output / "report.json"}')

if __name__ == '__main__':
    main()
