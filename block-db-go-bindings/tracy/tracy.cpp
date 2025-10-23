#include "tracy/TracyC.h"
#include "tracy.h"
#include <map>
#include <string>
#include <stdio.h>
#include <iostream>

typedef struct  ___tracy_source_location_data TracyCZoneLocation;

struct TracyZoneData
{
    TracyCZoneLocation loc;
    TracyCZoneCtx ctx;  
};

std::map<int, TracyZoneData*> TracyCZoneCtxMap;
int TracyCZoneCtxCounter = 0;

TracyZoneData* GetZoneContext(int c)
{
    auto search = TracyCZoneCtxMap.find(c);

    if(search == TracyCZoneCtxMap.end())
    {
        auto data = new TracyZoneData();
        TracyCZoneCtxMap[c] = data;
        return data;
    } 
    else {
        return search->second;
    }
   
}

void DelZoneContext(int c)
{
    auto it = TracyCZoneCtxMap.find(c);
    if(it!=TracyCZoneCtxMap.end())
        TracyCZoneCtxMap.erase(it);
}

int IsZoneContextExist(int c)
{
    auto search = TracyCZoneCtxMap.find(c);

    if(search == TracyCZoneCtxMap.end())
        return 0;
    return 1;
}

void GoTracyStartupProfiler() {
    ___tracy_startup_profiler();
}

void GoTracyShutdownProfiler() {
    ___tracy_shutdown_profiler();
}

void GoTracySetThreadName(const char*name)
{
    ___tracy_set_thread_name(name);
}

int GoTracyZoneBegin(const char*name,const char *function,const char*file, uint32_t line, uint32_t color)
{
    TracyCZoneCtxCounter++;
    TracyZoneData *data = GetZoneContext(TracyCZoneCtxCounter);
    data->ctx = TracyCZoneCtx {};
    data->loc = TracyCZoneLocation {};
    data->loc.name = name;
    data->loc.function = function;
    data->loc.file = file;
    data->loc.line = line;
    data->loc.color = color;
    data->ctx = ___tracy_emit_zone_begin( (___tracy_source_location_data*)&data->loc, 1);
    std::cout << "Zone begin: " << name << " (id " << TracyCZoneCtxCounter << ") with internal id " << data->ctx.id << std::endl;
    return TracyCZoneCtxCounter;
}

void GoTracyZoneEnd(int c){
    if (!IsZoneContextExist(c))
    {
        return;
    }

    TracyZoneData *data = GetZoneContext(c);

    ___tracy_emit_zone_end(data->ctx);

    DelZoneContext(c);
}
