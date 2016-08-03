#ifndef VESTIGE_STATS_H
#define VESTIGE_STATS_H 1

#if defined(__cplusplus)
extern "C" {
#if 0
} /* satisfy cc-mode */
#endif
#endif

struct rb_vestige_stats_struct {
    /* String representation of the tracing event */
    const char * event;
    /* Allow event-specific key/value pairs, keep
     * everything in plain-old-C for speed. */
    size_t size;
    const char ** keys;
    const char ** vals;
};

typedef struct rb_vestige_stats_struct rb_vestige_stats_t;

/* Various stats helpers */

#define VESTIGE_STATS_GENERATE_ENUM(ENUM) ENUM,
#define VESTIGE_STATS_GENERATE_STR(STRING) #STRING,
#define VESTIGE_STATS_GENERATE_NULL(ELEMENT) NULL,
#define VESTIGE_STATS_SIZE() (int) (sizeof vestige_stats_keys / sizeof (char*))
#define VESTIGE_STATS_UPDATE(entry, value) vestige_stats_vals[entry] = value

#define VESTIGE_STATS_ENUM(GEN_FUNC) \
    enum GC_VESTIGE_STATS_ENUM { \
	GEN_FUNC(VESTIGE_STATS_GENERATE_ENUM) \
    }

#define VESTIGE_STATS_ARR_ELEM(NAME) NAME,

#define VESTIGE_STATS_ARR(NAME, GEN_FUNC, ELEM_FUNC) \
    static const char * NAME[] = \
    { \
	GEN_FUNC(ELEM_FUNC) \
    }

#define VESTIGE_STATS_KEYS(GEN_FUNC) VESTIGE_STATS_ARR(vestige_stats_keys, GEN_FUNC, VESTIGE_STATS_GENERATE_STR)
#define VESTIGE_STATS_VALS(GEN_FUNC) VESTIGE_STATS_ARR(vestige_stats_vals, GEN_FUNC, VESTIGE_STATS_GENERATE_NULL)

#define VESTIGE_STATS_SETUP(GEN_FUNC, STATS) \
    VESTIGE_STATS_ENUM(GEN_FUNC); \
    VESTIGE_STATS_KEYS(GEN_FUNC); \
    VESTIGE_STATS_VALS(GEN_FUNC); \
    STATS.size = VESTIGE_STATS_SIZE(); \
    STATS.vals = &vestige_stats_vals[0]; \
    STATS.keys = &vestige_stats_keys[0]

#define VESTIGE_STATS_DUMP(stats) \
    for(int i = 0; i < stats->size; i++) { \
	fprintf(stderr, "Key '%s' -> Value '%s'\n", stats->keys[i], stats->vals[i]); \
    }

#if defined(__cplusplus)
#if 0
{ /* satisfy cc-mode */
#endif
}  /* extern "C" { */
#endif
#endif /* VESTIGE_STATS_H */