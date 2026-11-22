//! AC-only CLI protection, owned by execution rather than a window or history.
use std::io;

pub(crate) struct Lease {
    #[cfg(windows)]
    stop: std::sync::mpsc::Sender<()>,
    #[cfg(windows)]
    worker: Option<std::thread::JoinHandle<()>>,
}

#[cfg(all(test, windows))]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(all(test, windows))]
static ACTIVE_REQUESTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(all(test, windows))]
pub(crate) fn test_requests() -> usize { ACTIVE_REQUESTS.load(std::sync::atomic::Ordering::SeqCst) }
#[cfg(all(test, windows))]
pub(crate) fn test_on_ac() -> bool { native::Backend::default().on_ac().unwrap() }

impl Lease {
    pub(crate) fn acquire() -> io::Result<Self> {
        #[cfg(windows)]
        { Self::start(native::Backend::default, std::time::Duration::from_secs(1)) }
        #[cfg(not(windows))]
        { Ok(Self {}) }
    }

    #[cfg(windows)]
    fn start<B: Backend + 'static>(
        factory: impl FnOnce() -> B + Send + 'static,
        interval: std::time::Duration,
    ) -> io::Result<Self> {
        use std::sync::mpsc::{channel, sync_channel, RecvTimeoutError};
        let (stop, receiver) = channel();
        let (ready, result) = sync_channel(1);
        let worker = std::thread::Builder::new().name("velella-power".into()).spawn(move || {
            // Native handles are created, used and destroyed on this thread.
            let mut backend = factory();
            if let Err(error) = reconcile(&mut backend) {
                let _ = ready.send(Err(error));
                return;
            }
            if ready.send(Ok(())).is_err() { return; }
            let mut warned = false;
            loop {
                match receiver.recv_timeout(interval) {
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        if let Err(error) = reconcile(&mut backend) {
                            // Release immediately, but keep observing: the same live
                            // CLI must regain protection after confirmed AC returns.
                            if let Err(cleanup) = backend.set_required(false) {
                                eprintln!("[velella:power] protection cleanup failed: {cleanup}");
                                break; // dropping the backend also closes native handles
                            }
                            if !warned { eprintln!("[velella:power] protection released: {error}"); }
                            warned = true;
                        } else if warned {
                            eprintln!("[velella:power] power monitoring recovered");
                            warned = false;
                        }
                    }
                }
            }
        })?;
        match result.recv() {
            Ok(Ok(())) => Ok(Self { stop, worker: Some(worker) }),
            outcome => {
                let _ = worker.join();
                Err(match outcome {
                    Ok(Err(error)) => error,
                    _ => io::Error::other("power protection worker stopped during startup"),
                })
            }
        }
    }
}

#[cfg(windows)]
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if self.worker.take().is_some_and(|worker| worker.join().is_err()) {
            eprintln!("[velella:power] protection worker panicked");
        }
    }
}

#[cfg(windows)]
trait Backend {
    fn on_ac(&self) -> io::Result<bool>;
    fn set_required(&mut self, required: bool) -> io::Result<()>;
}

#[cfg(windows)]
fn reconcile(backend: &mut impl Backend) -> io::Result<()> {
    let required = backend.on_ac()?;
    backend.set_required(required)
}

