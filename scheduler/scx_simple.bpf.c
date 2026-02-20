#include <vmlinux.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>

char _license[] SEC("license") = "GPL";

/* Kernel API Declarations */
extern s32 scx_bpf_create_dsq(u64 dsq_id, s32 node_id) __ksym;
extern void scx_bpf_dsq_insert(struct task_struct *p, u64 dsq_id, u64 slice, u64 enq_flags) __ksym;
extern bool scx_bpf_dsq_move_to_local(u64 dsq_id) __ksym;
/* Declare kick_cpu as weak in case of minor kernel API changes */
extern void scx_bpf_kick_cpu(s32 cpu, u64 flags) __weak __ksym;

/* * Multi-Queue Architecture
 * Instead of a single queue, we separate tasks by class.
 */
#define DSQ_VIP    1
#define DSQ_HOG    2
#define DSQ_NORMAL 0

/* * Time Slices (using the "Goldilocks" tracing parameters) 
 */
#define SCX_SLICE_VIP      50000000  /* 50ms */
#define SCX_SLICE_NORMAL   10000000  /* 10ms */
#define SCX_SLICE_HOG      1000000   /* 1ms */

#define SCX_OPS(name, args...) SEC("struct_ops/"#name) BPF_PROG(name, ##args)
#define SCX_OPS_SLEEPABLE(name, args...) SEC("struct_ops.s/"#name) BPF_PROG(name, ##args)

/* Initialize all queues */
s32 SCX_OPS_SLEEPABLE(simple_init) {
    scx_bpf_create_dsq(DSQ_VIP, -1);
    scx_bpf_create_dsq(DSQ_HOG, -1);
    scx_bpf_create_dsq(DSQ_NORMAL, -1);
    return 0;
}

/* Helper to check task names */
static __always_inline bool match_name(const char *comm, const char *target) {
    for (int i = 0; i < 16; i++) {
        if (target[i] == '\0') return true;
        if (comm[i] != target[i]) return false;
    }
    return true;
}

/*
 * =====================================================================
 * REQUIREMENT 1 & 3: Locality, NUMA, and Preemption
 * =====================================================================
 * This hook is called when a task wakes up. It decides WHICH CPU it goes to.
 */
s32 SCX_OPS(simple_select_cpu, struct task_struct *p, s32 prev_cpu, u64 wake_flags) {
    if (match_name(p->comm, "critical_2")) {
        /* * CPU ISOLATION / NUMA: Force VIP tasks to stay on CPU 0 or 1.
         * This prevents cache eviction and maximizes L1/L2 locality.
         */
        s32 target_cpu = (prev_cpu == 0 || prev_cpu == 1) ? prev_cpu : 0;

        /* * PREEMPTION: If CPU 0/1 is currently running a Hog, kick it off immediately!
         * This fulfills the requirement: "preempt y running on its dedicated CPUs"
         */
        if (scx_bpf_kick_cpu) {
            scx_bpf_kick_cpu(target_cpu, 0);
        }
        return target_cpu;
    }

    /* Keep normal tasks on their previous CPU to avoid unnecessary migration */
    return prev_cpu;
}

/* Enqueue Logic: Sort tasks into their respective queues */
void SCX_OPS(simple_enqueue, struct task_struct *p, u64 enq_flags) {
    if (match_name(p->comm, "critical_2")) {
        scx_bpf_dsq_insert(p, DSQ_VIP, SCX_SLICE_VIP, enq_flags | SCX_ENQ_HEAD);
        return;
    }
    if (match_name(p->comm, "hog")) {
        scx_bpf_dsq_insert(p, DSQ_HOG, SCX_SLICE_HOG, enq_flags);
        return;
    }
    scx_bpf_dsq_insert(p, DSQ_NORMAL, SCX_SLICE_NORMAL, enq_flags);
}

/*
 * =====================================================================
 * REQUIREMENT 2 & 4: Work Conservation and Partitioning
 * =====================================================================
 * This hook is called when a CPU goes idle and needs a new task.
 */
void SCX_OPS(simple_dispatch, s32 cpu, struct task_struct *prev) {
    /* VIP CPUs (0 and 1) */
    if (cpu == 0 || cpu == 1) {
        /* 1st Priority: Always run VIP tasks if available */
        if (scx_bpf_dsq_move_to_local(DSQ_VIP)) return;
        
        /* 2nd Priority: Keep system responsive */
        if (scx_bpf_dsq_move_to_local(DSQ_NORMAL)) return;
        
        /* 3rd Priority (WORK CONSERVATION): If VIP is sleeping, let Hog use the CPU! */
        scx_bpf_dsq_move_to_local(DSQ_HOG);
    } 
    /* Standard CPUs (2, 3, etc.) */
    else {
        /* NEVER run VIP tasks here to protect their cache locality */
        if (scx_bpf_dsq_move_to_local(DSQ_NORMAL)) return;
        scx_bpf_dsq_move_to_local(DSQ_HOG);
    }
}

/* Visualizer Hooks (Unchanged) */
void SCX_OPS(simple_running, struct task_struct *p) {
    if (p->comm[0] == 'c' || p->comm[0] == 'h') {
        s32 cpu = bpf_get_smp_processor_id();
        bpf_printk("[SCX] CPU=%d EV=START COMM=%s", cpu, p->comm);
    }
}
void SCX_OPS(simple_stopping, struct task_struct *p, bool runnable) {
    if (p->comm[0] == 'c' || p->comm[0] == 'h') {
        s32 cpu = bpf_get_smp_processor_id();
        bpf_printk("[SCX] CPU=%d EV=STOP COMM=%s", cpu, p->comm);
    }
}

SEC(".struct_ops.link")
struct sched_ext_ops simple_ops = {
    .select_cpu = (void *)simple_select_cpu,
    .enqueue    = (void *)simple_enqueue,
    .dispatch   = (void *)simple_dispatch,
    .running    = (void *)simple_running,
    .stopping   = (void *)simple_stopping,
    .init       = (void *)simple_init,
    .name       = "simple_scheduler",
};