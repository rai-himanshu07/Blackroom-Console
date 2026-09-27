//! Owned EI client socket for a RemoteDesktop session.

use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::lease::InputAuthorization;
use zbus::zvariant;

pub struct EiConnection {
    context: reis::ei::Context,
    connection: Option<reis::event::Connection>,
    converter: Option<reis::event::EiEventConverter>,
    sequence: u32,
    active_devices: Vec<(reis::event::Device, u32)>,
}

impl EiConnection {
    pub fn from_fd(fd: zvariant::OwnedFd) -> Result<Self, BlackroomError> {
        let fd: OwnedFd = fd.into();
        let context = reis::ei::Context::new(UnixStream::from(fd)).map_err(|_| {
            BlackroomError::new(
                ErrorCode::MutterUnavailable,
                "EIS socket initialization failed",
            )
        })?;
        Ok(Self {
            context,
            connection: None,
            converter: None,
            sequence: 0,
            active_devices: Vec::new(),
        })
    }

    pub fn handshake_sender(&mut self, timeout: Duration) -> Result<(), BlackroomError> {
        let deadline = Instant::now() + timeout;
        let mut handshake = reis::handshake::EiHandshaker::new(
            "Blackroom Console",
            reis::ei::handshake::ContextType::Sender,
        );
        loop {
            if Instant::now() >= deadline {
                return Err(BlackroomError::new(
                    ErrorCode::IpcTimeout,
                    "EIS sender handshake timed out",
                ));
            }
            while let Some(message) = self.context.pending_event() {
                if Instant::now() >= deadline {
                    return Err(BlackroomError::new(
                        ErrorCode::IpcTimeout,
                        "EIS sender handshake timed out",
                    ));
                }
                let reis::PendingRequestResult::Request(event) = message else {
                    return Err(BlackroomError::new(
                        ErrorCode::MutterUnavailable,
                        "invalid EIS handshake message",
                    ));
                };
                if let Some(response) = handshake.handle_event(event).map_err(|_| {
                    BlackroomError::new(ErrorCode::MutterUnavailable, "EIS sender handshake failed")
                })? {
                    let converter = reis::event::EiEventConverter::new(&self.context, response);
                    self.connection = Some(converter.connection().clone());
                    self.converter = Some(converter);
                    return Ok(());
                }
            }
            if !Self::readable_until(&self.context, deadline)? {
                return Err(BlackroomError::new(
                    ErrorCode::IpcTimeout,
                    "EIS sender handshake timed out",
                ));
            }
            match self.context.read() {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {
                    return Err(BlackroomError::new(
                        ErrorCode::MutterUnavailable,
                        "EIS handshake socket closed",
                    ));
                }
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        self.connection.is_some() && self.converter.is_some()
    }

    pub fn bind_seat(
        &mut self,
        added: &reis::event::SeatAdded,
        capabilities: reis::event::DeviceCapability,
    ) -> Result<(), BlackroomError> {
        if !self.is_ready() {
            return Err(BlackroomError::new(
                ErrorCode::MutterUnavailable,
                "EIS sender is not ready",
            ));
        }
        added.seat.bind_capabilities(capabilities.into());
        self.context
            .flush()
            .map_err(|_| BlackroomError::new(ErrorCode::MutterUnavailable, "EIS seat bind failed"))
    }

    pub fn send_key_tap(
        &mut self,
        authorization: &InputAuthorization<'_>,
        resumed: &reis::event::DeviceResumed,
        keycode: u32,
    ) -> Result<(), BlackroomError> {
        authorization.validate()?;
        let deadline = Instant::now() + Duration::from_millis(10);
        while Instant::now() < deadline {
            if self
                .next_event_until(deadline.saturating_duration_since(Instant::now()))?
                .is_none()
            {
                break;
            }
        }
        authorization.dispatch(keycode, |keycode| {
            if !self.is_ready() {
                return Err(BlackroomError::new(
                    ErrorCode::MutterUnavailable,
                    "EIS sender is not ready",
                ));
            }
            if !self
                .active_devices
                .iter()
                .any(|(device, serial)| device == &resumed.device && *serial == resumed.serial)
            {
                return Err(BlackroomError::new(
                    ErrorCode::MutterUnavailable,
                    "EIS keyboard device is not active",
                ));
            }
            let keyboard = resumed
                .device
                .interface::<reis::ei::Keyboard>()
                .ok_or_else(|| {
                    BlackroomError::new(ErrorCode::MutterUnavailable, "EIS keyboard unavailable")
                })?;
            let sequence = self.sequence.checked_add(1).ok_or_else(|| {
                BlackroomError::new(ErrorCode::MutterUnavailable, "EIS sequence exhausted")
            })?;
            let pressed_at = monotonic_micros()?;
            let released_at = monotonic_micros()?.max(pressed_at.saturating_add(1));
            self.sequence = sequence;
            resumed
                .device
                .device()
                .start_emulating(resumed.serial, sequence);
            keyboard.key(keycode, reis::ei::keyboard::KeyState::Press);
            resumed.device.device().frame(resumed.serial, pressed_at);
            keyboard.key(keycode, reis::ei::keyboard::KeyState::Released);
            resumed.device.device().frame(resumed.serial, released_at);
            resumed.device.device().stop_emulating(resumed.serial);
            if self.context.flush().is_err() {
                self.connection = None;
                self.converter = None;
                return Err(BlackroomError::new(
                    ErrorCode::MutterUnavailable,
                    "EIS key delivery failed",
                ));
            }
            Ok(())
        })
    }

    pub fn send_button_click(
        &mut self,
        authorization: &InputAuthorization<'_>,
        resumed: &reis::event::DeviceResumed,
        button_code: u32,
    ) -> Result<(), BlackroomError> {
        authorization.validate()?;
        let deadline = Instant::now() + Duration::from_millis(10);
        while Instant::now() < deadline {
            if self
                .next_event_until(deadline.saturating_duration_since(Instant::now()))?
                .is_none()
            {
                break;
            }
        }
        authorization.dispatch(button_code, |button_code| {
            if !self.is_ready()
                || !self
                    .active_devices
                    .iter()
                    .any(|(device, serial)| device == &resumed.device && *serial == resumed.serial)
            {
                return Err(BlackroomError::new(
                    ErrorCode::MutterUnavailable,
                    "EIS button device is not active",
                ));
            }
            let button = resumed
                .device
                .interface::<reis::ei::Button>()
                .ok_or_else(|| {
                    BlackroomError::new(ErrorCode::MutterUnavailable, "EIS button unavailable")
                })?;
            let sequence = self.sequence.checked_add(1).ok_or_else(|| {
                BlackroomError::new(ErrorCode::MutterUnavailable, "EIS sequence exhausted")
            })?;
            let pressed_at = monotonic_micros()?;
            let released_at = monotonic_micros()?.max(pressed_at.saturating_add(1));
            self.sequence = sequence;
            resumed
                .device
                .device()
                .start_emulating(resumed.serial, sequence);
            button.button(button_code, reis::ei::button::ButtonState::Press);
            resumed.device.device().frame(resumed.serial, pressed_at);
            button.button(button_code, reis::ei::button::ButtonState::Released);
            resumed.device.device().frame(resumed.serial, released_at);
            resumed.device.device().stop_emulating(resumed.serial);
            if self.context.flush().is_err() {
                self.connection = None;
                self.converter = None;
                return Err(BlackroomError::new(
                    ErrorCode::MutterUnavailable,
                    "EIS button delivery failed",
                ));
            }
            Ok(())
        })
    }

    pub fn send_key_chord(
        &mut self,
        authorization: &InputAuthorization<'_>,
        resumed: &reis::event::DeviceResumed,
        modifier_keycode: u32,
        keycode: u32,
    ) -> Result<(), BlackroomError> {
        if modifier_keycode == keycode {
            return Err(BlackroomError::new(
                ErrorCode::IpcInvalidMessage,
                "EIS modifier and key must be different",
            ));
        }
        authorization.validate()?;
        let deadline = Instant::now() + Duration::from_millis(10);
        while Instant::now() < deadline {
            if self
                .next_event_until(deadline.saturating_duration_since(Instant::now()))?
                .is_none()
            {
                break;
            }
        }
        authorization.dispatch(
            (modifier_keycode, keycode),
            |(modifier_keycode, keycode)| {
                if !self.is_ready()
                    || !self.active_devices.iter().any(|(device, serial)| {
                        device == &resumed.device && *serial == resumed.serial
                    })
                {
                    return Err(BlackroomError::new(
                        ErrorCode::MutterUnavailable,
                        "EIS keyboard device is not active",
                    ));
                }
                let keyboard = resumed
                    .device
                    .interface::<reis::ei::Keyboard>()
                    .ok_or_else(|| {
                        BlackroomError::new(
                            ErrorCode::MutterUnavailable,
                            "EIS keyboard unavailable",
                        )
                    })?;
                let sequence = self.sequence.checked_add(1).ok_or_else(|| {
                    BlackroomError::new(ErrorCode::MutterUnavailable, "EIS sequence exhausted")
                })?;
                let started_at = monotonic_micros()?;
                self.sequence = sequence;
                resumed
                    .device
                    .device()
                    .start_emulating(resumed.serial, sequence);
                keyboard.key(modifier_keycode, reis::ei::keyboard::KeyState::Press);
                resumed.device.device().frame(resumed.serial, started_at);
                keyboard.key(keycode, reis::ei::keyboard::KeyState::Press);
                resumed
                    .device
                    .device()
                    .frame(resumed.serial, started_at.saturating_add(1));
                keyboard.key(keycode, reis::ei::keyboard::KeyState::Released);
                resumed
                    .device
                    .device()
                    .frame(resumed.serial, started_at.saturating_add(2));
                keyboard.key(modifier_keycode, reis::ei::keyboard::KeyState::Released);
                resumed
                    .device
                    .device()
                    .frame(resumed.serial, started_at.saturating_add(3));
                resumed.device.device().stop_emulating(resumed.serial);
                if self.context.flush().is_err() {
                    self.connection = None;
                    self.converter = None;
                    return Err(BlackroomError::new(
                        ErrorCode::MutterUnavailable,
                        "EIS key chord delivery failed",
                    ));
                }
                Ok(())
            },
        )
    }

    pub fn send_pointer_motion(
        &mut self,
        authorization: &InputAuthorization<'_>,
        resumed: &reis::event::DeviceResumed,
        dx: f32,
        dy: f32,
    ) -> Result<(), BlackroomError> {
        if !dx.is_finite() || !dy.is_finite() {
            return Err(BlackroomError::new(
                ErrorCode::IpcInvalidMessage,
                "EIS pointer motion must be finite",
            ));
        }
        authorization.validate()?;
        let deadline = Instant::now() + Duration::from_millis(10);
        while Instant::now() < deadline {
            if self
                .next_event_until(deadline.saturating_duration_since(Instant::now()))?
                .is_none()
            {
                break;
            }
        }
        authorization.dispatch((dx, dy), |(dx, dy)| {
            if !self.is_ready()
                || !self
                    .active_devices
                    .iter()
                    .any(|(device, serial)| device == &resumed.device && *serial == resumed.serial)
            {
                return Err(BlackroomError::new(
                    ErrorCode::MutterUnavailable,
                    "EIS pointer device is not active",
                ));
            }
            let pointer = resumed
                .device
                .interface::<reis::ei::Pointer>()
                .ok_or_else(|| {
                    BlackroomError::new(ErrorCode::MutterUnavailable, "EIS pointer unavailable")
                })?;
            let sequence = self.sequence.checked_add(1).ok_or_else(|| {
                BlackroomError::new(ErrorCode::MutterUnavailable, "EIS sequence exhausted")
            })?;
            let timestamp = monotonic_micros()?;
            self.sequence = sequence;
            resumed
                .device
                .device()
                .start_emulating(resumed.serial, sequence);
            pointer.motion_relative(dx, dy);
            resumed.device.device().frame(resumed.serial, timestamp);
            resumed.device.device().stop_emulating(resumed.serial);
            if self.context.flush().is_err() {
                self.connection = None;
                self.converter = None;
                return Err(BlackroomError::new(
                    ErrorCode::MutterUnavailable,
                    "EIS pointer delivery failed",
                ));
            }
            Ok(())
        })
    }

    pub fn send_scroll_delta(
        &mut self,
        authorization: &InputAuthorization<'_>,
        resumed: &reis::event::DeviceResumed,
        dx: f32,
        dy: f32,
    ) -> Result<(), BlackroomError> {
        if !dx.is_finite() || !dy.is_finite() {
            return Err(BlackroomError::new(
                ErrorCode::IpcInvalidMessage,
                "EIS scroll delta must be finite",
            ));
        }
        authorization.validate()?;
        let deadline = Instant::now() + Duration::from_millis(10);
        while Instant::now() < deadline {
            if self
                .next_event_until(deadline.saturating_duration_since(Instant::now()))?
                .is_none()
            {
                break;
            }
        }
        authorization.dispatch((dx, dy), |(dx, dy)| {
            if !self.is_ready()
                || !self
                    .active_devices
                    .iter()
                    .any(|(device, serial)| device == &resumed.device && *serial == resumed.serial)
            {
                return Err(BlackroomError::new(
                    ErrorCode::MutterUnavailable,
                    "EIS scroll device is not active",
                ));
            }
            let scroll = resumed
                .device
                .interface::<reis::ei::Scroll>()
                .ok_or_else(|| {
                    BlackroomError::new(ErrorCode::MutterUnavailable, "EIS scroll unavailable")
                })?;
            let sequence = self.sequence.checked_add(1).ok_or_else(|| {
                BlackroomError::new(ErrorCode::MutterUnavailable, "EIS sequence exhausted")
            })?;
            let timestamp = monotonic_micros()?;
            self.sequence = sequence;
            resumed
                .device
                .device()
                .start_emulating(resumed.serial, sequence);
            scroll.scroll(dx, dy);
            resumed.device.device().frame(resumed.serial, timestamp);
            resumed.device.device().stop_emulating(resumed.serial);
            if self.context.flush().is_err() {
                self.connection = None;
                self.converter = None;
                return Err(BlackroomError::new(
                    ErrorCode::MutterUnavailable,
                    "EIS scroll delivery failed",
                ));
            }
            Ok(())
        })
    }

    pub fn next_event_until(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<reis::event::EiEvent>, BlackroomError> {
        let deadline = Instant::now() + timeout;
        let converter = self.converter.as_mut().ok_or_else(|| {
            BlackroomError::new(ErrorCode::MutterUnavailable, "EIS handshake is not ready")
        })?;
        loop {
            if Instant::now() >= deadline {
                return Ok(None);
            }
            if let Some(event) = converter.next_event() {
                Self::track_device_event(&mut self.active_devices, &event);
                return Ok(Some(event));
            }
            while let Some(message) = self.context.pending_event() {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                let reis::PendingRequestResult::Request(event) = message else {
                    return Err(BlackroomError::new(
                        ErrorCode::MutterUnavailable,
                        "invalid EIS device event",
                    ));
                };
                converter.handle_event(event).map_err(|_| {
                    BlackroomError::new(ErrorCode::MutterUnavailable, "EIS device event failed")
                })?;
            }
            if let Some(event) = converter.next_event() {
                Self::track_device_event(&mut self.active_devices, &event);
                return Ok(Some(event));
            }
            if !Self::readable_until(&self.context, deadline)? {
                return Ok(None);
            }
            match self.context.read() {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {
                    self.active_devices.clear();
                    self.connection = None;
                    return Err(BlackroomError::new(
                        ErrorCode::MutterUnavailable,
                        "EIS device socket closed",
                    ));
                }
            }
        }
    }

    fn track_device_event(
        active_devices: &mut Vec<(reis::event::Device, u32)>,
        event: &reis::event::EiEvent,
    ) {
        match event {
            reis::event::EiEvent::DeviceResumed(resumed) => {
                active_devices.retain(|(device, _)| device != &resumed.device);
                active_devices.push((resumed.device.clone(), resumed.serial));
            }
            reis::event::EiEvent::DevicePaused(paused) => {
                active_devices.retain(|(device, _)| device != &paused.device);
            }
            reis::event::EiEvent::DeviceRemoved(removed) => {
                active_devices.retain(|(device, _)| device != &removed.device);
            }
            reis::event::EiEvent::SeatRemoved(_) | reis::event::EiEvent::Disconnected(_) => {
                active_devices.clear();
            }
            _ => {}
        }
    }

    fn readable_until(
        context: &reis::ei::Context,
        deadline: Instant,
    ) -> Result<bool, BlackroomError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        let timeout = rustix::event::Timespec {
            tv_sec: i64::try_from(remaining.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(remaining.subsec_nanos()),
        };
        let mut fds = [rustix::event::PollFd::new(
            context,
            rustix::event::PollFlags::IN,
        )];
        rustix::event::poll(&mut fds, Some(&timeout))
            .map(|ready| ready > 0)
            .map_err(|_| {
                BlackroomError::new(ErrorCode::MutterUnavailable, "EIS socket poll failed")
            })
    }
}

fn monotonic_micros() -> Result<u64, BlackroomError> {
    let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let seconds = u64::try_from(now.tv_sec)
        .map_err(|_| BlackroomError::new(ErrorCode::MutterUnavailable, "invalid EIS clock"))?;
    let nanos = u64::try_from(now.tv_nsec)
        .map_err(|_| BlackroomError::new(ErrorCode::MutterUnavailable, "invalid EIS clock"))?;
    seconds
        .checked_mul(1_000_000)
        .and_then(|micros| micros.checked_add(nanos / 1_000))
        .ok_or_else(|| BlackroomError::new(ErrorCode::MutterUnavailable, "invalid EIS clock"))
}

impl AsFd for EiConnection {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.context.as_fd()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime};

    use blackroom_core::epoch::SecurityEpoch;
    use blackroom_core::lease::{Capability, ControlLease};
    use blackroom_core::state::State;

    use super::*;

    #[test]
    fn silent_ei_peer_respects_read_deadline() -> Result<(), Box<dyn std::error::Error>> {
        let (client, mut server) = UnixStream::pair()?;
        let connection = EiConnection::from_fd(zvariant::OwnedFd::from(OwnedFd::from(client)))?;
        assert!(!EiConnection::readable_until(
            &connection.context,
            Instant::now() + Duration::from_millis(30)
        )?);
        use std::io::Write;
        server.write_all(&[1])?;
        assert!(EiConnection::readable_until(
            &connection.context,
            Instant::now() + Duration::from_millis(100)
        )?);
        Ok(())
    }

    #[test]
    fn ei_connection_owns_and_closes_the_returned_socket() -> Result<(), Box<dyn std::error::Error>>
    {
        let (client, mut server) = UnixStream::pair()?;
        server.set_read_timeout(Some(Duration::from_secs(1)))?;
        let fd = zvariant::OwnedFd::from(OwnedFd::from(client));
        let connection = EiConnection::from_fd(fd)?;
        let _ = connection.as_fd();
        drop(connection);

        let mut buffer = [0_u8; 1];
        assert_eq!(server.read(&mut buffer)?, 0);
        Ok(())
    }

    #[test]
    fn sender_handshake_with_synthetic_eis_peer() -> Result<(), Box<dyn std::error::Error>> {
        let (client, server_socket) = UnixStream::pair()?;
        let (done_tx, done_rx) = mpsc::channel();
        let server = thread::spawn(move || -> Result<(), std::io::Error> {
            let context = reis::eis::Context::new(server_socket)?;
            let mut handshake = reis::handshake::EisHandshaker::new(&context, 1);
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                match context.read() {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::yield_now();
                        continue;
                    }
                    Err(error) => return Err(error),
                }
                while let Some(request) = context.pending_request() {
                    let reis::PendingRequestResult::Request(request) = request else {
                        return Err(std::io::Error::other("invalid synthetic EIS request"));
                    };
                    if let Some(response) = handshake
                        .handle_request(request)
                        .map_err(|error| std::io::Error::other(error.to_string()))?
                    {
                        if response.context_type != reis::eis::handshake::ContextType::Sender {
                            return Err(std::io::Error::other(
                                "EI client did not negotiate Sender",
                            ));
                        }
                        context
                            .flush()
                            .map_err(|error| std::io::Error::other(error.to_string()))?;
                        let mut converter =
                            reis::request::EisRequestConverter::new(&context, response, 1);
                        let _seat = converter.handle().add_seat(
                            Some("synthetic-keyboard"),
                            reis::request::DeviceCapability::Keyboard
                                | reis::request::DeviceCapability::Pointer
                                | reis::request::DeviceCapability::Scroll
                                | reis::request::DeviceCapability::Button,
                        );
                        context
                            .flush()
                            .map_err(|error| std::io::Error::other(error.to_string()))?;
                        let mut key_states = Vec::new();
                        let mut key_times = Vec::new();
                        let mut keyboard_device = None;
                        let mut keyboard_complete = false;
                        let mut motion_phase = 0;
                        let mut active_serial = None;
                        let mut motion_time = None;
                        let mut scroll_time = None;
                        let mut button_pressed_at = None;
                        let mut button_released_at = None;
                        let mut chord_times = Vec::new();
                        let deadline = Instant::now() + Duration::from_secs(2);
                        while Instant::now() < deadline {
                            match context.read() {
                                Ok(_) => {}
                                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                    thread::yield_now();
                                    continue;
                                }
                                Err(error) => return Err(error),
                            }
                            while let Some(request) = context.pending_request() {
                                let reis::PendingRequestResult::Request(request) = request else {
                                    return Err(std::io::Error::other("invalid seat-bind request"));
                                };
                                converter
                                    .handle_request(request)
                                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                            }
                            while let Some(request) = converter.next_request() {
                                if let reis::request::EisRequest::Bind(binding) = request {
                                    let device = binding.seat.add_device(
                                        Some("synthetic-keyboard-device"),
                                        reis::eis::device::DeviceType::Virtual,
                                        reis::request::DeviceCapability::Keyboard
                                            | reis::request::DeviceCapability::Pointer
                                            | reis::request::DeviceCapability::Scroll
                                            | reis::request::DeviceCapability::Button,
                                        |_| {},
                                    );
                                    device.resumed();
                                    keyboard_device = Some(device);
                                    context.flush().map_err(|error| {
                                        std::io::Error::other(error.to_string())
                                    })?;
                                } else if let reis::request::EisRequest::DeviceStartEmulating(
                                    start,
                                ) = request
                                {
                                    if keyboard_complete {
                                        let sequence = match motion_phase {
                                            0 => 2,
                                            4 => 3,
                                            8 => 4,
                                            14 => 5,
                                            _ => {
                                                return Err(std::io::Error::other(
                                                    "unexpected emulation start",
                                                ));
                                            }
                                        };
                                        if start.sequence != sequence {
                                            return Err(std::io::Error::other(
                                                "unexpected emulation sequence",
                                            ));
                                        }
                                        active_serial = Some(start.last_serial);
                                        motion_phase += 1;
                                    }
                                } else if let reis::request::EisRequest::KeyboardKey(key) = request
                                {
                                    if !keyboard_complete {
                                        if key.key != 30 {
                                            return Err(std::io::Error::other(
                                                "unexpected synthetic key",
                                            ));
                                        }
                                        key_states.push(key.state);
                                        key_times.push(key.time);
                                    } else {
                                        motion_phase = match (motion_phase, key.key, key.state) {
                                            (15, 29, reis::eis::keyboard::KeyState::Press) => 16,
                                            (17, 30, reis::eis::keyboard::KeyState::Press) => 18,
                                            (19, 30, reis::eis::keyboard::KeyState::Released) => 20,
                                            (21, 29, reis::eis::keyboard::KeyState::Released) => 22,
                                            _ => {
                                                return Err(std::io::Error::other(
                                                    "unexpected chord key",
                                                ));
                                            }
                                        };
                                        chord_times.push(key.time);
                                    }
                                } else if let reis::request::EisRequest::PointerMotion(motion) =
                                    request
                                {
                                    if motion_phase != 1 || motion.dx != 2.0 || motion.dy != -3.0 {
                                        return Err(std::io::Error::other(
                                            "unexpected pointer motion",
                                        ));
                                    }
                                    motion_time = Some(motion.time);
                                    motion_phase = 2;
                                } else if let reis::request::EisRequest::ScrollDelta(delta) =
                                    request
                                {
                                    if motion_phase != 5 || delta.dx != 0.0 || delta.dy != 5.0 {
                                        return Err(std::io::Error::other(
                                            "unexpected scroll delta",
                                        ));
                                    }
                                    scroll_time = Some(delta.time);
                                    motion_phase = 6;
                                } else if let reis::request::EisRequest::Button(button) = request {
                                    if button.button != 272 {
                                        return Err(std::io::Error::other(
                                            "unexpected button code",
                                        ));
                                    }
                                    match (motion_phase, button.state) {
                                        (9, reis::eis::button::ButtonState::Press) => {
                                            button_pressed_at = Some(button.time);
                                            motion_phase = 10;
                                        }
                                        (11, reis::eis::button::ButtonState::Released) => {
                                            button_released_at = Some(button.time);
                                            motion_phase = 12;
                                        }
                                        _ => {
                                            return Err(std::io::Error::other(
                                                "unexpected button state",
                                            ));
                                        }
                                    }
                                } else if let reis::request::EisRequest::Frame(frame) = request {
                                    if keyboard_complete {
                                        let event_time = match motion_phase {
                                            2 => motion_time,
                                            6 => scroll_time,
                                            10 => button_pressed_at,
                                            12 => button_released_at,
                                            16 | 18 | 20 | 22 => chord_times.last().copied(),
                                            _ => {
                                                return Err(std::io::Error::other(
                                                    "unexpected motion frame",
                                                ));
                                            }
                                        };
                                        if frame.time == 0
                                            || event_time != Some(frame.time)
                                            || active_serial != Some(frame.last_serial)
                                        {
                                            return Err(std::io::Error::other(
                                                "invalid motion frame",
                                            ));
                                        }
                                        if motion_phase == 12
                                            && button_released_at <= button_pressed_at
                                        {
                                            return Err(std::io::Error::other(
                                                "button release must follow press",
                                            ));
                                        }
                                        if motion_phase == 22
                                            && (chord_times.len() != 4
                                                || chord_times
                                                    .windows(2)
                                                    .any(|times| times[0] >= times[1]))
                                        {
                                            return Err(std::io::Error::other(
                                                "chord key frames must increase",
                                            ));
                                        }
                                        motion_phase += 1;
                                    }
                                } else if let reis::request::EisRequest::DeviceStopEmulating(stop) =
                                    request
                                {
                                    if !keyboard_complete {
                                        if key_states
                                            != [
                                                reis::eis::keyboard::KeyState::Press,
                                                reis::eis::keyboard::KeyState::Released,
                                            ]
                                            || key_times.len() != 2
                                            || key_times[0] >= key_times[1]
                                        {
                                            return Err(std::io::Error::other(
                                                "key tap did not release",
                                            ));
                                        }
                                        keyboard_complete = true;
                                        let device = keyboard_device.as_ref().ok_or_else(|| {
                                            std::io::Error::other(
                                                "synthetic keyboard device missing",
                                            )
                                        })?;
                                        device.paused();
                                        device.resumed();
                                        context.flush().map_err(|error| {
                                            std::io::Error::other(error.to_string())
                                        })?;
                                    } else {
                                        if active_serial != Some(stop.last_serial) {
                                            return Err(std::io::Error::other(
                                                "unexpected stop serial",
                                            ));
                                        }
                                        match motion_phase {
                                            3 => motion_phase = 4,
                                            7 => motion_phase = 8,
                                            13 => motion_phase = 14,
                                            23 => {
                                                done_rx
                                                    .recv_timeout(Duration::from_secs(2))
                                                    .map_err(std::io::Error::other)?;
                                                return Ok(());
                                            }
                                            _ => {
                                                return Err(std::io::Error::other(
                                                    "unexpected emulation stop",
                                                ));
                                            }
                                        }
                                    }
                                } else if keyboard_complete {
                                    return Err(std::io::Error::other(
                                        "unexpected synthetic EIS request",
                                    ));
                                }
                            }
                        }
                        return Err(std::io::Error::other("synthetic keyboard bind timed out"));
                    }
                }
            }
            Err(std::io::Error::other("synthetic EI handshake timed out"))
        });

