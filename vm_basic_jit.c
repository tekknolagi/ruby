#if OPT_BASIC_JIT
struct rb_jit_code_cache {
    void *start;
    size_t size;
};

static void
enable_execution_in_jit_code_cache(struct rb_jit_code_cache *cache)
{
    if (mprotect(cache->start, cache->size, PROT_READ | PROT_EXEC)) {
        rb_sys_fail("mprotect");
    }
}

static void
enable_write_in_jit_code_cache(struct rb_jit_code_cache *cache)
{
    if (mprotect(cache->start, cache->size, PROT_READ | PROT_WRITE)) {
        rb_sys_fail("mprotect");
    }
}

void
rb_iseq_free_jit_compiled_iseq(void *jit_compiled_iseq)
{
    rb_vm_t *vm = GET_VM();
    /*
    TODO: free block in jit code cache
    */
}

static void *
rb_iseq_allocate_jit_compiled_iseq(unsigned long size)
{
    /*
    TODO: allocate chunk of size from jit code cache
    */
}

static unsigned long
rb_iseq_jit_compiled_size(rb_iseq_t *iseq, const void * const *insns_address_table, void **end_insns)
{
    unsigned long size = 0;
    unsigned long i = 0;
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
    /*
    TODO: compute the compiled iseq size, allocate chunk from jit code cache, copy into chunk
    copy by walking instructions in iseq, copying from insns_address_table[i] to insns_address_table[i+1]
    */
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
#endif

#include "vmtc.inc"
    if (UNLIKELY(th->cfp->iseq->jit_compiled_iseq == 0)) {
        if (rb_iseq_jit_compile(th->cfp->iseq, insns_address_table, LABEL_PTR(__END__))) {
            return -1;
        }
    }

    reg_cfp = th->cfp;
    reg_pc = reg_cfp->pc;

  first:
    goto *reg_cfp->iseq->jit_compiled_iseq;
/*****************/
 #include "vm.inc"
/*****************/
LABEL(__END__):

    /* unreachable */
    rb_bug("vm_eval: unreachable");
    goto first;
}
#endif
