/*
 * libunslot-pico8.so, and libunslot-pico8-64.so for pico8_64: the owner's PICO-8 as one of slot's
 * cores, preloaded by `slot --pico8`. Built without SDL's headers, PICO-8 bringing its own SDL or
 * using BaseOS's, so the few SDL types it touches are written out here.
 *
 * SDL_Init: PICO-8 asks SDL for every subsystem, and BaseOS's 32-bit SDL was built without
 * haptics and sensors, so SDL_Init fails as a whole and PICO-8 will not start. PICO-8 needs
 * neither, so they are left out of what it asks for.
 *
 * With UNSLOT_BRIDGE set, slot drives PICO-8 as it drives a core of its own (tools/bridge/
 * bridge.h): the picture PICO-8 uploads to its texture goes to slot rather than the screen, its
 * sound callback is slot's to pull, and the pad is slot's, given to PICO-8 as its controller.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

#include "bridge.h"

#define SDL_INIT_HAPTIC 0x00001000u
#define SDL_INIT_SENSOR 0x00008000u
#define UNBUILT (SDL_INIT_HAPTIC | SDL_INIT_SENSOR)

#define AUDIO_S16LSB 0x8010
#define AUDIO_F32LSB 0x8120

/* SDL_QUIT, the keyboard's events, 0x300 to 0x3ff, and the joystick and game controller
 * events, 0x600 to 0x6ff. */
#define SDL_QUIT 0x100
#define SDL_KEY_FIRST 0x300
#define SDL_KEY_LAST 0x3ff
#define SDL_JOY_FIRST 0x600
#define SDL_JOY_LAST 0x6ff
#define SDL_CONTROLLERAXISMOTION 0x650
#define SDL_CONTROLLERBUTTONDOWN 0x651
#define SDL_CONTROLLERBUTTONUP 0x652

/* The texture formats PICO-8 may upload in. */
#define PIXEL_ARGB8888 0x16362004u
#define PIXEL_RGB888 0x16161804u
#define PIXEL_ABGR8888 0x16762004u
#define PIXEL_BGR888 0x16561804u
#define PIXEL_RGBA8888 0x16462004u
#define PIXEL_BGRA8888 0x16862004u
#define PIXEL_RGB565 0x15151002u

typedef void (*audio_cb)(void *, uint8_t *, int);

typedef struct {
	int freq;
	uint16_t format;
	uint8_t channels;
	uint8_t silence;
	uint16_t samples;
	uint16_t padding;
	uint32_t size;
	audio_cb callback;
	void *userdata;
} AudioSpec;

/* SDL_Event is a union of 56 bytes whose every member starts with its type and timestamp. */
typedef union {
	struct {
		uint32_t type;
		uint32_t timestamp;
	} common;
	struct {
		uint32_t type;
		uint32_t timestamp;
		int32_t which;
		uint8_t button;
		uint8_t state;
	} cbutton;
	struct {
		uint32_t type;
		uint32_t timestamp;
		int32_t which;
		uint8_t axis;
		uint8_t padding[3];
		int16_t value;
	} caxis;
	uint8_t padding[56];
} Event;

typedef struct {
	int x, y, w, h;
} Rect;

static audio_cb game_cb;

static int call(const char *name, uint32_t flags) {
	int (*real)(uint32_t) = (int (*)(uint32_t))dlsym(RTLD_NEXT, name);
	if (!real) {
		fprintf(stderr, "unslot-pico8: %s not found\n", name);
		return -1;
	}
	return real(flags & ~UNBUILT);
}

int SDL_Init(uint32_t flags) {
	return call("SDL_Init", flags);
}

int SDL_InitSubSystem(uint32_t flags) {
	return call("SDL_InitSubSystem", flags);
}

static uint32_t ticks(void) {
	struct timespec t;
	clock_gettime(CLOCK_MONOTONIC, &t);
	return (uint32_t)(t.tv_sec * 1000 + t.tv_nsec / 1000000);
}

/* Driven, PICO-8's sound is pulled by slot's bridge thread, held off while PICO-8 locks it. */
static pthread_mutex_t audio_lock;

