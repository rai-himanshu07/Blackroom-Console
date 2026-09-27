import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, CirclePlay, Keyboard, LockKeyhole, MousePointer2, ShieldOff, SquareMousePointer, Terminal, createIcons } from 'lucide';
import './style.css';

type InputEvent =
  | { kind: 'key'; code: number }
  | { kind: 'move'; dx: number; dy: number }
  | { kind: 'click'; button: number }
  | { kind: 'scroll'; dy: number };

interface Snapshot {
  mode: 'OFFLINE_SIMULATION';
  live_control: false;
  authority_store: 'EPHEMERAL' | 'PERSISTED' | 'SEPARATE';
  state: string;
  epoch: number;
  events: InputEvent[];
  pointer: { x: number; y: number };
}

const icons = { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, CirclePlay, Keyboard, LockKeyhole, MousePointer2, ShieldOff, SquareMousePointer, Terminal };
const app = document.querySelector<HTMLDivElement>('#app')!;
app.innerHTML = `
  <div class="shell">
    <header class="topbar">
      <div class="brand"><span class="brand-mark" aria-hidden="true">B<span>_</span></span><div><strong>Blackroom Console</strong><small>LOCAL CONTROL LAB</small></div></div>
      <div class="mode"><span class="mode-dot"></span> OFFLINE SIMULATION <span class="mode-separator">/</span> LIVE CONTROL DISABLED</div>
    </header>
    <main>
      <div class="page-title"><div><p class="eyebrow">CONTROL / 01</p><h1>Session console</h1></div><p class="session-label" id="connection">Connecting to local simulator...</p></div>
      <div class="workspace">
        <section class="stage" aria-label="Synthetic desktop preview">
          <div class="stage-head"><div><span class="signal"></span> SYNTHETIC DESKTOP <span class="stage-id">SIM-01</span></div><span id="stage-status">LOCKED</span></div>
          <div class="screen" id="screen">
            <div class="screen-top"><span>BLACKROOM / TEST SURFACE</span><span id="screen-state">LOCAL_LOCKED</span></div>
            <div class="screen-content"><div class="screen-symbol">B_</div><div class="screen-title" id="screen-title">Simulation locked</div><div class="screen-subtitle" id="screen-subtitle">No desktop video or real input is connected.</div></div>
            <div class="sim-workspace" id="sim-workspace" aria-hidden="true">
              <div class="sim-window">
                <div class="sim-window-head"><span>SIM-01 / INPUT MONITOR</span><span class="sim-window-indicator">SIMULATED</span></div>
                <div class="sim-window-body">
                  <div class="sim-readout"><span>AUTHORITY</span><strong>GRANTED / SYNTHETIC</strong></div>
                  <div class="sim-readout"><span>LAST LOGGED EVENT</span><strong id="sim-event">No events recorded</strong></div>
                  <div class="sim-readout"><span>RECENT EVENTS</span><strong id="sim-count">00</strong></div>
                </div>
              </div>
            </div>
            <div class="pointer" id="pointer" aria-hidden="true"><i data-lucide="mouse-pointer-2"></i></div>
            <div class="screen-bottom"><span>TEST PATTERN / NO LIVE PIXELS</span><span>1280 x 720</span></div>
          </div>
          <div class="stage-foot"><span><span class="signal"></span> FAKE TRANSPORT</span><span>NO GNOME CONNECTION</span></div>
        </section>
        <aside class="controls" aria-label="Simulation controls">
          <section class="control-group session-control">
            <div class="section-heading"><span>01 / AUTHORITY</span><i data-lucide="lock-keyhole"></i></div>
            <div class="status-row"><span>Host state</span><strong id="state">LOCAL_LOCKED</strong></div>
            <div class="status-row"><span>Security epoch</span><strong id="epoch">0</strong></div>
            <div class="status-row"><span>Authority store</span><strong id="storage">EPHEMERAL</strong></div>
            <div class="session-buttons"><button id="start" class="primary"><i data-lucide="circle-play"></i><span>Start simulation</span></button><button id="revoke" class="secondary" disabled><i data-lucide="shield-off"></i><span>Revoke & lock</span></button></div>
          </section>
          <section class="control-group input-control">
            <div class="section-heading"><span>02 / INPUT LAB</span><i data-lucide="square-mouse-pointer"></i></div>
            <div class="control-label">Pointer movement</div>
            <div class="dpad" aria-label="Simulated pointer movement">
              <button class="dpad-up input-button" data-event='{"kind":"move","dx":0,"dy":-24}' title="Move pointer up" aria-label="Move pointer up"><i data-lucide="arrow-up"></i></button>
              <button class="dpad-left input-button" data-event='{"kind":"move","dx":-24,"dy":0}' title="Move pointer left" aria-label="Move pointer left"><i data-lucide="arrow-left"></i></button>
              <button class="dpad-right input-button" data-event='{"kind":"move","dx":24,"dy":0}' title="Move pointer right" aria-label="Move pointer right"><i data-lucide="arrow-right"></i></button>
              <button class="dpad-down input-button" data-event='{"kind":"move","dx":0,"dy":24}' title="Move pointer down" aria-label="Move pointer down"><i data-lucide="arrow-down"></i></button>
            </div>
            <div class="input-actions"><button class="input-button" data-event='{"kind":"click","button":272}' title="Simulate left click"><i data-lucide="mouse-pointer-2"></i><span>Click</span></button><button class="input-button" data-event='{"kind":"scroll","dy":5}' title="Simulate scroll down"><i data-lucide="arrow-down"></i><span>Scroll</span></button><button class="input-button" data-event='{"kind":"key","code":30}' title="Simulate A key"><i data-lucide="keyboard"></i><span>Key A</span></button></div>
          </section>
          <section class="control-group journal"><div class="section-heading"><span>03 / EVENT LOG</span><i data-lucide="terminal"></i></div><ol id="events" aria-live="polite"><li class="empty">No simulated input yet.</li></ol></section>
        </aside>
      </div>
      <div class="feedback" id="feedback" role="status" aria-live="polite">Simulation only. Commands never reach the desktop.</div>
    </main>
  </div>
`;

