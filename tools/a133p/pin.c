/*
 * pin: taskset and chrt for TrimUI's A133P firmware, whose BusyBox has neither. Measurement only;
 * nothing shipped runs it. Build: tools/a133p/zcc -O2 -o pin tools/a133p/pin.c
 *
 *   pin CPUS [-n NICE] [-f PRIO|-r PRIO|-o] -p TID...   change running threads
 *   pin CPUS [-n NICE] [-f PRIO|-r PRIO|-o] CMD ARG...  run a command
 *
 * CPUS is a list like 0-1 or 0,3, or - to leave the CPUs. -f/-r set SCHED_FIFO/SCHED_RR at
 * PRIO, -o puts a thread back on SCHED_OTHER.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <unistd.h>

static int parse_cpus(const char *s, cpu_set_t *set) {
  CPU_ZERO(set);
  while (*s) {
    char *end;
    long lo = strtol(s, &end, 10), hi = lo;
    if (end == s) return -1;
    if (*end == '-') {
      s = end + 1;
      hi = strtol(s, &end, 10);
      if (end == s) return -1;
    }
    for (long c = lo; c <= hi; c++) CPU_SET((int)c, set);
    s = *end == ',' ? end + 1 : end;
    if (*end && *end != ',') return -1;
  }
  return 0;
}

int main(int argc, char **argv) {
  if (argc < 3) {
    fprintf(stderr, "usage: pin CPUS [-n NICE] [-f PRIO|-r PRIO|-o] -p TID... | CMD...\n");
    return 2;
  }
  cpu_set_t set;
  int cpus = strcmp(argv[1], "-") != 0;
  if (cpus && parse_cpus(argv[1], &set)) {
    fprintf(stderr, "pin: bad cpu list %s\n", argv[1]);
    return 2;
  }
  int i = 2, nice = 0, have_nice = 0, policy = -1, prio = 0, pids = 0;
  for (; i < argc && argv[i][0] == '-'; i++) {
    if (!strcmp(argv[i], "-n") && i + 1 < argc) {
      nice = atoi(argv[++i]);
      have_nice = 1;
    } else if (!strcmp(argv[i], "-f") && i + 1 < argc) {
      policy = SCHED_FIFO;
      prio = atoi(argv[++i]);
    } else if (!strcmp(argv[i], "-r") && i + 1 < argc) {
      policy = SCHED_RR;
      prio = atoi(argv[++i]);
    } else if (!strcmp(argv[i], "-o")) {
      policy = SCHED_OTHER;
    } else if (!strcmp(argv[i], "-p")) {
      pids = 1;
    } else {
      break;
    }
  }
  int rc = 0;
  if (pids) {
    for (; i < argc; i++) {
      pid_t tid = (pid_t)atoi(argv[i]);
      if (cpus && sched_setaffinity(tid, sizeof(set), &set)) {
        fprintf(stderr, "pin: %d: affinity: %s\n", tid, strerror(errno));
        rc = 1;
      }
      if (policy >= 0) {
        struct sched_param sp = {.sched_priority = prio};
        if (sched_setscheduler(tid, policy, &sp)) {
          fprintf(stderr, "pin: %d: policy: %s\n", tid, strerror(errno));
          rc = 1;
        }
      }
      if (have_nice && setpriority(PRIO_PROCESS, (id_t)tid, nice)) {
        fprintf(stderr, "pin: %d: nice: %s\n", tid, strerror(errno));
        rc = 1;
      }
    }
    return rc;
  }
  if (i >= argc) return 2;
  if (cpus && sched_setaffinity(0, sizeof(set), &set)) perror("pin: affinity");
  if (policy >= 0) {
    struct sched_param sp = {.sched_priority = prio};
    if (sched_setscheduler(0, policy, &sp)) perror("pin: policy");
  }
  if (have_nice && setpriority(PRIO_PROCESS, 0, nice)) perror("pin: nice");
  execvp(argv[i], argv + i);
  perror("pin: exec");
  return 127;
}
