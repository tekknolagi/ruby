#ifndef TRACING_STATS_H
#define TRACING_STATS_H 1

#if defined(__cplusplus)
extern "C" {
#if 0
} /* satisfy cc-mode */
#endif
#endif

struct rb_tracing_stats_struct {
    /* String representation of the tracing event */
    const char * event;
    /* Allow event-specific key/value pairs, keep
     * everything in plain-old-C for speed. */
    size_t size;
    const char ** keys;
    const char ** vals;
};

typedef struct rb_tracing_stats_struct rb_tracing_stats_t;

/* Various stats helpers */

#define TRACING_STATS_GENERATE_ENUM(ENUM) ENUM,
#define TRACING_STATS_GENERATE_STR(STRING) #STRING,
#define TRACING_STATS_GENERATE_NULL(ELEMENT) NULL,
#define TRACING_STATS_SIZE() (int) (sizeof tracing_stats_keys / sizeof (char*))
#define TRACING_STATS_UPDATE(entry, value) tracing_stats_vals[entry] = value

#define TRACING_STATS_ENUM(GEN_FUNC) \
    enum GC_TRACING_STATS_ENUM { \
	GEN_FUNC(TRACING_STATS_GENERATE_ENUM) \
    }

#define TRACING_STATS_ARR_ELEM(NAME) NAME,

#define TRACING_STATS_ARR(NAME, GEN_FUNC, ELEM_FUNC) \
    static const char * NAME[] = \
    { \
	GEN_FUNC(ELEM_FUNC) \
    }

#define TRACING_STATS_KEYS(GEN_FUNC) TRACING_STATS_ARR(tracing_stats_keys, GEN_FUNC, TRACING_STATS_GENERATE_STR)
#define TRACING_STATS_VALS(GEN_FUNC) TRACING_STATS_ARR(tracing_stats_vals, GEN_FUNC, TRACING_STATS_GENERATE_NULL)

#define TRACING_STATS_SETUP(GEN_FUNC, STATS) \
    TRACING_STATS_ENUM(GEN_FUNC); \
    TRACING_STATS_KEYS(GEN_FUNC); \
    TRACING_STATS_VALS(GEN_FUNC); \
    STATS.size = TRACING_STATS_SIZE(); \
    STATS.vals = &tracing_stats_vals[0]; \
    STATS.keys = &tracing_stats_keys[0]

#define TRACING_STATS_DUMP(stats) \
    for(int i = 0; i < stats->size; i++) { \
	fprintf(stderr, "Key '%s' -> Value '%s'\n", stats->keys[i], stats->vals[i]); \
    }

#if defined(__cplusplus)
#if 0
{ /* satisfy cc-mode */
#endif
}  /* extern "C" { */
#endif
#endif /* TRACING_STATS_H */