createIcons({ icons, attrs: { width: '18', height: '18', 'stroke-width': '1.8' } });

const $ = <T extends HTMLElement>(selector: string) => document.querySelector<T>(selector)!;
let snapshot: Snapshot | null = null;
let pending = false;

function describe(event: InputEvent): string {
  switch (event.kind) {
    case 'key': return `KEY / code ${event.code}`;
    case 'move': return `POINTER / ${event.dx}, ${event.dy}`;
    case 'click': return `BUTTON / ${event.button === 272 ? 'left' : 'right'}`;
    case 'scroll': return `SCROLL / ${event.dy}`;
  }
}

function render(value: Snapshot): void {
  snapshot = value;
  const active = value.state === 'REMOTE_ACTIVE';
  const failedSafe = value.state === 'FAILED_SAFE';
  $('#state').textContent = value.state;
  $('#epoch').textContent = String(value.epoch);
  $('#storage').textContent = value.authority_store;
  $('#stage-status').textContent = failedSafe ? 'RECOVERY REQUIRED' : active ? 'SIMULATING' : 'LOCKED';
  $('#screen-state').textContent = value.state;
  $('#screen-title').textContent = failedSafe ? 'Offline recovery required' : active ? 'Synthetic session active' : 'Simulation locked';
  $('#screen-subtitle').textContent = failedSafe ? 'Emergency stop persisted. Control is disabled.' : active ? 'Input is recorded to a fake transport only.' : 'No desktop video or real input is connected.';
  $('#connection').textContent = failedSafe ? 'Offline authority stopped' : active ? 'Synthetic control granted' : 'Local simulation ready';
  $('#connection').classList.toggle('error', failedSafe);
  $('#screen').classList.toggle('active', active);
  $('#sim-workspace').setAttribute('aria-hidden', String(!active));
  $('#sim-event').textContent = value.events.length ? describe(value.events[value.events.length - 1]) : 'No events recorded';
  $('#sim-count').textContent = String(value.events.length).padStart(2, '0');
  $('#start').toggleAttribute('disabled', active || failedSafe || pending);
  $('#revoke').toggleAttribute('disabled', !active || pending);
  document.querySelectorAll<HTMLButtonElement>('.input-button').forEach((button) => { button.disabled = !active || pending; });
  const events = $('#events');
  events.replaceChildren();
  if (!value.events.length) {
    const empty = document.createElement('li');
    empty.className = 'empty';
    empty.textContent = 'No simulated input yet.';
    events.append(empty);
  }
  value.events.slice().reverse().forEach((event, index) => {
    const row = document.createElement('li');
    const counter = document.createElement('span');
    counter.textContent = String(value.events.length - index).padStart(2, '0');
    row.append(counter, document.createTextNode(describe(event)));
    events.append(row);
  });
  const pointer = $('#pointer');
  pointer.style.left = `${value.pointer.x}%`;
  pointer.style.top = `${value.pointer.y}%`;
}

