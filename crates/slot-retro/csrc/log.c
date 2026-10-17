/* A core's log lines: libretro's log callback is printf-like, which Rust cannot take, so the
 * line is formatted here and handed over whole. */
#include <stdarg.h>
#include <stdio.h>

extern void slot_retro_log_line(unsigned level, const char *line);

void slot_retro_log(unsigned level, const char *fmt, ...)
{
	char line[1024];
	va_list ap;
	va_start(ap, fmt);
	vsnprintf(line, sizeof line, fmt, ap);
	va_end(ap);
	slot_retro_log_line(level, line);
}