static void bridged_audio(void *userdata, uint8_t *stream, int len) {
	pthread_mutex_lock(&audio_lock);
	game_cb(userdata, stream, len);
	pthread_mutex_unlock(&audio_lock);
}

void SDL_LockAudio(void) {
	static void (*real)(void);
	if (bridge_on()) {
		pthread_mutex_lock(&audio_lock);
		return;
	}
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_LockAudio");
	if (real)
		real();
}

void SDL_UnlockAudio(void) {
	static void (*real)(void);
	if (bridge_on()) {
		pthread_mutex_unlock(&audio_lock);
		return;
	}
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_UnlockAudio");
	if (real)
		real();
}

void SDL_PauseAudio(int pause) {
	static void (*real)(int);
	if (bridge_on())
		return;
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_PauseAudio");
	if (real)
		real(pause);
}

void SDL_CloseAudio(void) {
	static void (*real)(void);
	if (bridge_on()) {
		bridge_audio(NULL, NULL, 0, 2, 0, 60.0);
		return;
	}
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_CloseAudio");
	if (real)
		real();
}

int SDL_OpenAudio(AudioSpec *desired, AudioSpec *obtained) {
	static int (*real)(AudioSpec *, AudioSpec *);
	if (bridge_on() && desired && desired->callback) {
		int is_float = desired->format == AUDIO_F32LSB;
		if (!is_float && desired->format != AUDIO_S16LSB)
			fprintf(stderr, "unslot-pico8: sound in format %#x is not slot's\n", desired->format);
		game_cb = desired->callback;
		int size = is_float ? 4 : 2;
		desired->size = (uint32_t)desired->samples * desired->channels * size;
		if (obtained)
			*obtained = *desired;
		bridge_audio(bridged_audio, desired->userdata, desired->freq, desired->channels,
				is_float, 60.0);
		return 0;
	}
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_OpenAudio");
	return real ? real(desired, obtained) : -1;
}

/* PICO-8 locks its sound inside its callback as well as around it. */
__attribute__((constructor)) static void start(void) {
	pthread_mutexattr_t recursive;
	pthread_mutexattr_init(&recursive);
	pthread_mutexattr_settype(&recursive, PTHREAD_MUTEX_RECURSIVE);
	pthread_mutex_init(&audio_lock, &recursive);
}

/* The controller PICO-8 opened first, which slot's pad is given to it as. */
static int32_t pad_id = -1;

void *SDL_GameControllerOpen(int index) {
	static void *(*real)(int);
	static void *(*joystick)(void *);
	static int32_t (*instance)(void *);
	if (!real) {
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_GameControllerOpen");
		*(void **)&joystick = dlsym(RTLD_NEXT, "SDL_GameControllerGetJoystick");
		*(void **)&instance = dlsym(RTLD_NEXT, "SDL_JoystickInstanceID");
	}
	void *c = real ? real(index) : NULL;
	if (c && pad_id < 0 && joystick && instance)
		pad_id = instance(joystick(c));
	return c;
}

/* The controller button each of the pad's buttons is, as libretro numbers them: the bottom
 * button is A, which PICO-8 takes for O. */
static const int pad_button[16] = {
	0 /* B: A */, 2 /* Y: X */, 4 /* Select: Back */, 6 /* Start */, 11, 12, 13, 14 /* d-pad */,
	1 /* A: B */, 3 /* X: Y */, 9, 10 /* shoulders */, -1, -1 /* triggers */, 7, 8 /* sticks */,
};

static int pad_down(int button) {
	for (int bit = 0; bit < 16; bit++)
		if (pad_button[bit] == button)
			return (bridge->buttons >> bit) & 1;
	return 0;
}

static int16_t pad_axis(int axis) {
	if (axis < 4)
		return bridge->sticks[axis];
	return (bridge->buttons >> (axis == 4 ? 12 : 13) & 1) ? 32767 : 0;
}

uint8_t SDL_GameControllerGetButton(void *c, int button) {
	static uint8_t (*real)(void *, int);
	if (bridge_on())
		return (uint8_t)pad_down(button);
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_GameControllerGetButton");
	return real ? real(c, button) : 0;
}

