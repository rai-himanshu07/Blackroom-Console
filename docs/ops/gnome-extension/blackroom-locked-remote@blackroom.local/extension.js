import Meta from 'gi://Meta';
import {InjectionManager} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

// GNOME Shell calls inhibit_remote_access() when the screen locks and uninhibit_remote_access()
// when it unlocks; the first ends every remote session and refuses new ones. While this extension
// is enabled both are no-ops, so locking no longer ends remote sessions. Enable it BEFORE locking
// where possible; enabled on an already locked screen it lifts the block that is already in place.
export default class BlackroomLockedRemote {
    enable() {
        if (this._injections) return;
        const controller = global.backend.get_remote_access_controller();
        const proto = Meta.RemoteAccessController.prototype;
        const realUninhibit = proto.uninhibit_remote_access;

        this._cancelPendingReinhibit();
        this._injections = new InjectionManager();
        this._injections.overrideMethod(proto, 'inhibit_remote_access', () => function () {});
        this._injections.overrideMethod(proto, 'uninhibit_remote_access', () => function () {});

        // Remote handles that are alive; disable() waits for them so it never cuts a live session.
        this._handles = new Set();
        this._newHandleId = controller.connect('new-handle', (_controller, handle) => {
            this._handles.add(handle);
            handle.connect('stopped', () => this._handles.delete(handle));
        });

        if (Main.sessionMode.isLocked) {
            realUninhibit.call(controller);
            console.log('Blackroom: lifted the remote-access block of the locked screen');
        }
        console.log('Blackroom: remote sessions are allowed on the lock screen');
    }

    disable() {
        if (!this._injections) return;
        const controller = global.backend.get_remote_access_controller();
        controller.disconnect(this._newHandleId);
        this._newHandleId = 0;
        this._injections.clear();
        this._injections = null;

        // The Shell still counts the lock as inhibiting and will uninhibit on unlock, so a locked
        // screen gets its block back, but only once the live remote sessions have ended (Stop).
        if (Main.sessionMode.isLocked) this._reinhibitWhenIdle(controller, [...this._handles]);
        this._handles = null;
        console.log('Blackroom: remote sessions are blocked on the lock screen again');
    }

    _reinhibitWhenIdle(controller, handles) {
        const finish = () => {
            this._cancelPendingReinhibit();
            if (Main.sessionMode.isLocked) controller.inhibit_remote_access();
        };
        const alive = new Set(handles);
        if (alive.size === 0) {
            finish();
            return;
        }
        this._pending = {handles: [], modeId: 0};
        for (const handle of handles) {
            const id = handle.connect('stopped', () => {
                alive.delete(handle);
                if (alive.size === 0) finish();
            });
            this._pending.handles.push([handle, id]);
        }
        // Unlocked meanwhile: the Shell uninhibits itself, nothing to restore.
        this._pending.modeId = Main.sessionMode.connect('updated', () => {
            if (!Main.sessionMode.isLocked) this._cancelPendingReinhibit();
        });
    }

    _cancelPendingReinhibit() {
        if (!this._pending) return;
        for (const [handle, id] of this._pending.handles) handle.disconnect(id);
        Main.sessionMode.disconnect(this._pending.modeId);
        this._pending = null;
    }
}
