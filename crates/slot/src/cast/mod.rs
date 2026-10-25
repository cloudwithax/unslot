//! Casting: the screen and its sound mirrored to a Chromecast, or a TV with one built in, over
//! the Wi-Fi with Cast Streaming, as Chrome does for a tab.

pub mod channel;
pub mod codec;
pub mod mdns;
pub mod rtp;
pub mod session;
mod tls;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver as Jobs, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use mdns::Receiver;

/// How long a search listens for answers.
const SEARCH: Duration = Duration::from_secs(3);

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum CastBusy {
    #[default]
    Idle,
    Searching,
    Connecting(String),
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CastView {
    pub receivers: Vec<Receiver>,
    pub busy: CastBusy,
    /// The name of the receiver being cast to.
    pub casting: Option<String>,
    pub error: Option<String>,
}

pub trait CastBackend: Send {
    fn search(&mut self) -> Result<Vec<Receiver>, String>;
    fn start(&mut self, to: &Receiver) -> Result<(), String>;
    fn stop(&mut self);
    /// Why the cast ended, once it has; empty when stopped from here.
    fn ended(&mut self) -> Option<String>;
}

enum Job {
    Search,
    Start(Receiver),
    Stop,
}

pub struct Cast {
    jobs: Sender<Job>,
    view: Arc<Mutex<CastView>>,
}

impl Cast {
    pub fn spawn(backend: Box<dyn CastBackend>) -> Cast {
        let (jobs, rx) = channel();
        let view = Arc::new(Mutex::new(CastView::default()));
        let shared = view.clone();
        let spawned = std::thread::Builder::new()
            .name("slot-cast-worker".into())
            .spawn(move || work(backend, rx, shared));
        if let Err(e) = spawned {
            eprintln!("slot: cast: {e}");
        }
        Cast { jobs, view }
    }

    /// The real thing on the device, and off it when `SLOT_CAST_LIB` says where the encoders
    /// are; a pretend receiver otherwise.
    pub fn for_card(root: &Path) -> Cast {
        let real = cfg!(feature = "device") || std::env::var_os("SLOT_CAST_LIB").is_some();
        match real {
            true => Cast::spawn(Box::new(NetCast {
                lib: codec::library(root),
                session: None,
            })),
            false => Cast::spawn(Box::new(SimCast::default())),
        }
    }

    pub fn view(&self) -> CastView {
        self.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn search(&self) {
        let _ = self.jobs.send(Job::Search);
    }

    pub fn start(&self, to: &Receiver) {
        let _ = self.jobs.send(Job::Start(to.clone()));
    }

    pub fn stop(&self) {
        let _ = self.jobs.send(Job::Stop);
    }
}

fn work(mut backend: Box<dyn CastBackend>, jobs: Jobs<Job>, view: Arc<Mutex<CastView>>) {
    let view = || view.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        let job = match jobs.recv_timeout(Duration::from_millis(500)) {
            Ok(job) => job,
            Err(RecvTimeoutError::Timeout) => {
                let casting = view().casting.is_some();
                if let Some(why) = casting.then(|| backend.ended()).flatten() {
                    let mut v = view();
                    v.casting = None;
                    v.error = (!why.is_empty()).then_some(why);
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                backend.stop();
                return;
            }
        };
        match job {
            Job::Search => {
                {
                    let mut v = view();
                    v.busy = CastBusy::Searching;
                    v.error = None;
                }
                let found = backend.search();
                let mut v = view();
                v.busy = CastBusy::Idle;
                match found {
                    Ok(r) => v.receivers = r,
                    Err(e) => v.error = Some(e),
                }
            }
            Job::Start(to) => {
                backend.stop();
                {
                    let mut v = view();
                    v.casting = None;
                    v.busy = CastBusy::Connecting(to.name.clone());
                    v.error = None;
                }
                let started = backend.start(&to);
                let mut v = view();
                v.busy = CastBusy::Idle;
                match started {
                    Ok(()) => v.casting = Some(to.name),
                    Err(e) => v.error = Some(e),
                }
            }
            Job::Stop => {
                backend.stop();
                view().casting = None;
            }
        }
    }
}

/// Casting over the network.
struct NetCast {
    lib: PathBuf,
    session: Option<session::Session>,
}

impl CastBackend for NetCast {
    fn search(&mut self) -> Result<Vec<Receiver>, String> {
        mdns::search(SEARCH).map_err(|e| e.to_string())
    }

    fn start(&mut self, to: &Receiver) -> Result<(), String> {
        let s = session::Session::start(&self.lib, to).map_err(|e| {
            eprintln!("slot: cast: to {}: {e}", to.name);
            e
        })?;
        self.session = Some(s);
        Ok(())
    }

    fn stop(&mut self) {
        // Dropping it stops it.
        self.session = None;
    }

    fn ended(&mut self) -> Option<String> {
        let why = self.session.as_ref()?.ended()?;
        self.session = None;
        Some(why)
    }
}

/// A pretend receiver in the next room, for off the device.
#[derive(Default)]
pub struct SimCast {
    casting: bool,
}

impl CastBackend for SimCast {
    fn search(&mut self) -> Result<Vec<Receiver>, String> {
        Ok(vec![Receiver {
            name: "Living Room TV".into(),
            model: "Chromecast".into(),
            addr: ([192, 168, 1, 50], 8009).into(),
        }])
    }

    fn start(&mut self, _: &Receiver) -> Result<(), String> {
        self.casting = true;
        Ok(())
    }

    fn stop(&mut self) {
        self.casting = false;
    }

    fn ended(&mut self) -> Option<String> {
        (!self.casting).then(String::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn until(cast: &Cast, what: impl Fn(&CastView) -> bool) -> CastView {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let v = cast.view();
            if what(&v) || Instant::now() > end {
                return v;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_receiver_is_found_cast_to_and_let_go() {
        let cast = Cast::spawn(Box::new(SimCast::default()));
        cast.search();
        let v = until(&cast, |v| !v.receivers.is_empty());
        assert_eq!(v.receivers[0].name, "Living Room TV");
        cast.start(&v.receivers[0]);
        let v = until(&cast, |v| v.casting.is_some());
        assert_eq!(v.casting.as_deref(), Some("Living Room TV"));
        cast.stop();
        let v = until(&cast, |v| v.casting.is_none());
        assert_eq!((v.casting, v.error), (None, None));
    }
}