int16_t SDL_GameControllerGetAxis(void *c, int axis) {
	static int16_t (*real)(void *, int);
	if (bridge_on())
		return pad_axis(axis);
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_GameControllerGetAxis");
	return real ? real(c, axis) : 0;
}

/* Driven: SDL_QUIT once slot closes PICO-8, then slot's pad, as presses and stick moves of the
 * controller PICO-8 opened, and none of the real pad's or keyboard's. */
static int bridged_event(Event *e, int (*real)(Event *)) {
	static int asked;
	static uint32_t sent;
	static int16_t moved[6];
	uint32_t now = ticks();
	memset(e, 0, sizeof *e);
	if (bridge_quitting() && !asked) {
		asked = 1;
		e->common.type = SDL_QUIT;
		e->common.timestamp = now;
		return 1;
	}
	uint32_t buttons = bridge->buttons;
	for (int bit = 0; bit < 16 && pad_id >= 0; bit++) {
		uint32_t m = 1u << bit;
		if (pad_button[bit] < 0 || (buttons & m) == (sent & m))
			continue;
		sent ^= m;
		int down = (buttons & m) != 0;
		e->cbutton.type = down ? SDL_CONTROLLERBUTTONDOWN : SDL_CONTROLLERBUTTONUP;
		e->cbutton.timestamp = now;
		e->cbutton.which = pad_id;
		e->cbutton.button = (uint8_t)pad_button[bit];
		e->cbutton.state = (uint8_t)down;
		return 1;
	}
	for (int axis = 0; axis < 6 && pad_id >= 0; axis++) {
		int16_t v = pad_axis(axis);
		if (v == moved[axis])
			continue;
		moved[axis] = v;
		e->caxis.type = SDL_CONTROLLERAXISMOTION;
		e->caxis.timestamp = now;
		e->caxis.which = pad_id;
		e->caxis.axis = (uint8_t)axis;
		e->caxis.value = v;
		return 1;
	}
	for (;;) {
		int got = real(e);
		if (!got)
			return 0;
		uint32_t type = e->common.type;
		if ((type >= SDL_JOY_FIRST && type <= SDL_JOY_LAST) ||
				(type >= SDL_KEY_FIRST && type <= SDL_KEY_LAST))
			continue;
		return got;
	}
}

int SDL_PollEvent(Event *e) {
	static int (*real)(Event *);
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_PollEvent");
	if (!real)
		return 0;
	if (bridge_on() && e)
		return bridged_event(e, real);
	return real(e);
}

/* Driven: the picture PICO-8 uploads to its texture, into slot's frame, and the size of the last;
 * SDL_RenderPresent hands it over. */
static int frame_w, frame_h;
static void *locked_texture;
static void *locked_pixels;
static int locked_pitch;
static Rect locked_rect;

static uint32_t xrgb(uint32_t format, const uint8_t *p) {
	uint32_t v = *(const uint32_t *)p;
	switch (format) {
	case PIXEL_ARGB8888:
	case PIXEL_RGB888:
		return v | 0xff000000u;
	case PIXEL_ABGR8888:
	case PIXEL_BGR888:
		return 0xff000000u | (v & 0xff) << 16 | (v & 0xff00) | (v >> 16 & 0xff);
	case PIXEL_RGBA8888:
		return 0xff000000u | v >> 8;
	case PIXEL_BGRA8888:
		return 0xff000000u | (v >> 8 & 0xff) << 16 | (v >> 16 & 0xff) << 8 | v >> 24;
	default: {
		uint16_t c = *(const uint16_t *)p;
		uint32_t r = c >> 11 & 31, g = c >> 5 & 63, b = c & 31;
		return 0xff000000u | (r << 3 | r >> 2) << 16 | (g << 2 | g >> 4) << 8 | (b << 3 | b >> 2);
	}
	}
}

