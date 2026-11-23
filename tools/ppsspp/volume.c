/*
 * libunslot-ppsspp.so: standalone PPSSPPSDL as one of slot's cores. Preloaded into PPSSPPSDL by
 * System/ppsspp/launch.sh, with UNSLOT_BRIDGE set (see tools/bridge/bridge.h) each frame is read
 * back at the swap and handed to slot instead of shown, the sound callback is slot's to pull, and
 * the pad is slot's, F2 and F4 pressed for a save or load. Select + X is turned into
 * SDL_CONTROLLER_BUTTON_GUIDE, which PPSSPP maps to NKCODE_BACK, its pause menu.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdlib.h>
#include <string.h>

#include <GLES2/gl2.h>
#include "SDL.h"
#include "bridge.h"

/* The audio device PPSSPP is handed when slot pulls its sound, which SDL never opened. */
#define BRIDGE_AUDIO_DEV 0x7fff0000u
/* PPSSPP's frames a second, which slot pulls its sound at. */
#define PSP_FPS 59.94

SDL_AudioDeviceID SDL_OpenAudioDevice(const char *device, int iscapture,
		const SDL_AudioSpec *desired, SDL_AudioSpec *obtained, int allowed_changes) {
	static SDL_AudioDeviceID (*real)(const char *, int, const SDL_AudioSpec *, SDL_AudioSpec *, int);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_OpenAudioDevice");
	if (bridge_on() && !iscapture && desired && desired->callback) {
		bridge_audio(desired->callback, desired->userdata, desired->freq, desired->channels,
				desired->format == AUDIO_F32SYS, PSP_FPS);
		if (obtained)
			*obtained = *desired;
		return BRIDGE_AUDIO_DEV;
	}
	return real(device, iscapture, desired, obtained, allowed_changes);
}

void SDL_PauseAudioDevice(SDL_AudioDeviceID dev, int pause_on) {
	static void (*real)(SDL_AudioDeviceID, int);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_PauseAudioDevice");
	if (dev != BRIDGE_AUDIO_DEV)
		real(dev, pause_on);
}

void SDL_CloseAudioDevice(SDL_AudioDeviceID dev) {
	static void (*real)(SDL_AudioDeviceID);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_CloseAudioDevice");
	if (dev == BRIDGE_AUDIO_DEV) {
		bridge_audio(NULL, NULL, bridge->rate, 2, 0, PSP_FPS);
		return;
	}
	real(dev);
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

static SDL_JoystickID pad_id = -1;

SDL_GameController *SDL_GameControllerOpen(int index) {
	static SDL_GameController *(*real)(int);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_GameControllerOpen");
	SDL_GameController *c = real(index);
	if (c && pad_id < 0)
		pad_id = SDL_JoystickInstanceID(SDL_GameControllerGetJoystick(c));
	return c;
}

/* The pad's buttons as PPSSPP reads a controller's, in libretro's order, B first. */
static const int pad_button[16] = {
	SDL_CONTROLLER_BUTTON_B, SDL_CONTROLLER_BUTTON_Y, SDL_CONTROLLER_BUTTON_BACK,
	SDL_CONTROLLER_BUTTON_START, SDL_CONTROLLER_BUTTON_DPAD_UP, SDL_CONTROLLER_BUTTON_DPAD_DOWN,
	SDL_CONTROLLER_BUTTON_DPAD_LEFT, SDL_CONTROLLER_BUTTON_DPAD_RIGHT, SDL_CONTROLLER_BUTTON_A,
	SDL_CONTROLLER_BUTTON_X, SDL_CONTROLLER_BUTTON_LEFTSHOULDER,
	SDL_CONTROLLER_BUTTON_RIGHTSHOULDER, -1, -1, SDL_CONTROLLER_BUTTON_LEFTSTICK,
	SDL_CONTROLLER_BUTTON_RIGHTSTICK,
};

/* The pad as slot reads it, a change an event, Select + X turned into Guide, and the
 * keys controls.ini puts a save and a load on when slot asks for one; what PPSSPP would read of
 * the pad and the keys itself is thrown away. */
static int bridged_event(SDL_Event *e, int (*real)(SDL_Event *)) {
	static int asked, held, guide;
	static unsigned held_from;
	static uint32_t sent;
	static int16_t sticks[4];
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
			key = op == 's' ? SDLK_F2 : SDLK_F4;
			held = 1;
			held_from = bridge_shown;
			e->type = SDL_KEYDOWN;
			e->key.state = SDL_PRESSED;
			e->key.keysym.sym = key;
			e->key.keysym.scancode = SDL_GetScancodeFromKey(key);
			e->key.timestamp = now;
			return 1;
		}
	} else if (bridge_shown - held_from >= 2) {
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
	for (int bit = 0; bit < 16; bit++) {
		uint32_t m = 1u << bit;
		if (pad_button[bit] < 0 || (buttons & m) == (sent & m))
			continue;
		sent ^= m;
		int down = (buttons & m) != 0;
		int button = pad_button[bit];
		if (button == SDL_CONTROLLER_BUTTON_X && (down ? (sent & 1u << 2) != 0 : guide)) {
			guide = down;
			button = SDL_CONTROLLER_BUTTON_GUIDE;
		}
		e->type = down ? SDL_CONTROLLERBUTTONDOWN : SDL_CONTROLLERBUTTONUP;
		e->cbutton.which = pad_id;
		e->cbutton.button = button;
		e->cbutton.state = down ? SDL_PRESSED : SDL_RELEASED;
		e->cbutton.timestamp = now;
		return 1;
	}
	for (int axis = 0; axis < 4; axis++) {
		int16_t v = bridge->sticks[axis];
		if (v == sticks[axis])
			continue;
		sticks[axis] = v;
		e->type = SDL_CONTROLLERAXISMOTION;
		e->caxis.which = pad_id;
		e->caxis.axis = SDL_CONTROLLER_AXIS_LEFTX + axis;
		e->caxis.value = v;
		e->caxis.timestamp = now;
		return 1;
	}
	for (;;) {
		int got = real(e);
		if (!got)
			return got;
		if (is_pad_event(e->type) || e->type == SDL_KEYDOWN || e->type == SDL_KEYUP)
			continue;
		return got;
	}
}

