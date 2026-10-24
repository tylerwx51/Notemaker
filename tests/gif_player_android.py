"""Live GIF player check against an already running Android Activity session.

Run inside nix-shell with --proxy-file, --session, --document, and --id.
"""
import argparse
import json
import subprocess
import time

p = argparse.ArgumentParser()
p.add_argument('--proxy-file', required=True)
p.add_argument('--session', required=True)
p.add_argument('--document', required=True)
p.add_argument('--id', required=True)
p.add_argument('--other-session')
a = p.parse_args()

def cli(*args, payload=None):
    command = ['apps/cli/notemaker', '--proxy-file', a.proxy_file]
    if args[0] == 'status': command += ['--document', a.document]
    command += list(args)
    result = subprocess.run(command, input=json.dumps(payload) if payload is not None else None,
                            text=True, capture_output=True, check=True)
    return json.loads(result.stdout)

def action(name, payload=None):
    return cli('ui', 'action', a.session, name, '-', payload=payload or {})['state']['gifPlayer']

def selected_state():
    player = cli('ui', 'state', a.session)['gifPlayer']
    return next(clip for clip in player['clips'] if clip['id'] == a.id)

registry = cli('ui', 'actions', a.session)['actions']
for name in ('gif.import.open','gif.player.open','gif.select','gif.play','gif.pause','gif.next','gif.previous','gif.seek'):
    assert name in registry, name
assert 'interact' in registry['tool.select']
assert cli('ui', 'action', a.session, 'tool.select', '-', payload={'name': 'interact'})['state']['tool'] == 'interact'
revision = cli('status')['revision']
if a.other_session:
    cli('ui', 'action', a.other_session, 'gif.select', '-', payload={'id': a.id})
    cli('ui', 'action', a.other_session, 'gif.seek', '-', payload={'frame': 2})
state = action('gif.select', {'id': a.id})
clip = next(clip for clip in state['clips'] if clip['id'] == a.id)
assert clip['frame'] == 0 and not clip['playing'] and clip['frames'] == 6, clip
assert next(clip for clip in action('gif.next')['clips'] if clip['id'] == a.id)['frame'] == 1
assert next(clip for clip in action('gif.previous')['clips'] if clip['id'] == a.id)['frame'] == 0
assert next(clip for clip in action('gif.seek', {'frame': 4})['clips'] if clip['id'] == a.id)['frame'] == 4
action('gif.seek', {'frame': 0})
assert next(clip for clip in action('gif.play')['clips'] if clip['id'] == a.id)['playing']
time.sleep(.65)
assert selected_state()['frame'] != 0, selected_state()
action('gif.pause')
paused = selected_state()['frame']
time.sleep(.6)
assert selected_state()['frame'] == paused and not selected_state()['playing']
action('gif.seek', {'frame': 0})
assert cli('status')['revision'] == revision, 'Playback changed document history'
if a.other_session:
    other = cli('ui', 'state', a.other_session)['gifPlayer']
    other_clip = next(c for c in other['clips'] if c['id'] == a.id)
    assert other_clip['frame'] == 2 and not other_clip['playing'], other_clip
print(json.dumps({'session': a.session, 'id': a.id, 'frames': 6,
                  'checked': ['registry', 'interact tool', 'paused start', 'next', 'previous', 'seek',
                              'play advance', 'pause freeze', 'no document write'] +
                             (['session isolation'] if a.other_session else [])}))
