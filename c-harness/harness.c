/*
 * Concurrency harness for the C interface of the TimeTug engine (include/timetug_engine.h). It is built with the
 * system C compiler against the header and the library only, so the same program proves the Swift library (while it
 * exists) and the Rust library keep the documented contract: ordered, serialized callbacks, coalescing states, events
 * never dropped, send and reply from any thread (also from inside a callback), destroy from outside and from inside
 * a callback, stale and NULL handles, and many engines at once.
 *
 * Usage: harness <scratch-directory> [--iterations N] [--case NAME]
 *   <scratch-directory> exists and is writable; every engine gets its own data directory below it.
 *   Each case runs N times (default 200). Exit 0 when every case passed; otherwise the first failed assertion
 *   prints "FAIL <case> (iteration <i>): <what>" to stderr and the process exits with 1 at once.
 *
 * Determinism: no case asserts how fast anything is. Waits are loops that end when an event is seen (a "sync" marker
 * command whose takeoverDue event comes back after everything sent before it), with a 25 second limit, and each
 * iteration of each case runs under a 30 second watchdog that fails the run instead of hanging it. The only fixed
 * pauses are the windows in which something must NOT happen (a callback after destroy), which cannot make a correct
 * engine fail on a slow runner. The callback delays the cases impose (sleep on a state, sleep on every tenth
 * callback) only slow the host down; the assertions are about counts and order.
 *
 * Every Host is allocated once and never freed, so a callback that wrongly runs after destroy cannot touch freed
 * memory: it is counted and fails the case instead of crashing the harness.
 */
#if !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#endif

#include <stdarg.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
#else
#include <pthread.h>
#include <sched.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#endif

#include "timetug_engine.h"

/* ---------------------------------------------------------------- platform shim */

typedef void (*thread_fn)(void *);

#ifdef _WIN32
typedef HANDLE thread_t;
typedef struct {
    thread_fn fn;
    void *arg;
} thread_start_t;

static DWORD WINAPI thread_main(LPVOID raw) {
    thread_start_t start = *(thread_start_t *)raw;
    free(raw);
    start.fn(start.arg);
    return 0;
}
static int thread_spawn(thread_t *t, thread_fn fn, void *arg) {
    thread_start_t *start = malloc(sizeof *start);
    if (!start) return -1;
    start->fn = fn;
    start->arg = arg;
    *t = CreateThread(NULL, 0, thread_main, start, 0, NULL);
    if (!*t) {
        free(start);
        return -1;
    }
    return 0;
}
static void thread_join(thread_t t) {
    WaitForSingleObject(t, INFINITE);
    CloseHandle(t);
}
static void sleep_ms(unsigned ms) { Sleep(ms); }
static void thread_yield(void) { SwitchToThread(); }
static int64_t now_ms(void) { return (int64_t)GetTickCount64(); }
static void make_directory(const char *path) { CreateDirectoryA(path, NULL); }
#define PATH_SEPARATOR "\\"
#else
typedef pthread_t thread_t;
typedef struct {
    thread_fn fn;
    void *arg;
} thread_start_t;

static void *thread_main(void *raw) {
    thread_start_t start = *(thread_start_t *)raw;
    free(raw);
    start.fn(start.arg);
    return NULL;
}
static int thread_spawn(thread_t *t, thread_fn fn, void *arg) {
    thread_start_t *start = malloc(sizeof *start);
    if (!start) return -1;
    start->fn = fn;
    start->arg = arg;
    if (pthread_create(t, NULL, thread_main, start) != 0) {
        free(start);
        return -1;
    }
    return 0;
}
static void thread_join(thread_t t) { pthread_join(t, NULL); }
static void sleep_ms(unsigned ms) {
    struct timespec t = {(time_t)(ms / 1000), (long)(ms % 1000) * 1000000L};
    nanosleep(&t, NULL);
}
static void thread_yield(void) { sched_yield(); }
static int64_t now_ms(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (int64_t)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}
static void make_directory(const char *path) { mkdir(path, 0777); }
#define PATH_SEPARATOR "/"
#endif

#if defined(_WIN32)
#define PLATFORM "windows"
#elif defined(__APPLE__)
#define PLATFORM "macos"
#else
#define PLATFORM "linux"
#endif

/* ---------------------------------------------------------------- failure and watchdog */

#define ITERATION_LIMIT_MS 30000 /* the watchdog: one iteration of one case */
#define WAIT_LIMIT_MS 25000      /* a single wait inside an iteration, shorter than the watchdog */

static const char *volatile g_case = "(startup)";
static atomic_int g_iteration;
static atomic_llong g_deadline; /* 0 while no iteration runs */

