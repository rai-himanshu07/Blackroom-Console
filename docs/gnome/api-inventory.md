# GNOME/Mutter/logind API inventory

Captured by Experiment 2 (`exp02_mutter_inventory`), read-only introspection only.
Raw XML: `docs/gnome/introspection/*.xml`. Standard `org.freedesktop.DBus.{Introspectable,Properties,Peer}` interfaces exist on every object below and are
omitted from these tables for brevity (present verbatim in the raw XML).

## Mutter.DisplayConfig (`session` bus)

Destination: `org.gnome.Mutter.DisplayConfig`  
Object path: `/org/gnome/Mutter/DisplayConfig`

### `org.gnome.Mutter.DisplayConfig`

| Method | In | Out |
|---|---|---|
| `GetResources` | `` | `u, a(uxiiiiiuaua{sv}), a(uxiausauaua{sv}), a(uxuudu), i, i` |
| `ApplyConfiguration` | `u, b, a(uiiiuaua{sv}), a(ua{sv})` | `` |
| `ChangeBacklight` | `u, u, i` | `i` |
| `SetBacklight` | `u, s, i` | `` |
| `GetCrtcGamma` | `u, u` | `aq, aq, aq` |
| `SetCrtcGamma` | `u, u, aq, aq, aq` | `` |
| `GetCurrentState` | `` | `u, a((ssss)a(siiddada{sv})a{sv}), a(iiduba(ssss)a{sv}), a{sv}` |
| `ApplyMonitorsConfig` | `u, u, a(iiduba(ssa{sv})), a{sv}` | `` |
| `SetOutputCTM` | `u, u, (ttttttttt)` | `` |

| Property | Type | Access |
|---|---|---|
| `Backlight` | `(uaa{sv})` | read |
| `PowerSaveMode` | `i` | readwrite |
| `PanelOrientationManaged` | `b` | read |
| `ApplyMonitorsConfigAllowed` | `b` | read |
| `NightLightSupported` | `b` | read |
| `HasExternalMonitor` | `b` | read |

| Signal | Args |
|---|---|
| `MonitorsChanged` | `` |

## Mutter.RemoteDesktop (`session` bus)

Destination: `org.gnome.Mutter.RemoteDesktop`  
Object path: `/org/gnome/Mutter/RemoteDesktop`

### `org.gnome.Mutter.RemoteDesktop`

| Method | In | Out |
|---|---|---|
| `CreateSession` | `` | `o` |

| Property | Type | Access |
|---|---|---|
| `SupportedDeviceTypes` | `u` | read |
| `Version` | `i` | read |

## Mutter.ScreenCast (`session` bus)

Destination: `org.gnome.Mutter.ScreenCast`  
Object path: `/org/gnome/Mutter/ScreenCast`

### `org.gnome.Mutter.ScreenCast`

| Method | In | Out |
|---|---|---|
| `CreateSession` | `a{sv}` | `o` |

| Property | Type | Access |
|---|---|---|
| `Version` | `i` | read |

## Mutter.InputCapture (`session` bus)

Destination: `org.gnome.Mutter.InputCapture`  
Object path: `/org/gnome/Mutter/InputCapture`

### `org.gnome.Mutter.InputCapture`

| Method | In | Out |
|---|---|---|
| `CreateSession` | `u` | `o` |

| Property | Type | Access |
|---|---|---|
| `SupportedCapabilities` | `u` | read |

## Mutter.InputMapping (`session` bus)

Destination: `org.gnome.Mutter.InputMapping`  
Object path: `/org/gnome/Mutter/InputMapping`

### `org.gnome.Mutter.InputMapping`

| Method | In | Out |
|---|---|---|
| `GetDeviceMapping` | `o` | `(iiii)` |

## Mutter.ServiceChannel (`session` bus)

Destination: `org.gnome.Mutter.ServiceChannel`  
Object path: `/org/gnome/Mutter/ServiceChannel`

### `org.gnome.Mutter.ServiceChannel`

