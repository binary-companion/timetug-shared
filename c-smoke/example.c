/*
 * The example in docs/api.md, part "C interface": create an engine, start it, read `ready` and `state`, answer a
 * host-service request, send a command, and shut down. Built and run by c-smoke/run.sh (macOS and Linux) against
 * include/timetug_engine.h and the TimeTugEngineC library only.
 * Usage: example <data-directory>
 *
 * The callback runs on an engine thread, so everything it touches is atomic. A real host parses the JSON with a JSON
 * library; this example only looks for substrings so that it needs nothing else.
 */
#define _POSIX_C_SOURCE 200809L

#include <inttypes.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "timetug_engine.h"

typedef struct {
    tt_engine *volatile engine; /* set after tt_engine_create returns; the callback may run before that */
    atomic_int ready;
    atomic_int states;
    atomic_int diagnostics;
} host;

static void on_message(void *context, const char *json) {
    host *h = context;
    /* `json` is valid only during this call: copy anything you keep. */
    if (strstr(json, "\"type\":\"ready\"")) atomic_store(&h->ready, 1);
    if (strstr(json, "\"type\":\"state\"")) atomic_fetch_add(&h->states, 1);
    if (strstr(json, "\"type\":\"event.diagnostic\"")) atomic_fetch_add(&h->diagnostics, 1);

    /* A service request: {"v":1,"type":"service.xxx","id":<n>,"payload":{...}}. `id` precedes `payload`, so the
     * first "id": is the request id. Every request must be answered exactly once; this host has no services. */
    if (strstr(json, "\"type\":\"service.")) {
        const char *id = strstr(json, "\"id\":");
        if (id && h->engine) {
            uint64_t request_id = strtoull(id + 5, NULL, 10);
            /* Allowed from inside the callback: reply never blocks. */
            tt_engine_reply(h->engine, request_id,
                            "{\"v\":1,\"error\":{\"kind\":\"notFound\",\"message\":\"this host has no services\"}}");
        }
    }
}

static int wait_for(atomic_int *flag, int minimum) {
    struct timespec pause = {0, 25 * 1000 * 1000};
    for (int i = 0; i < 400; i++) { /* at most ten seconds */
        if (atomic_load(flag) >= minimum) return 1;
        nanosleep(&pause, NULL);
    }
    return 0;
}

int main(int argc, char **argv) {
    const char *directory = argc > 1 ? argv[1] : ".";
#if defined(__APPLE__)
    const char *platform = "macos";
#else
    const char *platform = "linux";
#endif
    char config[1400];
    /* Escape the directory properly in real code (backslashes and quotes); the example's path has neither. */
    snprintf(config, sizeof config, "{\"v\":1,\"dataDirectory\":\"%s\",\"platform\":\"%s\"}", directory, platform);

    host h = {0};
    tt_engine *engine = tt_engine_create(config, on_message, &h); /* NULL: the config was rejected */
    if (!engine) { fprintf(stderr, "the engine rejected the configuration\n"); return 1; }
    h.engine = engine;

    tt_engine_send(engine, "{\"v\":1,\"type\":\"start\"}"); /* nothing is delivered before this */
    if (!wait_for(&h.ready, 1) || !wait_for(&h.states, 1)) { fprintf(stderr, "no ready/state\n"); tt_engine_destroy(engine); return 2; }

    int seen = atomic_load(&h.states);
    tt_engine_send(engine, "{\"v\":1,\"type\":\"refresh\",\"id\":\"r1\"}");
    tt_engine_send(engine, "{\"v\":1,\"type\":\"frobnicate\",\"id\":\"bad\"}"); /* an event.diagnostic comes back */
    if (!wait_for(&h.states, seen + 1) || !wait_for(&h.diagnostics, 1)) { fprintf(stderr, "no answer\n"); tt_engine_destroy(engine); return 3; }

    tt_engine_destroy(engine); /* returns when no callback is running; none runs after it */
    printf("example: ok (%d states, %d diagnostics)\n", atomic_load(&h.states), atomic_load(&h.diagnostics));
    return 0;
}
