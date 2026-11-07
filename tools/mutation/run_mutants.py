#!/usr/bin/env python3
"""Mutation testing driver with per-test kill attribution.

cargo-mutants generates the Rust mutants; this driver applies each one in an
isolated copy of the repository and runs the Rust unit tests *and* the local
Python tier, recording which individual tests failed. That shows both which
code is poorly tested and which tests actually detect faults.

Subcommands (run inside nix-shell):

  sample  --stride N --min-per-file M LIST.json... > sample.json
  run     --workers W --workdir DIR --out results.jsonl [--baselines B] sample.json
  report  --out-md report.md --out-json summary.json results.jsonl

See docs/mutation-testing.md for the workflow and the latest report.
"""
import argparse
import collections
import concurrent.futures
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
LOCAL_SUITES = ['framework_checks', 'server_checks', 'cli_checks', 'drawing_checks', 'drawing_tools_checks',
                'stroke_rendering_checks', 'coverage_checks', 'drawing_browser', 'selection_checks',
                'conflict_checks', 'proxy_checks', 'chat_checks', 'topology_properties']
SUITE_SECONDS = {'framework_checks': 3, 'server_checks': 1, 'cli_checks': 31, 'drawing_checks': 5,
                 'drawing_tools_checks': 9, 'stroke_rendering_checks': 30, 'coverage_checks': 11,
                 'drawing_browser': 5, 'selection_checks': 30, 'conflict_checks': 55, 'proxy_checks': 9,
                 'chat_checks': 8, 'topology_properties': 102}
RUST_TIMEOUT = 600
MEMORY_LIMIT = os.environ.get('NOTEMAKER_MUTANT_MEMORY', '4G')  # per command; '' disables
TARGETED = 5  # a kill with at most this many failing tests counts as targeted
CONFIRM_THRESHOLD = 3  # failing tests (or bad suites) at or below which suites rerun
BUILD_TIMEOUT = 600


# ---------------------------------------------------------------- sampling