static void fail(const char *format, ...) {
    va_list args;
    fprintf(stderr, "FAIL %s (iteration %d): ", g_case, atomic_load(&g_iteration));
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
    fputc('\n', stderr);
    fflush(NULL);
    _exit(1);
}

#define CHECK(condition, ...)                  \
    do {                                       \
        if (!(condition)) fail(__VA_ARGS__);   \
    } while (0)

#define WAIT_FOR(condition, what)                                                      \
    do {                                                                               \
        int64_t wait_start_ = now_ms();                                                \
        while (!(condition)) {                                                         \
            if (now_ms() - wait_start_ > WAIT_LIMIT_MS) fail("timed out waiting for %s", what); \
            sleep_ms(1);                                                               \
        }                                                                              \
    } while (0)

static void watchdog_main(void *unused) {
    (void)unused;
    for (;;) {
        sleep_ms(100);
        int64_t deadline = atomic_load(&g_deadline);
        if (deadline != 0 && now_ms() > deadline) fail("watchdog: the iteration did not finish in 30 seconds");
    }
}

/* ---------------------------------------------------------------- tiny JSON reading */

/* Finds `key` among the members of the object at nesting level `want`, scanning from `p` at nesting `depth`. A whole
 * message is searched with depth 0 and want 1 (its top-level members); the pointer this function returns for an
 * object-valued member is searched again with depth 0 and want 1 (that object's members). Returns a pointer to the value (after the colon and white space), or NULL. A string is skipped
 * as a whole, so a key-like text inside a string never matches. Enough for the engine's own output, which is valid
 * JSON; the key order differs between the Swift and the Rust encoder, so no position is assumed. */
static const char *find_key(const char *p, int depth, int want, const char *key) {
    size_t key_length = strlen(key);
    while (*p) {
        if (*p == '{' || *p == '[') {
            depth++;
        } else if (*p == '}' || *p == ']') {
            depth--;
            if (depth < want) return NULL;
        } else if (*p == '"') {
            const char *start = ++p;
            while (*p && *p != '"') p += (*p == '\\' && p[1]) ? 2 : 1;
            if (!*p) return NULL;
            const char *end = p; /* closing quote */
            const char *after = end + 1;
            while (*after == ' ' || *after == '\n' || *after == '\t' || *after == '\r') after++;
            if (*after == ':' && depth == want && (size_t)(end - start) == key_length &&
                memcmp(start, key, key_length) == 0) {
                after++;
                while (*after == ' ' || *after == '\n' || *after == '\t' || *after == '\r') after++;
                return after;
            }
        }
        p++;
    }
    return NULL;
}

/* True when the string value at `value` starts with `prefix`. */
static int string_value_starts_with(const char *value, const char *prefix) {
    return value && value[0] == '"' && strncmp(value + 1, prefix, strlen(prefix)) == 0;
}

/* The message's "type" member, copied (truncated) into `out`. */
static void message_type(const char *json, char *out, size_t size) {
    const char *value = find_key(json, 0, 1, "type");
    size_t i = 0;
    if (value && value[0] == '"') {
        for (value++; *value && *value != '"' && i + 1 < size; value++) out[i++] = *value;
    }
    out[i] = 0;
}

/* ---------------------------------------------------------------- the host side of one engine */

#define RING_SIZE 256

typedef struct Host {
    const char *name;
    _Atomic(tt_engine *) engine;

    /* what the callback saw */
    atomic_int in_callback;
    atomic_int overlaps;       /* callbacks that began while another was running */
    atomic_llong callbacks;    /* all callbacks (the number of the newest one that began) */
    atomic_llong readys, states, dues, diagnostics, requests;
    atomic_ullong last_revision, max_revision;
    atomic_int revision_errors; /* a state whose revision was not above the previous one */
    atomic_int working;         /* the newest state's accountStatus.isWorking: -1 unknown, 0 false, 1 true */
    atomic_llong sync_seen;
    atomic_llong sync_sent;

    /* knobs, set before start or while the engine runs */
    atomic_int state_sleep_ms;  /* sleep this long in every state callback */
    atomic_int every_n_sleep;   /* with sleep_ms: sleep in every n-th callback (0 = never) */
    atomic_int every_sleep_ms;
    atomic_int spin;            /* busy work per callback, widens the window of overlapping callbacks */
    atomic_int send_in_callback;/* every callback sends refresh while fewer than this many were sent */
    atomic_llong callback_sends;
    atomic_int destroy_in_first_state;
    atomic_int answer_requests; /* 1: a helper thread replies to every service request */

    /* destroy bookkeeping */
    atomic_int destroy_returned;     /* set by the host right after tt_engine_destroy returned outside a callback */
    atomic_llong destroy_callback;   /* number of the callback that called destroy, 0 = none */
    atomic_int destroy_in_callback_returned;
    atomic_llong late_callbacks;     /* callbacks that began or ended after destroy returned / was called in a callback */

    /* requests for the answering helper (single producer: the callback thread; single consumer: the helper) */
    uint64_t ring[RING_SIZE];
    char ring_begins_redirect[RING_SIZE]; /* the request is service.auth.beginRedirect */
    atomic_llong ring_head, ring_tail;
    atomic_llong replied;
    atomic_int helper_stop;
} Host;

