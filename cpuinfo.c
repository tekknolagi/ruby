#include <cpuid.h>
#include <stdbool.h>
#include "internal/cpuinfo.h"

typedef struct rb_cpu_features {
    bool SSE2;
    bool AVX2;
} rb_cpu_features;

static rb_cpu_features _rb_cpu_features = {0};
static bool _rb_cpu_features_initialized = false;

typedef struct rb_cpu_info {
    uint32_t eax;
    uint32_t ebx;
    uint32_t ecx;
    uint32_t edx;
} rb_cpu_info;

static void
rb_cpuid(uint32_t func, uint32_t subfunc, rb_cpu_info *cpuinfo) {
    __cpuid_count(func, subfunc, cpuinfo->eax, cpuinfo->ebx, cpuinfo->ecx, cpuinfo->edx);
}

static rb_cpu_features
rb_get_cpu_features() {
    rb_cpu_info cpu_info;
    int max_feature;

    if (!_rb_cpu_features_initialized) {
        rb_cpuid(0, 0, &cpu_info);
        max_feature = cpu_info.eax;

        if (max_feature >= 0) {
            rb_cpuid(1, 0, &cpu_info);
            _rb_cpu_features.SSE2 = cpu_info.edx & 1<<26;
        }

        if (max_feature >= 7) {
            rb_cpuid(7, 0, &cpu_info);

            _rb_cpu_features.AVX2 = cpu_info.ebx & 1<<5;
        }

        _rb_cpu_features_initialized = true;
    }
    return _rb_cpu_features;
}
