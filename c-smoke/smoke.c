/*
 * C smoke test of the TimeTug engine's C interface: built with the system C compiler against
 * include/timetug_engine.h and the TimeTugEngineC library only. It creates an engine, waits for
 * "ready" and a "state" after "start", sends "refresh", destroys the engine, checks that no
 * callback runs after tt_engine_destroy returns and that an invalid config returns NULL.
 * Usage: smoke <data-directory>. Exit 0 on success; any other code names the failed check.
 */
#if !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#endif

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
#else
#include <time.h>
#endif

#include "timetug_engine.h"

/* Written on the engine's delivery thread, read on the main thread; volatile is enough for flags polled in a loop. */
static volatile int saw_ready = 0, saw_state = 0, saw_refresh_state = 0, after_destroy = 0, destroyed = 0;
static volatile int states = 0;

static void pause_ms(int ms) {
#ifdef _WIN32
    Sleep((DWORD)ms);
#else
    struct timespec t = {ms / 1000, (long)(ms % 1000) * 1000000L};
    nanosleep(&t, NULL);
#endif
}

static void on_message(void *context, const char *json) {
    (void)context;
    if (destroyed) after_destroy++;
    if (strstr(json, "\"type\":\"ready\"")) saw_ready = 1;
    if (strstr(json, "\"type\":\"state\"")) {
        saw_state = 1;
        states++;
    }
}

int main(int argc, char **argv) {
    const char *dir = argc > 1 ? argv[1] : ".";
    char escaped[1024], config[1400];
    size_t j = 0;
    for (const char *c = dir; *c && j + 2 < sizeof escaped; c++) {
        if (*c == '\\' || *c == '"') escaped[j++] = '\\';
        escaped[j++] = *c;
    }
    escaped[j] = 0;
#if defined(_WIN32)
    const char *platform = "windows";
#elif defined(__APPLE__)
    const char *platform = "macos";
#else
    const char *platform = "linux";
#endif
    snprintf(config, sizeof config, "{\"v\":1,\"dataDirectory\":\"%s\",\"platform\":\"%s\"}", escaped, platform);

    if (tt_engine_create("{bad", on_message, NULL) != NULL) { fprintf(stderr, "invalid config accepted\n"); return 3; }
    tt_engine_send(NULL, "{\"v\":1,\"type\":\"start\"}");
    tt_engine_destroy(NULL);

    tt_engine *e = tt_engine_create(config, on_message, NULL);
    if (!e) { fprintf(stderr, "create failed\n"); return 1; }
    tt_engine_send(e, "{\"v\":1,\"type\":\"start\"}");
    for (int i = 0; i < 400 && !(saw_ready && saw_state); i++) pause_ms(25);
    if (!saw_ready || !saw_state) { fprintf(stderr, "no ready/state\n"); tt_engine_destroy(e); return 2; }

    int before = states;
    tt_engine_send(e, "{\"v\":1,\"type\":\"refresh\"}");
    for (int i = 0; i < 400 && states == before; i++) pause_ms(25);
    saw_refresh_state = states > before;

    tt_engine_destroy(e);
    destroyed = 1;
    tt_engine_destroy(e); /* a second destroy of the same handle is ignored */
    tt_engine_send(e, "{\"v\":1,\"type\":\"refresh\"}");
    pause_ms(200);
    if (!saw_refresh_state) { fprintf(stderr, "no state after refresh\n"); return 5; }
    if (after_destroy != 0) { fprintf(stderr, "%d callback(s) after destroy\n", after_destroy); return 4; }
    printf("c-smoke: ok (%d states)\n", states);
    return 0;
}
