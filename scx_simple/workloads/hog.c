/* workloads/hog.c */
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>

volatile int running = 1;

void handle_sigint(int sig) {
    running = 0;
}

/* Function to drain CPU */
void* cpu_hog(void* arg) {
    while (running) {
        __asm__ volatile ("nop");
    }
    return NULL;
}

int main(int argc, char* argv[]) {
    if (argc < 2) {
        fprintf(stderr, "Usage: %s <num_threads>\n", argv[0]);
        return 1;
    }

    int num_threads = atoi(argv[1]);
    pthread_t* threads = malloc(num_threads * sizeof(pthread_t));
    
    signal(SIGINT, handle_sigint);
    
    printf("Starting %d CPU hog threads (Press Ctrl+C to stop)...\n", num_threads);

    /* Create threads based on input arguments */
    for (long i = 0; i < num_threads; i++) {
        pthread_create(&threads[i], NULL, cpu_hog, (void*)i);
    }

    for (int i = 0; i < num_threads; i++) {
        pthread_join(threads[i], NULL);
    }

    printf("\nStopped.\n");
    return 0;
}