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
import time

ROOT = Path(__file__).resolve().parents[1]
SUITES = {
    'local': ['framework_checks', 'server_checks', 'cli_checks', 'drawing_checks', 'drawing_tools_checks', 'drawing_browser', 'selection_checks', 'conflict_checks', 'proxy_checks',
              'chat_checks', 'topology_properties'],
    'emulator': ['integration', 'cli_android', 'drawing_android', 'chat_ui', 'performance'],
    'hardware': ['tablet_checks'],
    'network': ['network_checks'],
}


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
    args = parser.parse_args()
    tiers = args.tier or ['local']
    if (args.install_tablet or args.reverse_port) and ('hardware' not in tiers or not args.serial):
        parser.error('Tablet installation/port mapping requires --tier hardware --serial SERIAL')
    try:
        checks = plan(tiers, args.suite, args.serial, args.topology)
    except ValueError as error:
        parser.error(str(error))
    if args.list:
        print(json.dumps(checks, indent=2))
        return 0
    if not os.environ.get('IN_NIX_SHELL'):
        parser.error('Run through nix-shell')
    stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    out = ROOT / '.test-artifacts' / 'runs' / stamp
    out.mkdir(parents=True)
    report = dict(tiers=tiers, results=[])
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
                continue
            command = [sys.executable, str(ROOT / 'tests' / (name + '.py'))]
            if name == 'tablet_checks':
                command += ['--serial', args.serial]
                if args.install_tablet:
                    command += ['--install']
                if args.reverse_port:
                    command += ['--reverse-port', str(args.reverse_port)]
            if name == 'network_checks':
                command += ['--topology', str(args.topology.resolve())]
            print(f'RUN {name}', flush=True)
            started = time.monotonic()
            code = execute(command, out / (name + '.log'))
            record(dict(name=name, status='skip' if code == 77 else 'pass' if code == 0 else 'fail',
                        exit_code=code, seconds=round(time.monotonic() - started, 2)))
    except KeyboardInterrupt:
        record(dict(name='runner', status='fail', reason='Interrupted'))
    except Exception as error:
        record(dict(name='runner', status='fail', reason=str(error)))
    print('Report: ' + str(out / 'report.json'))
    return int(any(r['status'] == 'fail' or (args.require_resources and r['status'] == 'skip')
                   for r in report['results']))


if __name__ == '__main__':
    sys.exit(main())