| Method | In | Out |
|---|---|---|
| `OpenWaylandServiceConnection` | `u` | `h` |
| `OpenWaylandConnection` | `a{sv}` | `h` |

## Mutter.IdleMonitor (`session` bus)

Destination: `org.gnome.Mutter.IdleMonitor`  
Object path: `/org/gnome/Mutter/IdleMonitor`

*(no non-standard interface at this path)*

## Mutter.IdleMonitor.Core (`session` bus)

Destination: `org.gnome.Mutter.IdleMonitor`  
Object path: `/org/gnome/Mutter/IdleMonitor/Core`

### `org.gnome.Mutter.IdleMonitor`

| Method | In | Out |
|---|---|---|
| `GetIdletime` | `` | `t` |
| `AddIdleWatch` | `t` | `u` |
| `AddUserActiveWatch` | `` | `u` |
| `RemoveWatch` | `u` | `` |
| `ResetIdletime` | `` | `` |

| Signal | Args |
|---|---|
| `WatchFired` | `u` |

## Shell.ScreenShield-at-declared-path (`session` bus)

Destination: `org.gnome.Shell.ScreenShield`  
Object path: `/org/gnome/Shell/ScreenShield`

*(no non-standard interface at this path)*

## Shell.ScreenShield-at-legacy-path (`session` bus)

Destination: `org.gnome.Shell.ScreenShield`  
Object path: `/org/gnome/ScreenSaver`

### `org.gnome.ScreenSaver`

| Method | In | Out |
|---|---|---|
| `Lock` | `` | `` |
| `GetActive` | `` | `b` |
| `SetActive` | `b` | `` |
| `GetActiveTime` | `` | `u` |

| Signal | Args |
|---|---|
| `ActiveChanged` | `b` |
| `WakeUpScreen` | `` |

## ScreenSaver (`session` bus)

Destination: `org.gnome.ScreenSaver`  
Object path: `/org/gnome/ScreenSaver`

### `org.gnome.ScreenSaver`

| Method | In | Out |
|---|---|---|
| `Lock` | `` | `` |
| `GetActive` | `` | `b` |
| `SetActive` | `b` | `` |
| `GetActiveTime` | `` | `u` |

| Signal | Args |
|---|---|
| `ActiveChanged` | `b` |
| `WakeUpScreen` | `` |

## login1.Manager (`system` bus)

Destination: `org.freedesktop.login1`  
Object path: `/org/freedesktop/login1`

### `org.freedesktop.login1.Manager`

