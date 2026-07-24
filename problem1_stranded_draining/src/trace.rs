use std::fmt::Write;

use crate::protocol::{CpuId, LlcId, PartitionId, ProtocolState, TaskId, CPU1, LLC0};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceEvent {
    pub seq: u64,
    pub mode: String,
    pub event: String,
    pub cpu: Option<CpuId>,
    pub task: Option<TaskId>,
    pub partition: PartitionId,
    pub llc: LlcId,
    pub mask_generation: u64,
    pub q: usize,
    pub c: bool,
    pub d: bool,
    pub pending_enqueues: usize,
    pub task_progress: u64,
    pub adapter_observed: bool,
    pub note: String,
}

impl TraceEvent {
    fn from_state(seq: u64, state: &ProtocolState, spec: EventSpec<'_>) -> Self {
        let task_progress = spec.task.map_or(0, |task_id| state.task_progress(task_id));

        Self {
            seq,
            mode: spec.mode.to_string(),
            event: spec.event.to_string(),
            cpu: spec.cpu,
            task: spec.task,
            partition: state.partition_id(),
            llc: spec.llc,
            mask_generation: state.mask_generation(),
            q: state.queue_len(spec.llc),
            c: state.has_cpu_in_llc(spec.llc),
            d: state.drain_enabled(spec.llc),
            pending_enqueues: state.pending_enqueue_count(),
            task_progress,
            adapter_observed: false,
            note: spec.note.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct EventSpec<'a> {
    mode: &'a str,
    event: &'a str,
    cpu: Option<CpuId>,
    task: Option<TaskId>,
    llc: LlcId,
    note: &'a str,
}

pub fn deterministic_protocol_trace() -> Vec<TraceEvent> {
    let mut state = ProtocolState::example_topology();
    let mut seq = 0;
    let mut events = Vec::new();

    push_event(
        &mut events,
        &mut seq,
        &state,
        EventSpec {
            mode: "protocol_model",
            event: "initial_state",
            cpu: None,
            task: Some(100),
            llc: LLC0,
            note: "CPU0/LLC0 and CPU1/LLC1 are both published for partition A.",
        },
    );

    let ticket = state.enqueue_select(100, LLC0);
    push_event(
        &mut events,
        &mut seq,
        &state,
        EventSpec {
            mode: "protocol_model",
            event: "enqueue_select",
            cpu: Some(0),
            task: Some(ticket.task_id),
            llc: ticket.target_llc,
            note: "Enqueue observes old mask generation 0 and selects LLC0.",
        },
    );

    state.publish_mask([CPU1]);
    push_event(
        &mut events,
        &mut seq,
        &state,
        EventSpec {
            mode: "protocol_model",
            event: "publish_mask",
            cpu: Some(2),
            task: Some(ticket.task_id),
            llc: LLC0,
            note: "Mask generation 1 removes CPU0, so partition A has no CPU in LLC0.",
        },
    );

    state.update_observe_queue(LLC0);
    push_event(
        &mut events,
        &mut seq,
        &state,
        EventSpec {
            mode: "protocol_model",
            event: "update_observe_queue",
            cpu: Some(2),
            task: Some(ticket.task_id),
            llc: LLC0,
            note: "Updater observes Q=0 and therefore leaves the drain bit disabled.",
        },
    );

    state.enqueue_commit(ticket);
    push_event(
        &mut events,
        &mut seq,
        &state,
        EventSpec {
            mode: "protocol_model",
            event: "enqueue_commit",
            cpu: Some(0),
            task: Some(100),
            llc: LLC0,
            note: "Enqueue commits the stale LLC0 placement after the updater returned.",
        },
    );

    push_event(
        &mut events,
        &mut seq,
        &state,
        EventSpec {
            mode: "protocol_model",
            event: "stable_invalid_state",
            cpu: None,
            task: Some(100),
            llc: LLC0,
            note:
                "Both operations have returned: Q>0, C=false, D=false, and CPU1 remains eligible.",
        },
    );

    events
}

pub fn trace_to_json(events: &[TraceEvent]) -> String {
    let mut out = String::new();
    out.push_str("[\n");

    for (index, event) in events.iter().enumerate() {
        out.push_str("  {\n");
        write_field_u64(&mut out, "seq", event.seq, true);
        write_field_str(&mut out, "mode", &event.mode, true);
        write_field_str(&mut out, "event", &event.event, true);
        write_field_opt_u16(&mut out, "cpu", event.cpu, true);
        write_field_opt_u32(&mut out, "task", event.task, true);
        write_field_u16(&mut out, "partition", event.partition, true);
        write_field_u16(&mut out, "llc", event.llc, true);
        write_field_u64(&mut out, "mask_generation", event.mask_generation, true);
        write_field_usize(&mut out, "q", event.q, true);
        write_field_bool(&mut out, "c", event.c, true);
        write_field_bool(&mut out, "d", event.d, true);
        write_field_usize(&mut out, "pending_enqueues", event.pending_enqueues, true);
        write_field_u64(&mut out, "task_progress", event.task_progress, true);
        write_field_bool(&mut out, "adapter_observed", event.adapter_observed, true);
        write_field_str(&mut out, "note", &event.note, false);
        out.push_str("  }");
        if index + 1 != events.len() {
            out.push(',');
        }
        out.push('\n');
    }

    out.push_str("]\n");
    out
}

fn push_event(
    events: &mut Vec<TraceEvent>,
    seq: &mut u64,
    state: &ProtocolState,
    spec: EventSpec<'_>,
) {
    events.push(TraceEvent::from_state(*seq, state, spec));
    *seq += 1;
}

fn write_field_str(out: &mut String, key: &str, value: &str, comma: bool) {
    let comma = if comma { "," } else { "" };
    writeln!(
        out,
        "    \"{}\": \"{}\"{}",
        key,
        escape_json_string(value),
        comma
    )
    .expect("writing to String cannot fail");
}

fn write_field_bool(out: &mut String, key: &str, value: bool, comma: bool) {
    let comma = if comma { "," } else { "" };
    writeln!(out, "    \"{}\": {}{}", key, value, comma).expect("writing to String cannot fail");
}

fn write_field_u16(out: &mut String, key: &str, value: u16, comma: bool) {
    let comma = if comma { "," } else { "" };
    writeln!(out, "    \"{}\": {}{}", key, value, comma).expect("writing to String cannot fail");
}

fn write_field_u64(out: &mut String, key: &str, value: u64, comma: bool) {
    let comma = if comma { "," } else { "" };
    writeln!(out, "    \"{}\": {}{}", key, value, comma).expect("writing to String cannot fail");
}

fn write_field_u32(out: &mut String, key: &str, value: u32, comma: bool) {
    let comma = if comma { "," } else { "" };
    writeln!(out, "    \"{}\": {}{}", key, value, comma).expect("writing to String cannot fail");
}

fn write_field_usize(out: &mut String, key: &str, value: usize, comma: bool) {
    let comma = if comma { "," } else { "" };
    writeln!(out, "    \"{}\": {}{}", key, value, comma).expect("writing to String cannot fail");
}

fn write_field_opt_u16(out: &mut String, key: &str, value: Option<u16>, comma: bool) {
    match value {
        Some(value) => write_field_u16(out, key, value, comma),
        None => write_null(out, key, comma),
    }
}

fn write_field_opt_u32(out: &mut String, key: &str, value: Option<u32>, comma: bool) {
    match value {
        Some(value) => write_field_u32(out, key, value, comma),
        None => write_null(out, key, comma),
    }
}

fn write_null(out: &mut String, key: &str, comma: bool) {
    let comma = if comma { "," } else { "" };
    writeln!(out, "    \"{}\": null{}", key, comma).expect("writing to String cannot fail");
}

fn escape_json_string(value: &str) -> String {
    let mut escaped = String::new();
    for c in value.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => {
                write!(escaped, "\\u{:04x}", c as u32).expect("writing to String cannot fail");
            }
            c => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_trace_ends_in_invalid_protocol_state() {
        let events = deterministic_protocol_trace();
        let last = events.last().expect("trace should contain events");

        assert_eq!(last.event, "stable_invalid_state");
        assert_eq!(last.q, 1);
        assert!(!last.c);
        assert!(!last.d);
        assert_eq!(last.pending_enqueues, 0);
        assert!(!last.adapter_observed);
    }

    #[test]
    fn json_renderer_preserves_required_fields() {
        let json = trace_to_json(&deterministic_protocol_trace());

        assert!(json.contains("\"mask_generation\": 1"));
        assert!(json.contains("\"event\": \"stable_invalid_state\""));
        assert!(json.contains("\"adapter_observed\": false"));
    }
}
