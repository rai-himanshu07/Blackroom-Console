// Tests the indicator's pure logic: node docs/ops/indicator-logic-test.mjs
import assert from 'node:assert/strict';
import {formatDuration, pageUrl, parseStatus, pendingNotice, stateOf, transitionNotice, view} from './gnome-extension/blackroom-indicator@blackroom.local/logic.js';

const idle = {phase: 'idle', mode: 'private', session_secs: 0, blank_panel: true, block_local_input: true, audio: 'off',
    version: '0.1.0', local_url: 'https://localhost:8443/', last_stop: null};
const running = {...idle, phase: 'running', session_secs: 754, audio: 'on'};
const shared = {...running, mode: 'shared', blank_panel: false, block_local_input: false, audio: 'off'};

assert.equal(formatDuration(0), '0:00');
assert.equal(formatDuration(754), '12:34');
assert.equal(formatDuration(3725), '1:02:05');
assert.equal(formatDuration(-5), '0:00');
assert.equal(formatDuration('x'), '0:00');

const local = 'https://localhost:8443/';
assert.equal(pageUrl(local, 'https://192.168.1.50:8443/?t=0123456789abcdef0123\nhttp://127.0.0.1:8080/?t=0123456789abcdef0123'), `${local}?t=0123456789abcdef0123`);
assert.equal(pageUrl(local, 'https://192.168.1.50:8443/\n'), local, 'no token with a hostd login');
assert.equal(pageUrl(local, ''), local);
assert.equal(pageUrl(local, undefined), local);
assert.equal(pageUrl(local, '?t=short'), local, 'a short value is not a token');
assert.equal(pageUrl(null, '?t=0123456789abcdef0123'), null);
assert.equal(parseStatus('{"phase":"idle"}').phase, 'idle');
for (const bad of ['', 'nope', '[1]', 'null', '3']) assert.equal(parseStatus(bad), null, bad);

assert.deepEqual([null, idle, {phase: 'starting'}, running, {phase: 'stopping'}, {phase: 'weird'}].map(stateOf),
    ['off', 'idle', 'starting', 'running', 'stopping', 'idle']);

const off = view(null);
assert.ok(off.canStart && !off.canStop && !off.canDisconnect && !off.canOpen);
const ready = view(idle);
assert.ok(ready.canStop && ready.canOpen && !ready.canDisconnect && ready.badge === '');
const live = view(running);
assert.ok(live.canDisconnect && live.canStop && live.badge === '12:34');
assert.match(live.lines[0], /^Private mode: screen blank, laptop keyboard and touchpad blocked$/);
assert.ok(live.lines.some(line => /sound/.test(line)));
assert.match(view(shared).lines[0], /^Shared mode: screen visible, laptop keyboard and touchpad usable$/);
assert.ok(!view(shared).lines.some(line => /sound/.test(line)));
assert.ok(!view({...idle, local_url: null}).canOpen);
assert.ok(view({...idle, host_url: 'http://localhost:8090/'}).canHost && !view(idle).canHost && !view(null).canHost);

assert.equal(transitionNotice(undefined, running), null, 'no message for the first reading');
assert.equal(transitionNotice(idle, idle), null);
assert.equal(transitionNotice(null, idle), null, 'console start is silent');
assert.equal(transitionNotice(idle, null), null, 'console stop while idle is silent');
assert.match(transitionNotice(idle, running).body, /blank/);
assert.match(transitionNotice(idle, shared).body, /both use/);
assert.equal(transitionNotice(idle, {...idle, phase: 'starting'}).title, 'Remote session started');
assert.equal(transitionNotice(running, {...idle, phase: 'stopping'}), null);
assert.equal(transitionNotice({phase: 'stopping'}, idle).title, 'Remote session ended', 'a Disconnect is seen as stopping, then idle');
const ended = transitionNotice(running, {...idle, last_stop: {reason: 'x', locked: true, restored: true}});
assert.equal(ended.title, 'Remote session ended');
assert.equal(ended.body, 'The screen was restored and the laptop was locked.');
assert.equal(transitionNotice(running, idle).body, 'The remote device is disconnected.');
assert.match(transitionNotice(running, null).title, /stopped during a remote session/);

const waiting = {...idle, pending: {id: 4, mode: 'private', device: '192.168.1.52 (Chrome)', secs_left: 28}};
const asking = view(waiting);
assert.ok(asking.canApprove && asking.pendingId === 4 && asking.badge === '?');
assert.match(asking.title, /waiting for your approval/);
assert.match(asking.lines[0], /^Private session from 192\.168\.1\.9 \(Chrome\)$/);
assert.ok(!view(idle).canApprove);
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
console.log('INDICATOR LOGIC OK');
