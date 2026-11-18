/*
 * libunslot-ppsspp.so: slot's volume, menu isolation, and hotkeys inside standalone PPSSPPSDL.
 * Preloaded into PPSSPPSDL by System/ppsspp/launch.sh, it scales the sound PPSSPP's audio
 * callback hands SDL by the level slot --ppsspp leaves in UNSLOT_VOLUME, squared as slot scales
 * its own, and draws a bar over the game for a moment each time a volume key is pressed.
 *
 * UNSLOT_VOLUME holds "<level> <presses> <quit>": the level 0 to 100 (0 when muted), a count
 * that moves with every volume key press so a press at 0 or 100 still shows the bar, and 1
 * once Exit Game was picked in slot's menu, when PPSSPP is asked via SDL_QUIT to save and close.
 *
 * slot SIGSTOPs PPSSPP while its menu is up, and pad events pressed in the menu wait in SDL/input
 * queues; on SIGCONT (detected by a >250ms poll gap) controller/joystick events are dropped for
 * 100ms so the game never sees them. Select + X is mapped to SDL_CONTROLLER_BUTTON_GUIDE (which
 * PPSSPP maps to NKCODE_BACK -> pause menu), while raw standalone Guide is suppressed.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <limits.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include <GLES2/gl2.h>
#include "SDL.h"

#define SHOW_MS 1500
#define READ_EVERY_US 20000
/* A wait between two polls this long means PPSSPP was SIGSTOPped by slot --ppsspp. */
#define STOPPED_MS 250
/* How long after SIGCONT queued pad events are dropped. */
#define DROP_MS 100
#define MAX_AUDIO_HOOKS 16
#define MAX_WATCHES 8

static atomic_int level = 100;
static atomic_uint shown_at;
static atomic_int showing;
static atomic_int quit;
static atomic_int watcher_started;
static char vol_path[PATH_MAX];

typedef struct {
	int active;
	SDL_AudioDeviceID dev;
	SDL_AudioCallback callback;
	void *userdata;
	atomic_int format;
} AudioHook;

static AudioHook audio_hooks[MAX_AUDIO_HOOKS];
static pthread_mutex_t audio_lock = PTHREAD_MUTEX_INITIALIZER;

typedef struct {
	int active;
	SDL_EventFilter filter;
	void *userdata;
} WatchHook;

static WatchHook watch_hooks[MAX_WATCHES];
static pthread_mutex_t watch_lock = PTHREAD_MUTEX_INITIALIZER;
static atomic_int watch_registered;

static Uint32 last_event_ticks;
static Uint32 dropping_until_ticks;
static int select_held;
static int menu_held;

static void *watch(void *path) {
	unsigned seen = 0;
	int first = 1;
	for (;;) {
		FILE *f = fopen((const char *)path, "r");
		if (f) {
			int l, q = 0;
			unsigned presses;
			if (fscanf(f, "%d %u %d", &l, &presses, &q) >= 2 && l >= 0 && l <= 100) {
				atomic_store(&level, l);
				atomic_store(&quit, q == 1);
				if (!first && presses != seen) {
					atomic_store(&shown_at, SDL_GetTicks());
					atomic_store(&showing, 1);
				}
				seen = presses;
				first = 0;
			}
			fclose(f);
		}
		usleep(READ_EVERY_US);
	}
	return NULL;
}

static void start_watcher(void) {
	const char *path = getenv("UNSLOT_VOLUME");
	if (!path || !*path)
		return;
	if (atomic_exchange(&watcher_started, 1) != 0)
		return;
	snprintf(vol_path, sizeof(vol_path), "%s", path);
	pthread_t t;
	if (pthread_create(&t, NULL, watch, vol_path) == 0)
		pthread_detach(t);
	else
		atomic_store(&watcher_started, 0);
}

static AudioHook *alloc_audio_hook(SDL_AudioCallback cb, void *ud, SDL_AudioFormat fmt) {
	pthread_mutex_lock(&audio_lock);
	AudioHook *h = NULL;
	for (int i = 0; i < MAX_AUDIO_HOOKS; i++) {
		if (!audio_hooks[i].active) {
			h = &audio_hooks[i];
			h->active = 1;
			h->dev = 0;
			h->callback = cb;
			h->userdata = ud;
			atomic_store(&h->format, (int)fmt);
			break;
		}
	}
	pthread_mutex_unlock(&audio_lock);
	return h;
}