static Host *new_host(const char *name) {
    Host *h = calloc(1, sizeof *h); /* never freed, see the file comment */
    if (!h) fail("out of memory");
    h->name = name;
    atomic_init(&h->working, -1);
    return h;
}

static void busy_work(int rounds) {
    volatile unsigned sink = 0;
    for (int i = 0; i < rounds; i++) sink += (unsigned)i;
    (void)sink;
}

static void on_message(void *context, const char *json) {
    Host *h = context;
    if (atomic_exchange(&h->in_callback, 1) != 0) atomic_fetch_add(&h->overlaps, 1);
    long long number = atomic_fetch_add(&h->callbacks, 1) + 1;
    long long destroyed_at = atomic_load(&h->destroy_callback);
    if (atomic_load(&h->destroy_returned) || (destroyed_at != 0 && number > destroyed_at)) {
        atomic_fetch_add(&h->late_callbacks, 1);
    }

    char type[64];
    message_type(json, type, sizeof type);
    int is_state = strcmp(type, "state") == 0;

    if (strcmp(type, "ready") == 0) {
        atomic_fetch_add(&h->readys, 1);
    } else if (is_state) {
        atomic_fetch_add(&h->states, 1);
        const char *payload = find_key(json, 0, 1, "payload");
        const char *revision = payload ? find_key(payload, 0, 1, "revision") : NULL;
        if (revision) {
            unsigned long long value = strtoull(revision, NULL, 10);
            if (value <= atomic_load(&h->last_revision) && atomic_load(&h->states) > 1) {
                atomic_fetch_add(&h->revision_errors, 1);
            }
            atomic_store(&h->last_revision, value);
            if (value > atomic_load(&h->max_revision)) atomic_store(&h->max_revision, value);
        } else {
            atomic_fetch_add(&h->revision_errors, 1);
        }
        const char *status = payload ? find_key(payload, 0, 1, "accountStatus") : NULL;
        const char *working = status ? find_key(status, 0, 1, "isWorking") : NULL;
        if (working) atomic_store(&h->working, strncmp(working, "true", 4) == 0 ? 1 : 0);
    } else if (strcmp(type, "event.takeoverDue") == 0) {
        const char *payload = find_key(json, 0, 1, "payload");
        const char *event = payload ? find_key(payload, 0, 1, "event") : NULL;
        const char *title = event ? find_key(event, 0, 1, "title") : NULL;
        if (string_value_starts_with(title, "sync-")) {
            atomic_store(&h->sync_seen, strtoll(title + 6, NULL, 10));
        } else {
            atomic_fetch_add(&h->dues, 1);
        }
    } else if (strcmp(type, "event.diagnostic") == 0) {
        atomic_fetch_add(&h->diagnostics, 1);
    } else if (strncmp(type, "service.", 8) == 0) {
        const char *id = find_key(json, 0, 1, "id");
        atomic_fetch_add(&h->requests, 1);
        if (id && atomic_load(&h->answer_requests)) {
            long long head = atomic_load(&h->ring_head);
            if (head - atomic_load(&h->ring_tail) < RING_SIZE) {
                h->ring[head % RING_SIZE] = strtoull(id, NULL, 10);
                h->ring_begins_redirect[head % RING_SIZE] = strcmp(type, "service.auth.beginRedirect") == 0;
                atomic_store(&h->ring_head, head + 1);
            }
        }
    }

    int spin = atomic_load(&h->spin);
    if (spin) busy_work(spin);
    if (is_state && atomic_load(&h->state_sleep_ms) > 0) sleep_ms((unsigned)atomic_load(&h->state_sleep_ms));
    int every = atomic_load(&h->every_n_sleep);
    if (every > 0 && number % every == 0) sleep_ms((unsigned)atomic_load(&h->every_sleep_ms));

    int limit = atomic_load(&h->send_in_callback);
    if (limit > 0 && atomic_fetch_add(&h->callback_sends, 1) < limit) {
        tt_engine_send(atomic_load(&h->engine), "{\"v\":1,\"type\":\"refresh\"}");
    }

    if (is_state && atomic_load(&h->destroy_in_first_state)) {
        int expected = 1;
        if (atomic_compare_exchange_strong(&h->destroy_in_first_state, &expected, 2)) {
            atomic_store(&h->destroy_callback, number);
            tt_engine_destroy(atomic_load(&h->engine));
            atomic_store(&h->destroy_in_callback_returned, 1);
        }
    }

    if (atomic_load(&h->destroy_returned)) atomic_fetch_add(&h->late_callbacks, 1);
    atomic_store(&h->in_callback, 0);
}

