#!/usr/bin/env python3
"""Tiered test entry point; external devices are always explicitly selected."""
import argparse
import datetime
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
SUITES = {
    'local': ['framework_checks', 'server_checks', 'cli_checks', 'drawing_checks', 'drawing_tools_checks', 'coverage_checks', 'drawing_browser', 'selection_checks', 'conflict_checks', 'proxy_checks',
              'chat_checks', 'topology_properties'],
    'emulator': ['integration', 'cli_android', 'drawing_android', 'files_android', 'chat_ui', 'performance'],
    'hardware': ['tablet_checks'],
    'network': ['network_checks'],
}
DEFAULT_SECONDS = {
    'framework_checks': 3, 'server_checks': 1, 'cli_checks': 31,
    'drawing_checks': 5, 'drawing_tools_checks': 9, 'coverage_checks': 11,
    'drawing_browser': 5, 'selection_checks': 30, 'conflict_checks': 55,
    'proxy_checks': 9, 'chat_checks': 8, 'topology_properties': 102,
    'integration': 353, 'cli_android': 341, 'drawing_android': 253,
    'files_android': 186, 'chat_ui': 150, 'performance': 207,
    'tablet_checks': 60, 'network_checks': 60,
}
CLAIMS = {
    'integration': {'emu:5580', 'emu:5582', 'server:18765', 'browser:19222'},
    'cli_android': {'emu:5584', 'server:18766', 'browser:19224'},
    'drawing_android': {'emu:5590'},
    'files_android': {'emu:5590'},
    'chat_ui': {'emu:5586'},
    'performance': {'emu:5586', 'performance'},
}
EMULATOR_SUITES = {'integration', 'cli_android', 'drawing_android', 'files_android', 'chat_ui', 'performance'}


def recent_averages(root=ROOT, limit=5):
    """Mean of the latest completed durations for each suite, with historical fallbacks."""
    history = root / '.test-artifacts' / 'runs' / 'timing-history.json'
    records = []
    if history.exists():
        try:
            value = json.loads(history.read_text())
            records = value.get('runs', []) if isinstance(value, dict) else []
        except (OSError, ValueError):
            records = []
    known_runs = {run.get('timestamp') for run in records if run.get('timestamp')}
    for path in sorted((root / '.test-artifacts' / 'runs').glob('*/report.json'),
                       key=lambda item: item.stat().st_mtime):
        run_stamp = path.parent.name
        if run_stamp in known_runs:
            continue
        try:
            report = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        records.append({'timestamp': run_stamp, 'results': report.get('results', [])})
    durations = {}
    for run in records:
        for result in run.get('results', []):
            if result.get('status') in ('pass', 'fail') and isinstance(result.get('seconds'), (int, float)):
                durations.setdefault(result['name'], []).append(float(result['seconds']))
    return {name: sum(values[-limit:]) / len(values[-limit:]) for name, values in durations.items()}


def suite_estimate(name, averages):
    return averages.get(name, DEFAULT_SECONDS.get(name, 30.0))


def claims_conflict(name, active, active_emulator_count, isolated_networks=False):
    if not isolated_networks and CLAIMS.get(name, set()) & active:
        return True
    # Integration owns two emulators at once. Keep it to the two-emulator
    # resource budget on this host by not pairing it with another emulator
    # suite, even when suite containers isolate their ports.
    if name == 'integration' and active_emulator_count:
        return True
    if name in EMULATOR_SUITES and 'integration' in active:
        return True
    if name == 'performance' and active_emulator_count:
        return True
    if name in EMULATOR_SUITES and 'performance' in active:
        return True
    return False


def plan(tiers, suites, serial=None, topology=None):
    selected = list(dict.fromkeys(name for tier in tiers for name in SUITES[tier]))
    if suites:
        unknown = set(suites) - set(selected)
        if unknown:
            raise ValueError('Suites outside selected tiers: ' + ', '.join(sorted(unknown)))
        selected = [name for name in selected if name in suites]
    return [dict(name=name, skip=('No --serial supplied' if name == 'tablet_checks' and not serial
                                 else 'No --topology supplied' if name == 'network_checks' and not topology
                                 else None)) for name in selected]


def execute(command, log):
    """Keep a complete log while streaming output; return the actual child status."""
    with log.open('w') as output:
        process = subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, text=True,
                                   env=dict(os.environ, PYTHONUNBUFFERED='1'))
        try:
            for line in process.stdout:
                output.write(line)
                output.flush()
                print(line, end='', flush=True)
            return process.wait()
        except BaseException:
            # Let Python suites run their finally blocks and stop owned emulators.
            process.send_signal(signal.SIGINT)
            try:
                process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            raise
        finally:
            process.stdout.close()


