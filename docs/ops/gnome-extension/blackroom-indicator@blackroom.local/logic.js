// Pure logic (no GNOME imports; tested by docs/ops/indicator-logic-test.mjs).

export function parseStatus(text) {
    try {
        const value = JSON.parse(text);
        return value && typeof value === 'object' && !Array.isArray(value) ? value : null;
    } catch (_error) {
        return null;
    }
}

// A console started with the one-time URL token (no hostd login) refuses a page opened without it: take the token from
// the console's own `url` file (owner-only) and add it to the local address. With a hostd or TOTP login there is none.
export function pageUrl(localUrl, urlFileText) {
    if (!localUrl)
        return null;
    const token = /[?&]t=([0-9a-f]{16,})/.exec(urlFileText ?? '');
    return token ? `${localUrl}?t=${token[1]}` : localUrl;
}

export function formatDuration(secs) {
    const total = Math.max(0, Math.floor(Number(secs) || 0));
    const pad = n => String(n).padStart(2, '0');
    const hours = Math.floor(total / 3600);
    const minutes = Math.floor((total % 3600) / 60);
    const seconds = total % 60;
    return hours > 0 ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${minutes}:${pad(seconds)}`;
}

const capital = text => text.charAt(0).toUpperCase() + text.slice(1);

// `status` is the console's reply, or null when the console is not running.
export function stateOf(status) {
    if (!status)
        return 'off';
    switch (status.phase) {
    case 'starting': return 'starting';
    case 'running': return 'running';
    case 'stopping': return 'stopping';
    default: return 'idle';
    }
}

const timerShown = status => !status || !status.indicator || status.indicator.show_timer !== false;
const noticesWanted = status => !status || !status.indicator || status.indicator.notify !== false;

export function pendingOf(status) {
    return status && status.pending && typeof status.pending.id === 'number' ? status.pending : null;
}

// A connection that waits for the owner: shown whatever the notification setting says, since it needs an answer.
export function pendingNotice(previous, next) {
    const now = pendingOf(next);
    if (!now || (pendingOf(previous) && pendingOf(previous).id === now.id))
        return null;
    return {id: now.id, title: 'Connection request',
        body: `A ${now.mode} session from ${now.device} is waiting for you. It is denied automatically in ${now.secs_left} seconds.`};
}

export function view(status) {
    const shown = viewOf(status);
    const pending = pendingOf(status);
    if (pending) {
        return {...shown, title: 'A connection is waiting for your approval', canApprove: true, pendingId: pending.id,
            badge: '?', lines: [`${capital(pending.mode)} session from ${pending.device}`,
                `Denied automatically in ${pending.secs_left} seconds`]};
    }
    return {...shown, canApprove: false, pendingId: null, badge: timerShown(status) || shown.badge === '…' ? shown.badge : ''};
}

function viewOf(status) {
    const state = stateOf(status);
    const base = {
        state,
        badge: '',
        title: '',
        lines: [],
        canDisconnect: false,
        canOpen: false,
        canStart: false,
        canStop: false,
    };
    switch (state) {
    case 'off':
        return {...base, title: 'Console is not running', canStart: true};
    case 'idle':
        return {...base, title: 'Ready: nobody is connected', canOpen: Boolean(status.local_url), canStop: true,
            lines: [`Version ${status.version}`]};
    case 'starting':
        return {...base, badge: '…', title: 'A remote session is starting', canDisconnect: true};
    case 'stopping':
        return {...base, badge: '…', title: 'Ending the remote session', canStop: true};
    default: {
        const lines = [
            `${capital(status.mode)} mode: ${status.blank_panel ? 'screen blank' : 'screen visible'}, laptop keyboard and touchpad ${status.block_local_input ? 'blocked' : 'usable'}`,
            `Connected for ${formatDuration(status.session_secs)}`,
        ];
        if (status.audio === 'on')
            lines.push('Laptop sound is sent to the remote device');
        return {...base, badge: formatDuration(status.session_secs), title: 'A remote session is active',
            lines, canDisconnect: true, canOpen: Boolean(status.local_url), canStop: true};
    }
    }
}

// A message for the moment the state changes; null for no message. `previous` is undefined on the first reading.
export function transitionNotice(previous, next) {
    if (previous === undefined)
        return null;
    const before = stateOf(previous);
    const after = stateOf(next);
    if (before === after || !noticesWanted(next ?? previous))
        return null;
    const inSession = state => state === 'running' || state === 'starting' || state === 'stopping';
    if (!inSession(before) && (after === 'running' || after === 'starting')) {
        return {title: 'Remote session started',
            body: next.blank_panel
                ? 'This laptop\'s screen is blank and its keyboard and touchpad are blocked.'
                : 'You can both use this laptop. Disconnect from the top-bar icon.'};
    }
    if (inSession(before) && after === 'idle') {
        const stop = next.last_stop;
        const parts = [];
        if (stop && stop.restored === true)
            parts.push('the screen was restored');
        if (stop && stop.locked === true)
            parts.push('the laptop was locked');
        return {title: 'Remote session ended',
            body: parts.length ? `${capital(parts.join(' and '))}.` : 'The remote device is disconnected.'};
    }
    if (inSession(before) && after === 'off') {
        return {title: 'The console stopped during a remote session',
            body: 'If the screen is still blank or the keyboard is blocked, see the runbook (docs/ops/README.md).'};
    }
    return null;
}