int SDL_PollEvent(SDL_Event *e) {
	static int (*real)(SDL_Event *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_PollEvent");
	if (bridge_on() && e)
		return bridged_event(e, real);
	return real(e);
}

/* The game's picture, read back from the frame PPSSPP would show and handed to slot instead: the
 * 16:9 the card's ppsspp.ini crops the PSP's screen to, as large as the window fits it. */
static void bridged_swap(SDL_Window *window) {
	static uint8_t *px;
	static size_t cap;
	int w = 0, h = 0;
	SDL_GL_GetDrawableSize(window, &w, &h);
	int cw = w, ch = w * 9 / 16;
	if (ch > h) {
		ch = h;
		cw = h * 16 / 9;
	}
	if (cw <= 0 || ch <= 0 || (long)cw * ch > BRIDGE_MAX_PIXELS || bridge_quitting())
		return;
	size_t len = (size_t)cw * ch * 4;
	if (len > cap) {
		free(px);
		px = malloc(len);
		cap = px ? len : 0;
	}
	if (!px)
		return;
	GLint prev_fbo = 0, prev_pack = 4;
	glGetIntegerv(GL_FRAMEBUFFER_BINDING, &prev_fbo);
	glGetIntegerv(GL_PACK_ALIGNMENT, &prev_pack);
	if (prev_fbo != 0)
		glBindFramebuffer(GL_FRAMEBUFFER, 0);
	glPixelStorei(GL_PACK_ALIGNMENT, 4);
	glReadPixels((w - cw) / 2, (h - ch) / 2, cw, ch, GL_RGBA, GL_UNSIGNED_BYTE, px);
	glPixelStorei(GL_PACK_ALIGNMENT, prev_pack);
	if (prev_fbo != 0)
		glBindFramebuffer(GL_FRAMEBUFFER, (GLuint)prev_fbo);
	uint32_t *f = bridge_frame();
	for (int y = 0; y < ch; y++) {
		const uint8_t *in = px + (size_t)(ch - 1 - y) * cw * 4;
		uint32_t *out = f + (size_t)y * cw;
		for (int x = 0; x < cw; x++, in += 4)
			out[x] = 0xff000000u | (uint32_t)in[0] << 16 | (uint32_t)in[1] << 8 | in[2];
	}
	bridge_present(cw, ch);
}

void SDL_GL_SwapWindow(SDL_Window *window) {
	static void (*real)(SDL_Window *);
	if (!real)
		real = dlsym(RTLD_NEXT, "SDL_GL_SwapWindow");
	if (bridge_on()) {
		bridged_swap(window);
		return;
	}
	real(window);
}
