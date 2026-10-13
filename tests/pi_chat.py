#!/usr/bin/env python3
"""Opt-in live Pi/provider check. Uses existing auth and an isolated test server."""
import json
import time
from chat_checks import ChatChecks

check = ChatChecks()
check.setUp()
try:
    chat = check.create('pi')
    check.send(chat, 'Use the NoteMaker CLI with the supplied agent state and actor. Create the document '
               'chat-check if it does not exist, add a layer named Pi verification using layer put, '
               'sync it to the supplied upstream, and inspect it to verify the layer. '
               'Do not edit application source. Briefly report the result.')
    state = check.wait_done(chat, timeout=180)
    events = check.get('/api/chats/' + chat + '/events')['events']
    if state['status'] != 'done':
        raise AssertionError(json.dumps([row for row in events if row['event']['kind'] in ('status', 'error')], indent=2))
    kinds = {row['event']['kind'] for row in events}
    assert {'delta', 'tool', 'tool_end', 'assistant_end'} <= kinds, kinds
    rows = check.get('/api/events?document=chat-check')['events']
    actions = [action for row in rows for action in row['op'].get('actions', [row['op']])]
    assert any(action.get('name') == 'Pi verification' for action in actions), rows
    assert all(row['op'].get('actor') == 'agent-' + chat for row in rows), (chat, rows)
    check.send(chat, 'What was the exact name of the layer you just created? Answer briefly without using tools.')
    assert check.wait_done(chat, timeout=90)['status'] == 'done'
    events = check.get('/api/chats/' + chat + '/events')['events']
    reply = [row['event']['text'] for row in events if row['event']['kind'] == 'assistant_end'][-1]
    assert 'Pi verification' in reply, reply
    print('PASS: real Pi hooks streamed text/tools and CLI edits synced with the chat actor')
finally:
    check.tearDown()
