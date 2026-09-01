use sched_ext_verus::mock::{verified_single_task_round_trip, Event, MockApi, MockTask};
use sched_ext_verus::{MAIN_DSQ, NO_NUMA_NODE, SLICE_NS};

#[test]
fn one_task_crosses_the_mock_dsq() {
    let mut api = MockApi::default();
    let task = MockTask { id: 42 };

    assert_eq!(verified_single_task_round_trip(&mut api, task), 0);
    assert!(api.queue().is_empty());
    assert_eq!(api.moved(), &[task]);
    assert_eq!(
        api.events(),
        &[
            Event::CreateDsq {
                dsq_id: MAIN_DSQ,
                node_id: NO_NUMA_NODE,
            },
            Event::Insert {
                task,
                dsq_id: MAIN_DSQ,
                slice_ns: SLICE_NS,
                enq_flags: 0,
            },
            Event::MoveToLocal {
                dsq_id: MAIN_DSQ,
                moved: true,
            },
        ]
    );
}
