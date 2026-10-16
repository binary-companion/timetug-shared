/*
 * timetug_engine.h - the C interface of the TimeTug engine (protocol v1).
 *
 * All strings are NUL-terminated UTF-8 JSON. The engine copies what it keeps from tt_engine_send and
 * tt_engine_reply before they return; a string passed to on_message is valid only during that call.
 *
 * - tt_engine_create returns NULL (and makes no callback) for an invalid config. The engine does not start
 *   until the host sends {"v":1,"type":"start"}; the first callback is then "ready".
 * - tt_engine_send and tt_engine_reply never block and may be called from any thread, including from inside
 *   on_message.
 * - Callbacks are serialized and ordered, on an engine thread. States coalesce (the newest wins); events and
 *   service requests are never dropped or reordered.
 * - tt_engine_destroy fails pending service requests with "cancelled". Called outside a callback it returns
 *   only when no callback runs, and none runs after it. Called inside a callback it returns at once and no
 *   further callback starts.
 * - NULL, destroyed and unknown handles are ignored by every function, as are replies for unknown or already
 *   answered request ids.
 */
#ifndef TIMETUG_ENGINE_H
#define TIMETUG_ENGINE_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stdint.h>

typedef struct tt_engine tt_engine;
typedef void (*tt_message_fn)(void *context, const char *message_json);

#if defined(_WIN32)
#define TT_EXPORT __declspec(dllimport)
#else
#define TT_EXPORT __attribute__((visibility("default")))
#endif

TT_EXPORT tt_engine *tt_engine_create(const char *config_json, tt_message_fn on_message, void *context);
TT_EXPORT void tt_engine_send(tt_engine *engine, const char *command_json);
TT_EXPORT void tt_engine_reply(tt_engine *engine, uint64_t request_id, const char *reply_json);
TT_EXPORT void tt_engine_destroy(tt_engine *engine);

#ifdef __cplusplus
}
#endif

#endif /* TIMETUG_ENGINE_H */
