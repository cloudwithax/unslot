/*
 * libunslot-drastic.so: DraStic as one of slot's cores. Preloaded into DraStic by
 * System/drastic/launch.sh, with UNSLOT_BRIDGE set it hands slot each frame DraStic draws, its
 * sound and the pad (tools/bridge/bridge.h), so slot's volume and menu are DraStic's too.
 *
 * This DraStic has no hotkeys, its menu being on one button alone, so X pressed with Select held
 * is turned into a press of a button the pad hasn't, which drastic.cfg puts DraStic's menu on.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdlib.h>
#include <string.h>

#include "SDL.h"
#include "bridge.h"

/* The pad's buttons as SDL numbers them, and the one drastic.cfg puts DraStic's menu on. */
#define SELECT 6
#define X 3
#define MENU 31

int SDL_OpenAudio(SDL_AudioSpec *desired, SDL_AudioSpec *obtained) {
	static int (*real)(SDL_AudioSpec *, SDL_AudioSpec *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_OpenAudio");
	if (bridge_on()) {
		/* slot plays the sound, a frame's worth after each frame, from the DS's 59.83. */
		bridge_sound_by_frame = 1;
		bridge_audio((void (*)(void *, uint8_t *, int))desired->callback, desired->userdata,
				desired->freq, desired->channels, SDL_AUDIO_ISFLOAT(desired->format), 59.8261);
		if (obtained)
			*obtained = *desired;
		return 0;
	}
	return real(desired, obtained);
}

void SDL_PauseAudio(int pause_on) {
	static void (*real)(int);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_PauseAudio");
	if (!bridge_on())
		real(pause_on);
}

/* slot paces DraStic a frame at a time when it drives it, so DraStic's own waits go. */
void SDL_Delay(Uint32 ms) {
	static void (*real)(Uint32);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_Delay");
	if (!bridge_on())
		real(ms);
}

static SDL_JoystickID pad_id;

SDL_Joystick *SDL_JoystickOpen(int index) {
	static SDL_Joystick *(*real)(int);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_JoystickOpen");
	SDL_Joystick *j = real(index);
	if (j)
		pad_id = SDL_JoystickInstanceID(j);
	return j;
}

/* The pad's buttons as SDL numbers the H700's, in libretro's order, B first. */
static const int pad_button[16] = { 1, 2, SELECT, 7, -1, -1, -1, -1, 0, X, 4, 5, 9, 10, 11, 12 };

static int pad_hat(uint32_t buttons) {
	return (buttons & 1 << 4 ? SDL_HAT_UP : 0) | (buttons & 1 << 5 ? SDL_HAT_DOWN : 0) |
			(buttons & 1 << 6 ? SDL_HAT_LEFT : 0) | (buttons & 1 << 7 ? SDL_HAT_RIGHT : 0);
}

/* The pad as slot reads it, a change an event, and the keys drastic.cfg puts a save and a load
 * on when slot asks for one; what DraStic would read of the pad itself is thrown away. */
static int bridged_event(SDL_Event *e, int (*real)(SDL_Event *)) {
	static int asked, hat, held, drained;
	static uint32_t sent;
	static SDL_Keycode key;
	Uint32 now = SDL_GetTicks();
	memset(e, 0, sizeof(*e));
	if (bridge_quitting()) {
		if (asked)
			return 0;
		asked = 1;
		e->type = SDL_QUIT;
		e->quit.timestamp = now;
		return 1;
	}
	if (!held) {
		int op = bridge_hotkey();
		if (op) {
			key = op == 's' ? SDLK_0 : SDLK_1;
			held = 1;
			drained = 0;
			e->type = SDL_KEYDOWN;
			e->key.state = SDL_PRESSED;
			e->key.keysym.sym = key;
			e->key.keysym.scancode = SDL_GetScancodeFromKey(key);
			e->key.timestamp = now;
			return 1;
		}
	} else if (drained) {
		held = 0;
		e->type = SDL_KEYUP;
		e->key.state = SDL_RELEASED;
		e->key.keysym.sym = key;
		e->key.keysym.scancode = SDL_GetScancodeFromKey(key);
		e->key.timestamp = now;
		bridge_hotkey_up();
		return 1;
	}
	uint32_t buttons = bridge->buttons;
	int h = pad_hat(buttons);
	if (h != hat) {
		hat = h;
		e->type = SDL_JOYHATMOTION;
		e->jhat.which = pad_id;
		e->jhat.value = h;
		e->jhat.timestamp = now;
		return 1;
	}
	for (int bit = 0; bit < 16; bit++) {
		uint32_t m = 1u << bit;
		if (pad_button[bit] < 0 || (buttons & m) == (sent & m))
			continue;
		sent ^= m;
		int down = (buttons & m) != 0;
		e->type = down ? SDL_JOYBUTTONDOWN : SDL_JOYBUTTONUP;
		e->jbutton.which = pad_id;
		e->jbutton.button = pad_button[bit];
		e->jbutton.state = down ? SDL_PRESSED : SDL_RELEASED;
		e->jbutton.timestamp = now;
		return 2;
	}
	for (;;) {
		int got = real(e);
		if (!got) {
			drained = held;
			return got;
		}
		switch (e->type) {
		case SDL_JOYBUTTONDOWN:
		case SDL_JOYBUTTONUP:
		case SDL_JOYAXISMOTION:
		case SDL_JOYHATMOTION:
		case SDL_KEYDOWN:
		case SDL_KEYUP:
			continue;
		}
		return got;
	}
}

int SDL_PollEvent(SDL_Event *e) {
	static int (*real)(SDL_Event *);
	static int select, menu;
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_PollEvent");
	if (bridge_on()) {
		int got = bridged_event(e, real);
		if (got != 2)
			return got;
		/* A button of slot's: X with Select held opens DraStic's own menu. */
		int down = e->type == SDL_JOYBUTTONDOWN;
		if (e->jbutton.button == SELECT)
			select = down;
		else if (e->jbutton.button == X && (down ? select : menu)) {
			menu = down;
			e->jbutton.button = MENU;
		}
		return 1;
	}
	return real(e);
}

/* DraStic's pictures, as it last left each, in XRGB8888, for slot's frame to be put together
 * from where DraStic draws them: the screens, its menu, its cursor. */
#define TEXTURES 16
static struct picture {
	SDL_Texture *t;
	int w, h, blend;
	Uint32 format;
	void *locked;
	int pitch;
	SDL_Rect at;
	uint32_t *px;
} pictures[TEXTURES];
static Uint8 clear_rgb[3];
/* The part of the frame pictures were copied to since the last one shown, x0, y0, x1, y1. */
static int drawn[4] = { 1 << 30, 1 << 30, 0, 0 };

static struct picture *picture_of(SDL_Texture *t) {
	for (int i = 0; i < TEXTURES; i++)
		if (pictures[i].t == t)
			return &pictures[i];
	return NULL;
}

/* Rows of pixels in `format` into the picture's own, at `at`. */
static void take(struct picture *p, const SDL_Rect *at, const void *pixels, int pitch) {
	SDL_Rect r = at ? *at : (SDL_Rect){ 0, 0, p->w, p->h };
	for (int y = 0; y < r.h && r.y + y < p->h; y++) {
		const Uint8 *row = (const Uint8 *)pixels + (size_t)y * pitch;
		uint32_t *out = p->px + (size_t)(r.y + y) * p->w + r.x;
		int n = r.x + r.w > p->w ? p->w - r.x : r.w;
		switch (p->format) {
		case SDL_PIXELFORMAT_ARGB8888:
		case SDL_PIXELFORMAT_RGB888:
			memcpy(out, row, n * 4);
			break;
		case SDL_PIXELFORMAT_ABGR8888:
		case SDL_PIXELFORMAT_BGR888:
			for (int x = 0; x < n; x++) {
				uint32_t c = ((const uint32_t *)row)[x];
				out[x] = (c & 0xff00ff00) | (c & 0xff) << 16 | (c >> 16 & 0xff);
			}
			break;
		case SDL_PIXELFORMAT_RGB565:
			for (int x = 0; x < n; x++) {
				uint16_t c = ((const uint16_t *)row)[x];
				uint32_t r5 = c >> 11, g6 = c >> 5 & 0x3f, b5 = c & 0x1f;
				out[x] = 0xff000000 | (r5 << 3 | r5 >> 2) << 16 | (g6 << 2 | g6 >> 4) << 8 |
						(b5 << 3 | b5 >> 2);
			}
			break;
		}
	}
}

SDL_Texture *SDL_CreateTexture(SDL_Renderer *r, Uint32 format, int access, int w, int h) {
	static SDL_Texture *(*real)(SDL_Renderer *, Uint32, int, int, int);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_CreateTexture");
	SDL_Texture *t = real(r, format, access, w, h);
	struct picture *p = bridge_on() && t ? picture_of(NULL) : NULL;
	if (p) {
		*p = (struct picture){ .t = t, .w = w, .h = h, .format = format };
		p->px = calloc((size_t)w * h, 4);
		if (!p->px)
			p->t = NULL;
	}
	return t;
}

void SDL_DestroyTexture(SDL_Texture *t) {
	static void (*real)(SDL_Texture *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_DestroyTexture");
	struct picture *p = t ? picture_of(t) : NULL;
	if (p) {
		free(p->px);
		*p = (struct picture){ 0 };
	}
	real(t);
}

int SDL_SetTextureBlendMode(SDL_Texture *t, SDL_BlendMode mode) {
	static int (*real)(SDL_Texture *, SDL_BlendMode);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_SetTextureBlendMode");
	struct picture *p = t ? picture_of(t) : NULL;
	if (p)
		p->blend = mode == SDL_BLENDMODE_BLEND;
	return real(t, mode);
}

int SDL_LockTexture(SDL_Texture *t, const SDL_Rect *at, void **pixels, int *pitch) {
	static int (*real)(SDL_Texture *, const SDL_Rect *, void **, int *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_LockTexture");
	int r = real(t, at, pixels, pitch);
	struct picture *p = r == 0 ? picture_of(t) : NULL;
	if (p) {
		p->locked = *pixels;
		p->pitch = *pitch;
		p->at = at ? *at : (SDL_Rect){ 0, 0, p->w, p->h };
	}
	return r;
}

void SDL_UnlockTexture(SDL_Texture *t) {
	static void (*real)(SDL_Texture *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_UnlockTexture");
	struct picture *p = picture_of(t);
	if (p && p->locked) {
		take(p, &p->at, p->locked, p->pitch);
		p->locked = NULL;
	}
	real(t);
}

int SDL_UpdateTexture(SDL_Texture *t, const SDL_Rect *at, const void *pixels, int pitch) {
	static int (*real)(SDL_Texture *, const SDL_Rect *, const void *, int);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_UpdateTexture");
	struct picture *p = picture_of(t);
	if (p)
		take(p, at, pixels, pitch);
	return real(t, at, pixels, pitch);
}

/* The size of the frame DraStic draws: what it set the renderer's logical size to. */
static void frame_size(SDL_Renderer *r, int *w, int *h) {
	*w = *h = 0;
	SDL_RenderGetLogicalSize(r, w, h);
	if (*w == 0 || *h == 0)
		SDL_GetRendererOutputSize(r, w, h);
	if ((long)*w * *h > BRIDGE_MAX_PIXELS)
		*w = *h = 0;
}

int SDL_SetRenderDrawColor(SDL_Renderer *r, Uint8 red, Uint8 green, Uint8 blue, Uint8 a) {
	static int (*real)(SDL_Renderer *, Uint8, Uint8, Uint8, Uint8);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_SetRenderDrawColor");
	clear_rgb[0] = red;
	clear_rgb[1] = green;
	clear_rgb[2] = blue;
	return real(r, red, green, blue, a);
}

int SDL_RenderClear(SDL_Renderer *r) {
	static int (*real)(SDL_Renderer *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_RenderClear");
	if (!bridge_on())
		return real(r);
	int w, h;
	frame_size(r, &w, &h);
	uint32_t c = 0xff000000 | clear_rgb[0] << 16 | clear_rgb[1] << 8 | clear_rgb[2];
	uint32_t *f = bridge_frame();
	for (long i = 0; i < (long)w * h; i++)
		f[i] = c;
	return 0;
}

/* The picture, or the part of it `src` says, into slot's frame at `dst`, scaled as SDL would. */
int SDL_RenderCopy(SDL_Renderer *r, SDL_Texture *t, const SDL_Rect *src, const SDL_Rect *dst) {
	static int (*real)(SDL_Renderer *, SDL_Texture *, const SDL_Rect *, const SDL_Rect *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_RenderCopy");
	if (!bridge_on())
		return real(r, t, src, dst);
	struct picture *p = picture_of(t);
	int w, h;
	frame_size(r, &w, &h);
	if (!p || w == 0)
		return 0;
	SDL_Rect s = src ? *src : (SDL_Rect){ 0, 0, p->w, p->h };
	SDL_Rect d = dst ? *dst : (SDL_Rect){ 0, 0, w, h };
	if (s.w <= 0 || s.h <= 0 || d.w <= 0 || d.h <= 0)
		return 0;
	uint32_t *f = bridge_frame();
	int x0 = d.x < 0 ? 0 : d.x, y0 = d.y < 0 ? 0 : d.y;
	int x1 = d.x + d.w > w ? w : d.x + d.w, y1 = d.y + d.h > h ? h : d.y + d.h;
	if (x0 < x1 && y0 < y1) {
		drawn[0] = x0 < drawn[0] ? x0 : drawn[0];
		drawn[1] = y0 < drawn[1] ? y0 : drawn[1];
		drawn[2] = x1 > drawn[2] ? x1 : drawn[2];
		drawn[3] = y1 > drawn[3] ? y1 : drawn[3];
	}
	for (int y = d.y < 0 ? -d.y : 0; y < d.h && d.y + y < h; y++) {
		int sy = s.y + (int)((long)y * s.h / d.h);
		if (sy < 0 || sy >= p->h)
			continue;
		const uint32_t *in = p->px + (size_t)sy * p->w;
		uint32_t *out = f + (size_t)(d.y + y) * w;
		for (int x = d.x < 0 ? -d.x : 0; x < d.w && d.x + x < w; x++) {
			int sx = s.x + (int)((long)x * s.w / d.w);
			if (sx < 0 || sx >= p->w)
				continue;
			uint32_t c = in[sx], a = c >> 24;
			if (p->blend && a < 255) {
				uint32_t b = out[d.x + x], rb, g;
				rb = ((c & 0xff00ff) * a + (b & 0xff00ff) * (255 - a)) >> 8 & 0xff00ff;
				g = ((c & 0xff00) * a + (b & 0xff00) * (255 - a)) >> 8 & 0xff00;
				c = rb | g;
			}
			out[d.x + x] = c | 0xff000000;
		}
	}
	return 0;
}

void SDL_RenderPresent(SDL_Renderer *r) {
	static void (*real)(SDL_Renderer *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_RenderPresent");
	if (bridge_on()) {
		int w, h;
		frame_size(r, &w, &h);
		/* slot is given only what DraStic drew its screens to, not the window's bars round
		 * them, so it shows the screens as large as they go; a frame with nothing drawn to it
		 * keeps the last one's. */
		static int kept_w, kept_h;
		if (drawn[0] < drawn[2] && drawn[1] < drawn[3] && drawn[2] <= w && drawn[3] <= h) {
			kept_w = drawn[2] - drawn[0];
			kept_h = drawn[3] - drawn[1];
			uint32_t *f = bridge_frame();
			for (int y = 0; y < kept_h; y++)
				memmove(f + (size_t)y * kept_w, f + (size_t)(drawn[1] + y) * w + drawn[0],
						(size_t)kept_w * sizeof *f);
		}
		if (kept_w && kept_w <= w && kept_h <= h) {
			w = kept_w;
			h = kept_h;
		}
		drawn[0] = drawn[1] = 1 << 30;
		drawn[2] = drawn[3] = 0;
		bridge_present(w, h);
		return;
	}
	real(r);
}
