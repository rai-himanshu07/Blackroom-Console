// Tests the indicator's pure logic: node docs/ops/indicator-logic-test.mjs
import assert from 'node:assert/strict';
import {exitOutcome, formatDuration, parseStatus, pendingNotice, stateOf, transitionNotice, view} from './gnome-extension/blackroom-indicator@blackroom.local/logic.js';

const idle = {phase: 'idle', mode: 'private', session_secs: 0, blank_panel: true, block_local_input: true, audio: 'off',
    version: '0.1.0', last_stop: null};
const running = {...idle, phase: 'running', session_secs: 754, audio: 'on'};
const shared = {...running, mode: 'shared', blank_panel: false, block_local_input: false, audio: 'off'};

assert.equal(formatDuration(0), '0:00');
assert.equal(formatDuration(754), '12:34');
assert.equal(formatDuration(3725), '1:02:05');
assert.equal(formatDuration(-5), '0:00');
assert.equal(formatDuration('x'), '0:00');

assert.equal(parseStatus('{"phase":"idle"}').phase, 'idle');
for (const bad of ['', 'nope', '[1]', 'null', '3']) assert.equal(parseStatus(bad), null, bad);

assert.deepEqual([null, idle, {phase: 'starting'}, running, {phase: 'stopping'}, {phase: 'weird'}].map(stateOf),
    ['off', 'idle', 'starting', 'running', 'stopping', 'idle']);

const off = view(null);
assert.ok(off.canStart && !off.canStop && !off.canDisconnect);
const ready = view(idle);
assert.ok(ready.canStop && !ready.canDisconnect && ready.badge === '');
const live = view(running);
assert.ok(live.canDisconnect && live.canStop && live.badge === '12:34');
assert.match(live.lines[0], /^Private mode: screen blank, laptop keyboard and touchpad blocked$/);
assert.ok(live.lines.some(line => /sound/.test(line)));
assert.match(view(shared).lines[0], /^Shared mode: screen visible, laptop keyboard and touchpad usable$/);
assert.ok(!view(shared).lines.some(line => /sound/.test(line)));
assert.ok(view({...idle, host_url: 'http://localhost:8090/'}).canHost && !view(idle).canHost && !view(null).canHost);

assert.equal(transitionNotice(undefined, running), null, 'no message for the first reading');
assert.equal(transitionNotice(idle, idle), null);
assert.equal(transitionNotice(null, idle), null, 'console start is silent');
assert.equal(transitionNotice(idle, null), null, 'console stop while idle is silent');
assert.match(transitionNotice(idle, running).body, /screen is blank; its keyboard and touchpad are blocked/);
assert.match(transitionNotice(idle, shared).body, /screen stays visible; its keyboard and touchpad still work/);
assert.match(transitionNotice(idle, {...running, block_local_input: false}).body, /blank; its keyboard and touchpad still work/, 'a blank screen alone does not claim blocked input');
assert.match(transitionNotice(idle, {...running, blank_panel: false}).body, /stays visible; its keyboard and touchpad are blocked/, 'blocked input alone does not claim a blank screen');
const starting = transitionNotice(idle, {...idle, phase: 'starting'});
assert.equal(starting.title, 'Remote session starting');
assert.doesNotMatch(starting.body, /is blank|are blocked|stays visible/, 'nothing is claimed before isolation is done');
assert.equal(transitionNotice({...idle, phase: 'starting'}, running).title, 'Remote session started', 'the protections are announced when running');
assert.equal(transitionNotice(running, {...idle, phase: 'stopping'}), null);
assert.equal(transitionNotice({phase: 'stopping'}, idle).title, 'Remote session ended', 'a Disconnect is seen as stopping, then idle');
const ended = transitionNotice(running, {...idle, last_stop: {reason: 'x', locked: true, restored: true}});
assert.equal(ended.title, 'Remote session ended');
assert.equal(ended.body, 'The screen was restored and the laptop was locked.');
assert.equal(transitionNotice(running, idle).body, 'The remote device is disconnected.');
const failed = transitionNotice(running, {...idle, last_stop: {reason: 'x', locked: false, restored: false, grab_released: false}});
assert.match(failed.title, /check this laptop/);
assert.match(failed.body, /screen could not be confirmed restored; the laptop could not be locked; release of the laptop keyboard and touchpad was not confirmed/);
assert.match(transitionNotice(running, {...idle, last_stop: {reason: 'x', locked: false, restored: true}}).body, /^The laptop could not be locked\./, 'a failed lock is never reported as success');
assert.equal(transitionNotice(running, {...idle, last_stop: {reason: 'x', locked: null, restored: true, grab_released: true}}).title, 'Remote session ended', 'null means not asked for, not failed');
assert.match(transitionNotice(running, null).title, /stopped during a remote session/);

const waiting = {...idle, pending: {id: 4, mode: 'private', device: '192.168.1.52 (Chrome)', secs_left: 28}};
const asking = view(waiting);
assert.ok(asking.canApprove && asking.pendingId === 4 && asking.badge === '?');
assert.match(asking.title, /waiting for your approval/);
assert.match(asking.lines[0], /^Private session from 192\.168\.1\.52 \(Chrome\)$/);
assert.ok(!view(idle).canApprove);
assert.ok(!view(idle).lines.some(line => /certificate/.test(line)));
assert.ok(view({...idle, cert_note: 'ends in 9 days'}).lines.some(line => /certificate needs renewing/.test(line)));
assert.equal(view({...running, cert_note: 'x'}).lines.length, 4, 'the menu has a fourth line for it');
assert.equal(pendingNotice(idle, waiting).id, 4);
assert.equal(pendingNotice(waiting, waiting), null, 'one notice per request');
assert.equal(pendingNotice(waiting, idle), null);
assert.equal(pendingNotice(undefined, waiting).id, 4);
assert.equal(pendingNotice(waiting, {...waiting, pending: {...waiting.pending, id: 5}}).id, 5);
const quiet = {...idle, indicator: {notify: false, show_timer: false}};
assert.equal(pendingNotice(quiet, {...waiting, indicator: quiet.indicator}).id, 4, 'a request is never silenced');
assert.equal(transitionNotice(quiet, {...running, indicator: quiet.indicator}), null, 'notices can be switched off');
assert.equal(view({...running, indicator: quiet.indicator}).badge, '', 'the timer can be hidden');
assert.equal(view({...running, indicator: {notify: true, show_timer: true}}).badge, '12:34');
assert.equal(view({phase: 'starting', indicator: quiet.indicator}).badge, '…', 'progress dots stay');
// R24: only a bus error that says the name has no owner proves the console is gone.
assert.equal(exitOutcome(true, false), 'running');
assert.equal(exitOutcome(false, true), 'gone');
assert.equal(exitOutcome(false, false), 'unknown');

console.log('INDICATOR LOGIC OK');
