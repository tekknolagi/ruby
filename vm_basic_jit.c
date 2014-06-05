#if OPT_BASIC_JIT

#include <sys/mman.h>
#include "insns_info.inc"

struct rb_jit_free_list {
    size_t size;
    struct rb_jit_free_list *next;
};

struct rb_jit_code_cache {
    size_t size;
    struct rb_jit_code_cache *next;
    struct rb_jit_free_list *free_list;
};

struct rb_jit_code_chunk {
    size_t size;
};

static void
enable_execution_in_jit_code_cache(struct rb_jit_code_cache *cache)
{
    if (mprotect(cache, cache->size, PROT_READ | PROT_EXEC)) {
        rb_sys_fail("mprotect");
    }
}

static void
enable_write_in_jit_code_cache(struct rb_jit_code_cache *cache)
{
    if (mprotect(cache, cache->size, PROT_READ | PROT_WRITE)) {
        rb_sys_fail("mprotect");
    }
}

void
rb_iseq_free_jit_compiled_iseq(void *jit_compiled_iseq)
{
    char *seq = (char *) jit_compiled_iseq;
    rb_vm_t *vm = GET_VM();
    struct rb_jit_code_cache *cache = vm->jit_code_cache;
    if (jit_compiled_iseq == 0) {
        return;
    }
    while (cache) {
        char *start = (char *) cache;
        char *end = ((char *) cache) + cache->size;
        if (start <= seq && seq <= end) {
            /* TODO search for adjacent free chunk */
            struct rb_jit_code_chunk *chunk = (struct rb_jit_code_chunk *) seq;
            struct rb_jit_free_list *free_list = (struct rb_jit_free_list *) (chunk - 1);
            enable_write_in_jit_code_cache(cache);
            free_list->next = cache->free_list;
            cache->free_list = free_list;
            enable_execution_in_jit_code_cache(cache);
            return;
        }
        cache = cache->next;
    }
}

static struct rb_jit_code_cache *
rb_iseq_allocate_jit_compiled_iseq(rb_iseq_t *iseq, size_t size)
{
    rb_vm_t *vm = GET_VM();
    struct rb_jit_code_cache *cache = vm->jit_code_cache;
    if (size == 0) {
        return 0;
    }
    while (cache) {
        struct rb_jit_free_list **free_list_ptr = &cache->free_list;
        struct rb_jit_free_list *free_list = *free_list_ptr;
        while (free_list) {
            if (free_list->size >= size) {
                struct rb_jit_code_chunk *chunk = (struct rb_jit_code_chunk *) free_list;
                enable_write_in_jit_code_cache(cache);
                if (free_list->size > size + sizeof(struct rb_jit_code_chunk)) {
                    /* TODO add remainder to free list */
                } else {
                    *free_list_ptr = free_list->next;
                }
                chunk->size = size + sizeof(struct rb_jit_code_chunk *);
                iseq->jit_compiled_iseq = (void *)(chunk + 1);
                enable_execution_in_jit_code_cache(cache);
                return cache;
            }
            free_list_ptr = &free_list->next;
            free_list = *free_list_ptr;
        }
        cache = cache->next;
    }
    /* TODO allocate a new JIT code cache region */
    return cache;
}

static size_t
rb_iseq_jit_compiled_size(rb_iseq_t *iseq, const void * const *insns_address_table, void **end_insns)
{
    size_t size = 0;
    size_t i = 0;
    while (i < iseq->iseq_size) {
        int insn = (int)iseq->iseq[i];
        char *beg = (char *)insns_address_table[insn];
        char *end = (char *)end_insns;
        if (insn + 1 < VM_INSTRUCTION_SIZE) {
            end = (char *)insns_address_table[insn + 1];
        }
        size += end - beg;
        /* TODO add space for branch instructions? */
        i += insn_len(insn);
    }
    return size;
}

static int
rb_iseq_jit_compile(rb_iseq_t *iseq, const void * const *insns_address_table, void **end_insns)
{
    size_t size = rb_iseq_jit_compiled_size(iseq, insns_address_table, end_insns);
    struct rb_jit_code_cache *cache = rb_iseq_allocate_jit_compiled_iseq(iseq, size);
    void *code = iseq->jit_compiled_iseq;
    size_t i = 0;

    if (code == 0) {
        return -1;
    }

    enable_write_in_jit_code_cache(cache);
    while (i < iseq->iseq_size) {
        int insn = (int)iseq->iseq[i];
        char *beg = (char *)insns_address_table[insn];
        char *end = (char *)end_insns;
        if (insn + 1 < VM_INSTRUCTION_SIZE) {
            end = (char *)insns_address_table[insn + 1];
        }
        /* TODO something special for branch instructions? */
        memcpy(code, beg, end - beg);
        code = (void *)((char *)code + (end - beg));
        i += insn_len(insn);
    }
    enable_execution_in_jit_code_cache(cache);

    return 0;
}

static int
vm_exec_jit(rb_thread_t *th, VALUE initial)
{
    DECL_SC_REG(VALUE *, pc, "14");
    DECL_SC_REG(rb_control_frame_t *, cfp, "15");

#undef  RESTORE_REGS
#define RESTORE_REGS() \
{ \
  REG_CFP = th->cfp; \
  reg_pc  = reg_cfp->pc; \
}

#undef  REG_PC
#define REG_PC reg_pc
#undef  GET_PC
#define GET_PC() (reg_pc)
#undef  SET_PC
#define SET_PC(x) (reg_cfp->pc = REG_PC = (x))

#undef LABEL
#define LABEL(x)  INSN_LABEL_##x
#undef LABEL_PTR
#define LABEL_PTR(x) &&LABEL(x)
#include "vmtc.inc"

    if (LIKELY(th->cfp->iseq->call_count != 0 && th->cfp->iseq->exec_count != 0)) {
        return -1;
    }
    if (UNLIKELY(th->cfp->iseq->jit_compiled_iseq == 0)) {
        if (rb_iseq_jit_compile(th->cfp->iseq, insns_address_table, LABEL_PTR(__END__))) {
            return -1;
        }
    }

    reg_cfp = th->cfp;
    reg_pc = reg_cfp->pc;

  first:
    goto *reg_cfp->iseq->jit_compiled_iseq;
    /* TODO define macros (see vm_exec.h) for vm.inc */
#undef INSN_ENTRY
#define INSN_ENTRY(insn) \
    LABEL(insn): \
    __asm__ __volatile__("" : "=r" (REG_PC), "=r" (REG_CFP) : : ); \

#undef END_INSN
#define END_INSN(insn) \
    __asm__ __volatile__("" : : "r" (REG_PC), "r" (REG_CFP) : )
/*****************/
 #include "vm.inc"
/*****************/
LABEL(__END__):

    /* unreachable */
    rb_bug("vm_eval: unreachable");
    goto first;
}
#endif