| Method | In | Out |
|---|---|---|
| `GetSession` | `s` | `o` |
| `GetSessionByPID` | `u` | `o` |
| `GetUser` | `u` | `o` |
| `GetUserByPID` | `u` | `o` |
| `GetSeat` | `s` | `o` |
| `ListSessions` | `` | `a(susso)` |
| `ListSessionsEx` | `` | `a(sussussbto)` |
| `ListUsers` | `` | `a(uso)` |
| `ListSeats` | `` | `a(so)` |
| `ListInhibitors` | `` | `a(ssssuu)` |
| `CreateSession` | `u, u, s, s, s, s, s, u, s, s, b, s, s, a(sv)` | `s, o, s, h, u, s, u, b` |
| `CreateSessionWithPIDFD` | `u, h, s, s, s, s, s, u, s, s, b, s, s, t, a(sv)` | `s, o, s, h, u, s, u, b` |
| `ReleaseSession` | `s` | `` |
| `ActivateSession` | `s` | `` |
| `ActivateSessionOnSeat` | `s, s` | `` |
| `LockSession` | `s` | `` |
| `UnlockSession` | `s` | `` |
| `LockSessions` | `` | `` |
| `UnlockSessions` | `` | `` |
| `KillSession` | `s, s, i` | `` |
| `KillUser` | `u, i` | `` |
| `TerminateSession` | `s` | `` |
| `TerminateUser` | `u` | `` |
| `TerminateSeat` | `s` | `` |
| `SetUserLinger` | `u, b, b` | `` |
| `AttachDevice` | `s, s, b` | `` |
| `FlushDevices` | `b` | `` |
| `PowerOff` | `b` | `` |
| `PowerOffWithFlags` | `t` | `` |
| `Reboot` | `b` | `` |
| `RebootWithFlags` | `t` | `` |
| `Halt` | `b` | `` |
| `HaltWithFlags` | `t` | `` |
| `Suspend` | `b` | `` |
| `SuspendWithFlags` | `t` | `` |
| `Hibernate` | `b` | `` |
| `HibernateWithFlags` | `t` | `` |
| `HybridSleep` | `b` | `` |
| `HybridSleepWithFlags` | `t` | `` |
| `SuspendThenHibernate` | `b` | `` |
| `SuspendThenHibernateWithFlags` | `t` | `` |
| `Sleep` | `t` | `` |
| `CanPowerOff` | `` | `s` |
| `CanReboot` | `` | `s` |
| `CanHalt` | `` | `s` |
| `CanSuspend` | `` | `s` |
| `CanHibernate` | `` | `s` |
| `CanHybridSleep` | `` | `s` |
| `CanSuspendThenHibernate` | `` | `s` |
| `CanSleep` | `` | `s` |
| `ScheduleShutdown` | `s, t` | `` |
| `CancelScheduledShutdown` | `` | `b` |
| `Inhibit` | `s, s, s, s` | `h` |
| `CanRebootParameter` | `` | `s` |
| `SetRebootParameter` | `s` | `` |
| `CanRebootToFirmwareSetup` | `` | `s` |
| `SetRebootToFirmwareSetup` | `b` | `` |
| `CanRebootToBootLoaderMenu` | `` | `s` |
| `SetRebootToBootLoaderMenu` | `t` | `` |
| `CanRebootToBootLoaderEntry` | `` | `s` |
| `SetRebootToBootLoaderEntry` | `s` | `` |
| `SetWallMessage` | `s, b` | `` |

| Property | Type | Access |
|---|---|---|
| `EnableWallMessages` | `b` | readwrite |
| `WallMessage` | `s` | readwrite |
| `NAutoVTs` | `u` | read |
| `KillOnlyUsers` | `as` | read |
| `KillExcludeUsers` | `as` | read |
| `KillUserProcesses` | `b` | read |
| `RebootParameter` | `s` | read |
| `RebootToFirmwareSetup` | `b` | read |
| `RebootToBootLoaderMenu` | `t` | read |
| `RebootToBootLoaderEntry` | `s` | read |
| `BootLoaderEntries` | `as` | read |
| `IdleHint` | `b` | read |
| `IdleSinceHint` | `t` | read |
| `IdleSinceHintMonotonic` | `t` | read |
| `BlockInhibited` | `s` | read |
| `BlockWeakInhibited` | `s` | read |
| `DelayInhibited` | `s` | read |
| `InhibitDelayMaxUSec` | `t` | read |
| `UserStopDelayUSec` | `t` | read |
| `SleepOperation` | `as` | read |
| `HandlePowerKey` | `s` | read |
| `HandlePowerKeyLongPress` | `s` | read |
| `HandleRebootKey` | `s` | read |
| `HandleRebootKeyLongPress` | `s` | read |
| `HandleSuspendKey` | `s` | read |
| `HandleSuspendKeyLongPress` | `s` | read |
| `HandleHibernateKey` | `s` | read |
| `HandleHibernateKeyLongPress` | `s` | read |
| `HandleLidSwitch` | `s` | read |
| `HandleLidSwitchExternalPower` | `s` | read |
| `HandleLidSwitchDocked` | `s` | read |
| `HandleSecureAttentionKey` | `s` | read |
| `HoldoffTimeoutUSec` | `t` | read |
| `IdleAction` | `s` | read |
| `IdleActionUSec` | `t` | read |
| `PreparingForShutdown` | `b` | read |
| `PreparingForShutdownWithMetadata` | `a{sv}` | read |
| `PreparingForSleep` | `b` | read |
| `ScheduledShutdown` | `(st)` | read |
| `DesignatedMaintenanceTime` | `s` | read |
| `Docked` | `b` | read |
| `LidClosed` | `b` | read |
| `OnExternalPower` | `b` | read |
| `RemoveIPC` | `b` | read |
| `RuntimeDirectorySize` | `t` | read |
| `RuntimeDirectoryInodesMax` | `t` | read |
| `InhibitorsMax` | `t` | read |
| `NCurrentInhibitors` | `t` | read |
| `SessionsMax` | `t` | read |
| `NCurrentSessions` | `t` | read |
| `StopIdleSessionUSec` | `t` | read |

