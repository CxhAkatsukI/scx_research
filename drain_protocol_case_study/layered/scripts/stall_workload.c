#define _GNU_SOURCE
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <time.h>
#include <unistd.h>

static volatile sig_atomic_t stop;

static void on_signal(int sig)
{
    (void)sig;
    stop = 1;
}

static long long now_ns(void)
{
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts)) {
        perror("clock_gettime");
        exit(1);
    }
    return (long long)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

static void sleep_ns(long long ns)
{
    struct timespec req = {
        .tv_sec = ns / 1000000000LL,
        .tv_nsec = ns % 1000000000LL,
    };
    while (!stop && nanosleep(&req, &req) && errno == EINTR)
        ;
}

int main(int argc, char **argv)
{
    long long start, period = 20000000LL;
    int seconds, load;
    const char *name;

    if (argc != 4 || (strcmp(argv[1], "stall-target") &&
                      strcmp(argv[1], "stall-load"))) {
        fprintf(stderr, "usage: %s stall-target|stall-load SECONDS LOAD_PERCENT\n", argv[0]);
        return 2;
    }
    name = argv[1];
    seconds = atoi(argv[2]);
    load = atoi(argv[3]);
    if (seconds < 1 || seconds > 3600 || load < 1 || load > 100) {
        fprintf(stderr, "invalid duration or load\n");
        return 2;
    }
    if (prctl(PR_SET_NAME, name, 0, 0, 0)) {
        perror("prctl");
        return 1;
    }
    signal(SIGTERM, on_signal);
    signal(SIGINT, on_signal);

    start = now_ns();
    if (!strcmp(name, "stall-target")) {
        /* Heavy load runs for 2 s, then the target wakes for the 2 s shrink window. */
        period = 4000000000LL;
        load = 50;
        sleep_ns(2000000000LL);
    }
    while (!stop && now_ns() - start < (long long)seconds * 1000000000LL) {
        long long begin = now_ns();
        long long busy = period * load / 100;
        while (!stop && now_ns() - begin < busy)
            ;
        if (!stop)
            sleep_ns(period - (now_ns() - begin));
    }
    return 0;
}