static void capture_pixels(uint32_t format, int w, int h, const void *pixels, int pitch) {
	static int told;
	if (!pixels)
		return;
	int bpp = format == PIXEL_RGB565 ? 2 : 4;
	if (told < 4 && (w != frame_w || h != frame_h)) {
		told++;
		fprintf(stderr, "unslot-pico8: picture %dx%d in format %#x\n", w, h, format);
	}
	if (bpp == 4 && format != PIXEL_ARGB8888 && format != PIXEL_RGB888 &&
			format != PIXEL_ABGR8888 && format != PIXEL_BGR888 &&
			format != PIXEL_RGBA8888 && format != PIXEL_BGRA8888)
		return;
	if (w <= 0 || h <= 0 || (long)w * h > BRIDGE_MAX_PIXELS)
		return;
	uint32_t *out = bridge_frame();
	for (int y = 0; y < h; y++) {
		const uint8_t *row = (const uint8_t *)pixels + (long)y * pitch;
		for (int x = 0; x < w; x++)
			out[y * w + x] = xrgb(format, row + x * bpp);
	}
	frame_w = w;
	frame_h = h;
}

static void capture(void *texture, const Rect *rect, const void *pixels, int pitch) {
	static int (*query)(void *, uint32_t *, int *, int *, int *);
	if (!query)
		*(void **)&query = dlsym(RTLD_NEXT, "SDL_QueryTexture");
	uint32_t format = 0;
	int access, w = 0, h = 0;
	if (!query || query(texture, &format, &access, &w, &h) != 0)
		return;
	if (rect) {
		w = rect->w;
		h = rect->h;
	}
	capture_pixels(format, w, h, pixels, pitch);
}

/* The start of SDL_Surface and of the SDL_PixelFormat it points to. */
typedef struct {
	uint32_t format;
} PixelFormat;

typedef struct {
	uint32_t flags;
	PixelFormat *format;
	int w, h;
	int pitch;
	void *pixels;
} Surface;

int SDL_LockTexture(void *texture, const Rect *rect, void **pixels, int *pitch) {
	static int (*real)(void *, const Rect *, void **, int *);
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_LockTexture");
	if (!real)
		return -1;
	int r = real(texture, rect, pixels, pitch);
	if (r == 0 && bridge_on()) {
		locked_texture = texture;
		locked_pixels = *pixels;
		locked_pitch = *pitch;
		if (rect)
			locked_rect = *rect;
		else
			locked_rect.w = 0;
	}
	return r;
}

void SDL_UnlockTexture(void *texture) {
	static void (*real)(void *);
	if (bridge_on() && texture == locked_texture) {
		capture(texture, locked_rect.w ? &locked_rect : NULL, locked_pixels, locked_pitch);
		locked_texture = NULL;
	}
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_UnlockTexture");
	if (real)
		real(texture);
}

int SDL_RenderClear(void *renderer) {
	static int (*real)(void *);
	if (bridge_on())
		return 0;
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_RenderClear");
	if (!real)
		return -1;
	return real(renderer);
}

int SDL_RenderCopy(void *renderer, void *texture, const void *src, const void *dst) {
	static int (*real)(void *, void *, const void *, const void *);
	if (bridge_on())
		return 0;
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_RenderCopy");
	if (!real)
		return -1;
	return real(renderer, texture, src, dst);
}

int SDL_UpdateTexture(void *texture, const void *rect, const void *pixels, int pitch) {
	static int (*real)(void *, const void *, const void *, int);
	if (bridge_on()) {
		capture(texture, rect, pixels, pitch);
		return 0;
	}
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_UpdateTexture");
	if (!real)
		return -1;
	return real(texture, rect, pixels, pitch);
}

void SDL_RenderPresent(void *renderer) {
	static void (*real)(void *);
	if (bridge_on()) {
		bridge_present(frame_w, frame_h);
		return;
	}
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_RenderPresent");
	if (real)
		real(renderer);
}

int SDL_UpdateWindowSurface(void *window) {
	static int (*real)(void *);
	static Surface *(*surface_of)(void *);
	if (bridge_on()) {
		if (!surface_of)
			*(void **)&surface_of = dlsym(RTLD_NEXT, "SDL_GetWindowSurface");
		Surface *surface = surface_of ? surface_of(window) : NULL;
		if (surface && surface->format)
			capture_pixels(surface->format->format, surface->w, surface->h, surface->pixels,
					surface->pitch);
		bridge_present(frame_w, frame_h);
		return 0;
	}
	if (!real)
		*(void **)&real = dlsym(RTLD_NEXT, "SDL_UpdateWindowSurface");
	if (!real)
		return -1;
	return real(window);
}
