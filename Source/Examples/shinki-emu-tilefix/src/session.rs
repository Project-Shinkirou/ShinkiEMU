// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::tile::Tile;
use shinki_emu_abi as abi;
use shinki_emu_sdk::native::{NativeFailure as Failure, ServiceContext, lease_cpu};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) const INITIAL_CONTEXT: u64 = 1;
pub(super) const OUTPUT_EPOCH: u64 = 1; // The raster size is fixed for each session.
const QUEUE_CAPACITY: usize = 32;

pub(super) enum Command {
    Flip,
    SetPaused(bool),
    Wake,
}

struct Published {
    generation: u64,
    timestamp_ns: u64,
    pixels: Arc<Vec<u8>>,
}

struct Shared {
    services: ServiceContext,
    surface: abi::SurfaceInfo,
    published: Mutex<Option<Published>>,
    leases: Arc<AtomicUsize>,
    state: AtomicU32,
}

impl Shared {
    fn new(services: ServiceContext, surface: abi::SurfaceInfo) -> Self {
        Self {
            services,
            surface,
            published: Mutex::new(None),
            leases: Arc::new(AtomicUsize::new(0)),
            state: AtomicU32::new(abi::SESSION_RUNNING),
        }
    }

    fn frame(&self, previous: abi::FrameIdentity) -> Result<abi::FrameSurface, Failure> {
        if self.state.load(Ordering::Acquire) == abi::SESSION_FAILED {
            return Err(worker_failure());
        }
        if self.services.cancelled.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        let mut output = abi::FrameSurface {
            state: abi::FRAME_PENDING,
            reserved: 0,
            identity: abi::FrameIdentity {
                session_epoch: self.services.session,
                output_epoch: OUTPUT_EPOCH,
                generation: 0,
            },
            timestamp_ns: 0,
            info: self.surface,
            cpu: abi::CpuLease {
                pixels: abi::Bytes::EMPTY,
                owner: ptr::null_mut(),
                release: None,
            },
        };
        if let Some(published) = self.published.lock().unwrap().as_ref() {
            output.identity.generation = published.generation;
            output.timestamp_ns = published.timestamp_ns;
            if output.identity == previous {
                output.state = abi::FRAME_UNCHANGED;
            } else {
                output.cpu = lease_cpu(&published.pixels, &self.leases, |pixels| pixels.as_slice())
                    .ok_or_else(|| {
                        Failure::new(abi::CAPACITY, "TILEFIX_LEASES", "return a CPU frame lease")
                    })?;
                output.state = abi::FRAME_READY;
            }
        }
        Ok(output)
    }
}

pub(super) struct Session {
    shared: Arc<Shared>,
    commands: SyncSender<Command>,
    worker: JoinHandle<bool>,
}

impl Session {
    pub(super) fn services(session: u64, cancellation: abi::CancellationService) -> ServiceContext {
        ServiceContext {
            session,
            epoch: AtomicU64::new(INITIAL_CONTEXT),
            cancelled: AtomicBool::new(false),
            waiting: AtomicBool::new(false),
            text: None,
            window: None,
            host_cancel: Some(cancellation),
        }
    }

    pub(super) fn start(
        services: ServiceContext,
        tile: Tile,
        surface: abi::SurfaceInfo,
    ) -> Result<Self, Failure> {
        let shared = Arc::new(Shared::new(services, surface));
        let (commands, receiver) = sync_channel(QUEUE_CAPACITY);
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("tilefix".into())
            .spawn(move || {
                let completed =
                    catch_unwind(AssertUnwindSafe(|| run(&worker_shared, receiver, tile))).is_ok();
                worker_shared.services.invalidate();
                worker_shared.state.store(
                    if completed {
                        abi::SESSION_FINISHED
                    } else {
                        abi::SESSION_FAILED
                    },
                    Ordering::Release,
                );
                completed
            })
            .map_err(|_| {
                Failure::new(
                    abi::CORE_ERROR,
                    "TILEFIX_WORKER_START",
                    "could not start the Tilefix worker",
                )
            })?;
        Ok(Self {
            shared,
            commands,
            worker,
        })
    }