/* ---------------------------------------------------------------- helpers for the cases */

static const char *g_scratch;

#define START_COMMAND "{\"v\":1,\"type\":\"start\"}"
#define REFRESH_COMMAND "{\"v\":1,\"type\":\"refresh\"}"
#define TAKEOVER_COMMAND "{\"v\":1,\"type\":\"takeover.test\",\"payload\":{\"title\":\"harness\",\"calendarTitle\":\"Harness\"}}"
#define ERROR_REPLY "{\"v\":1,\"error\":{\"kind\":\"failed\",\"message\":\"harness\"}}"

enum { WITH_GOOGLE = 1, WITH_BROKERED = 2 };

/* The data directory of one engine: unique per case, iteration and index. */
static void data_directory(char *out, size_t size, int index) {
    snprintf(out, size, "%s" PATH_SEPARATOR "%s-%d-%d", g_scratch, g_case, atomic_load(&g_iteration), index);
}

static void json_escape(const char *text, char *out, size_t size) {
    size_t j = 0;
    for (; *text && j + 2 < size; text++) {
        if (*text == '\\' || *text == '"') out[j++] = '\\';
        out[j++] = *text;
    }
    out[j] = 0;
}

/* Creates (does not start) an engine for `h` on a fresh data directory. */
static void create_engine(Host *h, int index, int options) {
    char dir[512], escaped[1100], config[2048];
    data_directory(dir, sizeof dir, index);
    json_escape(dir, escaped, sizeof escaped);
    int length = snprintf(config, sizeof config,
        "{\"v\":1,\"dataDirectory\":\"%s\",\"platform\":\"" PLATFORM "\",\"testing\":true,"
        "\"testClockStart\":\"2026-10-16T13:00:00.000Z\"", escaped);
    if (options & WITH_BROKERED) length += snprintf(config + length, sizeof config - (size_t)length, ",\"externalTokens\":true");
    if (options) {
        length += snprintf(config + length, sizeof config - (size_t)length, ",\"connectors\":{");
        const char *separator = "";
        if (options & WITH_GOOGLE) {
            length += snprintf(config + length, sizeof config - (size_t)length,
                "\"google\":{\"clientID\":\"harness-client\",\"clientSecret\":\"harness-secret\"}");
            separator = ",";
        }
        if (options & WITH_BROKERED) {
            length += snprintf(config + length, sizeof config - (size_t)length,
                "%s\"microsoft\":{\"clientID\":\"harness-client\"}", separator);
        }
        length += snprintf(config + length, sizeof config - (size_t)length, "}");
    }
    snprintf(config + length, sizeof config - (size_t)length, "}");
    tt_engine *engine = tt_engine_create(config, on_message, h);
    CHECK(engine != NULL, "tt_engine_create returned NULL for a valid config");
    atomic_store(&h->engine, engine);
}

/* Seeds `count` brokered Microsoft accounts: on start the engine asks the host for a token for each (one
 * service.token.get request per account, all at once), and the host never has to answer. */
static void seed_accounts(int index, int count) {
    char dir[512], path[600];
    data_directory(dir, sizeof dir, index);
    make_directory(dir);
    snprintf(path, sizeof path, "%s" PATH_SEPARATOR "accounts.json", dir);
    FILE *file = fopen(path, "wb");
    CHECK(file != NULL, "cannot write %s", path);
    fputc('[', file);
    for (int i = 0; i < count; i++) {
        fprintf(file,
            "%s{\"kindID\":\"microsoft-brokered\",\"connectionID\":\"conn-%d\",\"displayName\":\"user%d@example.com\","
            "\"config\":{\"email\":\"user%d@example.com\",\"authMode\":\"external\",\"externalAccountID\":\"acct-%d\"}}",
            i ? "," : "", i, i, i, i);
    }
    fputc(']', file);
    fclose(file);
}

static void start_engine(Host *h) {
    tt_engine_send(atomic_load(&h->engine), START_COMMAND);
    WAIT_FOR(atomic_load(&h->readys) >= 1, "the ready message");
}

/* A round trip through the takeover lane: the marker's takeoverDue arrives after every takeoverDue sent before it. */
static void sync_engine(Host *h) {
    long long number = atomic_fetch_add(&h->sync_sent, 1) + 1;
    char command[200];
    snprintf(command, sizeof command,
        "{\"v\":1,\"type\":\"takeover.test\",\"payload\":{\"title\":\"sync-%lld\",\"calendarTitle\":\"Harness\"}}", number);
    tt_engine_send(atomic_load(&h->engine), command);
    WAIT_FOR(atomic_load(&h->sync_seen) >= number, "the sync marker");
}

