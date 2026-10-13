// SPDX-License-Identifier: GPL-2.0-only
//! Executable accounting and callback obligations; kernel effects are separate.
// Word bodies retain a named result for their corresponding proof annotations.
#[allow(clippy::let_and_return)]
pub mod word;

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidLlc,
    InvalidCoverage,
    Full,
    AlreadyRegistered,
    WrongPartition,
    WrongTask,
    WrongGeneration,
    GenerationExhausted,
    NotRegistered,
    NotRemoved,
    Underflow,
    IdentityExhausted,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn identity() -> Result<u64, Error> {
    NEXT_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| Error::IdentityExhausted)
}

/// A complete snapshot of one partition's two LLC coverage bits and accounting.
pub struct Partition {
    id: u64,
    state: AtomicU64,
}

/// Private fields prevent forging a registration or duplicating its ownership.
/// A kernel adapter stores this record in task storage, never keyed by PID alone.
pub struct TaskRecord {
    id: u64,
    generation: u64,
    ticket: Option<Ticket>,
}

struct Ticket {
    partition: u64,
    llc: u8,
    generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Placement {
    pub partition: u64,
    pub task: u64,
    pub llc: u8,
    pub generation: u64,
}

impl Default for TaskRecord {
    fn default() -> Self {
        Self::new().expect("task identity space exhausted")
    }
}

impl TaskRecord {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            id: identity()?,
            generation: 0,
            ticket: None,
        })
    }

    pub fn placement(&self) -> Option<Placement> {
        self.ticket.as_ref().map(|t| Placement {
            partition: t.partition,
            task: self.id,
            llc: t.llc,
            generation: t.generation,
        })
    }
}

impl Partition {
    pub fn new(coverage: u8) -> Result<Self, Error> {
        if coverage >= 4 {
            return Err(Error::InvalidCoverage);
        }
        Ok(Self {
            id: identity()?,
            state: AtomicU64::new(coverage as u64),
        })
    }

    pub fn snapshot(&self) -> u64 {
        self.state.load(Ordering::Relaxed)
    }

    /// The successful CAS is the effect. A failed CAS changes no shared state.
    /// Relaxed is enough for this single-word invariant; this does not publish
    /// task memory or synchronize with the kernel's separate DSQ locks.
    fn update(&self, f: impl Fn(u64) -> Result<u64, Error>) -> Result<(), Error> {
        let mut old = self.snapshot();
        loop {
            let next = f(old)?;
            match self
                .state
                .compare_exchange_weak(old, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return Ok(()),
                Err(actual) => old = actual,
            }
        }
    }

    /// Register before requesting insertion. Returning a placement does not
    /// assert that the task has landed. Caller serializes this task's record.
    pub fn enqueue(&self, task: &mut TaskRecord, llc: u8) -> Result<Placement, Error> {
        if llc >= 2 {
            return Err(Error::InvalidLlc);
        }
        if task.ticket.is_some() {
            return Err(Error::AlreadyRegistered);
        }
        let generation = task
            .generation
            .checked_add(1)
            .ok_or(Error::GenerationExhausted)?;
        self.update(|w| {
            if word::count(w, llc) == word::LIMIT {
                return Err(Error::Full);
            }
            Ok(word::register(w, llc))
        })?;
        let partition = self.id;
        task.generation = generation;
        task.ticket = Some(Ticket {
            partition,
            llc,
            generation,
        });
        Ok(Placement {
            partition,
            task: task.id,
            llc,
            generation,
        })
    }

    /// Publishes the entire two-LLC coverage configuration in the same CAS as
    /// its necessary drain requests. Stale placement choices remain safe.
    pub fn config_update(&self, coverage: u8) -> Result<(), Error> {
        if coverage >= 4 {
            return Err(Error::InvalidCoverage);
        }
        self.update(|w| Ok(word::publish(w, coverage)))
    }

    /// Call after either consume outcome. Failure says nothing about emptiness.
    /// Successful moves also keep N until running or confirmed removal.
    pub fn drain_after_attempt(&self, llc: u8) -> Result<(), Error> {
        if llc >= 2 {
            return Err(Error::InvalidLlc);
        }
        self.update(|w| Ok(word::retire(w, llc)))
    }

    /// A trusted adapter must establish `removed`: this generation has left
    /// the user DSQ AND cannot later land there. Running satisfies this;
    /// dequeue/quiescent alone do not. The bool is a visible trust boundary,
    /// not kernel evidence manufactured by this library.
    pub fn finish(
        &self,
        task: &mut TaskRecord,
        placement: Placement,
        removed: bool,
    ) -> Result<(), Error> {
        if !removed {
            return Err(Error::NotRemoved);
        }
        let t = task.ticket.as_ref().ok_or(Error::NotRegistered)?;
        if t.partition != self.id || placement.partition != self.id {
            return Err(Error::WrongPartition);
        }
        if placement.task != task.id {
            return Err(Error::WrongTask);
        }
        if t.generation != placement.generation {
            return Err(Error::WrongGeneration);
        }
        if t.llc != placement.llc {
            return Err(Error::InvalidLlc);
        }
        let llc = t.llc;
        self.update(|w| {
            if word::count(w, llc) == 0 {
                return Err(Error::Underflow);
            }
            Ok(word::release(w, llc))
        })?;
        task.ticket = None;
        Ok(())
    }
}