static void release_audio_hook(AudioHook *h) {
	if (!h)
		return;
	pthread_mutex_lock(&audio_lock);
	h->active = 0;
	h->dev = 0;
	h->callback = NULL;
	h->userdata = NULL;
	pthread_mutex_unlock(&audio_lock);
}

/* Calls PPSSPP's callback with its own userdata, then scales samples by level^2 / 10000. */
static void scaled_audio(void *ctx, Uint8 *stream, int len) {
	AudioHook *h = (AudioHook *)ctx;
	h->callback(h->userdata, stream, len);
	int l = atomic_load(&level);
	if (l >= 100)
		return;
	int gain = l * l; /* out of 10000 */
	SDL_AudioFormat fmt = (SDL_AudioFormat)atomic_load(&h->format);
	if (fmt == AUDIO_S16SYS) {
		Sint16 *s = (Sint16 *)stream;
		for (int i = 0; i < len / 2; i++)
			s[i] = (Sint16)((int)s[i] * gain / 10000);
	} else if (fmt == AUDIO_F32SYS) {
		float *s = (float *)stream;
		float scale = gain / 10000.0f;
		for (int i = 0; i < len / 4; i++)
			s[i] *= scale;
	}
}

SDL_AudioDeviceID SDL_OpenAudioDevice(const char *device, int iscapture,
		const SDL_AudioSpec *desired, SDL_AudioSpec *obtained, int allowed_changes) {
	static SDL_AudioDeviceID (*real)(const char *, int, const SDL_AudioSpec *, SDL_AudioSpec *, int);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_OpenAudioDevice");
	start_watcher();
	const char *path = getenv("UNSLOT_VOLUME");
	if (!path || !*path || iscapture || !desired || !desired->callback || desired->callback == scaled_audio)
		return real(device, iscapture, desired, obtained, allowed_changes);

	AudioHook *h = alloc_audio_hook(desired->callback, desired->userdata, desired->format);
	if (!h)
		return real(device, iscapture, desired, obtained, allowed_changes);

	SDL_AudioSpec d = *desired;
	d.callback = scaled_audio;
	d.userdata = h;

	SDL_AudioDeviceID id = real(device, iscapture, &d, obtained, allowed_changes);
	if (id <= 0) {
		release_audio_hook(h);
		return id;
	}

	SDL_AudioFormat fmt = obtained ? obtained->format : d.format;
	atomic_store(&h->format, (int)fmt);
	pthread_mutex_lock(&audio_lock);
	h->dev = id;
	pthread_mutex_unlock(&audio_lock);

	if (obtained && obtained->callback == scaled_audio) {
		obtained->callback = h->callback;
		obtained->userdata = h->userdata;
	}
	if (fmt != AUDIO_S16SYS && fmt != AUDIO_F32SYS)
		fprintf(stderr, "unslot-ppsspp: sound in format %#x is left at full\n", fmt);
	return id;
}

