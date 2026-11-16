/* Host-only renderer regression. Build against a native Nodosumi checkout:
 * cc -I SOURCE/deps/glad/include -I SOURCE/src tools/a133p/nodosumi-gles-smoke.c \
 *    -o /tmp/nodosumi-gles-smoke -lEGL -ldl
 * LIBGL_ALWAYS_SOFTWARE=1 MESA_GLES_VERSION_OVERRIDE=3.0 \
 *    /tmp/nodosumi-gles-smoke SOURCE/build-host/nodosumi_libretro.so
 * No BIOS, game, device or firmware libraries are used.
 */
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <glad/glad.h>
#include <dlfcn.h>
#include <stdio.h>
#include "render/render_backend.h"

#define REQUIRE(x) do { \
  if (!(x)) { \
    fprintf(stderr, "failed: %s (EGL 0x%x)\n", #x, eglGetError()); \
    return 1; \
  } \
} while (0)

static void *get_proc(const char *name) {
  return (void *)eglGetProcAddress(name);
}

int main(int argc, char **argv) {
  REQUIRE(argc == 2);
  EGLDisplay display = eglGetPlatformDisplay(
      EGL_PLATFORM_SURFACELESS_MESA, EGL_DEFAULT_DISPLAY, NULL);
  REQUIRE(display != EGL_NO_DISPLAY && eglInitialize(display, NULL, NULL));
  REQUIRE(eglBindAPI(EGL_OPENGL_ES_API));
  const EGLint attrs[] = {
      EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_ES3_BIT,
      EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_NONE};
  EGLConfig config;
  EGLint count;
  REQUIRE(eglChooseConfig(display, attrs, &config, 1, &count) && count == 1);
  const EGLint context_attrs[] = {EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE};
  EGLContext context = eglCreateContext(display, config, EGL_NO_CONTEXT, context_attrs);
  const EGLint surface_attrs[] = {EGL_WIDTH, 8, EGL_HEIGHT, 8, EGL_NONE};
  EGLSurface surface = eglCreatePbufferSurface(display, config, surface_attrs);
  REQUIRE(context != EGL_NO_CONTEXT && surface != EGL_NO_SURFACE);
  REQUIRE(eglMakeCurrent(display, surface, surface, context));
  void *lib = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
  if (!lib) {
    fprintf(stderr, "%s\n", dlerror());
    return 1;
  }
  int (*load)(GLADloadproc) = dlsym(lib, "gladLoadGLES2Loader");
  struct render_backend *(*create)(int, int, const char *) = dlsym(lib, "r_create");
  void (*destroy)(struct render_backend *) = dlsym(lib, "r_destroy");
  void (*viewport)(struct render_backend *, int, int, int, int) = dlsym(lib, "r_viewport");
  void (*draw)(struct render_backend *, const unsigned char *, int, int, int, int) =
      dlsym(lib, "r_draw_pixels");
  REQUIRE(load && create && destroy && viewport && draw && load(get_proc));
  const unsigned char *(*get_string)(GLenum) = get_proc("glGetString");
  fprintf(stderr, "GL_VERSION=%s\n", get_string(GL_VERSION));

  /* r_create used to call an unloaded desktop/ES3.2 function here. The ES3.0
   * attachment must both initialize and survive a real upload and blit. */
  struct render_backend *renderer = create(8, 8, NULL);
  viewport(renderer, 0, 0, 8, 8);
  const unsigned char pixels[12] = {255,0,0, 255,0,0, 255,0,0, 255,0,0};
  void (*pixel_store)(GLenum, GLint) = get_proc("glPixelStorei");
  pixel_store(GL_UNPACK_ALIGNMENT, 1);
  draw(renderer, pixels, 0, 0, 2, 2);
  unsigned char out[8 * 8 * 4];
  void (*read_pixels)(GLint, GLint, GLsizei, GLsizei, GLenum, GLenum, void *) =
      get_proc("glReadPixels");
  read_pixels(0, 0, 8, 8, GL_RGBA, GL_UNSIGNED_BYTE, out);
  for (unsigned i = 0; i < sizeof(out); i += 4) {
    REQUIRE(out[i] > 250 && out[i + 1] < 5 && out[i + 2] < 5);
  }
  destroy(renderer);
  dlclose(lib);
  eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
  eglDestroySurface(display, surface);
  eglDestroyContext(display, context);
  eglTerminate(display);
  puts("renderer created and 8x8 red blit verified");
  return 0;
}
