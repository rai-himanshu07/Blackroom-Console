import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as MessageTray from 'resource:///org/gnome/shell/ui/messageTray.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

import {exitOutcome, parseStatus, pendingNotice, stateOf, transitionNotice, view} from './logic.js';

const BUS_NAME = 'org.blackroom.Console';
const OBJECT_PATH = '/org/blackroom/Console';
const INTERFACE = 'org.blackroom.Console1';
const UNIT = 'blackroom-console.service';
const LOCK_EXTENSION = 'blackroom-locked-remote@blackroom.local';
const POLL_SECONDS = 2;

// Only these prove nobody owns the console's bus name; a timeout or any other error says nothing about the console.
const isAbsent = error => error.matches(Gio.DBusError, Gio.DBusError.SERVICE_UNKNOWN) ||
    error.matches(Gio.DBusError, Gio.DBusError.NAME_HAS_NO_OWNER);

const Indicator = GObject.registerClass(
class BlackroomIndicator extends PanelMenu.Button {
    _init(uuid) {
        super._init(0.0, 'Blackroom Console', false);
        this._uuid = uuid;
        this._cancellable = new Gio.Cancellable();
        this._status = undefined;

        const box = new St.BoxLayout({style_class: 'panel-status-menu-box'});
        this._icon = new St.Icon({icon_name: 'video-display-symbolic', style_class: 'system-status-icon'});
        this._label = new St.Label({style_class: 'blackroom-label', y_align: Clutter.ActorAlign.CENTER, visible: false});
        box.add_child(this._icon);
        box.add_child(this._label);
        this.add_child(box);

        this._title = new PopupMenu.PopupMenuItem('', {reactive: false});
        this._lines = [new PopupMenu.PopupMenuItem('', {reactive: false}), new PopupMenu.PopupMenuItem('', {reactive: false}),
            new PopupMenu.PopupMenuItem('', {reactive: false}), new PopupMenu.PopupMenuItem('', {reactive: false})];
        this.menu.addMenuItem(this._title);
        for (const line of this._lines)
            this.menu.addMenuItem(line);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        this._accept = this._action('Accept the connection', () => this._answer('Approve'));
        this._deny = this._action('Deny the connection', () => this._answer('Deny'));
        this._disconnect = this._action('Disconnect the remote user', () => this._call('Disconnect'));
        this._lock = this._action('Lock this screen now', () => Main.screenShield.lock(true));
        this._host = this._action('Host settings...', () => {
            if (this._status?.host_url)
                Gio.AppInfo.launch_default_for_uri(this._status.host_url, global.create_app_launch_context(0, -1));
        });
        this._lockSwitch = new PopupMenu.PopupSwitchMenuItem('Remote use on the lock screen', false);
        this._lockSwitch.connect('toggled', (_item, on) => this._setLockAccess(on));
        this.menu.addMenuItem(this._lockSwitch);
        this._lockNote = new PopupMenu.PopupMenuItem('While on, locking does not end a remote session', {reactive: false});
        this.menu.addMenuItem(this._lockNote);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._start = this._action('Start the console', () => this._systemctl('start'));
        this._stop = this._action('Stop the console', () => this._systemctl('stop'));
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._action('Exit (stop the console and remove this icon)', () => this._exitApp());

        this.menu.connect('open-state-changed', (_menu, open) => {
            if (open)
                this._refresh();
        });
        this._apply(null);
        this._refresh();
        this._timer = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, POLL_SECONDS, () => {
            this._refresh();
            return GLib.SOURCE_CONTINUE;
        });
    }

    _action(text, callback) {
        const item = new PopupMenu.PopupMenuItem(text);
        item.connect('activate', callback);
        this.menu.addMenuItem(item);
        return item;
    }

    _refresh() {
        Gio.DBus.session.call(BUS_NAME, OBJECT_PATH, INTERFACE, 'Status', null, new GLib.VariantType('(s)'),
            Gio.DBusCallFlags.NONE, 1500, this._cancellable, (connection, result) => {
                let status = null;
                try {
                    status = parseStatus(connection.call_finish(result).deepUnpack()[0]);
                } catch (error) {
                    if (error.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                        return;
                    if (!isAbsent(error)) {
                        this._markUnavailable();
                        return;
                    }
                }
                this._apply(status);
            });
    }

    // The console did not answer, which is not the same as the console being off: keep what is known and say so.
    _markUnavailable() {
        this._title.label.text = 'Console status unavailable: it did not answer';
        this._icon.opacity = 160;
    }

    _answer(method) {
        const id = this._status?.pending?.id;
        if (id === undefined)
            return;
        Gio.DBus.session.call(BUS_NAME, OBJECT_PATH, INTERFACE, method, new GLib.Variant('(t)', [id]),
            new GLib.VariantType('(b)'), Gio.DBusCallFlags.NONE, 5000, this._cancellable, (connection, result) => {
                try {
                    connection.call_finish(result);
                } catch (error) {
                    if (!error.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                        Main.notify('Blackroom Console', `${method} failed: ${error.message}`);
                    return;
                }
                this._refresh();
            });
    }

    _askNotice(notice) {
        this._dropAsk();
        try {
            this._source = new MessageTray.Source({title: 'Blackroom Console', iconName: 'video-display-symbolic'});
            Main.messageTray.add(this._source);
            this._ask = new MessageTray.Notification({source: this._source, title: notice.title, body: notice.body,
                urgency: MessageTray.Urgency.CRITICAL});
            this._ask.addAction('Accept', () => this._answer('Approve'));
            this._ask.addAction('Deny', () => this._answer('Deny'));
            this._source.addNotification(this._ask);
        } catch (error) {
            console.log(`Blackroom indicator: notification with buttons failed (${error.message}); plain notice`);
            Main.notify(notice.title, notice.body);
        }
    }

    _dropAsk() {
        this._ask?.destroy();
        this._ask = null;
        this._source?.destroy();
        this._source = null;
    }

    _call(method) {
        Gio.DBus.session.call(BUS_NAME, OBJECT_PATH, INTERFACE, method, null, new GLib.VariantType('(s)'),
            Gio.DBusCallFlags.NONE, 5000, this._cancellable, (connection, result) => {
                try {
                    connection.call_finish(result);
                } catch (error) {
                    if (!error.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                        Main.notify('Blackroom Console', `${method} failed: ${error.message}`);
                    return;
                }
                this._refresh();
            });
    }

    // Exit: stop the console, then remove this icon (disable this extension). A console that was started by hand keeps
    // running, so the icon stays rather than hiding the way to disconnect.
    _exitApp() {
        const running = stateOf(this._status) !== 'off';
        Main.notify('Blackroom Console', running ? 'Ending any remote session and closing the console...' : 'Closing...');
        const process = Gio.Subprocess.new(['systemctl', '--user', 'stop', UNIT], Gio.SubprocessFlags.STDERR_PIPE);
        process.communicate_utf8_async(null, null, () => {
            Gio.DBus.session.call(BUS_NAME, OBJECT_PATH, INTERFACE, 'Status', null, new GLib.VariantType('(s)'),
                Gio.DBusCallFlags.NONE, 1500, null, (connection, result) => {
                    let outcome = exitOutcome(true, false);
                    try {
                        connection.call_finish(result);
                    } catch (error) {
                        outcome = exitOutcome(false, isAbsent(error));
                    }
                    if (outcome === 'running') {
                        Main.notify('The console is still running',
                            'It was not started by the user service, so it was left alone and this icon stays.');
                        return;
                    }
                    if (outcome === 'unknown') {
                        Main.notify('Blackroom Console',
                            'The console did not answer, so it could not be confirmed stopped and this icon stays. Try Exit again.');
                        return;
                    }
                    Main.notify('Blackroom Console closed', 'Open it again from the applications menu.');
                    GLib.idle_add(GLib.PRIORITY_DEFAULT, () => {
                        Main.extensionManager.disableExtension(this._uuid);
                        return GLib.SOURCE_REMOVE;
                    });
                });
        });
    }

    _systemctl(verb) {
        const process = Gio.Subprocess.new(['systemctl', '--user', verb, UNIT], Gio.SubprocessFlags.STDERR_PIPE);
        process.communicate_utf8_async(null, this._cancellable, (proc, result) => {
            try {
                const [, , stderr] = proc.communicate_utf8_finish(result);
                if (!proc.get_successful())
                    Main.notify('Blackroom Console', `Could not ${verb} the console: ${stderr.trim().slice(0, 200)}`);
            } catch (error) {
                if (!error.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    Main.notify('Blackroom Console', `Could not ${verb} the console: ${error.message}`);
                return;
            }
            this._refresh();
        });
    }

    // The lock-screen extension is a separate extension: this switch enables or disables it.
    _lockAccess() {
        const extension = Main.extensionManager.lookup(LOCK_EXTENSION);
        // 1 = active, 8 = being switched on.
        return extension ? extension.state === 1 || extension.state === 8 : null;
    }

    _setLockAccess(on) {
        try {
            if (on) {
                Main.extensionManager.enableExtension(LOCK_EXTENSION);
                Main.notify('Remote use on the lock screen is on',
                    'Locking this laptop no longer ends a remote session, and any program of yours can open one on the lock screen. Turn it off when you are done.');
            } else {
                Main.extensionManager.disableExtension(LOCK_EXTENSION);
                Main.notify('Remote use on the lock screen is off',
                    'A remote session already open on the locked screen keeps running until it ends; new ones are refused after that.');
            }
        } catch (error) {
            Main.notify('Blackroom Console', `Could not change lock-screen access: ${error.message}`);
        }
        this._showLock();
    }

    _showLock() {
        const on = this._lockAccess();
        this._lockSwitch.visible = on !== null;
        this._lockNote.visible = on !== null;
        if (on !== null)
            this._lockSwitch.setToggleState(on);
        if (on !== this._lockOn) {
            this._lockOn = on;
            console.log(`Blackroom indicator: lock-screen access ${on === null ? 'not installed' : on ? 'on' : 'off'} (state ${Main.extensionManager.lookup(LOCK_EXTENSION)?.state})`);
        }
    }

    _apply(status) {
        const notice = transitionNotice(this._status, status);
        const ask = pendingNotice(this._status, status);
        if (stateOf(this._status) !== stateOf(status) || this._status === undefined)
            console.log(`Blackroom indicator: ${stateOf(status)}${status ? ` mode=${status.mode}` : ''}`);
        this._status = status;
        this._showLock();
        if (notice)
            Main.notify(notice.title, notice.body);
        if (ask) {
            console.log(`Blackroom indicator: connection request ${ask.id}`);
            this._askNotice(ask);
        } else if (!status?.pending) {
            this._dropAsk();
        }

        const shown = view(status);
        this._title.label.text = shown.title;
        this._lines.forEach((item, index) => {
            item.visible = index < shown.lines.length;
            item.label.text = shown.lines[index] ?? '';
        });
        this._accept.visible = shown.canApprove;
        this._deny.visible = shown.canApprove;
        this._disconnect.visible = shown.canDisconnect;
        this._host.visible = shown.canHost;
        this._start.visible = shown.canStart;
        this._stop.visible = shown.canStop;
        this._label.text = shown.badge;
        this._label.visible = shown.badge !== '';
        if (shown.state === 'running' || shown.state === 'starting')
            this._icon.add_style_class_name('blackroom-active');
        else
            this._icon.remove_style_class_name('blackroom-active');
        this._icon.opacity = shown.state === 'off' ? 110 : 255;
    }

    destroy() {
        this._cancellable.cancel();
        this._dropAsk();
        if (this._timer) {
            GLib.source_remove(this._timer);
            this._timer = 0;
        }
        super.destroy();
    }
});

export default class BlackroomIndicatorExtension extends Extension {
    enable() {
        this._indicator = new Indicator(this.uuid);
        Main.panel.addToStatusArea(this.uuid, this._indicator);
    }

    disable() {
        this._indicator?.destroy();
        this._indicator = null;
    }
}
