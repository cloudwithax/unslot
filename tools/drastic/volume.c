/*
 * libunslot-drastic.so: slot's volume and menu inside DraStic, which has no volume of its own.
 * Preloaded into DraStic by System/drastic/launch.sh, it scales the sound DraStic's callback
 * hands SDL by the level slot --drastic leaves in UNSLOT_VOLUME, squared as slot scales its own,
 * and draws a bar over the game for a moment each time a volume key is pressed.
 *
 * UNSLOT_VOLUME holds "<level> <presses> <quit>": the level 0 to 100, nothing when muted, a count
 * that moves with every press, so a press at the top or the bottom still shows the bar, and 1
 * once Exit Game was picked in slot's menu, when DraStic is asked to save and close.
 *
 * slot stops DraStic while its menu is up, and the pad's presses in the menu wait for DraStic in
 * the joystick's queue; they are let go of when DraStic goes on, so the game never sees them.
 *
 * This DraStic has no hotkeys, its menu being on one button alone, so X pressed with Select held
 * is turned into a press of a button the pad hasn't, which drastic.cfg puts DraStic's menu on.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#include "SDL.h"

#define SHOW_MS 1500
#define READ_EVERY_US 20000
/* A wait between two looks at the events this long means DraStic was stopped. */
#define STOPPED_MS 250
/* How long after that the pad's queued presses are let go of. */
#define DROP_MS 100
/* The pad's buttons as SDL numbers them, and the one drastic.cfg puts DraStic's menu on. */
#define SELECT 6
#define X 3
#define MENU 31

static atomic_int level = 100;
static atomic_uint shown_at;
static atomic_int showing;
static atomic_int quit;

static SDL_AudioCallback game_callback;
static SDL_AudioFormat game_format;

static void *watch(void *path) {
	unsigned seen = 0;
	int first = 1;
	for (;;) {
		FILE *f = fopen(path, "r");
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

/* DraStic's callback, then its samples scaled by the level squared. */
static void scaled(void *userdata, Uint8 *stream, int len) {
	game_callback(userdata, stream, len);
	int l = atomic_load(&level);
	if (l >= 100)
		return;
	int gain = l * l; /* out of 10000 */
	if (game_format == AUDIO_S16SYS) {
		Sint16 *s = (Sint16 *)stream;
		for (int i = 0; i < len / 2; i++)
			s[i] = (Sint16)(s[i] * gain / 10000);
	} else if (game_format == AUDIO_F32SYS) {
		float *s = (float *)stream;
		for (int i = 0; i < len / 4; i++)
			s[i] *= gain / 10000.0f;
	}
}

int SDL_OpenAudio(SDL_AudioSpec *desired, SDL_AudioSpec *obtained) {
	static int (*real)(SDL_AudioSpec *, SDL_AudioSpec *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_OpenAudio");
	const char *path = getenv("UNSLOT_VOLUME");
	if (!path || !desired->callback)
		return real(desired, obtained);
	game_callback = desired->callback;
	desired->callback = scaled;
	int r = real(desired, obtained);
	/* With nothing obtained, SDL converts to what was asked for. */
	game_format = obtained ? obtained->format : desired->format;
	if (game_format != AUDIO_S16SYS && game_format != AUDIO_F32SYS)
		fprintf(stderr, "unslot-drastic: sound in format %#x is left at full\n", game_format);
	pthread_t t;
	if (pthread_create(&t, NULL, watch, (void *)path) == 0)
		pthread_detach(t);
	return r;
}

/* The bar, along the foot of the screen, as wide as the level. */
static void draw_bar(SDL_Renderer *r) {
	int w, h;
	SDL_RenderGetLogicalSize(r, &w, &h);
	if (w == 0 || h == 0)
		SDL_GetRendererOutputSize(r, &w, &h);
	Uint8 cr, cg, cb, ca;
	SDL_BlendMode blend;
	SDL_GetRenderDrawColor(r, &cr, &cg, &cb, &ca);
	SDL_GetRenderDrawBlendMode(r, &blend);
	SDL_SetRenderDrawBlendMode(r, SDL_BLENDMODE_BLEND);
	int pad = h / 40 > 2 ? h / 40 : 2;
	SDL_Rect back = { w / 8, h - pad * 5, w * 3 / 4, pad * 3 };
	SDL_SetRenderDrawColor(r, 0, 0, 0, 160);
	SDL_RenderFillRect(r, &back);
	int l = atomic_load(&level);
	SDL_Rect fill = { back.x + pad, back.y + pad, (back.w - 2 * pad) * l / 100, pad };
	SDL_SetRenderDrawColor(r, 255, 255, 255, 230);
	if (fill.w > 0)
		SDL_RenderFillRect(r, &fill);
	SDL_SetRenderDrawColor(r, cr, cg, cb, ca);
	SDL_SetRenderDrawBlendMode(r, blend);
}

int SDL_PollEvent(SDL_Event *e) {
	static int (*real)(SDL_Event *);
	static Uint32 last, dropping_until;
	static int asked, select, menu;
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_PollEvent");
	Uint32 now = SDL_GetTicks();
	if (last && now - last > STOPPED_MS)
		dropping_until = now + DROP_MS;
	last = now;
	if (atomic_load(&quit) && !asked) {
		asked = 1;
		e->type = SDL_QUIT;
		e->quit.timestamp = now;
		return 1;
	}
	for (;;) {
		int got = real(e);
		if (!got)
			return got;
		if ((Sint32)(now - dropping_until) < 0) {
			switch (e->type) {
			case SDL_JOYBUTTONDOWN:
			case SDL_JOYBUTTONUP:
			case SDL_JOYAXISMOTION:
			case SDL_JOYHATMOTION:
				continue;
			}
		}
		if (e->type == SDL_JOYBUTTONDOWN || e->type == SDL_JOYBUTTONUP) {
			int down = e->type == SDL_JOYBUTTONDOWN;
			if (e->jbutton.button == SELECT)
				select = down;
			else if (e->jbutton.button == X && (down ? select : menu)) {
				menu = down;
				e->jbutton.button = MENU;
			}
		}
		return got;
	}
}

void SDL_RenderPresent(SDL_Renderer *r) {
	static void (*real)(SDL_Renderer *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_RenderPresent");
	if (atomic_load(&showing)) {
		if (SDL_GetTicks() - atomic_load(&shown_at) < SHOW_MS)
			draw_bar(r);
		else
			atomic_store(&showing, 0);
	}
	real(r);
}
