#ifndef INTERNAL_FREE_H
#define INTERNAL_FREE_H

#include "ruby/internal/cast.h"
#include "ruby/internal/fl_type.h"
#include "ruby/internal/value.h"
#include "sanitizers.h"

#define RFREE(obj) RBIMPL_CAST((struct RFree *)(obj))
#define RFREE_HEAD_MASK RUBY_FL_USER1

struct RFree {
    VALUE flags;
    union {
        struct {
            unsigned int size;
            struct RFree *prev;
            struct RFree *next;
        } head;
        struct {
            VALUE head; 
        } body;
    } as;
};

static bool
RFREE_HEAD_P(VALUE obj)
{
    return !!FL_TEST_RAW(obj, RFREE_HEAD_MASK);
}

static void
RFREE_HEAD_SET(VALUE obj)
{
    FL_SET_RAW(obj, RFREE_HEAD_MASK);
}

static void
RFREE_BODY_SET(VALUE obj)
{
    FL_UNSET_RAW(obj, RFREE_HEAD_MASK);
}

static VALUE
rfree_get_head(VALUE free)
{
    asan_unpoison_object(free, false);

    VALUE head = free;

    if (!RFREE_HEAD_P(free)) {
        head = rfree_get_head(RFREE(free)->as.body.head);
    }

    asan_poison_object(free);

    return head;
}

#endif /* INTERNAL_FREE_H */
