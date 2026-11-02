#!/usr/bin/env python3
"""Bind actual Rust work functions to the pinned, unchanged Lean Work model."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
REVISION = 'a9f4c2879b6565ae11c17d35efe14475c5830978'
MAX = (1 << 256) - 1


def corpus():
    values = {0, 1, MAX}
    for bit in range(256):
        values.update(v for v in ((1 << bit) - 1, 1 << bit, (1 << bit) + 1) if v <= MAX)
    pairs = set()
    for t in values:
        for h in (0, t - 1, t, t + 1, MAX):
            if 0 <= h <= MAX:
                pairs.add((t, h))
    for i in range(512):
        def word(label):
            return int.from_bytes(hashlib.sha256(f'lattice-work-{label}-{i}'.encode()).digest(), 'big')
        pairs.add((word('target'), word('hash')))
    return [dict(target=str(t), hash=str(h), targetWork='0', hashWork='0', meets=False)
            for t, h in sorted(pairs)]


def check_inputs(rows, original):
    if len(rows) != len(original):
        raise ValueError('case count differs')
    for row, expected in zip(rows, original):
        if set(row) != set(expected) or any(row[k] != expected[k] for k in ('target', 'hash')):
            raise ValueError('input changed')


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('formal', type=Path)
    parser.add_argument('--output-dir', type=Path, default=ROOT / 'target/formal-work')
    args = parser.parse_args()
    formal, out = args.formal.resolve(), args.output_dir.resolve()
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=formal, text=True).strip()
    if revision != REVISION or subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=formal):
        raise ValueError('formal checkout must be pinned and tracked-clean')
    out.mkdir(parents=True, exist_ok=True)
    report = dict(passed=False, formal_revision=revision)
    report_path = out / 'report.json'
    report_path.write_text(json.dumps(report, indent=2) + '\n')
    subprocess.run(['lake', 'build', 'LatticeProofs.Work', 'LatticeProofs.WorkProofs'], cwd=formal / 'lean', check=True)
    inputs = corpus()
    input_path, trace_path = out / 'inputs.json', out / 'work_trace.json'
    input_path.write_text(json.dumps(inputs))
    with trace_path.open('w') as stream:
        subprocess.run(['cargo', 'run', '--locked', '--quiet', '-p', 'lattice-pow', '--example', 'work_trace', '--', str(input_path)], cwd=ROOT, stdout=stream, check=True)
    rows = json.loads(trace_path.read_text())
    check_inputs(rows, inputs)
    checker = ROOT / 'scripts/lean/WorkCheck.lean'
    command = ['lake', 'env', 'lean', '--run', str(checker)]
    result = subprocess.run(command + [str(trace_path)], cwd=formal / 'lean', capture_output=True, text=True)
    (out / 'work.log').write_text(result.stdout + result.stderr)
    print(result.stdout + result.stderr, end='', flush=True)
    result.check_returncode()
    mutants = []
    for name, select, field, replacement in [
        ('zero-target-credit', lambda r: r['target'] == '0', 'targetWork', '1'),
        ('zero-hash-no-saturation', lambda r: r['hash'] == '0', 'hashWork', '0'),
        ('exclusive-boundary', lambda r: r['target'] == r['hash'] == '1', 'meets', False),
    ]:
        altered = copy.deepcopy(rows)
        next(row for row in altered if select(row))[field] = replacement
        path = out / (name + '.json')
        path.write_text(json.dumps(altered))
        result = subprocess.run(command + [str(path)], cwd=formal / 'lean', capture_output=True, text=True)
        (out / (name + '.log')).write_text(result.stdout + result.stderr)
        if result.returncode != 1 or 'differs' not in result.stderr:
            raise ValueError(f'mutant not rejected by model: {name}')
        mutants.append(name)
    report.update(passed=True, cases=len(rows), mutants_rejected=mutants,
                  input_sha256=digest(input_path), trace_sha256=digest(trace_path),
                  checker_sha256=digest(checker), model_sha256=digest(formal / 'lean/LatticeProofs/Work.lean'),
                  rust_source_sha256=digest(ROOT / 'crates/lattice-pow/src/lib.rs'),
                  cargo_lock_sha256=digest(ROOT / 'Cargo.lock'),
                  rustc=subprocess.check_output(['rustc', '--version'], text=True).strip(),
                  lean_toolchain=(formal / 'lean/lean-toolchain').read_text().strip())
    report_path.write_text(json.dumps(report, indent=2) + '\n')
    print(f'{len(mutants)} deliberate result mutations rejected; {report_path}')


if __name__ == '__main__':
    main()