int SDL_OpenAudio(SDL_AudioSpec *desired, SDL_AudioSpec *obtained) {
	static int (*real)(SDL_AudioSpec *, SDL_AudioSpec *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_OpenAudio");
	start_watcher();
	const char *path = getenv("UNSLOT_VOLUME");
	if (!path || !*path || !desired || !desired->callback || desired->callback == scaled_audio)
		return real(desired, obtained);

	SDL_AudioCallback orig_cb = desired->callback;
	void *orig_ud = desired->userdata;
	AudioHook *h = alloc_audio_hook(orig_cb, orig_ud, desired->format);
	if (!h)
		return real(desired, obtained);

	desired->callback = scaled_audio;
	desired->userdata = h;
	int r = real(desired, obtained);
	desired->callback = orig_cb;
	desired->userdata = orig_ud;
	if (r != 0) {
		release_audio_hook(h);
		return r;
	}

	SDL_AudioFormat fmt = obtained ? obtained->format : desired->format;
	atomic_store(&h->format, (int)fmt);
	pthread_mutex_lock(&audio_lock);
	h->dev = 1;
	pthread_mutex_unlock(&audio_lock);

	if (fmt != AUDIO_S16SYS && fmt != AUDIO_F32SYS)
		fprintf(stderr, "unslot-ppsspp: sound in format %#x is left at full\n", fmt);
	return r;
}

void SDL_CloseAudioDevice(SDL_AudioDeviceID dev) {
	static void (*real)(SDL_AudioDeviceID);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_CloseAudioDevice");
	real(dev);
	pthread_mutex_lock(&audio_lock);
	for (int i = 0; i < MAX_AUDIO_HOOKS; i++) {
		if (audio_hooks[i].active && audio_hooks[i].dev == dev) {
			audio_hooks[i].active = 0;
			audio_hooks[i].dev = 0;
			audio_hooks[i].callback = NULL;
			audio_hooks[i].userdata = NULL;
		}
	}
	pthread_mutex_unlock(&audio_lock);
}

void SDL_CloseAudio(void) {
	static void (*real)(void);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_CloseAudio");
	real();
	pthread_mutex_lock(&audio_lock);
	for (int i = 0; i < MAX_AUDIO_HOOKS; i++) {
		if (audio_hooks[i].active && audio_hooks[i].dev == 1) {
			audio_hooks[i].active = 0;
			audio_hooks[i].dev = 0;
			audio_hooks[i].callback = NULL;
			audio_hooks[i].userdata = NULL;
		}
	}
	pthread_mutex_unlock(&audio_lock);
}

static int is_pad_event(Uint32 type) {
	switch (type) {
	case SDL_CONTROLLERBUTTONDOWN:
	case SDL_CONTROLLERBUTTONUP:
	case SDL_CONTROLLERAXISMOTION:
	case SDL_CONTROLLERDEVICEREMAPPED:
	case SDL_JOYBUTTONDOWN:
	case SDL_JOYBUTTONUP:
	case SDL_JOYAXISMOTION:
	case SDL_JOYHATMOTION:
	case SDL_JOYBALLMOTION:
		return 1;
	default:
		return 0;
	}
}

/*
 * Filters and translates a single event: drops stale pad events during the post-SIGCONT window,
 * turns Select + X into SDL_CONTROLLER_BUTTON_GUIDE (PPSSPP pause menu), and suppresses raw
 * standalone Guide presses. Returns 1 to pass the event through, 0 to drop it.
 */
static int filter_pad_event(SDL_Event *e, Uint32 now) {
	if ((Sint32)(now - dropping_until_ticks) < 0 && is_pad_event(e->type))
		return 0;

	if (e->type == SDL_CONTROLLERBUTTONDOWN || e->type == SDL_CONTROLLERBUTTONUP) {
		int down = e->type == SDL_CONTROLLERBUTTONDOWN;
		if (e->cbutton.button == SDL_CONTROLLER_BUTTON_BACK) {
			select_held = down;
		} else if (e->cbutton.button == SDL_CONTROLLER_BUTTON_X && (down ? select_held : menu_held)) {
			menu_held = down;
			e->cbutton.button = SDL_CONTROLLER_BUTTON_GUIDE;
		} else if (e->cbutton.button == SDL_CONTROLLER_BUTTON_GUIDE) {
			return 0;
		}
	}
	return 1;
}

static void note_poll_time(Uint32 now) {
	if (last_event_ticks && now - last_event_ticks > STOPPED_MS) {
		dropping_until_ticks = now + DROP_MS;
		select_held = 0;
		menu_held = 0;
	}
	last_event_ticks = now;
}

static int SDLCALL wrapped_event_watch(void *ctx, SDL_Event *event) {
	WatchHook *w = (WatchHook *)ctx;
	Uint32 now = SDL_GetTicks();
	note_poll_time(now);
	if (!filter_pad_event(event, now))
		return 0;
	return w->filter(w->userdata, event);
}

void SDL_AddEventWatch(SDL_EventFilter filter, void *userdata) {
	static void (*real)(SDL_EventFilter, void *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_AddEventWatch");
	if (!filter) {
		real(filter, userdata);
		return;
	}
	pthread_mutex_lock(&watch_lock);
	WatchHook *w = NULL;
	for (int i = 0; i < MAX_WATCHES; i++) {
		if (!watch_hooks[i].active) {
			w = &watch_hooks[i];
			w->active = 1;
			w->filter = filter;
			w->userdata = userdata;
			break;
		}
	}
	pthread_mutex_unlock(&watch_lock);
	if (!w) {
		real(filter, userdata);
		return;
	}
	atomic_store(&watch_registered, 1);
	real(wrapped_event_watch, w);
}

void SDL_DelEventWatch(SDL_EventFilter filter, void *userdata) {
	static void (*real)(SDL_EventFilter, void *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_DelEventWatch");
	pthread_mutex_lock(&watch_lock);
	WatchHook *found = NULL;
	for (int i = 0; i < MAX_WATCHES; i++) {
		if (watch_hooks[i].active && watch_hooks[i].filter == filter && watch_hooks[i].userdata == userdata) {
			found = &watch_hooks[i];
			break;
		}
	}
	pthread_mutex_unlock(&watch_lock);
	if (found) {
		real(wrapped_event_watch, found);
		pthread_mutex_lock(&watch_lock);
		found->active = 0;
		found->filter = NULL;
		found->userdata = NULL;
		pthread_mutex_unlock(&watch_lock);
	} else {
		real(filter, userdata);
	}
}

int SDL_PollEvent(SDL_Event *e) {
	static int (*real)(SDL_Event *);
	static int asked;
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_PollEvent");
	start_watcher();
	Uint32 now = SDL_GetTicks();
	note_poll_time(now);

	if (atomic_load(&quit) && !asked) {
		asked = 1;
		if (e) {
			memset(e, 0, sizeof(*e));
			e->type = SDL_QUIT;
			e->quit.timestamp = now;
		}
		return 1;
	}

	for (;;) {
		int got = real(e);
		if (!got || !e)
			return got;
		/* If an event watch already filtered/translated this event, avoid running state twice. */
		if (!atomic_load(&watch_registered)) {
			if (!filter_pad_event(e, SDL_GetTicks()))
				continue;
		}
		return got;
	}
}

/* Draws the bottom volume bar via GLES scissor + clear, restoring all GL state before swap. */
static void draw_bar_gl(SDL_Window *window) {
	int w = 0, h = 0;
	SDL_GL_GetDrawableSize(window, &w, &h);
	if (w <= 0 || h <= 0)
		SDL_GetWindowSize(window, &w, &h);
	if (w <= 0 || h <= 0)
		return;

	GLint prev_fbo = 0;
	GLboolean prev_scissor = glIsEnabled(GL_SCISSOR_TEST);
	GLint prev_box[4] = { 0, 0, 0, 0 };
	GLfloat prev_clear[4] = { 0.0f, 0.0f, 0.0f, 0.0f };
	GLboolean prev_mask[4] = { GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE };

	glGetIntegerv(GL_FRAMEBUFFER_BINDING, &prev_fbo);
	glGetIntegerv(GL_SCISSOR_BOX, prev_box);
	glGetFloatv(GL_COLOR_CLEAR_VALUE, prev_clear);
	glGetBooleanv(GL_COLOR_WRITEMASK, prev_mask);

	if (prev_fbo != 0)
		glBindFramebuffer(GL_FRAMEBUFFER, 0);
	glColorMask(GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE);
	glEnable(GL_SCISSOR_TEST);

	int pad = h / 40 > 2 ? h / 40 : 2;
	int bx = w / 8;
	int by = pad * 2;
	int bw = w * 3 / 4;
	int bh = pad * 3;
	glScissor(bx, by, bw, bh);
	glClearColor(0.08f, 0.08f, 0.08f, 1.0f);
	glClear(GL_COLOR_BUFFER_BIT);

	int l = atomic_load(&level);
	int fw = (bw - 2 * pad) * l / 100;
	if (fw > 0) {
		glScissor(bx + pad, by + pad, fw, pad);
		glClearColor(0.95f, 0.95f, 0.95f, 1.0f);
		glClear(GL_COLOR_BUFFER_BIT);
	}

	glColorMask(prev_mask[0], prev_mask[1], prev_mask[2], prev_mask[3]);
	glClearColor(prev_clear[0], prev_clear[1], prev_clear[2], prev_clear[3]);
	glScissor(prev_box[0], prev_box[1], prev_box[2], prev_box[3]);
	if (!prev_scissor)
		glDisable(GL_SCISSOR_TEST);
	if (prev_fbo != 0)
		glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)prev_fbo);
}

void SDL_GL_SwapWindow(SDL_Window *window) {
	static void (*real)(SDL_Window *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_GL_SwapWindow");
	if (atomic_load(&showing)) {
		if ((Sint32)(SDL_GetTicks() - atomic_load(&shown_at)) < SHOW_MS)
			draw_bar_gl(window);
		else
			atomic_store(&showing, 0);
	}
	real(window);
}
