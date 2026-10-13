// SPDX-License-Identifier: GPL-2.0-only
/* Host-only bindings. These model an environment; they are not kernel proof. */
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>
#include <assert.h>
#include <stdlib.h>
#include <stdio.h>

typedef uint64_t u64;
typedef uint32_t u32;
typedef uint8_t u8;
typedef int32_t s32;

#define __ksym
#define SEC(name)
#define __uint(name, val) int (*name)[val]
#define __type(name, type) type *name
#define OP(name, args...) name(args)
#define SLEEPABLE_OP(name, args...) name(args)
#define P1_TOPOLOGY_CONST
#define BPF_MAP_TYPE_TASK_STORAGE 29
#define BPF_F_NO_PREALLOC 1
#define BPF_LOCAL_STORAGE_GET_F_CREATE 1
#define SCX_DSQ_GLOBAL ((1ULL << 63) | 1)
#define SCX_SLICE_DFL 20000000ULL
#define SCX_OPS_SWITCH_PARTIAL (1ULL << 3)

struct task_struct { int cpu; int pid; void *storage; bool pending; bool queued; u64 dsq; };
struct scx_init_task_args { bool dummy; };
struct scx_exit_task_args { bool cancelled; };
struct sched_ext_ops {
    void *select_cpu, *enqueue, *dispatch, *running, *disable, *init_task, *exit_task, *init;
    u64 flags;
    char name[128];
};

void *bpf_task_storage_get(void *map, struct task_struct *p, void *value, u64 flags);
int bpf_task_storage_delete(void *map, struct task_struct *p);