static void destroy_engine(Host *h) {
    tt_engine_destroy(atomic_load(&h->engine));
    atomic_store(&h->destroy_returned, 1);
}

static void check_clean(Host *h) {
    CHECK(atomic_load(&h->overlaps) == 0, "%lld callback(s) overlapped another", (long long)atomic_load(&h->overlaps));
    CHECK(atomic_load(&h->revision_errors) == 0, "%d state(s) with a revision not above the previous one",
        atomic_load(&h->revision_errors));
    CHECK(atomic_load(&h->late_callbacks) == 0, "%lld callback(s) ran after destroy", (long long)atomic_load(&h->late_callbacks));
}

/* The answering helper: replies to the requests the callback queued, one millisecond after it saw each. */
static void helper_main(void *context) {
    Host *h = context;
    while (!atomic_load(&h->helper_stop) || atomic_load(&h->ring_tail) < atomic_load(&h->ring_head)) {
        long long tail = atomic_load(&h->ring_tail);
        if (tail >= atomic_load(&h->ring_head)) {
            sleep_ms(1);
            continue;
        }
        uint64_t id = h->ring[tail % RING_SIZE];
        int begins_redirect = h->ring_begins_redirect[tail % RING_SIZE];
        sleep_ms(1);
        tt_engine *engine = atomic_load(&h->engine);
        /* The sign-in starts with a believable answer so the flow goes on (the next request is the browser step);
         * everything else is answered with an error. */
        tt_engine_reply(engine, id,
            begins_redirect ? "{\"v\":1,\"result\":{\"redirectURI\":\"http://127.0.0.1:53211\",\"sessionID\":\"s-1\"}}"
                            : ERROR_REPLY);
        tt_engine_reply(engine, id, ERROR_REPLY); /* a second reply to the same id is ignored */
        atomic_fetch_add(&h->replied, 1);
        atomic_store(&h->ring_tail, tail + 1);
    }
}

typedef struct {
    Host *h;
    int count;
    const char *command;
    atomic_int *barrier_ready, *barrier_go;
} Sender;

static void sender_main(void *context) {
    Sender *s = context;
    for (int i = 0; i < s->count; i++) {
        /* Alternates the command given with refresh so lanes interleave. */
        tt_engine_send(atomic_load(&s->h->engine), (i % 2 == 0) ? s->command : REFRESH_COMMAND);
    }
}

static void spawn(thread_t *thread, thread_fn fn, void *arg) {
    CHECK(thread_spawn(thread, fn, arg) == 0, "cannot start a thread");
}

/* ---------------------------------------------------------------- the cases */

static void case_ordered_events(void) {
    Host *h = new_host("ordered");
    atomic_store(&h->spin, 200);
    create_engine(h, 0, 0);
    start_engine(h);
    enum { THREADS = 4, PER_THREAD = 500 };
    Sender senders[THREADS];
    thread_t threads[THREADS];
    for (int i = 0; i < THREADS; i++) {
        senders[i] = (Sender){h, PER_THREAD, TAKEOVER_COMMAND, NULL, NULL};
        spawn(&threads[i], sender_main, &senders[i]);
    }
    for (int i = 0; i < THREADS; i++) thread_join(threads[i]);
    sync_engine(h);
    CHECK(atomic_load(&h->dues) == THREADS * PER_THREAD / 2, "expected %d takeoverDue events, saw %lld",
        THREADS * PER_THREAD / 2, (long long)atomic_load(&h->dues));
    check_clean(h);
    destroy_engine(h);
    check_clean(h);
}

