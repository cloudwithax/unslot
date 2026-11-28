/*
 * bridge.h: slot driving a standalone emulator as it drives a core of its own, for the shims
 * preloaded into DraStic, PPSSPPSDL and PICO-8. slot (crates/slot/src/standalone.rs) starts the
 * emulator with UNSLOT_BRIDGE="<memory> <socket>", two descriptors it leaves open: memory laid out
 * as struct unslot_bridge, which both map, and a socket slot asks over, a byte at a time:
 *
 *   'r'  let the emulator draw one more frame, then answer 'f' with its picture, its sound since
 *        the last and the pad for the next in the memory
 *   's'  save the emulator's state, 'l' load it, where slot knows to find it; 'k' once done
 *   'q'  close the emulator, which saves the game as its own Quit would
 *
 * The emulator's first frame is answered 'h', unasked, so slot knows it is up. Nothing reaches
 * the screen, the speaker or the emulator from the pad meanwhile: slot draws, plays and reads it.
 *
 * A shim includes this once, calls bridge_present from where the emulator shows a frame, having
 * left the picture in bridge_frame, hands the sound callback to bridge_audio, and asks
 * bridge_hotkey from where it reads the pad for a save or load to press for.
 */
#include <pthread.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

#define BRIDGE_MAX_PIXELS (2048 * 1088)
#define BRIDGE_MAX_SAMPLES 8192

struct unslot_bridge {
	/* The pad, as libretro numbers a pad's buttons, and its sticks. */
	uint32_t buttons;
	int16_t sticks[4];
	/* The frame, XRGB8888 rows width long. */
	uint32_t width, height;
	/* Its sound: rate samples a second, samples stereo pairs of them in audio. */
	uint32_t rate;
	uint32_t samples;
	int16_t audio[2 * BRIDGE_MAX_SAMPLES];
	uint32_t frame[BRIDGE_MAX_PIXELS];
};

/* A frame later than this is not coming: slot hears so rather than wait for good. */
#define BRIDGE_PATIENCE_MS 4000
/* How many frames a save or load is given after its key is let go of. */
#define BRIDGE_HOTKEY_FRAMES 4

static struct unslot_bridge *bridge;
static int bridge_sock = -1;
static pthread_mutex_t bridge_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t bridge_moved = PTHREAD_COND_INITIALIZER;
/* Frames shown, and frames slot has let the emulator go on to. */
static unsigned bridge_shown, bridge_allowed;
static atomic_int bridge_closing;
/* The save or load waiting to be pressed for, 0 for none, and the frame its key was let go on. */
static atomic_int bridge_key;
static atomic_uint bridge_key_done;
static atomic_int bridge_key_up;

static void (*bridge_callback)(void *, uint8_t *, int);
static void *bridge_userdata;
static int bridge_channels, bridge_float;
static double bridge_fps = 60.0, bridge_owed;
/* Set by a shim whose emulator's sound keeps to its frames, DraStic's, for a frame's worth after
 * each however fast slot runs them; DraStic waits for its sound to be taken before it draws
 * more. */
static int bridge_sound_by_frame;

static int bridge_on(void) {
	return bridge != NULL;
}

static uint32_t *bridge_frame(void) {
	return bridge->frame;
}

static int bridge_quitting(void) {
	return atomic_load(&bridge_closing);
}

/* The emulator's sound, which slot pulls a frame's worth of after each frame: the callback it
 * gave SDL, the format SDL was asked for, and the frames a second it runs at. */
static void bridge_audio(void (*callback)(void *, uint8_t *, int), void *userdata, int rate,
		int channels, int is_float, double fps) {
	pthread_mutex_lock(&bridge_lock);
	bridge_callback = callback;
	bridge_userdata = userdata;
	bridge_channels = channels;
	bridge_float = is_float;
	bridge_fps = fps;
	bridge->rate = rate;
	pthread_mutex_unlock(&bridge_lock);
}

static void bridge_deadline(struct timespec *t, int ms) {
	clock_gettime(CLOCK_REALTIME, t);
	t->tv_sec += ms / 1000;
	t->tv_nsec += (long)(ms % 1000) * 1000000;
	if (t->tv_nsec >= 1000000000) {
		t->tv_sec++;
		t->tv_nsec -= 1000000000;
	}
}

/* From where the emulator shows a frame, once it is in bridge_frame: waits for slot to let it go
 * on to the next. */
static void bridge_present(int width, int height) {
	pthread_mutex_lock(&bridge_lock);
	bridge->width = width;
	bridge->height = height;
	bridge_shown++;
	pthread_cond_broadcast(&bridge_moved);
	while (bridge_allowed < bridge_shown && !bridge_quitting())
		pthread_cond_wait(&bridge_moved, &bridge_lock);
	pthread_mutex_unlock(&bridge_lock);
}

/* Lets the emulator draw one more frame and waits for it; 0 if it never came. */
static int bridge_step(void) {
	struct timespec until;
	bridge_deadline(&until, BRIDGE_PATIENCE_MS);
	pthread_mutex_lock(&bridge_lock);
	unsigned want = bridge_shown + 1;
	bridge_allowed = bridge_shown;
	pthread_cond_broadcast(&bridge_moved);
	int r = 0;
	while (bridge_shown < want && r == 0 && !bridge_quitting())
		r = pthread_cond_timedwait(&bridge_moved, &bridge_lock, &until);
	int came = bridge_shown >= want;
	pthread_mutex_unlock(&bridge_lock);
	return came;
}

static double bridge_pulled;

static double bridge_now(void) {
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return t.tv_sec + t.tv_nsec / 1e9;
}

