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

import {pageUrl, parseStatus, pendingNotice, stateOf, transitionNotice, view} from './logic.js';

const BUS_NAME = 'org.blackroom.Console';
const OBJECT_PATH = '/org/blackroom/Console';
const INTERFACE = 'org.blackroom.Console1';
const UNIT = 'blackroom-console.service';
const POLL_SECONDS = 2;

const Indicator = GObject.registerClass(
class BlackroomIndicator extends PanelMenu.Button {
    _init() {
        super._init(0.0, 'Blackroom Console', false);
        this._cancellable = new Gio.Cancellable();
        this._status = undefined;
        this._url = null;

        const box = new St.BoxLayout({style_class: 'panel-status-menu-box'});
        this._icon = new St.Icon({icon_name: 'video-display-symbolic', style_class: 'system-status-icon'});
        this._label = new St.Label({style_class: 'blackroom-label', y_align: Clutter.ActorAlign.CENTER, visible: false});
        box.add_child(this._icon);
        box.add_child(this._label);
        this.add_child(box);

        this._title = new PopupMenu.PopupMenuItem('', {reactive: false});
        this._lines = [new PopupMenu.PopupMenuItem('', {reactive: false}), new PopupMenu.PopupMenuItem('', {reactive: false}),
            new PopupMenu.PopupMenuItem('', {reactive: false})];
        this.menu.addMenuItem(this._title);
        for (const line of this._lines)
            this.menu.addMenuItem(line);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        this._accept = this._action('Accept the connection', () => this._answer('Approve'));
        this._deny = this._action('Deny the connection', () => this._answer('Deny'));
        this._disconnect = this._action('Disconnect the remote user', () => this._call('Disconnect'));
        this._lock = this._action('Lock this screen now', () => Main.screenShield.lock(true));
        this._open = this._action('Open the console page', () => this._openPage());
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._start = this._action('Start the console', () => this._systemctl('start'));
        this._stop = this._action('Stop the console', () => this._systemctl('stop'));

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
                }
                this._apply(status);
            });
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

    _openPage() {
        let urlFile = '';
        try {
            const [, bytes] = GLib.file_get_contents(GLib.build_filenamev([GLib.get_user_runtime_dir(), 'blackroom-console', 'url']));
            urlFile = new TextDecoder().decode(bytes);
        } catch (_error) {
            // No url file (another state directory): open the plain address.
        }
        const url = pageUrl(this._url, urlFile);
        if (url)
            Gio.AppInfo.launch_default_for_uri(url, global.create_app_launch_context(0, -1));
    }

    _apply(status) {
        const notice = transitionNotice(this._status, status);
        const ask = pendingNotice(this._status, status);
        if (stateOf(this._status) !== stateOf(status) || this._status === undefined)
            console.log(`Blackroom indicator: ${stateOf(status)}${status ? ` mode=${status.mode}` : ''}`);
        this._status = status;
        this._url = status ? status.local_url : null;
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
        this._open.visible = shown.canOpen;
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
        this._indicator = new Indicator();
        Main.panel.addToStatusArea(this.uuid, this._indicator);
    }

    disable() {
        this._indicator?.destroy();
        this._indicator = null;
    }
}
