use std::fmt::Write;

use crate::protocol::{CpuId, LlcId, PartitionId, ProtocolState, TaskId, CPU1, LLC0};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdapterObservation {
    pub q: usize,
    pub c: bool,
    pub d: bool,
    pub pending_enqueues: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceEvent {
    pub seq: u64,
    pub timestamp_ns: Option<u64>,
    pub mode: String,
    pub event: String,
    pub source: String,
    pub cpu: Option<CpuId>,
    pub task: Option<TaskId>,
    pub partition: PartitionId,
    pub llc: LlcId,
    pub selected_target_llc: Option<LlcId>,
    pub task_allowed_cpus: Vec<CpuId>,
    pub mask_generation: u64,
    pub q: usize,
    pub c: bool,
    pub d: bool,
    pub pending_enqueues: usize,
    pub adapter_q: Option<usize>,
    pub adapter_c: Option<bool>,
    pub adapter_d: Option<bool>,
    pub adapter_pending_enqueues: Option<usize>,
    pub task_progress: u64,
    pub adapter_observed: bool,
    pub note: String,
}

impl TraceEvent {
    pub fn from_state(seq: u64, state: &ProtocolState, spec: EventSpec<'_>) -> Self {
        let task_progress = spec.task.map_or(0, |task_id| state.task_progress(task_id));
        let task_allowed_cpus = spec
            .task
            .map(|task_id| state.task_allowed_cpus(task_id))
            .unwrap_or_default();
        let adapter = spec.adapter;

        Self {
            seq,
            timestamp_ns: spec.timestamp_ns,
            mode: spec.mode.to_string(),
            event: spec.event.to_string(),
            source: spec.source.to_string(),
            cpu: spec.cpu,
            task: spec.task,
            partition: state.partition_id(),
            llc: spec.llc,
            selected_target_llc: spec.selected_target_llc,
            task_allowed_cpus,
            mask_generation: state.mask_generation(),
            q: state.queue_len(spec.llc),
            c: state.has_cpu_in_llc(spec.llc),
            d: state.drain_enabled(spec.llc),
            pending_enqueues: state.pending_enqueue_count(),
            adapter_q: adapter.map(|adapter| adapter.q),
            adapter_c: adapter.map(|adapter| adapter.c),
            adapter_d: adapter.map(|adapter| adapter.d),
            adapter_pending_enqueues: adapter.map(|adapter| adapter.pending_enqueues),
            task_progress,
            adapter_observed: adapter.is_some(),
            note: spec.note.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EventSpec<'a> {
    pub timestamp_ns: Option<u64>,
    pub mode: &'a str,
    pub event: &'a str,
    pub source: &'a str,
    pub cpu: Option<CpuId>,
    pub task: Option<TaskId>,
    pub llc: LlcId,
    pub selected_target_llc: Option<LlcId>,
    pub adapter: Option<AdapterObservation>,
    pub note: &'a str,
}

impl<'a> EventSpec<'a> {
    pub fn model(mode: &'a str, event: &'a str, llc: LlcId, note: &'a str) -> Self {
        Self {
            timestamp_ns: None,
            mode,
            event,
            source: "protocol_model",
            cpu: None,
            task: None,
            llc,
            selected_target_llc: None,
            adapter: None,
            note,
        }
    }

    pub fn with_cpu(mut self, cpu: CpuId) -> Self {
        self.cpu = Some(cpu);
        self
    }

    pub fn with_task(mut self, task: TaskId) -> Self {
        self.task = Some(task);
        self
    }

    pub fn with_selected_target_llc(mut self, llc: LlcId) -> Self {
        self.selected_target_llc = Some(llc);
        self
    }

    pub fn with_source(mut self, source: &'a str) -> Self {
        self.source = source;
        self
    }

    pub fn with_timestamp_ns(mut self, timestamp_ns: u64) -> Self {
        self.timestamp_ns = Some(timestamp_ns);
        self
    }

    pub fn with_adapter(mut self, adapter: AdapterObservation) -> Self {
        self.adapter = Some(adapter);
        self
    }
}

pub fn deterministic_protocol_trace() -> Vec<TraceEvent> {
    let mut state = ProtocolState::example_topology();
    let mut seq = 0;
    let mut events = Vec::new();

    push_event(
        &mut events,
        &mut seq,
        &state,
        EventSpec::model(
            "protocol_model",
            "initial_state",
            LLC0,
            "CPU0/LLC0 and CPU1/LLC1 are both published for partition A.",
        )
        .with_task(100),
    );

    let ticket = state.enqueue_select(100, LLC0);
    push_event(
        &mut events,
        &mut seq,
        &state,
        EventSpec::model(
            "protocol_model",
            "enqueue_select",
            ticket.target_llc,
            "Enqueue observes old mask generation 0 and selects LLC0.",
        )
        .with_cpu(0)
        .with_task(ticket.task_id)
        .with_selected_target_llc(ticket.target_llc),
    );

    state.publish_mask([CPU1]);
    push_event(
        &mut events,
        &mut seq,
        &state,
        EventSpec::model(
            "protocol_model",
            "publish_mask",
            LLC0,
            "Mask generation 1 removes CPU0, so partition A has no CPU in LLC0.",
        )
        .with_cpu(2)
        .with_task(ticket.task_id),
    );

    state.update_observe_queue(LLC0);
    push_event(
        &mut events,
        &mut seq,
        &state,
        EventSpec::model(
            "protocol_model",
            "update_observe_queue",
            LLC0,
            "Updater observes Q=0 and therefore leaves the drain bit disabled.",
        )
        .with_cpu(2)
        .with_task(ticket.task_id),
    );

    state.enqueue_commit(ticket);
    push_event(
        &mut events,
        &mut seq,
        &state,
        EventSpec::model(
            "protocol_model",
            "enqueue_commit",
            LLC0,
            "Enqueue commits the stale LLC0 placement after the updater returned.",
        )
        .with_cpu(0)
        .with_task(100)
        .with_selected_target_llc(LLC0),
    );

    push_event(
        &mut events,
        &mut seq,
        &state,
        EventSpec::model(
            "protocol_model",
            "stable_invalid_state",
            LLC0,
            "Both operations have returned: Q>0, C=false, D=false, and CPU1 remains eligible.",
        )
        .with_task(100),
    );

    events
}

pub fn trace_to_json(events: &[TraceEvent]) -> String {
    let mut out = String::new();
    out.push_str("[\n");

    for (index, event) in events.iter().enumerate() {
        out.push_str("  {\n");
        write_field_u64(&mut out, "seq", event.seq, true);
        write_field_opt_u64(&mut out, "timestamp_ns", event.timestamp_ns, true);
        write_field_str(&mut out, "mode", &event.mode, true);
        write_field_str(&mut out, "event", &event.event, true);
        write_field_str(&mut out, "source", &event.source, true);
        write_field_opt_u16(&mut out, "cpu", event.cpu, true);
        write_field_opt_u32(&mut out, "task", event.task, true);
        write_field_u16(&mut out, "partition", event.partition, true);
        write_field_u16(&mut out, "llc", event.llc, true);
        write_field_opt_u16(
            &mut out,
            "selected_target_llc",
            event.selected_target_llc,
            true,
        );
        write_field_u16_array(
            &mut out,
            "task_allowed_cpus",
            &event.task_allowed_cpus,
            true,
        );
        write_field_u64(&mut out, "mask_generation", event.mask_generation, true);
        write_field_usize(&mut out, "q", event.q, true);
        write_field_bool(&mut out, "c", event.c, true);
        write_field_bool(&mut out, "d", event.d, true);
        write_field_usize(&mut out, "pending_enqueues", event.pending_enqueues, true);
        write_field_opt_usize(&mut out, "adapter_q", event.adapter_q, true);
        write_field_opt_bool(&mut out, "adapter_c", event.adapter_c, true);
        write_field_opt_bool(&mut out, "adapter_d", event.adapter_d, true);
        write_field_opt_usize(
            &mut out,
            "adapter_pending_enqueues",
            event.adapter_pending_enqueues,
            true,
        );
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

fn write_field_opt_u64(out: &mut String, key: &str, value: Option<u64>, comma: bool) {
    match value {
        Some(value) => write_field_u64(out, key, value, comma),
        None => write_null(out, key, comma),
    }
}

fn write_field_u32(out: &mut String, key: &str, value: u32, comma: bool) {
    let comma = if comma { "," } else { "" };
    writeln!(out, "    \"{}\": {}{}", key, value, comma).expect("writing to String cannot fail");
}

fn write_field_usize(out: &mut String, key: &str, value: usize, comma: bool) {
    let comma = if comma { "," } else { "" };
    writeln!(out, "    \"{}\": {}{}", key, value, comma).expect("writing to String cannot fail");
}

fn write_field_opt_usize(out: &mut String, key: &str, value: Option<usize>, comma: bool) {
    match value {
        Some(value) => write_field_usize(out, key, value, comma),
        None => write_null(out, key, comma),
    }
}

fn write_field_opt_bool(out: &mut String, key: &str, value: Option<bool>, comma: bool) {
    match value {
        Some(value) => write_field_bool(out, key, value, comma),
        None => write_null(out, key, comma),
    }
}

fn write_field_u16_array(out: &mut String, key: &str, values: &[u16], comma: bool) {
    let comma = if comma { "," } else { "" };
    write!(out, "    \"{}\": [", key).expect("writing to String cannot fail");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        write!(out, "{value}").expect("writing to String cannot fail");
    }
    writeln!(out, "]{}", comma).expect("writing to String cannot fail");
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
