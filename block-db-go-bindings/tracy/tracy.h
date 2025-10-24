#pragma once

#include <stdint.h>
#include <string.h>

#ifdef __cplusplus
extern "C"
{
#endif
void Bertha_TracyStartupProfiler();
void Bertha_TracyShutdownProfiler();

int Bertha_TracyZoneBegin(const char*name, const char *function,const char*file, uint32_t line, uint32_t color);
void Bertha_TracyZoneEnd(int c);

#ifdef __cplusplus
}
#endif