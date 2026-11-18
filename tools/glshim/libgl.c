/*
 * libGL.so.1 for the handhelds, whose firmware has OpenGL ES 2 and no desktop OpenGL at all.
 *
 * Cores built by libretro's builders for a desktop link libGL whether or not they draw a frame
 * with it, and the loader refuses such a core outright on a firmware without that file: Mednafen
 * VB is one, so Virtual Boy games played slot's test pattern instead of the game. Every entry
 * point a core reaches through desktop GL that GLES 2 also has is handed to libGLESv2 here, at
 * its first call; one GLES 2 has not got does nothing, and says so once.
 *
 * The header the core was built against only names the symbols, so nothing of it is needed to
 * build this file: the ones below are the ones Mednafen VB imports.
 */
#include <dlfcn.h>
#include <stdio.h>

typedef unsigned int GLenum;
typedef unsigned int GLuint;
typedef unsigned int GLbitfield;
typedef int GLint;
typedef int GLsizei;
typedef unsigned char GLboolean;
typedef float GLfloat;

static void *gles(void) {
    static void *lib;
    static int tried;
    if (!tried) {
        tried = 1;
        lib = dlopen("libGLESv2.so.2", RTLD_NOW | RTLD_GLOBAL);
        if (!lib) {
            fprintf(stderr, "slot: libGL: no libGLESv2.so.2: %s\n", dlerror());
        }
    }
    return lib;
}

static void *entry(const char *name) {
    void *lib = gles();
    void *fn = lib ? dlsym(lib, name) : 0;
    if (!fn) {
        fprintf(stderr, "slot: libGL: GLES 2 has no %s\n", name);
    }
    return fn;
}

void glBindTexture(GLenum target, GLuint texture) {
    static void *fn;
    if (!fn) fn = entry("glBindTexture");
    if (fn) ((void (*)(GLenum, GLuint))fn)(target, texture);
}

void glClear(GLbitfield mask) {
    static void *fn;
    if (!fn) fn = entry("glClear");
    if (fn) ((void (*)(GLbitfield))fn)(mask);
}

void glClearColor(GLfloat r, GLfloat g, GLfloat b, GLfloat a) {
    static void *fn;
    if (!fn) fn = entry("glClearColor");
    if (fn) ((void (*)(GLfloat, GLfloat, GLfloat, GLfloat))fn)(r, g, b, a);
}

void glColorMask(GLboolean r, GLboolean g, GLboolean b, GLboolean a) {
    static void *fn;
    if (!fn) fn = entry("glColorMask");
    if (fn) ((void (*)(GLboolean, GLboolean, GLboolean, GLboolean))fn)(r, g, b, a);
}

void glDeleteTextures(GLsizei n, const GLuint *names) {
    static void *fn;
    if (!fn) fn = entry("glDeleteTextures");
    if (fn) ((void (*)(GLsizei, const GLuint *))fn)(n, names);
}

void glDisable(GLenum cap) {
    static void *fn;
    if (!fn) fn = entry("glDisable");
    if (fn) ((void (*)(GLenum))fn)(cap);
}

void glDrawArrays(GLenum mode, GLint first, GLsizei count) {
    static void *fn;
    if (!fn) fn = entry("glDrawArrays");
    if (fn) ((void (*)(GLenum, GLint, GLsizei))fn)(mode, first, count);
}

void glGenTextures(GLsizei n, GLuint *names) {
    static void *fn;
    if (!fn) fn = entry("glGenTextures");
    if (fn) ((void (*)(GLsizei, GLuint *))fn)(n, names);
}

void glTexImage2D(GLenum target, GLint level, GLint internal, GLsizei width, GLsizei height,
                  GLint border, GLenum format, GLenum type, const void *pixels) {
    static void *fn;
    if (!fn) fn = entry("glTexImage2D");
    if (fn) {
        ((void (*)(GLenum, GLint, GLint, GLsizei, GLsizei, GLint, GLenum, GLenum, const void *))fn)(
            target, level, internal, width, height, border, format, type, pixels);
    }
}

void glTexParameteri(GLenum target, GLenum name, GLint value) {
    static void *fn;
    if (!fn) fn = entry("glTexParameteri");
    if (fn) ((void (*)(GLenum, GLenum, GLint))fn)(target, name, value);
}

void glTexSubImage2D(GLenum target, GLint level, GLint x, GLint y, GLsizei width, GLsizei height,
                     GLenum format, GLenum type, const void *pixels) {
    static void *fn;
    if (!fn) fn = entry("glTexSubImage2D");
    if (fn) {
        ((void (*)(GLenum, GLint, GLint, GLint, GLsizei, GLsizei, GLenum, GLenum, const void *))fn)(
            target, level, x, y, width, height, format, type, pixels);
    }
}

void glViewport(GLint x, GLint y, GLsizei width, GLsizei height) {
    static void *fn;
    if (!fn) fn = entry("glViewport");
    if (fn) ((void (*)(GLint, GLint, GLsizei, GLsizei))fn)(x, y, width, height);
}
