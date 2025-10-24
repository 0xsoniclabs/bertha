#include "tracy/TracyC.h"

//#include "TracyClient.cpp"  // < import and build client as part of this module
#include "tracy.h"
#include <map>

void Bertha_TracyStartupProfiler() {
    ___tracy_startup_profiler();
}

void Bertha_TracyShutdownProfiler() {
    ___tracy_shutdown_profiler();
}

void Bertha_TracyFrameMark() {
    ___tracy_emit_frame_mark((char*)0);
}

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

int Bertha_TracyZoneBegin(const char*name,const char *function,const char*file, uint32_t line, uint32_t color)
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
    return TracyCZoneCtxCounter;
}

void Bertha_TracyZoneEnd(int c){
    if (!IsZoneContextExist(c))
    {
        return;
    }

    TracyZoneData *data = GetZoneContext(c);

    ___tracy_emit_zone_end(data->ctx);

    DelZoneContext(c);
}