        let mut sender = EiConnection::from_fd(zvariant::OwnedFd::from(OwnedFd::from(client)))?;
        assert!(!sender.is_ready());
        sender.handshake_sender(Duration::from_secs(2))?;
        assert!(sender.is_ready());
        let event = sender.next_event_until(Duration::from_secs(2))?;
        let Some(reis::event::EiEvent::SeatAdded(added)) = event else {
            return Err(std::io::Error::other("synthetic keyboard seat not advertised").into());
        };
        added.seat.bind_capabilities(
            reis::event::DeviceCapability::Keyboard
                | reis::event::DeviceCapability::Pointer
                | reis::event::DeviceCapability::Scroll
                | reis::event::DeviceCapability::Button,
        );
        sender.context.flush()?;
        let event = sender.next_event_until(Duration::from_secs(2))?;
        assert!(matches!(event, Some(reis::event::EiEvent::DeviceAdded(_))));
        let event = sender.next_event_until(Duration::from_secs(2))?;
        let Some(reis::event::EiEvent::DeviceResumed(resumed)) = event else {
            return Err(std::io::Error::other("synthetic device not resumed").into());
        };
        let now = SystemTime::now();
        let lease = ControlLease {
            session_id: "rs_SYNTHETIC".to_string(),
            host_id: "bc_SYNTHETIC".to_string(),
            user_id: "test".to_string(),
            client_id: "cl_SYNTHETIC".to_string(),
            security_epoch: SecurityEpoch::INITIAL,
            issued_at: now,
            expires_at: now + Duration::from_secs(30),
            capabilities: vec![Capability::View, Capability::Control],
        };
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32]);
        let signature = lease.sign(&signing_key);
        let verifying_key = signing_key.verifying_key();
        let mut authorization = InputAuthorization {
            lease: &lease,
            signature: &signature,
            verifying_key: &verifying_key,
            authenticated: true,
            authorized: true,
            current_epoch: SecurityEpoch::INITIAL,
            current_state: State::RemoteActive,
            current_session_id: "rs_SYNTHETIC",
            revoked: false,
            now,
        };
        sender.send_key_tap(&authorization, &resumed, 30)?;
        assert_eq!(sender.sequence, 1);
        authorization.revoked = true;
        assert_eq!(
            sender
                .send_key_tap(&authorization, &resumed, 30)
                .unwrap_err()
                .code,
            ErrorCode::LeaseRevoked
        );
        authorization.revoked = false;
        authorization.current_state = State::LocalLocked;
        assert_eq!(
            sender
                .send_key_tap(&authorization, &resumed, 30)
                .unwrap_err()
                .code,
            ErrorCode::LeaseInvalid
        );
        assert_eq!(sender.sequence, 1);
        authorization.current_state = State::RemoteActive;
        let event = sender.next_event_until(Duration::from_secs(2))?;
        assert!(matches!(event, Some(reis::event::EiEvent::DevicePaused(_))));
        assert_eq!(
            sender
                .send_key_tap(&authorization, &resumed, 30)
                .unwrap_err()
                .code,
            ErrorCode::MutterUnavailable
        );
        assert_eq!(sender.sequence, 1);
        let resumed_again = reis::event::DeviceResumed {
            device: resumed.device.clone(),
            serial: *sender
                .active_devices
                .iter()
                .find(|(device, _)| device == &resumed.device)
                .map(|(_, serial)| serial)
                .ok_or_else(|| std::io::Error::other("new device serial was not tracked"))?,
        };
        assert_ne!(resumed_again.serial, resumed.serial);
        assert_eq!(
            sender
                .send_pointer_motion(&authorization, &resumed, 2.0, -3.0)
                .unwrap_err()
                .code,
            ErrorCode::MutterUnavailable
        );
        assert_eq!(
            sender
                .send_scroll_delta(&authorization, &resumed, 0.0, 5.0)
                .unwrap_err()
                .code,
            ErrorCode::MutterUnavailable
        );
        EiConnection::track_device_event(
            &mut sender.active_devices,
            &reis::event::EiEvent::DevicePaused(reis::event::DevicePaused {
                device: resumed_again.device.clone(),
                serial: resumed_again.serial,
            }),
        );
        assert_eq!(
            sender
                .send_pointer_motion(&authorization, &resumed_again, 2.0, -3.0)
                .unwrap_err()
                .code,
            ErrorCode::MutterUnavailable
        );
        assert_eq!(
            sender
                .send_scroll_delta(&authorization, &resumed_again, 0.0, 5.0)
                .unwrap_err()
                .code,
            ErrorCode::MutterUnavailable
        );
        assert_eq!(sender.sequence, 1);
        EiConnection::track_device_event(
            &mut sender.active_devices,
            &reis::event::EiEvent::DeviceResumed(resumed_again.clone()),
        );
        let removed = reis::event::EiEvent::DeviceRemoved(reis::event::DeviceRemoved {
            device: resumed_again.device.clone(),
        });
        EiConnection::track_device_event(&mut sender.active_devices, &removed);
        assert_eq!(
            sender
                .send_key_tap(&authorization, &resumed_again, 30)
                .unwrap_err()
                .code,
            ErrorCode::MutterUnavailable
        );
        EiConnection::track_device_event(
            &mut sender.active_devices,
            &reis::event::EiEvent::DeviceResumed(resumed_again.clone()),
        );
        authorization.revoked = true;
        assert_eq!(
            sender
                .send_pointer_motion(&authorization, &resumed_again, 2.0, -3.0)
                .unwrap_err()
                .code,
            ErrorCode::LeaseRevoked
        );
        authorization.revoked = false;
        assert_eq!(
            sender
                .send_pointer_motion(&authorization, &resumed_again, f32::NAN, 0.0)
                .unwrap_err()
                .code,
            ErrorCode::IpcInvalidMessage
        );
        assert_eq!(sender.sequence, 1);
        sender.send_pointer_motion(&authorization, &resumed_again, 2.0, -3.0)?;
        assert_eq!(sender.sequence, 2);
        authorization.revoked = true;
        assert_eq!(
            sender
                .send_scroll_delta(&authorization, &resumed_again, 0.0, 5.0)
                .unwrap_err()
                .code,
            ErrorCode::LeaseRevoked
        );
        authorization.revoked = false;
        assert_eq!(
            sender
                .send_scroll_delta(&authorization, &resumed_again, f32::INFINITY, 5.0)
                .unwrap_err()
                .code,
            ErrorCode::IpcInvalidMessage
        );
        EiConnection::track_device_event(&mut sender.active_devices, &removed);
        assert_eq!(
            sender
                .send_scroll_delta(&authorization, &resumed_again, 0.0, 5.0)
                .unwrap_err()
                .code,
            ErrorCode::MutterUnavailable
        );
        assert_eq!(sender.sequence, 2);
        EiConnection::track_device_event(
            &mut sender.active_devices,
            &reis::event::EiEvent::DeviceResumed(resumed_again.clone()),
        );
        sender.send_scroll_delta(&authorization, &resumed_again, 0.0, 5.0)?;
        assert_eq!(sender.sequence, 3);
        authorization.revoked = true;
        assert_eq!(
            sender
                .send_button_click(&authorization, &resumed_again, 272)
                .unwrap_err()
                .code,
            ErrorCode::LeaseRevoked
        );
        authorization.revoked = false;
        assert_eq!(
            sender
                .send_button_click(&authorization, &resumed, 272)
                .unwrap_err()
                .code,
            ErrorCode::MutterUnavailable
        );
        assert_eq!(sender.sequence, 3);
        sender.send_button_click(&authorization, &resumed_again, 272)?;
        assert_eq!(sender.sequence, 4);
        authorization.revoked = true;
        assert_eq!(
            sender
                .send_key_chord(&authorization, &resumed_again, 29, 30)
                .unwrap_err()
                .code,
            ErrorCode::LeaseRevoked
        );
        authorization.revoked = false;
        assert_eq!(
            sender
                .send_key_chord(&authorization, &resumed_again, 29, 29)
                .unwrap_err()
                .code,
            ErrorCode::IpcInvalidMessage
        );
        assert_eq!(
            sender
                .send_key_chord(&authorization, &resumed, 29, 30)
                .unwrap_err()
                .code,
            ErrorCode::MutterUnavailable
        );
        assert_eq!(sender.sequence, 4);
        sender.send_key_chord(&authorization, &resumed_again, 29, 30)?;
        assert_eq!(sender.sequence, 5);
        assert!(
            sender
                .next_event_until(Duration::from_millis(30))?
                .is_none()
        );
        done_tx.send(())?;
        server
            .join()
            .map_err(|_| std::io::Error::other("synthetic EIS server panicked"))??;
        assert_eq!(
            sender
                .send_key_tap(&authorization, &resumed_again, 30)
                .unwrap_err()
                .code,
            ErrorCode::MutterUnavailable
        );
        assert!(!sender.is_ready());
        Ok(())
    }

    #[test]
    fn sender_handshake_times_out_with_silent_peer() -> Result<(), Box<dyn std::error::Error>> {
        let (client, _server) = UnixStream::pair()?;
        let mut sender = EiConnection::from_fd(zvariant::OwnedFd::from(OwnedFd::from(client)))?;
        let error = sender
            .handshake_sender(Duration::from_millis(30))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::IpcTimeout);
        assert!(!sender.is_ready());
        Ok(())
    }
}
