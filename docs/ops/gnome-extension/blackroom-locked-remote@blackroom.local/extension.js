export default class BlackroomLockedRemote {
  enable() {
    const controller = global.backend.get_remote_access_controller();
    if (this._original) return;
    this._original = controller.inhibit_remote_access;
    controller.inhibit_remote_access = () => {};
    console.log('Blackroom: remote sessions are allowed on the lock screen');
  }

  disable() {
    if (!this._original) return;
    global.backend.get_remote_access_controller().inhibit_remote_access = this._original;
    this._original = null;
    console.log('Blackroom: remote sessions are blocked on the lock screen again');
  }
}