static void case_state_coalescing(void) {
    enum { COMMANDS = 2000 };
    Host *h = new_host("coalescing");
    create_engine(h, 0, 0);
    start_engine(h);
    atomic_store(&h->state_sleep_ms, 50);
    for (int i = 0; i < COMMANDS; i++) tt_engine_send(atomic_load(&h->engine), TAKEOVER_COMMAND);
    WAIT_FOR(atomic_load(&h->dues) >= COMMANDS, "all takeoverDue events");
    long long coalesced_states = atomic_load(&h->states);
    CHECK(coalesced_states < 200, "%lld state callbacks for %d state changes: states are not coalescing",
        coalesced_states, COMMANDS);
    /* The final round trip: a refresh is handled after every takeover command was (one actor), so the states it
     * publishes are the newest the engine has. The slow state callbacks stop, and the newest state must arrive. */
    atomic_store(&h->state_sleep_ms, 0);
    long long before = atomic_load(&h->states);
    tt_engine_send(atomic_load(&h->engine), REFRESH_COMMAND);
    WAIT_FOR(atomic_load(&h->states) > before, "a state after the final refresh");
    sync_engine(h);
    long long seen;
    do { /* wait until no state arrives for a moment: the newest one is then the last one */
        seen = atomic_load(&h->states);
        sleep_ms(100);
    } while (atomic_load(&h->states) != seen);
    CHECK(atomic_load(&h->dues) == COMMANDS, "expected %d takeoverDue events, saw %lld", COMMANDS,
        (long long)atomic_load(&h->dues));
    CHECK(atomic_load(&h->last_revision) == atomic_load(&h->max_revision),
        "the last state (revision %llu) is not the newest (revision %llu)",
        atomic_load(&h->last_revision), atomic_load(&h->max_revision));
    CHECK(atomic_load(&h->last_revision) >= COMMANDS, "the last state has revision %llu, below the %d the engine published",
        atomic_load(&h->last_revision), COMMANDS);
    check_clean(h);
    destroy_engine(h);
    check_clean(h);
}

static void case_events_not_dropped(void) {
    enum { COMMANDS = 300 };
    Host *h = new_host("events");
    atomic_store(&h->every_n_sleep, 20);
    atomic_store(&h->every_sleep_ms, 5);
    create_engine(h, 0, 0);
    start_engine(h);
    for (int i = 0; i < COMMANDS; i++) tt_engine_send(atomic_load(&h->engine), TAKEOVER_COMMAND);
    sync_engine(h);
    CHECK(atomic_load(&h->dues) == COMMANDS, "expected exactly %d takeoverDue events, saw %lld", COMMANDS,
        (long long)atomic_load(&h->dues));
    check_clean(h);
    destroy_engine(h);
    check_clean(h);
}

static void case_send_from_callback(void) {
    enum { LIMIT = 1000, COMMANDS = 300 };
    Host *h = new_host("send-from-callback");
    atomic_store(&h->send_in_callback, LIMIT);
    create_engine(h, 0, 0);
    start_engine(h);
    for (int i = 0; i < COMMANDS; i++) tt_engine_send(atomic_load(&h->engine), TAKEOVER_COMMAND);
    sync_engine(h); /* the delivery loop still turns although every callback sends */
    CHECK(atomic_load(&h->dues) == COMMANDS, "expected %d takeoverDue events, saw %lld", COMMANDS,
        (long long)atomic_load(&h->dues));
    CHECK(atomic_load(&h->callback_sends) >= COMMANDS, "callbacks sent only %lld commands", (long long)atomic_load(&h->callback_sends));
    check_clean(h);
    destroy_engine(h);
    check_clean(h);
}

static void case_reply_from_other_thread(void) {
    enum { ACCOUNTS = 8 };
    Host *h = new_host("reply");
    atomic_store(&h->answer_requests, 1);
    seed_accounts(0, ACCOUNTS);
    create_engine(h, 0, WITH_GOOGLE | WITH_BROKERED);
    thread_t helper;
    spawn(&helper, helper_main, h);
    start_engine(h);
    tt_engine_send(atomic_load(&h->engine), "{\"v\":1,\"type\":\"account.add\",\"payload\":{\"kindID\":\"google\"}}");
    WAIT_FOR(atomic_load(&h->requests) >= ACCOUNTS + 2, "the token requests and the two sign-in requests");
    /* Every request is answered by the helper thread; the engine then settles (nothing left working) and the
     * delivery loop is still alive. */
    WAIT_FOR(atomic_load(&h->working) == 0 && atomic_load(&h->replied) >= atomic_load(&h->requests) &&
             atomic_load(&h->ring_tail) == atomic_load(&h->ring_head), "all requests answered and the engine idle");
    sync_engine(h);
    WAIT_FOR(atomic_load(&h->replied) == atomic_load(&h->requests), "the last reply");
    CHECK(atomic_load(&h->replied) >= ACCOUNTS + 2, "only %lld requests were answered", (long long)atomic_load(&h->replied));
    check_clean(h);
    destroy_engine(h);
    atomic_store(&h->helper_stop, 1);
    thread_join(helper);
    check_clean(h);
}

static void hammer_main(void *context) {
    Sender *s = context;
    while (!atomic_load(s->barrier_go)) {
        tt_engine_send(atomic_load(&s->h->engine), TAKEOVER_COMMAND);
        thread_yield();
    }
}

