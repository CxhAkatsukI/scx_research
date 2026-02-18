/* workloads/critical.c */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <pthread.h>
#include <stdatomic.h>
#include <time.h>
#include <signal.h>

/* Global atomic counter */
atomic_long global_counter = 0;
volatile int running = 1;

typedef struct {
    int id;
    int sleep_us; /* Sleep every n seconds */
} thread_arg_t;

void handle_sigint(int sig) {
    running = 0;
}

void* worker(void* arg) {
    thread_arg_t* t_arg = (thread_arg_t*)arg;
    int sleep_time = t_arg->sleep_us;
    
    while (running) {
        /* Critical transaction logic */
        atomic_fetch_add(&global_counter, 1);
        
        /* IO transactions and sleep logic */
        if (sleep_time > 0) {
            /* This program will sleep and will not become hog */
            if (atomic_load(&global_counter) % 1000 == 0) {
                 usleep(sleep_time);
            }
        }
    }
    return NULL;
}

/* Helper function to get elapsed time */
double get_elapsed(struct timespec start, struct timespec end) {
    return (end.tv_sec - start.tv_sec) + (end.tv_nsec - start.tv_nsec) / 1e9;
}

/* Monitor thread: prints throughput every second */
void* monitor(void* arg) {
    long last_count = 0;
    struct timespec last_time, current_time;
    
    clock_gettime(CLOCK_MONOTONIC, &last_time);

    while (running) {
        sleep(1); /* We don't trust the result of this function */

        /* Get precise time */
        clock_gettime(CLOCK_MONOTONIC, &current_time);
        double dt = get_elapsed(last_time, current_time);
        
        long current_count = atomic_load(&global_counter);
        long delta_ops = current_count - last_count;
        
        /* throughput using precise timing */
        double real_throughput = delta_ops / dt;

        printf("[App X] Time elapsed: %.4f s | Throughput: %.0f ops/sec\n", 
               dt, real_throughput);

        last_count = current_count;
        last_time = current_time;
    }
    return NULL;
}

int main(int argc, char* argv[]) {
    if (argc < 2) {
        fprintf(stderr, "Usage: %s <num_threads> [sleep_us]\n", argv[0]);
        return 1;
    }

    int num_threads = atoi(argv[1]);
    int sleep_us = (argc > 2) ? atoi(argv[2]) : 0;

    pthread_t* threads = malloc(num_threads * sizeof(pthread_t));
    pthread_t mon_thread;

    signal(SIGINT, handle_sigint);

    printf("Starting Critical App X with %d threads (Sleep: %dus)...\n", num_threads, sleep_us);

    /* Start monitor thread */
    pthread_create(&mon_thread, NULL, monitor, NULL);

    /* Start worker threads */
    thread_arg_t args = { .sleep_us = sleep_us };
    for (int i = 0; i < num_threads; i++) {
        pthread_create(&threads[i], NULL, worker, &args);
    }

    for (int i = 0; i < num_threads; i++) {
        pthread_join(threads[i], NULL);
    }
    
    pthread_join(mon_thread, NULL);
    return 0;
}