/* `since` seconds of the emulator's sound into the memory, as stereo 16-bit. */
static void bridge_pull(double since) {
	bridge_pulled = bridge_now();
	pthread_mutex_lock(&bridge_lock);
	bridge->samples = 0;
	if (bridge_callback && bridge->rate) {
		bridge_owed += bridge->rate * since;
		int n = (int)bridge_owed;
		if (n > BRIDGE_MAX_SAMPLES)
			n = BRIDGE_MAX_SAMPLES;
		bridge_owed -= n;
		int size = bridge_float ? 4 : 2;
		static uint8_t *buf;
		static int cap;
		int len = n * bridge_channels * size;
		if (len > cap) {
			free(buf);
			buf = malloc(len);
			cap = buf ? len : 0;
		}
		if (buf && n > 0) {
			memset(buf, 0, len);
			bridge_callback(bridge_userdata, buf, len);
			for (int i = 0; i < n; i++)
				for (int c = 0; c < 2; c++) {
					int from = i * bridge_channels + (bridge_channels > 1 ? c : 0);
					int16_t s;
					if (bridge_float) {
						float f = ((float *)buf)[from] * 32767.0f;
						s = f > 32767.0f ? 32767 : f < -32768.0f ? -32768 : (int16_t)f;
					} else {
						s = ((int16_t *)buf)[from];
					}
					bridge->audio[i * 2 + c] = s;
				}
			bridge->samples = n;
		}
	}
	pthread_mutex_unlock(&bridge_lock);
}

/* The emulator's sound since the last frame. That is the time since, not a frame's worth, unless
 * bridge_sound_by_frame: PPSSPP shows a game drawn at 30 frames a second 30 times a second,
 * keeping its own time, and its sound would come at half the rate. A wait longer than a few
 * frames, slot's menu up, is not made up for. */
static void bridge_pull_audio(void) {
	if (bridge_sound_by_frame) {
		bridge_pull(1.0 / bridge_fps);
		return;
	}
	double since = bridge_pulled ? bridge_now() - bridge_pulled : 1.0 / bridge_fps;
	bridge_pull(since > 4.0 / bridge_fps ? 4.0 / bridge_fps : since);
}

/* The save ('s') or load ('l') the pad should press for now, taken once; 0 for none. The shim
 * presses for it, and calls bridge_hotkey_up once it has let go; PICO-8's, keeping no states,
 * presses for none. */
__attribute__((unused)) static int bridge_hotkey(void) {
	return atomic_exchange(&bridge_key, 0);
}

__attribute__((unused)) static void bridge_hotkey_up(void) {
	pthread_mutex_lock(&bridge_lock);
	atomic_store(&bridge_key_done, bridge_shown);
	atomic_store(&bridge_key_up, 1);
	pthread_mutex_unlock(&bridge_lock);
}

static void bridge_close(void) {
	atomic_store(&bridge_closing, 1);
	pthread_mutex_lock(&bridge_lock);
	pthread_cond_broadcast(&bridge_moved);
	pthread_mutex_unlock(&bridge_lock);
}

/* Presses for a save or load and runs the frames it takes, their sound thrown away. Those run
 * as fast as the emulator draws them, so each takes a frame's worth. */
static int bridge_press(int op) {
	atomic_store(&bridge_key_up, 0);
	atomic_store(&bridge_key, op);
	for (int i = 0; i < 120; i++) {
		if (!bridge_step())
			return 0;
		bridge_pull(1.0 / bridge_fps);
		if (atomic_load(&bridge_key_up) &&
				bridge_shown - atomic_load(&bridge_key_done) >= BRIDGE_HOTKEY_FRAMES)
			break;
	}
	bridge->samples = 0;
	return 1;
}

static void *bridge_serve(void *unused) {
	(void)unused;
	pthread_mutex_lock(&bridge_lock);
	while (bridge_shown == 0)
		pthread_cond_wait(&bridge_moved, &bridge_lock);
	pthread_mutex_unlock(&bridge_lock);
	char hello = 'h';
	if (write(bridge_sock, &hello, 1) != 1)
		bridge_close();
	while (!bridge_quitting()) {
		char op, answer = 'k';
		if (read(bridge_sock, &op, 1) != 1)
			break;
		switch (op) {
		case 'r':
			if (!bridge_step())
				answer = 'e';
			bridge_pull_audio();
			answer = answer == 'e' ? 'e' : 'f';
			break;
		case 's':
		case 'l':
			if (!bridge_press(op))
				answer = 'e';
			break;
		case 'q':
			bridge_close();
			break;
		default:
			answer = 'e';
		}
		if (write(bridge_sock, &answer, 1) != 1)
			break;
	}
	/* slot is gone, or asked: the emulator closes as its own Quit would, and is let go of if
	 * it hangs doing so. */
	bridge_close();
	sleep(10);
	_exit(1);
	return NULL;
}

__attribute__((constructor)) static void bridge_open(void) {
	const char *fds = getenv("UNSLOT_BRIDGE");
	int mem, sock;
	if (!fds || sscanf(fds, "%d %d", &mem, &sock) != 2)
		return;
	/* The emulator's own children are not slot's to drive, nor should they preload the SDL shim. */
	unsetenv("UNSLOT_BRIDGE");
	unsetenv("LD_PRELOAD");
	void *m = mmap(NULL, sizeof(struct unslot_bridge), PROT_READ | PROT_WRITE, MAP_SHARED, mem, 0);
	close(mem);
	if (m == MAP_FAILED) {
		perror("unslot: bridge");
		return;
	}
	bridge = m;
	bridge_sock = sock;
	pthread_t t;
	if (pthread_create(&t, NULL, bridge_serve, NULL) == 0)
		pthread_detach(t);
}