static void case_destroy_outside_callback(void) {
    Host *h = new_host("destroy-outside");
    atomic_store(&h->every_n_sleep, 3);
    atomic_store(&h->every_sleep_ms, 1);
    create_engine(h, 0, 0);
    start_engine(h);
    atomic_int stop = 0;
    enum { HAMMERS = 2 };
    Sender senders[HAMMERS];
    thread_t threads[HAMMERS];
    for (int i = 0; i < HAMMERS; i++) {
        senders[i] = (Sender){h, 0, NULL, NULL, &stop};
        spawn(&threads[i], hammer_main, &senders[i]);
    }
    WAIT_FOR(atomic_load(&h->callbacks) >= 20, "callbacks while the engine is hammered");
    destroy_engine(h); /* the flag is set the moment destroy returns */
    sleep_ms(30);      /* the window in which a callback after destroy would show */
    atomic_store(&stop, 1);
    for (int i = 0; i < HAMMERS; i++) thread_join(threads[i]);
    check_clean(h);
}

static void case_destroy_inside_callback(void) {
    Host *h = new_host("destroy-inside");
    atomic_store(&h->destroy_in_first_state, 1);
    create_engine(h, 0, 0);
    tt_engine_send(atomic_load(&h->engine), START_COMMAND);
    for (int i = 0; i < 20; i++) tt_engine_send(atomic_load(&h->engine), TAKEOVER_COMMAND);
    WAIT_FOR(atomic_load(&h->destroy_in_callback_returned), "destroy to return inside the callback");
    sleep_ms(200); /* no further callback starts */
    long long destroyed_at = atomic_load(&h->destroy_callback);
    CHECK(atomic_load(&h->callbacks) == destroyed_at, "%lld callback(s) began after the one that called destroy",
        (long long)(atomic_load(&h->callbacks) - destroyed_at));
    tt_engine_send(atomic_load(&h->engine), TAKEOVER_COMMAND); /* the handle is gone: ignored */
    tt_engine_destroy(atomic_load(&h->engine));
    sleep_ms(20);
    CHECK(atomic_load(&h->callbacks) == destroyed_at, "a callback began after the handle was destroyed");
    check_clean(h);
}

static void case_destroy_with_pending_requests(void) {
    enum { REQUESTS = 10 };
    Host *h = new_host("pending");
    seed_accounts(0, REQUESTS);
    create_engine(h, 0, WITH_BROKERED);
    start_engine(h);
    WAIT_FOR(atomic_load(&h->requests) >= REQUESTS, "ten service requests");
    int64_t started = now_ms();
    destroy_engine(h);
    int64_t elapsed = now_ms() - started;
    CHECK(elapsed < 5000, "destroy with %d pending requests took %lld ms", REQUESTS, (long long)elapsed);
    tt_engine_reply(atomic_load(&h->engine), 1, ERROR_REPLY); /* a request of the destroyed engine: ignored */
    sleep_ms(30);
    check_clean(h);
}

static void case_stale_handles(void) {
    tt_engine *invented = (tt_engine *)(uintptr_t)0x1234;
    tt_engine_send(NULL, REFRESH_COMMAND);
    tt_engine_reply(NULL, 1, ERROR_REPLY);
    tt_engine_destroy(NULL);
    tt_engine_send(invented, REFRESH_COMMAND);
    tt_engine_reply(invented, 1, ERROR_REPLY);
    tt_engine_destroy(invented);

    Host *h = new_host("stale");
    create_engine(h, 0, 0);
    start_engine(h);
    /* A live engine ignores replies for ids it never issued, and a NULL command or reply. */
    tt_engine_reply(atomic_load(&h->engine), 0, ERROR_REPLY);
    tt_engine_reply(atomic_load(&h->engine), 987654321u, ERROR_REPLY);
    tt_engine_reply(atomic_load(&h->engine), UINT64_MAX, NULL);
    tt_engine_send(atomic_load(&h->engine), NULL);
    sync_engine(h);
    tt_engine *engine = atomic_load(&h->engine);
    destroy_engine(h);
    tt_engine_send(engine, REFRESH_COMMAND);
    tt_engine_send(engine, TAKEOVER_COMMAND);
    tt_engine_reply(engine, 1, ERROR_REPLY);
    tt_engine_destroy(engine);
    sleep_ms(30);
    check_clean(h);
}

typedef struct {
    Host *h;
    atomic_int *ready, *go;
} Destroyer;

static void destroyer_main(void *context) {
    Destroyer *d = context;
    atomic_fetch_add(d->ready, 1);
    while (!atomic_load(d->go)) thread_yield();
    tt_engine_destroy(atomic_load(&d->h->engine));
}