| Signal | Args |
|---|---|
| `SecureAttentionKey` | `s, o` |
| `SessionNew` | `s, o` |
| `SessionRemoved` | `s, o` |
| `UserNew` | `u, o` |
| `UserRemoved` | `u, o` |
| `SeatNew` | `s, o` |
| `SeatRemoved` | `s, o` |
| `PrepareForShutdown` | `b` |
| `PrepareForShutdownWithMetadata` | `b, a{sv}` |
| `PrepareForSleep` | `b` |

## login1.Session-selected (`system` bus)

Destination: `org.freedesktop.login1`  
Object path: `/org/freedesktop/login1/session/_32`

### `org.freedesktop.login1.Session`

| Method | In | Out |
|---|---|---|
| `Terminate` | `` | `` |
| `Activate` | `` | `` |
| `Lock` | `` | `` |
| `Unlock` | `` | `` |
| `SetIdleHint` | `b` | `` |
| `SetLockedHint` | `b` | `` |
| `Kill` | `s, i` | `` |
| `TakeControl` | `b` | `` |
| `ReleaseControl` | `` | `` |
| `SetType` | `s` | `` |
| `SetClass` | `s` | `` |
| `SetDisplay` | `s` | `` |
| `SetTTY` | `h` | `` |
| `TakeDevice` | `u, u` | `h, b` |
| `ReleaseDevice` | `u, u` | `` |
| `PauseDeviceComplete` | `u, u` | `` |
| `SetBrightness` | `s, s, u` | `` |

| Property | Type | Access |
|---|---|---|
| `Id` | `s` | read |
| `User` | `(uo)` | read |
| `Name` | `s` | read |
| `Timestamp` | `t` | read |
| `TimestampMonotonic` | `t` | read |
| `VTNr` | `u` | read |
| `Seat` | `(so)` | read |
| `TTY` | `s` | read |
| `Display` | `s` | read |
| `Remote` | `b` | read |
| `RemoteHost` | `s` | read |
| `RemoteUser` | `s` | read |
| `ExtraDeviceAccess` | `as` | read |
| `Service` | `s` | read |
| `Desktop` | `s` | read |
| `Scope` | `s` | read |
| `Leader` | `u` | read |
| `LeaderPIDFDId` | `t` | read |
| `Audit` | `u` | read |
| `Type` | `s` | read |
| `Class` | `s` | read |
| `Active` | `b` | read |
| `State` | `s` | read |
| `IdleHint` | `b` | read |
| `IdleSinceHint` | `t` | read |
| `IdleSinceHintMonotonic` | `t` | read |
| `CanIdle` | `b` | read |
| `CanLock` | `b` | read |
| `LockedHint` | `b` | read |

| Signal | Args |
|---|---|
| `PauseDevice` | `u, u, s` |
| `ResumeDevice` | `u, u, h` |
| `Lock` | `` |
| `Unlock` | `` |

## DisplayConfig.GetCurrentState summary

Serial: `1`

| Connector | Vendor | Product | Serial (hashed) | Current mode | Modes |
|---|---|---|---|---|---|
| `HDMI-1` | SAM | Smart M80C | `7fddfc2eecca3d01` | (none marked current) | 31 |
| `eDP-1` | AUO | 0xed8f | `cfb3d220f3780aae` | 1920x1080@120.21Hz (1920x1080@120.213) | 128 |

| Logical monitor | Position | Scale | Transform | Primary | Connectors |
|---|---|---|---|---|---|
| — | (0, 0) | 1 | 0 | true | eDP-1 |

