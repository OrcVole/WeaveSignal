// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Real-time scheduling for the desk's few audio threads, where the host allows it.
//!
//! A handful of threads at a real-time priority is how JACK and PipeWire keep audio on time
//! on a busy machine: the scheduler runs them as soon as they wake. The desk asks for
//! `SCHED_FIFO` for its mixing thread and, a little lower, its encoders; a host that refuses
//! (no `CAP_SYS_NICE`, no `RLIMIT_RTPRIO`, a container without the right) leaves them at the
//! normal priority, and the desk goes on: the answer is logged and shown in the status.

use thread_priority::unix::{
    RealtimeThreadSchedulePolicy, ThreadSchedulePolicy, set_thread_priority_and_policy,
    thread_native_id,
};
use thread_priority::{ThreadPriority, ThreadPriorityValue};

/// The mixing thread's priority (1 to 99; PipeWire's data threads use 88, JACK 70 to 80).
pub const MIX_PRIORITY: u8 = 60;
/// The encoders', under the mix.
pub const ENCODER_PRIORITY: u8 = 50;

/// Ask for `SCHED_FIFO` at `priority` for the calling thread; whether it was granted.
pub fn ask(priority: u8) -> bool {
    let Ok(value) = ThreadPriorityValue::try_from(priority) else {
        return false;
    };
    let name = std::thread::current().name().unwrap_or("?").to_owned();
    match set_thread_priority_and_policy(
        thread_native_id(),
        ThreadPriority::Crossplatform(value),
        ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
    ) {
        Ok(()) => {
            tracing::info!(thread = %name, priority, "real-time scheduling granted");
            true
        }
        Err(e) => {
            tracing::debug!(thread = %name, error = ?e, "real-time scheduling not granted; normal priority");
            false
        }
    }
}