static void case_double_destroy(void) {
    Host *h = new_host("double-destroy");
    atomic_store(&h->every_n_sleep, 2);
    atomic_store(&h->every_sleep_ms, 1);
    create_engine(h, 0, 0);
    start_engine(h);
    for (int i = 0; i < 50; i++) tt_engine_send(atomic_load(&h->engine), TAKEOVER_COMMAND);
    atomic_int ready = 0, go = 0;
    Destroyer destroyers[2] = {{h, &ready, &go}, {h, &ready, &go}};
    thread_t threads[2];
    for (int i = 0; i < 2; i++) spawn(&threads[i], destroyer_main, &destroyers[i]);
    WAIT_FOR(atomic_load(&ready) == 2, "both threads to be ready");
    atomic_store(&go, 1);
    for (int i = 0; i < 2; i++) thread_join(threads[i]);
    atomic_store(&h->destroy_returned, 1);
    tt_engine_destroy(atomic_load(&h->engine)); /* and a third time */
    sleep_ms(30);
    check_clean(h);
}

typedef struct {
    int index;
    atomic_int *ready, *go;
} Lifecycle;

static void lifecycle_main(void *context) {
    Lifecycle *l = context;
    Host *h = new_host("many");
    atomic_fetch_add(l->ready, 1);
    while (!atomic_load(l->go)) thread_yield();
    create_engine(h, l->index, 0);
    start_engine(h);
    for (int i = 0; i < 5; i++) tt_engine_send(atomic_load(&h->engine), TAKEOVER_COMMAND);
    tt_engine_send(atomic_load(&h->engine), REFRESH_COMMAND);
    sync_engine(h);
    CHECK(atomic_load(&h->dues) == 5, "engine %d: expected 5 takeoverDue events, saw %lld", l->index,
        (long long)atomic_load(&h->dues));
    destroy_engine(h);
    sleep_ms(5);
    check_clean(h);
}

static void case_many_engines(void) {
    enum { ENGINES = 16 };
    atomic_int ready = 0, go = 0;
    Lifecycle lifecycles[ENGINES];
    thread_t threads[ENGINES];
    for (int i = 0; i < ENGINES; i++) {
        lifecycles[i] = (Lifecycle){i, &ready, &go};
        spawn(&threads[i], lifecycle_main, &lifecycles[i]);
    }
    WAIT_FOR(atomic_load(&ready) == ENGINES, "all threads to be ready");
    atomic_store(&go, 1);
    for (int i = 0; i < ENGINES; i++) thread_join(threads[i]);
}

/* ---------------------------------------------------------------- main */

typedef struct {
    const char *name;
    void (*run)(void);
} Case;

static const Case CASES[] = {
    {"ordered_events", case_ordered_events},
    {"state_coalescing", case_state_coalescing},
    {"events_not_dropped", case_events_not_dropped},
    {"send_from_callback", case_send_from_callback},
    {"reply_from_other_thread", case_reply_from_other_thread},
    {"destroy_outside_callback", case_destroy_outside_callback},
    {"destroy_inside_callback", case_destroy_inside_callback},
    {"destroy_with_pending_requests", case_destroy_with_pending_requests},
    {"stale_handles", case_stale_handles},
    {"double_destroy", case_double_destroy},
    {"many_engines", case_many_engines},
};

int main(int argc, char **argv) {
    int iterations = 200;
    const char *only = NULL;
    if (argc < 2) {
        fprintf(stderr, "usage: harness <scratch-directory> [--iterations N] [--case NAME]\n");
        return 2;
    }
    g_scratch = argv[1];
    make_directory(g_scratch); /* a no-op when it exists */
    for (int i = 2; i < argc; i++) {
        if (strcmp(argv[i], "--iterations") == 0 && i + 1 < argc) {
            iterations = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--case") == 0 && i + 1 < argc) {
            only = argv[++i];
        } else {
            fprintf(stderr, "unknown argument: %s\n", argv[i]);
            return 2;
        }
    }
    if (iterations < 1) {
        fprintf(stderr, "--iterations must be at least 1\n");
        return 2;
    }
    thread_t watchdog;
    spawn(&watchdog, watchdog_main, NULL); /* runs for the life of the process */

    int ran = 0;
    for (size_t c = 0; c < sizeof CASES / sizeof CASES[0]; c++) {
        if (only && strcmp(only, CASES[c].name) != 0) continue;
        g_case = CASES[c].name;
        int64_t started = now_ms();
        for (int i = 0; i < iterations; i++) {
            atomic_store(&g_iteration, i);
            atomic_store(&g_deadline, now_ms() + ITERATION_LIMIT_MS);
            CASES[c].run();
            atomic_store(&g_deadline, 0);
        }
        printf("case %-30s ok, %d iterations, %lld ms\n", CASES[c].name, iterations, (long long)(now_ms() - started));
        fflush(stdout);
        ran++;
    }
    if (ran == 0) {
        fprintf(stderr, "no case named %s\n", only ? only : "(none)");
        return 2;
    }
    printf("c-harness: ok (%d cases)\n", ran);
    return 0;
}