def suite_command(name, container, worker_tmp):
    script = ROOT / 'tests' / (name + '.py')
    if not container:
        return [sys.executable, str(script)]
    (worker_tmp / 'home' / '.android').mkdir(parents=True, exist_ok=True)
    docker = os.environ.get('NOTEMAKER_DOCKER', 'docker')
    command = [docker, 'run', '--rm', '--network', 'bridge', '--shm-size', '1g',
               '--mount', f'type=bind,src=/nix/store,dst=/nix/store,readonly',
               '--mount', f'type=bind,src={ROOT},dst=/workspace',
               '--mount', f'type=bind,src={worker_tmp},dst=/tmp',
               '--workdir', '/workspace', '--env', 'HOME=/tmp/home', '--env', 'TMPDIR=/tmp',
               '--env', 'ANDROID_USER_HOME=/tmp/home/.android', '--env', 'ANDROID_AVD_HOME=/tmp/avd']
    runtime_environment = {
        'PATH', 'ANDROID_HOME', 'ANDROID_SDK_ROOT', 'JAVA_HOME',
        'NOTEMAKER_JSVG', 'NOTEMAKER_ANDROIDSVG', 'NOTEMAKER_GL_LIBRARY_PATH',
        'LD_LIBRARY_PATH', 'NIX_LD_LIBRARY_PATH', 'NIX_LD',
        'QT_PLUGIN_PATH', 'QT_QPA_PLATFORM_PLUGIN_PATH', 'GDK_PIXBUF_MODULE_FILE',
        'FONTCONFIG_FILE', 'FONTCONFIG_PATH', 'XKB_CONFIG_ROOT',
        'SSL_CERT_FILE', 'NIX_SSL_CERT_FILE', 'LANG', 'LC_ALL', 'TZ',
    }
    for key in runtime_environment:
        if key in os.environ:
            command += ['--env', key]
    if name in EMULATOR_SUITES:
        command += ['--device', '/dev/kvm:/dev/kvm']
    command += ['--entrypoint', sys.executable, os.environ.get('NOTEMAKER_TEST_IMAGE', 'notemaker-tests:dev'),
                '/workspace/tests/' + name + '.py']
    return command


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tier', action='append', choices=SUITES, help='Repeat to combine tiers; default local')
    parser.add_argument('--suite', action='append', help='Select named suites within the chosen tiers')
    parser.add_argument('--list', action='store_true', help='Print plan without building or accessing devices')
    parser.add_argument('--no-build', action='store_true', help='Use previously built artifacts')
    parser.add_argument('--serial', help='Explicit ADB serial for hardware tier')
    parser.add_argument('--topology', type=Path, help='External server/proxy configuration JSON')
    parser.add_argument('--install-tablet', action='store_true', help='Back up and upgrade selected tablet before checks')
    parser.add_argument('--reverse-port', type=int, help='Explicit USB port mapping for hardware tier')
    parser.add_argument('--require-resources', action='store_true', help='Treat missing device/config as failure')
    parser.add_argument('--jobs', type=int, default=2,
                        help='Maximum concurrent test environments (1-4; default 2 on this host)')
    parser.add_argument('--containers', action='store_true',
                        help='Run each suite in its own Docker network/process environment')
    args = parser.parse_args()
    if not 1 <= args.jobs <= 4:
        parser.error('--jobs must be between 1 and 4')
    tiers = args.tier or ['local']
    if (args.install_tablet or args.reverse_port) and ('hardware' not in tiers or not args.serial):
        parser.error('Tablet installation/port mapping requires --tier hardware --serial SERIAL')
    completed_durations = []
    try:
        checks = plan(tiers, args.suite, args.serial, args.topology)
    except ValueError as error:
        parser.error(str(error))
    if args.list:
        print(json.dumps(checks, indent=2))
        return 0
    if not os.environ.get('IN_NIX_SHELL'):
        parser.error('Run through nix-shell')
    if args.containers:
        if subprocess.run([os.environ.get('NOTEMAKER_DOCKER', 'docker'), 'image', 'inspect',
                            os.environ.get('NOTEMAKER_TEST_IMAGE', 'notemaker-tests:dev')],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
            parser.error('Container image not found; build and load it with: nix-build tests/container-image.nix --no-out-link | xargs docker load -i')
    stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    out = ROOT / '.test-artifacts' / 'runs' / stamp
    out.mkdir(parents=True)
    run_started = time.monotonic()
    report = dict(tiers=tiers, jobs=args.jobs, containers=args.containers, results=[])
    report_lock = threading.Lock()
    def record(result):
        report['results'].append(result)
        (out / 'report.json').write_text(json.dumps(report, indent=2))
    failed = False
    runnable = [c['name'] for c in checks if not c['skip']]
    builds = []
    if runnable and not args.no_build:
        builds.append(['apps/cli/build.sh', '--android'] if 'emulator' in tiers else ['apps/cli/build.sh'])
        if 'emulator' in tiers or args.install_tablet:
            builds.append(['apps/android/build.sh'])
    try:
        for index, command in enumerate(builds):
            code = execute(command, out / f'build-{index}.log')
            record(dict(name=' '.join(command), status='pass' if code == 0 else 'fail', exit_code=code))
            if code:
                failed = True
                break
        for check in checks:
            name = check['name']
            reason = check['skip'] or ('Build failed' if failed else None)
            if reason:
                print(f'SKIP {name}: {reason}', flush=True)
                record(dict(name=name, status='skip', reason=reason))
        runnable_checks = [c for c in checks if not c['skip'] and not failed]
        averages = recent_averages()
        pending = sorted(runnable_checks,
                         key=lambda c: suite_estimate(c['name'], averages), reverse=True)
        condition = threading.Condition()
        active_claims = set()
        active_emulator_count = 0
        active_count = 0

        def worker(worker_id):
            nonlocal active_emulator_count, active_count
            while True:
                with condition:
                    while True:
                        if not pending:
                            return
                        if active_count >= args.jobs:
                            condition.wait()
                            continue
                        selected = next((i for i, item in enumerate(pending)
                                         if not claims_conflict(item['name'], active_claims,
                                                                active_emulator_count,
                                                                isolated_networks=args.containers)), None)
                        if selected is None:
                            condition.wait()
                            continue
                        check = pending.pop(selected)
                        name = check['name']
                        claims = set(CLAIMS.get(name, set()))
                        if name == 'integration':
                            claims.add('integration')
                        active_claims.update(claims)
                        if name in EMULATOR_SUITES:
                            active_emulator_count += 1
                        active_count += 1
                        break
                worker_tmp = out / 'workers' / name
                worker_tmp.mkdir(parents=True, exist_ok=True)
                command = suite_command(name, args.containers, worker_tmp)
                if name == 'tablet_checks':
                    command += ['--serial', args.serial]
                    if args.install_tablet:
                        command += ['--install']
                    if args.reverse_port:
                        command += ['--reverse-port', str(args.reverse_port)]
                if name == 'network_checks':
                    command += ['--topology', str(args.topology.resolve())]
                print(f'RUN {name} (worker {worker_id}, estimate {suite_estimate(name, averages):.0f}s)', flush=True)
                started = time.monotonic()
                try:
                    code = execute(command, out / (name + '.log'))
                    seconds = round(time.monotonic() - started, 2)
                    result = dict(name=name, status='skip' if code == 77 else 'pass' if code == 0 else 'fail',
                                  exit_code=code, seconds=seconds, worker=worker_id,
                                  estimated_seconds=round(suite_estimate(name, averages), 2))
                    if code != 77:
                        completed_durations.append(dict(name=name, seconds=seconds))
                except BaseException as error:
                    result = dict(name=name, status='fail', reason=str(error), worker=worker_id)
                finally:
                    if args.containers:
                        import shutil
                        shutil.rmtree(worker_tmp, ignore_errors=True)
                with condition:
                    with report_lock:
                        record(result)
                    active_claims.difference_update(claims)
                    if name in EMULATOR_SUITES:
                        active_emulator_count -= 1
                    active_count -= 1
                    condition.notify_all()

        worker_count = min(args.jobs, len(pending))
        threads = [threading.Thread(target=worker, args=(i + 1,), daemon=True)
                   for i in range(worker_count)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
        for result in report['results']:
            if result['status'] == 'fail':
                failed = True
        if completed_durations:
            history_path = ROOT / '.test-artifacts' / 'runs' / 'timing-history.json'
            try:
                history = json.loads(history_path.read_text()) if history_path.exists() else {'runs': []}
            except (OSError, ValueError):
                history = {'runs': []}
            known_runs = {item.get('timestamp') for item in history.get('runs', [])}
            for old_report in sorted((ROOT / '.test-artifacts' / 'runs').glob('*/report.json'),
                                     key=lambda item: item.stat().st_mtime):
                old_stamp = old_report.parent.name
                if old_stamp == stamp or old_stamp in known_runs:
                    continue
                try:
                    old_results = json.loads(old_report.read_text()).get('results', [])
                except (OSError, ValueError):
                    continue
                history.setdefault('runs', []).append(dict(timestamp=old_stamp, results=old_results))
            history.setdefault('runs', []).append(dict(timestamp=stamp, results=completed_durations))
            history['runs'] = history['runs'][-100:]
            temporary = history_path.with_suffix('.json.tmp')
            temporary.write_text(json.dumps(history, indent=2))
            temporary.replace(history_path)
    except KeyboardInterrupt:
        record(dict(name='runner', status='fail', reason='Interrupted'))
    except Exception as error:
        record(dict(name='runner', status='fail', reason=str(error)))
    report['total_seconds'] = round(time.monotonic() - run_started, 2)
    (out / 'report.json').write_text(json.dumps(report, indent=2))
    print('Report: ' + str(out / 'report.json'))
    return int(any(r['status'] == 'fail' or (args.require_resources and r['status'] == 'skip')
                   for r in report['results']))


if __name__ == '__main__':
    sys.exit(main())
