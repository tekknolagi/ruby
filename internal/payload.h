#ifndef INTERNAL_PAYLOAD_H
#define INTERNAL_PAYLOAD_H

#include "ruby/internal/value.h"

#define PAYLOAD_LENGTH(obj) (RANY(obj)->as.payload.head.length)
#define PAYLOAD_DATA_START(obj) ((void *)(obj + sizeof(RVALUE) + sizeof(rpayload_head_t)))

typedef struct RPayloadHead {
    VALUE flags;
    unsigned short length;
} rpayload_head_t;

struct RPayload {
    rpayload_head_t head;
};

#endif /* INTERNAL_PAYLOAD_H */