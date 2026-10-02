import Meta from 'gi://Meta';
import {InjectionManager} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

// BEGIN pure logic (no GNOME imports; exercised by a node harness)
// The Shell calls inhibit_remote_access() once when a lock begins and uninhibit_remote_access() once when it
// ends, and re-runs enable()/disable() of extensions many times in a burst on that transition. So the block
// is reconciled from module state instead of being counted per enable()/disable(): the real inhibit count is
// 1 only while the Shell wants a block and this extension is not active.
export class RemoteBlock {
    constructor(realInhibit, realUninhibit, shellInhibited) {
        this._inhibit = realInhibit;
        this._uninhibit = realUninhibit;
        this.shellInhibited = shellInhibited ? 1 : 0;
        this.realCount = this.shellInhibited;
        this.active = false;
        this.deferring = false;
        this.handles = new Set();
    }

    reconcile() {
        if (this.deferring) return;
        const target = this.active ? 0 : this.shellInhibited;
        while (this.realCount > target) { this._uninhibit(); this.realCount--; }
        while (this.realCount < target) { this._inhibit(); this.realCount++; }
    }

    shellInhibit() { this.shellInhibited = 1; this.reconcile(); }

    shellUninhibit() { this.shellInhibited = 0; this.deferring = false; this.reconcile(); }

    enable() { this.active = true; this.deferring = false; this.reconcile(); }

    // A live remote session is never cut by disabling: the block returns once the handles have stopped.
    disable() {
        this.active = false;
        if (this.shellInhibited && this.handles.size > 0) this.deferring = true;
        else this.reconcile();
    }

    handleStarted(handle) { this.handles.add(handle); }

    handleStopped(handle) {
        this.handles.delete(handle);
        if (this.deferring && this.handles.size === 0) { this.deferring = false; this.reconcile(); }
    }
}
// END pure logic

let block = null;
let injections = null;

function install() {
    if (block) return;
    const controller = global.backend.get_remote_access_controller();
    let realInhibit = null;
    let realUninhibit = null;
    injections = new InjectionManager();
    const proto = Meta.RemoteAccessController.prototype;
    injections.overrideMethod(proto, 'inhibit_remote_access', original => {
        realInhibit = original;
        return function () { block.shellInhibit(); };
    });
    injections.overrideMethod(proto, 'uninhibit_remote_access', original => {
        realUninhibit = original;
        return function () { block.shellUninhibit(); };
    });
    // A Shell block that is already in place when the extension is first enabled counts as real.
    block = new RemoteBlock(
        () => realInhibit.call(controller),
        () => realUninhibit.call(controller),
        !Main.sessionMode.allowScreencast);
    controller.connect('new-handle', (_controller, handle) => {
        block.handleStarted(handle);
        handle.connect('stopped', () => block.handleStopped(handle));
    });
}

// The two overrides stay installed after disable() as pass-through that only keeps the count: tearing
// them down on every one of the Shell's enable/disable bursts would lose the Shell's own block state.
export default class BlackroomLockedRemote {
    enable() {
        install();
        const wasInhibited = block.realCount;
        block.enable();
        if (wasInhibited) console.log('Blackroom: lifted the remote-access block of the locked screen');
        console.log('Blackroom: remote sessions are allowed on the lock screen');
    }

    disable() {
        if (!block) return;
        block.disable();
        console.log('Blackroom: remote sessions are blocked on the lock screen again');
    }
}
