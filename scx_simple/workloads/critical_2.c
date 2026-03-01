/* critical_2.c */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <pthread.h>
#include <time.h>
#include <signal.h>

volatile int running = 1;

typedef struct {
    volatile long counter;    /* Actual Data */
    char padding[56];         /* Padding */
} __attribute__((aligned(64))) aligned_counter_t; /* Aligned to 64 bytes */

/* Independent thread counter fo each thread */
aligned_counter_t* thread_counters;
int g_num_threads;

void handle_sigint(int sig) {
    running = 0;
}

/* Helper function */
double get_elapsed(struct timespec start, struct timespec end) {
    return (end.tv_sec - start.tv_sec) + (end.tv_nsec - start.tv_nsec) / 1e9;
}

void* worker(void* arg) {
    long id = (long)arg;
    while (running) {
        thread_counters[id].counter++;
        for (volatile int i = 0; i < 50; i++); 
    }
    return NULL;
}

void* monitor(void* arg) {
    long last_total = 0;
    struct timespec last_time, current_time;
    clock_gettime(CLOCK_MONOTONIC, &last_time);

    while (running) {
        sleep(3);
        clock_gettime(CLOCK_MONOTONIC, &current_time);
        
        long current_total = 0;
        for (int i = 0; i < g_num_threads; i++) {
            current_total += thread_counters[i].counter;
        }
        
        double dt = get_elapsed(last_time, current_time);
        long delta = current_total - last_total;
        
        if (dt > 0) {
            printf("[App X] Throughput: %.0f ops/sec\n", (delta / dt));
        }

        last_total = current_total;
        last_time = current_time;
    }
    return NULL;
}

int main(int argc, char* argv[]) {
    if (argc < 2) {
        fprintf(stderr, "Usage: %s <num_threads>\n", argv[0]);
        return 1;
    }

    g_num_threads = atoi(argv[1]);
    thread_counters =  aligned_alloc(64, g_num_threads * sizeof(aligned_counter_t));
    
    pthread_t* threads = malloc(g_num_threads * sizeof(pthread_t));
    pthread_t mon_thread;

    signal(SIGINT, handle_sigint);

    printf("Starting Critical_2 App X with %d threads...\n", g_num_threads);

    pthread_create(&mon_thread, NULL, monitor, NULL);

    for (long i = 0; i < g_num_threads; i++) {
        pthread_create(&threads[i], NULL, worker, (void*)i);
    }

    for (int i = 0; i < g_num_threads; i++) {
        pthread_join(threads[i], NULL);
    }
    
    return 0;
}