async function request(path: string, event?: InputEvent): Promise<void> {
  if (pending) return;
  pending = true;
  if (snapshot) render(snapshot);
  $('#connection').textContent = 'Sending simulated command...';
  let failure: string | null = null;
  try {
    const response = await fetch(`/api/simulation${path}`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(event ?? {}),
    });
    const result: Snapshot | { code: string; message: string } = response.headers.get('content-type')?.includes('application/json')
      ? await response.json() as Snapshot | { code: string; message: string }
      : { code: `HTTP ${response.status}`, message: 'Request refused' };
    if (!response.ok) throw new Error('code' in result ? result.code : `HTTP ${response.status}`);
    render(result as Snapshot);
    $('#feedback').classList.remove('error');
    $('#feedback').textContent = `Simulation only. ${path === '/input' ? 'Event recorded in fake transport.' : path === '/revoke' ? 'Authority revoked; input refused.' : 'Synthetic control granted.'}`;
  } catch (error) {
    failure = error instanceof Error ? error.message : 'unavailable';
    try { await refresh(); } catch { /* Offline gateway may be stopped. */ }
  } finally {
    pending = false;
    if (snapshot) render(snapshot);
    if (failure) {
      $('#connection').textContent = `Command failed: ${failure}`;
      $('#connection').classList.add('error');
      $('#feedback').textContent = `Command refused: ${failure}. No real input was sent.`;
      $('#feedback').classList.add('error');
    }
  }
}

async function refresh(): Promise<void> {
  const response = await fetch('/api/simulation', { cache: 'no-store' });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  render(await response.json() as Snapshot);
}

$('#start').addEventListener('click', () => { void request('/start'); });
$('#revoke').addEventListener('click', () => { void request('/revoke'); });
document.querySelectorAll<HTMLButtonElement>('[data-event]').forEach((button) => {
  button.addEventListener('click', () => { void request('/input', JSON.parse(button.dataset.event!) as InputEvent); });
});

refresh().catch(() => {
  $('#connection').textContent = 'Local gateway unavailable';
  $('#connection').classList.add('error');
  $('#stage-status').textContent = 'OFFLINE';
  $('#feedback').textContent = 'Start the offline gateway to use the simulation. No real connection is available.';
  $('#feedback').classList.add('error');
  $('#start').setAttribute('disabled', '');
});

window.setInterval(() => {
  if (!pending) void refresh().catch(() => {
    $('#connection').textContent = 'Local gateway unavailable';
    $('#connection').classList.add('error');
    $('#start').setAttribute('disabled', '');
    $('#revoke').setAttribute('disabled', '');
    document.querySelectorAll<HTMLButtonElement>('.input-button').forEach((button) => { button.disabled = true; });
  });
}, 1500);