def cmd_sample(args):
    mutants = []
    for path in args.lists:
        mutants += json.loads(Path(path).read_text())
    by_file = collections.defaultdict(list)
    for m in mutants:
        if args.package and m['package'] not in args.package:
            continue
        by_file[m['file']].append(m)
    chosen = []
    for file, items in sorted(by_file.items()):
        items.sort(key=lambda m: (m['span']['start']['line'], m['span']['start']['column'], m['name']))
        want = max(args.min_per_file, -(-len(items) // args.stride))
        want = min(want, len(items))
        # Evenly spaced picks across the file, deterministic.
        picks = sorted({int(i * len(items) / want) for i in range(want)})
        chosen += [items[i] for i in picks]
    for index, m in enumerate(chosen):
        m['id'] = index
    json.dump(chosen, sys.stdout, indent=1)
    print(f'{len(chosen)} of {sum(map(len, by_file.values()))} mutants', file=sys.stderr)


# ---------------------------------------------------------------- applying

def apply_mutant(worker, m):
    """Rewrite the mutated file in the worker copy; return the original text."""
    path = worker / m['file']
    original = path.read_text()
    lines = original.split('\n')
    s, e = m['span']['start'], m['span']['end']

    def offset(line, column):
        return sum(len(l) + 1 for l in lines[:line - 1]) + column - 1

    a, b = offset(s['line'], s['column']), offset(e['line'], e['column'])
    replacement = m.get('insert', m['replacement'])
    if 'insert' in m:
        b = a
    path.write_text(original[:a] + replacement + original[b:])
    return original


# ---------------------------------------------------------------- running

def run_group(command, cwd, timeout, log, env=None):
    """Run in a new process group; kill the whole group on timeout or exit."""
    started = time.monotonic()
    if MEMORY_LIMIT:
        # Some mutants allocate without bound; confine each command to its own
        # cgroup so the kernel kills it rather than unrelated desktop processes.
        command = ['systemd-run', '--user', '--scope', '--quiet', '-p', f'MemoryMax={MEMORY_LIMIT}',
                   '-p', 'MemorySwapMax=0', '--'] + command
    with open(log, 'w') as output:
        process = subprocess.Popen(command, cwd=cwd, stdout=output, stderr=subprocess.STDOUT,
                                   start_new_session=True, env=env)
        timed_out = False
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
        finally:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
    return dict(code=process.returncode, timed_out=timed_out, seconds=round(time.monotonic() - started, 1))


RUNNING = re.compile(r'^\s+Running (?:unittests )?(\S+) \(.*?/deps/([A-Za-z0-9_]+)-[0-9a-f]+\)')
TEST_LINE = re.compile(r'^test (\S+) \.\.\. (ok|FAILED|ignored)')


def run_rust(worker, packages, log):
    command = ['cargo', 'test', '--no-fail-fast']
    command += ['--workspace'] if packages is None else [a for p in packages for a in ('-p', p)]
    status = run_group(command, worker, RUST_TIMEOUT, log)
    tests, binary = {}, None
    for line in Path(log).read_text(errors='replace').splitlines():
        match = RUNNING.match(line)
        if match:
            binary = f'{match.group(2)}:{match.group(1)}'
            continue
        match = TEST_LINE.match(line)
        if match and binary:
            tests[f'rust:{binary}::{match.group(1)}'] = {'ok': 'ok', 'FAILED': 'fail', 'ignored': 'skip'}[match.group(2)]
    compiled = 'error: could not compile' not in Path(log).read_text(errors='replace')
    return dict(status, tests=tests, compiled=compiled)


def run_suite(worker, suite, logdir, slow):
    out = logdir / f'{suite}.jsonl'
    timeout = (SUITE_SECONDS[suite] * 3 + 90) * slow
    status = run_group([sys.executable, str(HERE / 'unittest_json.py'), str(worker / 'tests' / f'{suite}.py'), str(out)],
                       worker, timeout, logdir / f'{suite}.log',
                       env=dict(os.environ, PYTHONUNBUFFERED='1', CARGO_TARGET_DIR=str(worker / 'target')))
    tests = {}
    if out.exists():
        for line in out.read_text().splitlines():
            try:
                record = json.loads(line)
            except ValueError:
                continue
            key = f'py:{suite}:{record["test"]}'
            # A subtest failure followed by the test's own result keeps the failure.
            if tests.get(key) not in ('fail', 'error'):
                tests[key] = record['status']
    return dict(status, tests=tests)


def plan_for(package):
    """(build release CLI, rust packages or None for workspace, run python suites)."""
    if package in ('notemaker_core', 'notemaker_cli', 'baseline'):
        return True, None, True
    if package == 'notemaker_android':
        return False, ['notemaker_android'], False
    if package == 'server':
        return False, False, True
    raise ValueError(package)


def evaluate(worker, m, logdir, slow):
    logdir.mkdir(parents=True, exist_ok=True)
    build, rust_packages, python = plan_for(m['package'])
    result = dict(id=m['id'], name=m['name'], package=m['package'], file=m['file'],
                  function=m.get('function', {}).get('function_name') if m.get('function') else None,
                  genre=m['genre'], line=m['span']['start']['line'], replacement=m['replacement'])
    original = apply_mutant(worker, m) if m['package'] != 'baseline' else None
    started = time.monotonic()
    try:
        if build:
            b = run_group(['cargo', 'build', '--release', '-p', 'notemaker_cli'], worker, BUILD_TIMEOUT,
                          logdir / 'build.log')
            result['build'] = b
            if b['code'] != 0:
                result['outcome'] = 'unviable'
                return result
        jobs = {}
        with concurrent.futures.ThreadPoolExecutor(max_workers=len(LOCAL_SUITES) + 1) as pool:
            if rust_packages is not False:
                jobs['rust'] = pool.submit(run_rust, worker, rust_packages, logdir / 'rust.log')
            if python:
                for suite in LOCAL_SUITES:
                    jobs[suite] = pool.submit(run_suite, worker, suite, logdir, slow)
            results = {name: job.result() for name, job in jobs.items()}
        rust = results.pop('rust', None)
        if rust is not None:
            if not rust['compiled'] and not build:
                result['outcome'] = 'unviable'
                return result
            result['rust'] = {k: rust[k] for k in ('code', 'timed_out', 'seconds', 'compiled')}
            result['tests'] = dict(rust['tests'])
        else:
            result['tests'] = {}
        # Confirm thin evidence: if few tests failed, rerun the failing suites
        # with the mutant still applied and keep only repeated failures. A
        # flaky test or a suite crash under load cannot then pose as a kill.
        failing = {s: [t for t, v in r['tests'].items() if v in ('fail', 'error')] for s, r in results.items()}
        bad = {s for s, r in results.items() if failing[s] or r['code'] != 0 or r['timed_out']}
        if bad and sum(max(1, len(failing[s])) for s in bad) <= CONFIRM_THRESHOLD:
            result['confirmed'] = sorted(bad)
            (logdir / 'confirm').mkdir(exist_ok=True)
            for suite in sorted(bad):
                again = run_suite(worker, suite, logdir / 'confirm', slow)
                first = results[suite]
                for t, v in first['tests'].items():
                    if v in ('fail', 'error') and again['tests'].get(t) not in ('fail', 'error'):
                        first['tests'][t] = 'ok-on-rerun'
                if first['code'] != 0 and again['code'] == 0:
                    first['code'] = 0
                if first['timed_out'] and not again['timed_out']:
                    first['timed_out'] = False
                    first['tests'].update({t: v for t, v in again['tests'].items() if t not in first['tests']})
        result['suites'] = {}
        for suite, r in results.items():
            result['suites'][suite] = {k: r[k] for k in ('code', 'timed_out', 'seconds')}
            result['tests'].update(r['tests'])
        result['outcome'] = 'ran'
        return result
    finally:
        result['seconds'] = round(time.monotonic() - started, 1)
        if original is not None:
            (worker / m['file']).write_text(original)


def prepare_worker(workdir, index):
    worker = workdir / f'w{index}'
    files = subprocess.run(['git', 'ls-files', '-co', '--exclude-standard'], cwd=ROOT, capture_output=True,
                           text=True, check=True).stdout.splitlines()
    files = [f for f in files if not f.startswith('boox-notes-decompiled/') and (ROOT / f).is_file()]
    list_file = workdir / f'files-{index}.txt'
    list_file.write_text('\n'.join(files) + '\n')
    # Compare by content and do not preserve mtimes: a file restored with its
    # old mtime would look older than artifacts built from a mutated copy, and
    # cargo would keep using the stale mutated build.
    subprocess.run(['rsync', '-rlpc', '--delete', '--exclude', 'target', '--exclude', '.test-artifacts',
                    '--exclude', '.hypothesis', '--files-from', str(list_file), str(ROOT) + '/', str(worker) + '/'],
                   check=True)
    if not (worker / 'target' / 'release').exists() and (ROOT / 'target' / 'release').exists():
        (worker / 'target').mkdir(exist_ok=True)
        shutil.copytree(ROOT / 'target' / 'release', worker / 'target' / 'release', symlinks=True)
    return worker


def cmd_run(args):
    workdir = Path(args.workdir).resolve()
    workdir.mkdir(parents=True, exist_ok=True)
    mutants = json.loads(Path(args.sample).read_text())
    out = Path(args.out)
    done = set()
    if out.exists():
        for line in out.read_text().splitlines():
            record = json.loads(line)
            done.add((record['package'], record['id']))
    queue = [dict(id=f'baseline-{i}', name='baseline', package='baseline', file='', genre='baseline',
                  replacement='', span={'start': {'line': 0}}) for i in range(args.baselines)]
    queue = [m for m in queue if ('baseline', m['id']) not in done]
    queue += [m for m in mutants if (m['package'], m['id']) not in done]
    if args.limit:
        queue = queue[:args.limit]
    print(f'{len(queue)} to run, {len(done)} already done', flush=True)
    workers = [prepare_worker(workdir, i) for i in range(args.workers)]
    lock = threading.Lock()
    pending = collections.deque(queue)
    counts = collections.Counter()
    started = time.monotonic()

    def loop(worker):
        while True:
            with lock:
                if not pending:
                    return
                m = pending.popleft()
            logdir = workdir / 'logs' / f'{m["package"]}-{m["id"]}'
            if logdir.exists():
                shutil.rmtree(logdir)
            try:
                result = evaluate(worker, m, logdir, args.slow)
            except Exception as error:  # Keep the run going; record the driver fault.
                result = dict(id=m['id'], name=m['name'], package=m['package'], outcome='driver-error',
                              error=repr(error))
            with lock:
                with out.open('a') as handle:
                    handle.write(json.dumps(result) + '\n')
                counts[result['outcome']] += 1
                total = sum(counts.values())
                rate = (time.monotonic() - started) / total
                print(f'[{total}/{len(queue)} eta {rate * (len(queue) - total) / 60:.0f}m] '
                      f'{result["outcome"]} {result.get("seconds")}s {m["name"]}', flush=True)
            if result['outcome'] == 'ran' and not result.get('confirmed') and not args.keep_logs:
                shutil.rmtree(logdir, ignore_errors=True)

    threads = [threading.Thread(target=loop, args=(w,)) for w in workers]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    print(dict(counts))


# ---------------------------------------------------------------- reporting

def load(path):
    return [json.loads(line) for line in Path(path).read_text().splitlines() if line.strip()]


def logical(test):
    """Collapse a test executed by several suites (inheritance or import) to its definition."""
    if test.startswith('py:') and ' [from ' in test:
        method = test.split(':', 2)[2].split(' [from ')[0].split('.')[-1]
        owner = test.rsplit(' [from ', 1)[1].rstrip(']')
        return f'py:{owner}.{method}'
    if test.startswith('py:'):
        _, suite, rest = test.split(':', 2)
        if rest.endswith(')') and ' (' not in rest:
            # unittest names a fixture error "setUpClass (module.Class)".
            return f'py:{suite}.{rest[:-1].rsplit(".", 1)[-1]}.<fixture>'
        return f'py:{suite}.{rest}'
    # cargo names the Android lib test binary after its lib target, `notemaker`.
    return test.replace('rust:notemaker:src/lib.rs::', 'rust:notemaker_android:src/lib.rs::')


def suite_of(test):
    return test.split(':')[1] if test.startswith('py:') else 'rust'


def cmd_report(args):
    records = load(args.results)
    baselines = [r for r in records if r['package'] == 'baseline' and r['outcome'] == 'ran']
    if not baselines:
        sys.exit('no baseline runs')
    # A test counts only if it passed in every baseline run.
    all_tests = set().union(*(r['tests'] for r in baselines))
    stable = {t for t in all_tests if all(r['tests'].get(t) == 'ok' for r in baselines)}
    flaky = sorted(t for t in all_tests if t not in stable
                   and any(r['tests'].get(t) == 'ok' for r in baselines))
    broken = sorted(t for t in all_tests if not any(r['tests'].get(t) == 'ok' for r in baselines))
    suite_tests = collections.defaultdict(set)
    for t in stable:
        if t.startswith('py:'):
            suite_tests[t.split(':')[1]].add(t)
    suite_seconds = collections.defaultdict(list)
    for r in baselines:
        for suite, v in r.get('suites', {}).items():
            suite_seconds[suite].append(v['seconds'])
        if r.get('rust'):
            suite_seconds['rust'].append(r['rust']['seconds'])

    mutants = [r for r in records if r['package'] != 'baseline']
    rerun_flakes = collections.Counter()
    summary = dict(baselines=len(baselines), stable_tests=sorted(stable), flaky_tests=flaky, broken_tests=broken,
                   suite_seconds={k: round(sum(v) / len(v), 1) for k, v in suite_seconds.items()}, mutants=[])
    for r in mutants:
        if r['outcome'] != 'ran':
            r['verdict'] = r['outcome']
            summary['mutants'].append(r)
            continue
        for t, v in r['tests'].items():
            if v == 'ok-on-rerun':
                rerun_flakes[t] += 1
        killers = sorted(t for t in stable if t in r['tests'] and r['tests'][t] in ('fail', 'error'))
        # Tests that never reported because their suite hung or crashed.
        timeouts = sorted(s for s, v in r.get('suites', {}).items() if v['timed_out'])
        crashed = []
        for suite, v in r.get('suites', {}).items():
            missing = [t for t in suite_tests[suite] if t not in r['tests']]
            if missing and not v['timed_out'] and v['code'] != 0:
                crashed.append(suite)
        rust = r.get('rust')
        rust_timeout = bool(rust and rust['timed_out'])
        rust_crash = bool(rust and rust['code'] != 0 and not rust['timed_out']
                          and not any(t.startswith('rust:') for t in killers))
        r['killers'] = killers
        r['timeout_suites'] = timeouts + (['rust'] if rust_timeout else [])
        r['crash_suites'] = crashed + (['rust'] if rust_crash else [])
        if killers or crashed or rust_crash:
            r['verdict'] = 'killed'
        elif timeouts or rust_timeout:
            r['verdict'] = 'timeout'
        else:
            r['verdict'] = 'survived'
        r.pop('tests', None)
        summary['mutants'].append(r)
    summary['rerun_flakes'] = dict(rerun_flakes)

    Path(args.out_json).write_text(json.dumps(summary, indent=1))
    Path(args.out_md).write_text(render_markdown(summary, stable))
    print(f'wrote {args.out_md} and {args.out_json}')


def score(items):
    viable = [m for m in items if m['verdict'] in ('killed', 'survived', 'timeout')]
    caught = [m for m in viable if m['verdict'] in ('killed', 'timeout')]
    return len(caught), len(viable), (100.0 * len(caught) / len(viable) if viable else float('nan'))


def caught_by(m):
    """Suites that caught a mutant (failing test, crash or timeout)."""
    return ({suite_of(t) for t in m['killers']} | set(m['crash_suites']) | set(m['timeout_suites']))


def render_markdown(summary, stable):
    mutants = summary['mutants']
    packages = sorted({m['package'] for m in mutants})
    lines = ['# Mutation testing results (generated)', '',
             f'Baseline runs: {summary["baselines"]}; stable test executions: {len(stable)} '
             f'({len({logical(t) for t in stable})} distinct tests); flaky tests excluded: '
             f'{len(summary["flaky_tests"])}; tests failing in every baseline: {len(summary["broken_tests"])}.',
             f'Failures that did not repeat on the confirmation rerun (dropped): '
             f'{sum(summary["rerun_flakes"].values())}.', '']
    verdicts = collections.Counter(m['verdict'] for m in mutants)
    lines += ['## Outcomes', '', '| Verdict | Count |', '|---|---|']
    lines += [f'| {k} | {v} |' for k, v in verdicts.most_common()]
    lines += ['']

    def table(title, key, minimum=1):
        groups = collections.defaultdict(list)
        for m in mutants:
            groups[key(m)].append(m)
        rows = []
        for name, items in groups.items():
            caught, viable, pct = score(items)
            if viable >= minimum:
                rows.append((pct, name, caught, viable, len(items) - viable))
        rows.sort(key=lambda r: (r[0], -r[3]))
        out = [f'## {title}', '', '| Score | Caught / viable | Unviable | Name |', '|---|---|---|---|']
        out += [f'| {pct:.0f}% | {c}/{v} | {u} | `{n}` |' for pct, n, c, v, u in rows]
        return out + ['']

    lines += table('By package', lambda m: m['package'])
    lines += table('By file', lambda m: m['file'])
    lines += table('By mutation genre', lambda m: f'{m["package"]}:{m["genre"]}')

    # Which suites catch which package's mutants.
    viable = [m for m in mutants if m['verdict'] in ('killed', 'survived', 'timeout')]
    grid = collections.defaultdict(collections.Counter)
    sole = collections.Counter()
    for m in viable:
        suites = caught_by(m)
        for s in suites:
            grid[s][m['package']] += 1
        if len(suites) == 1:
            sole[next(iter(suites))] += 1
    totals = collections.Counter(m['package'] for m in viable)
    lines += ['## Suites', '',
              'Mutants each suite caught, per package (viable mutants in parentheses). "Sole" counts mutants '
              'no other suite caught; "per min" is caught mutants per minute of baseline suite runtime.', '',
              '| Suite | ' + ' | '.join(f'{p} ({totals[p]})' for p in packages) + ' | Sole | Runtime s | Per min |',
              '|---|' + '---|' * (len(packages) + 3)]
    for s in sorted(grid, key=lambda s: -sum(grid[s].values())):
        seconds = summary['suite_seconds'].get(s, float('nan'))
        total = sum(grid[s].values())
        lines.append(f'| `{s}` | ' + ' | '.join(str(grid[s][p]) for p in packages)
                     + f' | {sole[s]} | {seconds} | {60 * total / seconds:.1f} |')
    never = sorted(set(summary['suite_seconds']) - set(grid))
    if never:
        lines += ['', 'Suites that caught nothing: ' + ', '.join(f'`{s}`' for s in never)]
    lines += ['']

    # Test effectiveness, with duplicates collapsed to the defining test.
    kills = collections.Counter()
    unique = collections.Counter()
    targeted = collections.Counter()
    executions = collections.Counter(logical(t) for t in stable)
    for m in viable:
        if m['verdict'] not in ('killed', 'timeout'):
            continue
        names = {logical(t) for t in m['killers']}
        for t in names:
            kills[t] += 1
            if len(names) <= TARGETED:
                targeted[t] += 1
        if len(names) == 1 and not m['crash_suites'] and not m['timeout_suites']:
            unique[next(iter(names))] += 1
    lines += ['## Tests by kills', '',
              'Duplicated executions (a `ServerChecks` test inherited by another suite) are collapsed. '
              '"Sole" = the only test that caught the mutant. "Targeted" = kills where at most '
              f'{TARGETED} tests failed, i.e. not a fault that breaks everything.', '',
              '| Test | Kills | Targeted | Sole kills | Executions per run |', '|---|---|---|---|---|']
    for t, n in kills.most_common(60):
        lines.append(f'| `{t}` | {n} | {targeted[t]} | {unique[t]} | {executions[t]} |')
    lines += ['', '## Tests by targeted kills', '', '| Test | Targeted | Kills |', '|---|---|---|']
    for t, n in targeted.most_common(30):
        lines.append(f'| `{t}` | {n} | {kills[t]} |')
    zero = sorted(t for t in executions if kills[t] == 0)
    lines += ['', f'## Tests that caught no mutant ({len(zero)} of {len(executions)})', '']
    lines += [f'- `{t}`' + (f' (runs {executions[t]}x)' if executions[t] > 1 else '') for t in zero]

    dup = sorted(((n, t) for t, n in executions.items() if n > 1), reverse=True)
    if dup:
        extra = sum(n - 1 for n, _ in dup)
        lines += ['', f'## Duplicate executions ({extra} redundant per run)', '']
        lines += [f'- `{t}`: {n}x' for n, t in dup]

    lines += ['', '## Functions with the most surviving mutants', '', '| Survived | Caught | Function |',
              '|---|---|---|']
    fn = collections.defaultdict(lambda: [0, 0])
    for m in viable:
        key = f'{m["file"]}::{m.get("function") or "?"}'
        fn[key][0 if m['verdict'] == 'survived' else 1] += 1
    for key, (s, c) in sorted(fn.items(), key=lambda kv: (-kv[1][0], kv[1][1]))[:40]:
        if s:
            lines.append(f'| {s} | {c} | `{key}` |')

    lines += ['', '## Surviving mutants', '']
    current = None
    for m in sorted((m for m in mutants if m['verdict'] == 'survived'), key=lambda m: (m['file'], m['line'])):
        if m['file'] != current:
            current = m['file']
            lines += ['', f'### `{current}`', '']
        lines.append(f'- `{m["name"].split(": ", 1)[0].rsplit("/", 1)[-1]}` {m["name"].split(": ", 1)[-1]}')
    caught_timeout = [m for m in mutants if m['verdict'] == 'timeout']
    if caught_timeout:
        lines += ['', '## Caught only by a timeout', ''] + [f'- `{m["name"]}`' for m in caught_timeout]
    if summary['flaky_tests']:
        lines += ['', '## Flaky in baseline (excluded)', ''] + [f'- `{t}`' for t in summary['flaky_tests']]
    if summary['rerun_flakes']:
        lines += ['', '## Failures dropped by the confirmation rerun', '']
        lines += [f'- `{t}`: {n}' for t, n in sorted(summary['rerun_flakes'].items(), key=lambda kv: -kv[1])]
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('sample')
    p.add_argument('lists', nargs='+')
    p.add_argument('--stride', type=int, default=12)
    p.add_argument('--min-per-file', type=int, default=3)
    p.add_argument('--package', action='append')
    p.set_defaults(func=cmd_sample)
    p = sub.add_parser('run')
    p.add_argument('sample')
    p.add_argument('--workers', type=int, default=4)
    p.add_argument('--workdir', default='/tmp/notemaker-mutants')
    p.add_argument('--out', required=True)
    p.add_argument('--baselines', type=int, default=0)
    p.add_argument('--limit', type=int, default=0)
    p.add_argument('--slow', type=float, default=1.0, help='Multiply suite timeouts')
    p.add_argument('--keep-logs', action='store_true')
    p.set_defaults(func=cmd_run)
    p = sub.add_parser('report')
    p.add_argument('results')
    p.add_argument('--out-md', required=True)
    p.add_argument('--out-json', required=True)
    p.set_defaults(func=cmd_report)
    args = parser.parse_args()
    args.func(args)


if __name__ == '__main__':
    main()