    pub(super) fn check_cancelled(&self) -> Result<(), Failure> {
        check_cancelled(&self.shared.services)
    }

    pub(super) fn enqueue(&self, command: Command) -> Result<(), Failure> {
        self.check_cancelled()?;
        self.commands.try_send(command).map_err(|error| match error {
            TrySendError::Full(_) => {
                Failure::new(abi::CAPACITY, "TILEFIX_QUEUE_FULL", "Tilefix input queue is full")
            }
            TrySendError::Disconnected(_) => worker_failure(),
        })
    }

    pub(super) fn state(&self) -> abi::SessionState {
        let shared = &self.shared;
        let mut state = shared.state.load(Ordering::Acquire);
        let error = if state == abi::SESSION_FAILED {
            worker_failure().diagnostic()
        } else {
            if shared.services.cancelled.load(Ordering::Acquire) {
                state = abi::SESSION_FINISHED;
            }
            abi::Error::default()
        };
        abi::SessionState {
            state,
            flags: 0,
            context_epoch: shared.services.epoch.load(Ordering::Acquire),
            output_epoch: OUTPUT_EPOCH,
            error,
        }
    }

    pub(super) fn frame(&self, previous: abi::FrameIdentity) -> Result<abi::FrameSurface, Failure> {
        self.shared.frame(previous)
    }

    pub(super) fn cancel(&self) {
        self.shared.services.cancel();
        // A full queue already wakes the worker; a disconnected queue needs no wakeup.
        let _ = self.commands.try_send(Command::Wake);
    }

    pub(super) fn stop(self) -> Result<(), Failure> {
        self.cancel();
        match self.worker.join() {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => Err(worker_failure()),
        }
    }
}

fn run(shared: &Shared, commands: Receiver<Command>, tile: Tile) {
    let start = Instant::now();
    let mut generation = 0u64;
    let mut paused = false;
    let mut flipped = false;
    let mut dirty = true;
    while !shared.services.is_cancelled() {
        if dirty {
            generation = generation.checked_add(1).expect("tilefix generation overflow");
            let next = Published {
                generation,
                timestamp_ns: u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX),
                pixels: Arc::new(tile.render(flipped)),
            };
            *shared.published.lock().unwrap() = Some(next);
            dirty = false;
        }
        match commands.recv_timeout(Duration::from_millis(50)) {
            Ok(Command::Flip) if !paused => {
                flipped = !flipped;
                dirty = true;
            }
            Ok(Command::SetPaused(value)) => {
                paused = value;
                shared.state.store(
                    if paused {
                        abi::SESSION_PAUSED
                    } else {
                        abi::SESSION_RUNNING
                    },
                    Ordering::Release,
                );
            }
            Ok(Command::Flip | Command::Wake) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

pub(super) fn check_cancelled(services: &ServiceContext) -> Result<(), Failure> {
    if services.is_cancelled() {
        return Err(cancelled());
    }
    Ok(())
}

fn cancelled() -> Failure {
    Failure::new(abi::CANCELLED, "TILEFIX_CANCELLED", "Tilefix session cancelled")
}

fn worker_failure() -> Failure {
    Failure::new(abi::PANICKED, "TILEFIX_WORKER", "Tilefix worker failed")
}

#[cfg(test)]
#[test]
fn pending_surface_belongs_to_the_current_output_without_a_lease() {
    let game = tempfile::tempdir().unwrap();
    std::fs::write(
        game.path().join("tilefix.dat"),
        b"TILE\x01\x01\0\x01\0\x11\x22\x33",
    )
    .unwrap();
    let tile = Tile::load(game.path()).unwrap().unwrap();
    let services = Session::services(
        7,
        abi::CancellationService {
            context: ptr::null_mut(),
            is_cancelled: None,
        },
    );
    let shared = Shared::new(services, tile.surface());
    let pending = shared.frame(abi::FrameIdentity::default()).unwrap();
    assert_eq!(pending.state, abi::FRAME_PENDING);
    assert_eq!(pending.validate_cpu(7), Ok(()));
    assert!(pending.cpu.release.is_none());
}