#[cfg(windows)]
mod native {
    use super::*;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{Power::*, Threading::{REASON_CONTEXT, REASON_CONTEXT_0, POWER_REQUEST_CONTEXT_SIMPLE_STRING}},
    };

    struct Request {
        handle: HANDLE,
        #[cfg(test)] counted: bool,
    }
    impl Drop for Request {
        fn drop(&mut self) {
            // Closing the unique handle removes this object's system/execution
            // requests, including on partial setup. Other sessions own other handles.
            if unsafe { CloseHandle(self.handle) } == 0 {
                eprintln!("[velella:power] close request failed: {}", io::Error::last_os_error());
            }
            #[cfg(test)]
            if self.counted { ACTIVE_REQUESTS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst); }
        }
    }

    #[derive(Default)]
    pub(super) struct Backend { request: Option<Request> }
    impl super::Backend for Backend {
        fn on_ac(&self) -> io::Result<bool> {
            let mut status = SYSTEM_POWER_STATUS::default();
            if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // 0=battery, 1=AC, 255=unknown. Only confirmed AC permits a lease.
            Ok(status.ACLineStatus == 1)
        }

        fn set_required(&mut self, required: bool) -> io::Result<()> {
            if !required { self.request = None; return Ok(()); }
            if self.request.is_some() { return Ok(()); }
            let mut reason: Vec<u16> = "Velella CLI running on AC power".encode_utf16().chain(Some(0)).collect();
            let context = REASON_CONTEXT {
                Version: 0, Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
                Reason: REASON_CONTEXT_0 { SimpleReasonString: reason.as_mut_ptr() },
            };
            let handle = unsafe { PowerCreateRequest(&context) };
            if handle.is_null() || handle == INVALID_HANDLE_VALUE { return Err(io::Error::last_os_error()); }
            let request = Request { handle, #[cfg(test)] counted: false };
            // SystemRequired prevents idle sleep; ExecutionRequired protects the
            // process during Modern Standby screen-off transitions. No display flag.
            for kind in [PowerRequestSystemRequired, PowerRequestExecutionRequired] {
                if unsafe { PowerSetRequest(request.handle, kind) } == 0 { return Err(io::Error::last_os_error()); }
            }
            #[cfg(test)]
            let request = {
                let mut request = request;
                request.counted = true;
                ACTIVE_REQUESTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                request
            };
            self.request = Some(request);
            Ok(())
        }
    }

    #[cfg(test)]
    #[test]
    fn native_request_counts_are_scoped_and_never_keep_display_on() {
        use super::Backend as _;
        let _serial = TEST_LOCK.lock().unwrap();
        let mut first = Backend::default();
        let mut second = Backend::default();
        // Exercise our own kernel objects; no elevation or global policy changes.
        first.set_required(true).unwrap(); second.set_required(true).unwrap();
        first.set_required(true).unwrap(); // must not increment counts again
        let handle = first.request.as_ref().unwrap().handle;
        unsafe {
            assert_ne!(PowerClearRequest(handle, PowerRequestSystemRequired), 0);
            assert_eq!(PowerClearRequest(handle, PowerRequestSystemRequired), 0);
            assert_ne!(PowerClearRequest(handle, PowerRequestExecutionRequired), 0);
            assert_eq!(PowerClearRequest(handle, PowerRequestDisplayRequired), 0);
        }
        first.set_required(false).unwrap();
        let handle = second.request.as_ref().unwrap().handle;
        unsafe {
            assert_ne!(PowerClearRequest(handle, PowerRequestSystemRequired), 0);
            assert_ne!(PowerClearRequest(handle, PowerRequestExecutionRequired), 0);
        }
        second.set_required(false).unwrap();
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::{AtomicU8, AtomicUsize, Ordering::SeqCst}};
    use std::time::{Duration, Instant};

    #[derive(Default)]
    struct State { power: AtomicU8, active: AtomicUsize }
    struct Fake { state: Arc<State>, active: bool, fail: bool }
    impl Backend for Fake {
        fn on_ac(&self) -> io::Result<bool> {
            match self.state.power.load(SeqCst) {
                254 => Err(io::Error::other("power query failed")),
                value => Ok(value == 1),
            }
        }
        fn set_required(&mut self, required: bool) -> io::Result<()> {
            if self.fail { return Err(io::Error::other("request denied")); }
            if required != self.active {
                if required { self.state.active.fetch_add(1, SeqCst); }
                else { self.state.active.fetch_sub(1, SeqCst); }
                self.active = required;
            }
            Ok(())
        }
    }
    impl Drop for Fake {
        fn drop(&mut self) { if self.active { self.state.active.fetch_sub(1, SeqCst); } }
    }
    fn lease(state: &Arc<State>, fail: bool) -> io::Result<Lease> {
        let state = state.clone();
        Lease::start(move || Fake { state, active: false, fail }, Duration::from_millis(5))
    }
    fn wait(state: &State, active: usize) {
        let end = Instant::now() + Duration::from_secs(2);
        while state.active.load(SeqCst) != active && Instant::now() < end { std::thread::sleep(Duration::from_millis(2)); }
        assert_eq!(state.active.load(SeqCst), active);
    }
    #[test]
    fn concurrent_leases_survive_other_exit_and_release_on_battery_unknown_or_error() {
        let state = Arc::new(State::default()); state.power.store(1, SeqCst);
        let first = lease(&state, false).unwrap(); let second = lease(&state, false).unwrap();
        wait(&state, 2); drop(first); wait(&state, 1);
        for power in [0, 255] {
            state.power.store(power, SeqCst); wait(&state, 0);
            state.power.store(1, SeqCst); wait(&state, 1);
        }
        state.power.store(254, SeqCst); wait(&state, 0);
        state.power.store(1, SeqCst); wait(&state, 1); drop(second); wait(&state, 0);
        state.power.store(0, SeqCst); let battery = lease(&state, false).unwrap(); wait(&state, 0); drop(battery);
    }
    #[test]
    fn acquisition_error_is_reported_and_final_drop_clears_request() {
        let state = Arc::new(State::default()); state.power.store(1, SeqCst);
        assert!(lease(&state, true).is_err()); wait(&state, 0);
        let last = lease(&state, false).unwrap(); wait(&state, 1); drop(last); wait(&state, 0);
    }
    #[test]
    fn native_ac_query_and_request_lifecycle() {
        let _serial = TEST_LOCK.lock().unwrap();
        let mut backend = native::Backend::default();
        let on_ac = backend.on_ac().unwrap();
        backend.set_required(on_ac).unwrap();
        backend.set_required(on_ac).unwrap(); // no duplicate increments
        backend.set_required(false).unwrap();
        backend.set_required(false).unwrap();
    }
}
