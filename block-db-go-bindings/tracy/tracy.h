#ifndef __MAIN_H__
#define __MAIN_H__

#include <stdint.h>
#include <string.h>

#ifdef __cplusplus
extern "C"
{
#endif
void GoTracyStartupProfiler();
void GoTracyShutdownProfiler();

void GoTracySetThreadName(const char*name);

int GoTracyZoneBegin(const char*name, const char *function,const char*file, uint32_t line, uint32_t color);
void GoTracyZoneEnd(int c);

#ifdef __cplusplus
}
#endif

#endif // __MAIN_H__
