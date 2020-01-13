#ifndef INTERNAL_CPUINFO_H /* -*- C -*- */
#define INTERNAL_CPUINFO_H

typedef struct rb_cpu_features {
    bool SSE2;
    bool AVX2;
} rb_cpu_features;

rb_cpu_features rb_get_cpu_features();

#endif /* INTERNAL_CPUINFO_H */
