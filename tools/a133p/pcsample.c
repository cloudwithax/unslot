/* PC sampler for slot's core host, preloaded with LD_PRELOAD (tools/a133p/ct2-bench.sh pcsample=1).
 *
 * In a process whose command line has --core-host, a helper thread waits for the thread named
 * $PCSAMPLE_THREAD (default "emu"), then samples it every $PCSAMPLE_US (default 250) microseconds
 * of that thread's CPU time with a SIGPROF timer aimed at it. Each sample is the interrupted pc and
 * x30 (lr), two u64s, appended to $PCSAMPLE_OUT/pcsample.bin about once a second; /proc/self/maps
 * goes to pcsample.bin.maps beside it. Other processes are left alone. */
#define _GNU_SOURCE
#include <dirent.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <time.h>
#include <ucontext.h>
#include <unistd.h>

#define CAP (1 << 20)

static uint64_t buf[CAP][2];
static volatile uint32_t head;
static uint32_t tail;

static void on_prof(int sig, siginfo_t *si, void *ctx) {
	(void)sig;
	(void)si;
	ucontext_t *uc = ctx;
	uint32_t h = head;
	if (h - tail >= CAP)
		return;
	buf[h % CAP][0] = uc->uc_mcontext.pc;
	buf[h % CAP][1] = uc->uc_mcontext.regs[30];
	__atomic_store_n(&head, h + 1, __ATOMIC_RELEASE);
}

static int find_thread(const char *want) {
	DIR *d = opendir("/proc/self/task");
	if (!d)
		return 0;
	struct dirent *e;
	int tid = 0;
	while (!tid && (e = readdir(d))) {
		if (e->d_name[0] == '.')
			continue;
		char p[96], comm[32] = {0};
		snprintf(p, sizeof(p), "/proc/self/task/%s/comm", e->d_name);
		FILE *f = fopen(p, "r");
		if (!f)
			continue;
		if (fgets(comm, sizeof(comm), f)) {
			comm[strcspn(comm, "\n")] = 0;
			if (!strcmp(comm, want))
				tid = atoi(e->d_name);
		}
		fclose(f);
	}
	closedir(d);
	return tid;
}

static void copy_maps(const char *path) {
	char tmp[600];
	snprintf(tmp, sizeof(tmp), "%s.maps", path);
	FILE *in = fopen("/proc/self/maps", "r"), *out = fopen(tmp, "w");
	if (in && out) {
		char line[512];
		while (fgets(line, sizeof(line), in))
			fputs(line, out);
	}
	if (in)
		fclose(in);
	if (out)
		fclose(out);
}

static void *run(void *arg) {
	(void)arg;
	const char *want = getenv("PCSAMPLE_THREAD");
	const char *dir = getenv("PCSAMPLE_OUT");
	const char *us = getenv("PCSAMPLE_US");
	long period = us ? strtol(us, NULL, 10) : 250;
	if (!want)
		want = "emu";
	if (!dir)
		dir = "/tmp";
	char path[512];
	snprintf(path, sizeof(path), "%s/pcsample.bin", dir);

	int tid = 0;
	while (!(tid = find_thread(want)))
		usleep(100000);

	struct sigaction sa = {0};
	sa.sa_sigaction = on_prof;
	sa.sa_flags = SA_SIGINFO | SA_RESTART;
	sigemptyset(&sa.sa_mask);
	sigaction(SIGPROF, &sa, NULL);

	/* The thread's CPU-time clock: MAKE_THREAD_CPUCLOCK(tid, CPUCLOCK_SCHED). */
	clockid_t clk = (clockid_t)((~(unsigned)tid << 3) | 4 | 2);
	struct sigevent sev = {0};
	sev.sigev_notify = 4; /* SIGEV_THREAD_ID */
	sev.sigev_signo = SIGPROF;
	sev._sigev_un._tid = tid;
	timer_t t;
	if (timer_create(clk, &sev, &t)) {
		perror("pcsample: timer_create");
		return NULL;
	}
	struct itimerspec its = {{0, period * 1000}, {0, period * 1000}};
	timer_settime(t, 0, &its, NULL);
	fprintf(stderr, "pcsample: sampling thread %d (%s) every %ld us of its CPU time into %s\n", tid,
		want, period, path);

	FILE *f = fopen(path, "w");
	if (!f) {
		perror("pcsample: fopen");
		return NULL;
	}
	for (int n = 0;; n++) {
		sleep(1);
		uint32_t h = __atomic_load_n(&head, __ATOMIC_ACQUIRE);
		while (tail != h) {
			uint32_t end = h;
			if (end / CAP != tail / CAP)
				end = (tail / CAP + 1) * CAP;
			fwrite(buf[tail % CAP], 16, end - tail, f);
			tail = end;
		}
		fflush(f);
		if (n % 10 == 0)
			copy_maps(path);
	}
	return NULL;
}

__attribute__((constructor)) static void init(void) {
	char cmd[4096];
	FILE *f = fopen("/proc/self/cmdline", "r");
	if (!f)
		return;
	size_t n = fread(cmd, 1, sizeof(cmd) - 1, f);
	fclose(f);
	cmd[n] = 0;
	int host = 0;
	for (size_t i = 0; i < n; i += strlen(cmd + i) + 1)
		host |= !strcmp(cmd + i, "--core-host");
	if (!host && !getenv("PCSAMPLE_ANY"))
		return;
	pthread_t th;
	if (!pthread_create(&th, NULL, run, NULL)) {
		pthread_setname_np(th, "pcsample");
		pthread_detach(th);
	}
}
