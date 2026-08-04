#[cfg(test)]
mod hir_opt_tests {
    use crate::hir::*;

    use crate::hir::tests::hir_build_tests::assert_contains_opcode;
    use crate::{hir_strings, options::*};
    use insta::assert_snapshot;

    #[track_caller]
    fn hir_string_function(function: &Function) -> String {
        format!("{}", FunctionPrinter::without_snapshot(function))
    }

    #[track_caller]
    fn hir_string_proc(proc: &str) -> String {
        let iseq = crate::cruby::with_rubyvm(|| get_proc_iseq(proc));
        unsafe { crate::cruby::rb_zjit_profile_disable(iseq) };
        let mut function = iseq_to_hir(iseq).unwrap();
        function.optimize();
        function.validate().unwrap();
        hir_string_function(&function)
    }

    #[track_caller]
    fn hir_string(method: &str) -> String {
        hir_string_proc(&format!("{}.method(:{})", "self", method))
    }

    #[test]
    fn test_fold_iftrue_away() {
        eval("
            def test
              cond = true
              if cond
                3
              else
                4
              end
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:TrueClass = Const Value(true)
          v23:Fixnum[3] = Const Value(3)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_fold_iftrue_into_jump() {
        eval("
            def test
              cond = false
              if cond
                3
              else
                4
              end
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:FalseClass = Const Value(false)
          v32:Fixnum[4] = Const Value(4)
          CheckInterrupts
          Return v32
        ");
    }

    #[test]
    fn test_fold_fixnum_add() {
        eval("
            def test
              1 + 2 + 3
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          v11:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1000, +@0x1008, cme:0x1010)
          v32:Fixnum[6] = Const Value(6)
          CheckInterrupts
          Return v32
        ");
    }

    #[test]
    fn test_fold_fixnum_add_zero() {
        eval("
            def test(n)
              0 + n + 0
            end
            test 1; test 2
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:Fixnum[0] = Const Value(0)
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v31:Fixnum = GuardType v10, Fixnum
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_fold_fixnum_sub() {
        eval("
            def test
              5 - 3 - 1
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[5] = Const Value(5)
          v11:Fixnum[3] = Const Value(3)
          PatchPoint MethodRedefined(Integer@0x1000, -@0x1008, cme:0x1010)
          v32:Fixnum[1] = Const Value(1)
          CheckInterrupts
          Return v32
        ");
    }

    #[test]
    fn test_fold_fixnum_sub_large_negative_result() {
        eval("
            def test
              0 - 1073741825
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[0] = Const Value(0)
          v11:Fixnum[1073741825] = Const Value(1073741825)
          PatchPoint MethodRedefined(Integer@0x1000, -@0x1008, cme:0x1010)
          v23:Fixnum[-1073741825] = Const Value(-1073741825)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_no_fold_fixnum_add_overflow() {
        eval(&format!("
            def test
              {RUBY_FIXNUM_MAX} + 1
            end
        "));
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[4611686018427387903] = Const Value(4611686018427387903)
          v11:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1000, +@0x1008, cme:0x1010)
          v22:Fixnum = FixnumAdd v9, v11
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_no_fold_fixnum_sub_underflow() {
        eval(&format!("
            def test
              {RUBY_FIXNUM_MIN} - 1
            end
        "));
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[-4611686018427387904] = Const Value(-4611686018427387904)
          v11:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1000, -@0x1008, cme:0x1010)
          v22:Fixnum = FixnumSub v9, v11
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_no_fold_fixnum_mult_overflow() {
        eval(&format!("
            def test
              {RUBY_FIXNUM_MAX} * 2
            end
        "));
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[4611686018427387903] = Const Value(4611686018427387903)
          v11:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1000, *@0x1008, cme:0x1010)
          v22:Fixnum = FixnumMult v9, v11
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_fold_fixnum_sub_zero() {
        eval("
            def test(n)
              n - 0
            end
            test 1; test 2
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[0] = Const Value(0)
          PatchPoint MethodRedefined(Integer@0x1008, -@0x1010, cme:0x1018)
          v25:Fixnum = GuardType v10, Fixnum recompile
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_fold_fixnum_mult() {
        eval("
            def test
              6 * 7
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[6] = Const Value(6)
          v11:Fixnum[7] = Const Value(7)
          PatchPoint MethodRedefined(Integer@0x1000, *@0x1008, cme:0x1010)
          v23:Fixnum[42] = Const Value(42)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_fold_fixnum_mult_zero() {
        eval("
            def test(n)
              0 * n + n * 0
            end
            test 1; test 2
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:Fixnum[0] = Const Value(0)
          PatchPoint MethodRedefined(Integer@0x1008, *@0x1010, cme:0x1018)
          v35:Fixnum = GuardType v10, Fixnum
          v45:Fixnum[0] = Const Value(0)
          v46:Fixnum[0] = Const Value(0)
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1040, cme:0x1048)
          CheckInterrupts
          Return v46
        ");
    }

    #[test]
    fn test_fold_fixnum_mult_one() {
        eval("
            def test(n)
              1 * n + n * 1
            end
            test 1; test 2
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, *@0x1010, cme:0x1018)
          v35:Fixnum = GuardType v10, Fixnum
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1040, cme:0x1048)
          v44:Fixnum = FixnumAdd v35, v35
          CheckInterrupts
          Return v44
        ");
    }

    #[test]
    fn test_fold_fixnum_div() {
        eval("
            def test
              7 / 3
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[7] = Const Value(7)
          v11:Fixnum[3] = Const Value(3)
          PatchPoint MethodRedefined(Integer@0x1000, /@0x1008, cme:0x1010)
          v23:Fixnum[2] = Const Value(2)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_dont_fold_fixnum_div_zero() {
        eval("
            def test
              7 / 0
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[7] = Const Value(7)
          v11:Fixnum[0] = Const Value(0)
          PatchPoint MethodRedefined(Integer@0x1000, /@0x1008, cme:0x1010)
          v22:Integer = FixnumDiv v9, v11
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_fold_fixnum_div_negative() {
        eval("
            def test
              7 / -3
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[7] = Const Value(7)
          v11:Fixnum[-3] = Const Value(-3)
          PatchPoint MethodRedefined(Integer@0x1000, /@0x1008, cme:0x1010)
          v23:Fixnum[-3] = Const Value(-3)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_dont_fold_fixnum_div_negative_one_overflow() {
        eval(&format!("
            def test
              {RUBY_FIXNUM_MIN} / -1
            end
        "));
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[-4611686018427387904] = Const Value(-4611686018427387904)
          v11:Fixnum[-1] = Const Value(-1)
          PatchPoint MethodRedefined(Integer@0x1000, /@0x1008, cme:0x1010)
          v22:Integer = FixnumDiv v9, v11
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_fold_fixnum_div_one() {
        eval("
            def test(n)
              n / 1
            end
            test 1; test 2
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, /@0x1010, cme:0x1018)
          v25:Fixnum = GuardType v10, Fixnum recompile
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_fold_fixnum_mod_zero_by_zero() {
        eval("
            def test
              0 % 0
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[0] = Const Value(0)
          v11:Fixnum[0] = Const Value(0)
          PatchPoint MethodRedefined(Integer@0x1000, %@0x1008, cme:0x1010)
          v22:Fixnum = FixnumMod v9, v11
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_fold_fixnum_mod_non_zero_by_zero() {
        eval("
            def test
              11 % 0
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[11] = Const Value(11)
          v11:Fixnum[0] = Const Value(0)
          PatchPoint MethodRedefined(Integer@0x1000, %@0x1008, cme:0x1010)
          v22:Fixnum = FixnumMod v9, v11
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_fold_fixnum_mod_zero_by_non_zero() {
        eval("
            def test
              0 % 11
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[0] = Const Value(0)
          v11:Fixnum[11] = Const Value(11)
          PatchPoint MethodRedefined(Integer@0x1000, %@0x1008, cme:0x1010)
          v23:Fixnum[0] = Const Value(0)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_fold_fixnum_mod() {
        eval("
            def test
              11 % 3
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[11] = Const Value(11)
          v11:Fixnum[3] = Const Value(3)
          PatchPoint MethodRedefined(Integer@0x1000, %@0x1008, cme:0x1010)
          v23:Fixnum[2] = Const Value(2)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_fold_fixnum_mod_negative_numerator() {
        eval("
            def test
              -7 % 3
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[-7] = Const Value(-7)
          v11:Fixnum[3] = Const Value(3)
          PatchPoint MethodRedefined(Integer@0x1000, %@0x1008, cme:0x1010)
          v23:Fixnum[2] = Const Value(2)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_fold_fixnum_mod_negative_denominator() {
        eval("
            def test
              7 % -3
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[7] = Const Value(7)
          v11:Fixnum[-3] = Const Value(-3)
          PatchPoint MethodRedefined(Integer@0x1000, %@0x1008, cme:0x1010)
          v23:Fixnum[-2] = Const Value(-2)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_fold_fixnum_mod_negative() {
        eval("
            def test
              -7 % -3
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[-7] = Const Value(-7)
          v11:Fixnum[-3] = Const Value(-3)
          PatchPoint MethodRedefined(Integer@0x1000, %@0x1008, cme:0x1010)
          v23:Fixnum[-1] = Const Value(-1)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_fold_fixnum_xor() {
        eval("
            def test
              2 ^ 5
            end
        ");

        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[2] = Const Value(2)
          v11:Fixnum[5] = Const Value(5)
          PatchPoint MethodRedefined(Integer@0x1000, ^@0x1008, cme:0x1010)
          v22:Fixnum[7] = Const Value(7)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_fold_fixnum_xor_same_negative_number() {
        eval("
            def test
              123 ^ -123
            end
        ");

        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[123] = Const Value(123)
          v11:Fixnum[-123] = Const Value(-123)
          PatchPoint MethodRedefined(Integer@0x1000, ^@0x1008, cme:0x1010)
          v22:Fixnum[-2] = Const Value(-2)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_fold_fixnum_and() {
        eval("
            def test
              4 & -7
            end
        ");

        assert_snapshot!(inspect("test"), @"0");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[4] = Const Value(4)
          v11:Fixnum[-7] = Const Value(-7)
          PatchPoint MethodRedefined(Integer@0x1000, &@0x1008, cme:0x1010)
          v24:Fixnum[0] = Const Value(0)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_fold_fixnum_and_with_negative_self() {
        eval("
            def test
              -4 & 7
            end
        ");

        assert_snapshot!(inspect("test"), @"4");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[-4] = Const Value(-4)
          v11:Fixnum[7] = Const Value(7)
          PatchPoint MethodRedefined(Integer@0x1000, &@0x1008, cme:0x1010)
          v24:Fixnum[4] = Const Value(4)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_fold_fixnum_or() {
        eval("
            def test
              4 | 1
            end
        ");

        assert_snapshot!(inspect("test"), @"5");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[4] = Const Value(4)
          v11:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1000, |@0x1008, cme:0x1010)
          v24:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_fold_fixnum_or_with_negative_self() {
        eval("
            def test
              -4 | 1
            end
        ");

        assert_snapshot!(inspect("test"), @"-3");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[-4] = Const Value(-4)
          v11:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1000, |@0x1008, cme:0x1010)
          v24:Fixnum[-3] = Const Value(-3)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_fold_fixnum_or_with_negative_other() {
        eval("
            def test
              4 | -1
            end
        ");

        assert_snapshot!(inspect("test"), @"-1");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[4] = Const Value(4)
          v11:Fixnum[-1] = Const Value(-1)
          PatchPoint MethodRedefined(Integer@0x1000, |@0x1008, cme:0x1010)
          v24:Fixnum[-1] = Const Value(-1)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_fold_fixnum_less() {
        eval("
            def test
              if 1 < 2
                3
              else
                4
              end
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          v11:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1000, <@0x1008, cme:0x1010)
          v22:Fixnum[3] = Const Value(3)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_fold_fixnum_less_equal() {
        eval("
            def test
              if 1 <= 2 && 2 <= 2
                3
              else
                4
              end
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          v11:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1000, <=@0x1008, cme:0x1010)
          v34:Fixnum[3] = Const Value(3)
          CheckInterrupts
          Return v34
        ");
    }

    #[test]
    fn test_fold_fixnum_greater() {
        eval("
            def test
              if 2 > 1
                3
              else
                4
              end
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[2] = Const Value(2)
          v11:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1000, >@0x1008, cme:0x1010)
          v22:Fixnum[3] = Const Value(3)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_fold_fixnum_greater_equal() {
        eval("
            def test
              if 2 >= 1 && 2 >= 2
                3
              else
                4
              end
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[2] = Const Value(2)
          v11:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1000, >=@0x1008, cme:0x1010)
          v34:Fixnum[3] = Const Value(3)
          CheckInterrupts
          Return v34
        ");
    }

    #[test]
    fn test_fold_fixnum_eq_false() {
        eval("
            def test
              if 1 == 2
                3
              else
                4
              end
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          v11:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1000, ==@0x1008, cme:0x1010)
          v30:Fixnum[4] = Const Value(4)
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_fold_fixnum_eq_true() {
        eval("
            def test
              if 2 == 2
                3
              else
                4
              end
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[2] = Const Value(2)
          v11:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1000, ==@0x1008, cme:0x1010)
          v22:Fixnum[3] = Const Value(3)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_fold_fixnum_neq_true() {
        eval("
            def test
              if 1 != 2
                3
              else
                4
              end
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          v11:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1000, !=@0x1008, cme:0x1010)
          PatchPoint BOPRedefined(INTEGER_REDEFINED_OP_FLAG, BOP_EQ)
          v22:Fixnum[3] = Const Value(3)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_fold_fixnum_neq_false() {
        eval("
            def test
              if 2 != 2
                3
              else
                4
              end
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[2] = Const Value(2)
          v11:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1000, !=@0x1008, cme:0x1010)
          PatchPoint BOPRedefined(INTEGER_REDEFINED_OP_FLAG, BOP_EQ)
          v30:Fixnum[4] = Const Value(4)
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_fold_unbox_fixnum() {
        eval("
            def test(arr) = arr[0]
            test([1,2,3])
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :arr@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[0] = Const Value(0)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          v26:ArrayExact = GuardType v10, ArrayExact recompile
          v34:CInt64[0] = Const CInt64(0)
          v28:CInt64 = ArrayLength v26
          v29:CInt64[0] = GuardLess v34, v28
          v33:BasicObject = ArrayAref v26, v29
          CheckInterrupts
          Return v33
        ");
    }

    #[test]
    fn test_fold_guard_greater_eq() {
        eval("
            def test(arr) = arr[0]
            test([1,2,3])
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :arr@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[0] = Const Value(0)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          v26:ArrayExact = GuardType v10, ArrayExact recompile
          v34:CInt64[0] = Const CInt64(0)
          v28:CInt64 = ArrayLength v26
          v29:CInt64[0] = GuardLess v34, v28
          v33:BasicObject = ArrayAref v26, v29
          CheckInterrupts
          Return v33
        ");
    }

    #[test]
    fn test_fold_guard_greater_eq_side_exit() {
        eval(r##"
            def test = [4,5,6].freeze[-10]
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(ARRAY_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v12:Fixnum[-10] = Const Value(-10)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          v31:CInt64[-10] = Const CInt64(-10)
          v32:CInt64[3] = Const CInt64(3)
          v27:CInt64 = AdjustBounds v31, v32
          v28:CInt64[0] = Const CInt64(0)
          v29:CInt64 = GuardGreaterEq v27, v28
          v30:BasicObject = ArrayAref v10, v29
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn neq_with_side_effect_not_elided () {
        let result = eval("
            class CustomEq
              attr_reader :count

              def ==(o)
                @count = @count.to_i + 1
                self.equal?(o)
              end
            end

            def test(object)
              # intentionally unused, but also can't assign to underscore
              object != object
              nil
            end

            custom = CustomEq.new
            test(custom)
            test(custom)

            custom.count
        ");
        assert_eq!(VALUE::fixnum_from_usize(2), result);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:13:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :object@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :object@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(CustomEq@0x1008)
          PatchPoint MethodRedefined(CustomEq@0x1008, !=@0x1010, cme:0x1018)
          v29:ObjectSubclass[class_exact:CustomEq] = GuardType v10, ObjectSubclass[class_exact:CustomEq] recompile
          v30:BoolExact = CCallWithFrame v29, :BasicObject#!=@0x1040, v29
          v20:NilClass = Const Value(nil)
          CheckInterrupts
          Return v20
        ");
    }

    #[test]
    fn test_replace_guard_if_known_fixnum() {
        eval("
            def test(a)
              a + 1
            end
            test(2); test(3)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v25:Fixnum = GuardType v10, Fixnum recompile
          v26:Fixnum = FixnumAdd v25, v14
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_param_forms_get_bb_param() {
        eval("
            def rest(*array) = array
            def kw(k:) = k
            def kw_rest(**k) = k
            def post(*rest, post) = post
            def block(&b) = nil
        ");
        assert_snapshot!(hir_strings!("rest", "kw", "kw_rest", "block", "post"), @"
        fn rest@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:ArrayExact = LoadField v2, :array@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :array@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          CheckInterrupts
          Return v10

        fn kw@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :k@0x1000
          v4:BasicObject = LoadField v2, :<empty>@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :k@1
          v9:CPtr = GetEP 0
          v10:BasicObject = LoadField v9, :<empty>@0x1002
          Jump bb3(v7, v8, v10)
        bb3(v12:BasicObject, v13:BasicObject, v14:BasicObject):
          CheckInterrupts
          Return v13

        fn kw_rest@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :k@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :k@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          CheckInterrupts
          Return v10

        fn block@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :b@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :b@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:NilClass = Const Value(nil)
          CheckInterrupts
          Return v13

        fn post@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:ArrayExact = LoadField v2, :rest@0x1000
          v4:BasicObject = LoadField v2, :post@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :rest@1
          v9:BasicObject = LoadArg :post@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_optimize_send_without_block_to_aliased_iseq() {
        eval("
            def foo = 1
            alias bar foo
            alias baz bar
            def test = baz
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, baz@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v18:Fixnum[1] = Const Value(1)
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_optimize_send_without_block_to_aliased_cfunc() {
        eval("
            alias bar itself
            alias baz bar
            def test = baz
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, baz@0x1008, cme:0x1010)
          v18:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_inline_nonparam_local_return() {
        eval("
            def foo(a)
              if false
                x = nil
              end
              x
            end
            def test = foo(1)
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:8:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v19:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v31:NilClass = Const Value(nil)
          PushInlineFrame v19 (0x1038), v10
          CheckInterrupts
          PopInlineFrame
          Return v31
        ");
    }

    #[test]
    fn test_optimize_send_to_aliased_cfunc() {
        eval("
            class C < Array
              alias fun_new_map map
            end
            def test(o) = o.fun_new_map {|e| e }
            test C.new; test C.new
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, fun_new_map@0x1010, cme:0x1018)
          v24:ArraySubclass[class_exact:C] = GuardType v10, ArraySubclass[class_exact:C] recompile
          v25:BasicObject = SendDirect v24, 0x1040, :fun_new_map (0x1068)
          PatchPoint NoEPEscape(test)
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_optimize_send_to_aliased_cfunc_from_module() {
        eval("
            class C
              include Enumerable
              def each; yield 1; end
              alias bar map
            end
            def test(o) = o.bar { |x| x }
            test C.new; test C.new
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, bar@0x1010, cme:0x1018)
          v25:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v26:BasicObject = CCallWithFrame v25, :Enumerable#bar@0x1040, block=0x1048
          PatchPoint NoEPEscape(test)
          CheckInterrupts
          Return v26
        ");
    }

    // Regression test: when specialized_instruction is disabled, the compiler
    // doesn't convert `send` to `opt_send_without_block`, so a no-block call
    // reaches ZJIT as `YARVINSN_send` with a null blockiseq. This becomes
    // `Send { blockiseq: Some(null_ptr) }` which must be normalized to None in
    // reduce_send_to_ccall, otherwise CCallWithFrame gens wrong block handler.
    #[test]
    fn test_send_to_cfunc_without_specialized_instruction() {
        eval_with_options("
            def test(a) = a.length
            test([1,2,3]); test([1,2,3])
        ", "{ specialized_instruction: false }");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, length@0x1010, cme:0x1018)
          v23:ArrayExact = GuardType v10, ArrayExact recompile
          v24:CInt64 = ArrayLength v23
          v25:Fixnum = BoxFixnum v24
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_optimize_nonexistent_top_level_call() {
        eval("
            def foo
            end
            def test
              foo
            end
            test; test
            undef :foo
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:BasicObject = Send v6, :foo # SendFallbackReason: Send: unsupported method type Null
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_optimize_call_with_overloaded_cme() {
        eval("
            def test
              Integer(3)
            end
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[3] = Const Value(3)
          PatchPoint MethodRedefined(Object@0x1000, Integer@0x1008, cme:0x1010)
          v19:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v19 (0x1038), v10
          v26:BasicObject = InvokeBuiltin rb_f_integer1, v19, v10
          CheckInterrupts
          PopInlineFrame
          Return v26
        ");
    }

    #[test]
    fn test_optimize_call_with_args() {
        eval("
            def foo(a, b) = []
            def test
              foo 1, 2
            end
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v21:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v21 (0x1038), v10, v12
          v29:ArrayExact = NewArray
          CheckInterrupts
          PopInlineFrame
          Return v29
        ");
    }

    #[test]
    fn test_optimize_send_no_optionals_passed() {
        eval("
            def foo(a=1, b=2) = a + b
            def test = foo
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v17 (0x1038)
          v24:Fixnum[1] = Const Value(1)
          v31:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1060, +@0x1068, cme:0x1070)
          v57:Fixnum[3] = Const Value(3)
          CheckInterrupts
          PopInlineFrame
          Return v57
        ");
    }

    #[test]
    fn test_optimize_send_one_optional_passed() {
        eval("
            def foo(a=1, b=2) = a + b
            def test = foo 3
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[3] = Const Value(3)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v19:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v19 (0x1038), v10
          v26:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1060, +@0x1068, cme:0x1070)
          v51:Fixnum[5] = Const Value(5)
          CheckInterrupts
          PopInlineFrame
          Return v51
        ");
    }

    #[test]
    fn test_optimize_send_all_optionals_passed() {
        eval("
            def foo(a=1, b=2) = a + b
            def test = foo 3, 4
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[3] = Const Value(3)
          v12:Fixnum[4] = Const Value(4)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v21:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v21 (0x1038), v10, v12
          PatchPoint MethodRedefined(Integer@0x1060, +@0x1068, cme:0x1070)
          v45:Fixnum[7] = Const Value(7)
          CheckInterrupts
          PopInlineFrame
          Return v45
        ");
    }

    #[test]
    fn test_call_with_correct_and_too_many_args_for_method() {
        eval("
            def target(a = 1, b = 2, c = 3, d = 4) = [a, b, c, d]
            def test = [target(), target(10, 20, 30), begin; target(10, 20, 30, 40, 50) rescue ArgumentError; end]
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, target@0x1008, cme:0x1010)
          v43:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v43 (0x1038)
          v55:Fixnum[1] = Const Value(1)
          v64:Fixnum[2] = Const Value(2)
          v73:Fixnum[3] = Const Value(3)
          v82:Fixnum[4] = Const Value(4)
          v96:ArrayExact = NewArray v55, v64, v73, v82
          CheckInterrupts
          PopInlineFrame
          v13:Fixnum[10] = Const Value(10)
          v15:Fixnum[20] = Const Value(20)
          v17:Fixnum[30] = Const Value(30)
          PushInlineFrame v43 (0x1038), v13, v15, v17
          v116:Fixnum[4] = Const Value(4)
          v130:ArrayExact = NewArray v13, v15, v17, v116
          PopInlineFrame
          v23:Fixnum[10] = Const Value(10)
          v25:Fixnum[20] = Const Value(20)
          v27:Fixnum[30] = Const Value(30)
          v29:Fixnum[40] = Const Value(40)
          v31:Fixnum[50] = Const Value(50)
          v33:BasicObject = Send v43, :target, v23, v25, v27, v29, v31 # SendFallbackReason: Argument count does not match parameter count
          v36:ArrayExact = NewArray v96, v130, v33
          CheckInterrupts
          Return v36
        ");
    }

    #[test]
    fn test_optimize_variadic_ccall() {
        eval("
            def test
              puts 'Hello'
            end
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v11:StringExact = StringCopy v10
          PatchPoint MethodRedefined(Object@0x1008, puts@0x1010, cme:0x1018)
          v21:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          v22:BasicObject = CCallVariadic v21, :Kernel#puts@0x1040, v11
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_dont_optimize_fixnum_add_if_redefined() {
        eval("
            class Integer
              def +(other)
                100
              end
            end
            def test(a, b) = a + b
            test(1,2); test(3,4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v26:Fixnum = GuardType v12, Fixnum recompile
          v27:Fixnum[100] = Const Value(100)
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_optimize_send_into_fixnum_add_both_profiled() {
        eval("
            def test(a, b) = a + b
            test(1,2); test(3,4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v27:Fixnum = GuardType v12, Fixnum recompile
          v28:Fixnum = GuardType v13, Fixnum
          v29:Fixnum = FixnumAdd v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_optimize_send_into_fixnum_add_left_profiled() {
        eval("
            def test(a) = a + 1
            test(1); test(3)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v25:Fixnum = GuardType v10, Fixnum recompile
          v26:Fixnum = FixnumAdd v25, v14
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_optimize_send_into_fixnum_add_right_profiled() {
        eval("
            def test(a) = 1 + a
            test(1); test(3)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v26:Fixnum = GuardType v10, Fixnum
          v27:Fixnum = FixnumAdd v13, v26
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn integer_aref_with_fixnum_emits_fixnum_aref() {
        eval("
            def test(a, b) = a[b]
            test(3, 4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, []@0x1010, cme:0x1018)
          v27:Fixnum = GuardType v12, Fixnum recompile
          v28:Fixnum = GuardType v13, Fixnum
          v29:Fixnum = FixnumAref v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn integer_aref_with_constant_index_strength_reduced() {
        eval("
            def test(a) = a[12]
            test(4096)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[12] = Const Value(12)
          PatchPoint MethodRedefined(Integer@0x1008, []@0x1010, cme:0x1018)
          v25:Fixnum = GuardType v10, Fixnum recompile
          v26:Fixnum[12] = Const Value(12)
          v27:Fixnum = FixnumRShift v25, v26
          v28:Fixnum[1] = Const Value(1)
          v29:Fixnum = FixnumAnd v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn integer_aref_with_constant_index_beyond_fixnum_width() {
        eval("
            def test(a) = a[100]
            test(-1)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[100] = Const Value(100)
          PatchPoint MethodRedefined(Integer@0x1008, []@0x1010, cme:0x1018)
          v25:Fixnum = GuardType v10, Fixnum recompile
          v26:Fixnum = FixnumAref v25, v14
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn elide_fixnum_aref() {
        eval("
            def test
              1[2]
              5
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          v11:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1000, []@0x1008, cme:0x1010)
          v18:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn do_not_optimize_integer_aref_with_too_many_args() {
        eval("
            def test = 1[2, 3]
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          v11:Fixnum[2] = Const Value(2)
          v13:Fixnum[3] = Const Value(3)
          PatchPoint MethodRedefined(Integer@0x1000, []@0x1008, cme:0x1010)
          v23:BasicObject = CCallVariadic v9, :Integer#[]@0x1038, v11, v13
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn do_not_optimize_integer_aref_with_non_fixnum() {
        eval(r#"
            def test = 1["x"]
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          v11:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v12:StringExact = StringCopy v11
          PatchPoint MethodRedefined(Integer@0x1008, []@0x1010, cme:0x1018)
          v23:BasicObject = CCallVariadic v9, :Integer#[]@0x1040, v12
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_optimize_send_into_fixnum_lt_both_profiled() {
        eval("
            def test(a, b) = a < b
            test(1,2); test(3,4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, <@0x1010, cme:0x1018)
          v27:Fixnum = GuardType v12, Fixnum recompile
          v28:Fixnum = GuardType v13, Fixnum
          v29:BoolExact = FixnumLt v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_optimize_send_into_fixnum_lt_left_profiled() {
        eval("
            def test(a) = a < 1
            test(1); test(3)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, <@0x1010, cme:0x1018)
          v25:Fixnum = GuardType v10, Fixnum recompile
          v26:BoolExact = FixnumLt v25, v14
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_optimize_send_into_fixnum_lt_right_profiled() {
        eval("
            def test(a) = 1 < a
            test(1); test(3)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, <@0x1010, cme:0x1018)
          v26:Fixnum = GuardType v10, Fixnum
          v27:BoolExact = FixnumLt v13, v26
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_optimize_new_range_fixnum_inclusive_literals() {
        eval("
            def test()
              a = 2
              (1..a)
            end
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:Fixnum[2] = Const Value(2)
          v16:Fixnum[1] = Const Value(1)
          v25:RangeExact = NewRangeFixnum v16 NewRangeInclusive v12
          CheckInterrupts
          Return v25
        ");
    }


    #[test]
    fn test_optimize_new_range_fixnum_exclusive_literals() {
        eval("
            def test()
              a = 2
              (1...a)
            end
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:Fixnum[2] = Const Value(2)
          v16:Fixnum[1] = Const Value(1)
          v25:RangeExact = NewRangeFixnum v16 NewRangeExclusive v12
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_optimize_new_range_fixnum_inclusive_high_guarded() {
        eval("
            def test(a)
              (1..a)
            end
            test(2); test(3)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:Fixnum[1] = Const Value(1)
          v22:Fixnum = GuardType v10, Fixnum
          v23:RangeExact = NewRangeFixnum v13 NewRangeInclusive v22
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_optimize_new_range_fixnum_exclusive_high_guarded() {
        eval("
            def test(a)
              (1...a)
            end
            test(2); test(3)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:Fixnum[1] = Const Value(1)
          v22:Fixnum = GuardType v10, Fixnum
          v23:RangeExact = NewRangeFixnum v13 NewRangeExclusive v22
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_optimize_new_range_fixnum_inclusive_low_guarded() {
        eval("
            def test(a)
              (a..10)
            end
            test(2); test(3)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[10] = Const Value(10)
          v22:Fixnum = GuardType v10, Fixnum
          v23:RangeExact = NewRangeFixnum v22 NewRangeInclusive v14
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_optimize_new_range_fixnum_exclusive_low_guarded() {
        eval("
            def test(a)
              (a...10)
            end
            test(2); test(3)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[10] = Const Value(10)
          v22:Fixnum = GuardType v10, Fixnum
          v23:RangeExact = NewRangeFixnum v22 NewRangeExclusive v14
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_do_not_eliminate_comment() {
        let mut function = Function::new(std::ptr::null());
        let block = function.entry_block;

        let comment = function.push_comment(block, "diagnostic".to_string());
        let dead_const = function.push_insn(block, Insn::Const { val: Const::CBool(false) });
        let return_val = function.push_insn(block, Insn::Const { val: Const::CBool(true) });
        function.push_insn(block, Insn::Return { val: return_val });
        function.seal_entries();

        function.eliminate_dead_code();

        let insns = &function.blocks[block.0].insns;
        assert!(insns.contains(&comment));
        assert!(!insns.contains(&dead_const));
    }

    #[test]
    fn test_eliminate_new_array() {
        eval("
            def test()
              c = []
              5
            end
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:ArrayExact = NewArray
          v16:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_opt_aref_array() {
        eval("
            arr = [1,2,3]
            def test(arr) = arr[0]
            test(arr)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :arr@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[0] = Const Value(0)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          v26:ArrayExact = GuardType v10, ArrayExact recompile
          v34:CInt64[0] = Const CInt64(0)
          v28:CInt64 = ArrayLength v26
          v29:CInt64[0] = GuardLess v34, v28
          v33:BasicObject = ArrayAref v26, v29
          CheckInterrupts
          Return v33
        ");
        assert_snapshot!(inspect("test [1,2,3]"), @"1");
    }

    #[test]
    fn test_opt_aref_hash() {
        eval("
            arr = {0 => 4}
            def test(arr) = arr[0]
            test(arr)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :arr@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[0] = Const Value(0)
          PatchPoint NoSingletonClass(Hash@0x1008)
          PatchPoint MethodRedefined(Hash@0x1008, []@0x1010, cme:0x1018)
          v26:HashExact = GuardType v10, HashExact recompile
          v27:BasicObject = HashAref v26, v14
          CheckInterrupts
          Return v27
        ");
        assert_snapshot!(inspect("test({0 => 4})"), @"4");
    }

    #[test]
    fn test_eliminate_new_range() {
        eval("
            def test()
              c = (1..2)
              5
            end
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:RangeExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v16:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_do_not_eliminate_new_range_non_fixnum() {
        eval("
            def test()
              _ = (-'a'..'b')
              0
            end
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          PatchPoint BOPRedefined(STRING_REDEFINED_OP_FLAG, BOP_UMINUS)
          v13:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v15:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v16:StringExact = StringCopy v15
          v18:RangeExact = NewRange v13 NewRangeInclusive v16
          PatchPoint NoEPEscape(test)
          v24:Fixnum[0] = Const Value(0)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_eliminate_new_array_with_elements() {
        eval("
            def test(a)
              c = [a]
              5
            end
            test(1); test(2)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:NilClass = Const Value(nil)
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:NilClass = Const Value(nil)
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:NilClass):
          v17:ArrayExact = NewArray v12
          v21:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v21
        ");
    }

    #[test]
    fn test_eliminate_new_hash() {
        eval("
            def test()
              c = {}
              5
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:HashExact = NewHash
          PatchPoint NoEPEscape(test)
          v18:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_no_eliminate_new_hash_with_elements() {
        eval("
            def test(aval, bval)
              c = {a: aval, b: bval}
              5
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :aval@0x1000
          v4:BasicObject = LoadField v2, :bval@0x1001
          v5:NilClass = Const Value(nil)
          Jump bb3(v1, v3, v4, v5)
        bb2():
          EntryPoint JIT(0)
          v8:BasicObject = LoadArg :self@0
          v9:BasicObject = LoadArg :aval@1
          v10:BasicObject = LoadArg :bval@2
          v11:NilClass = Const Value(nil)
          Jump bb3(v8, v9, v10, v11)
        bb3(v13:BasicObject, v14:BasicObject, v15:BasicObject, v16:NilClass):
          v19:StaticSymbol[:a] = Const Value(VALUE(0x1008))
          v22:StaticSymbol[:b] = Const Value(VALUE(0x1010))
          v25:HashExact = NewHash v19: v14, v22: v15
          PatchPoint NoEPEscape(test)
          v31:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_eliminate_array_dup() {
        eval("
            def test
              c = [1, 2]
              5
            end
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v13:ArrayExact = ArrayDup v12
          v17:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v17
        ");
    }

    #[test]
    fn test_eliminate_hash_dup() {
        eval("
            def test
              c = {a: 1, b: 2}
              5
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:HashExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v13:HashExact = HashDup v12
          v17:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v17
        ");
    }

    #[test]
    fn test_eliminate_putself() {
        eval("
            def test()
              c = self
              5
            end
            test; test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v15:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v15
        ");
    }

    #[test]
    fn test_eliminate_string_copy() {
        eval(r#"
            def test()
              c = "abc"
              5
            end
            test; test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v13:StringExact = StringCopy v12
          v17:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v17
        ");
    }

    #[test]
    fn test_eliminate_fixnum_add() {
        eval("
            def test(a, b)
              a + b
              5
            end
            test(1, 2); test(3, 4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v31:Fixnum = GuardType v12, Fixnum recompile
          v32:Fixnum = GuardType v13, Fixnum
          v23:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_eliminate_fixnum_sub() {
        eval("
            def test(a, b)
              a - b
              5
            end
            test(1, 2); test(3, 4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, -@0x1010, cme:0x1018)
          v31:Fixnum = GuardType v12, Fixnum recompile
          v32:Fixnum = GuardType v13, Fixnum
          v23:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_eliminate_fixnum_mul() {
        eval("
            def test(a, b)
              a * b
              5
            end
            test(1, 2); test(3, 4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, *@0x1010, cme:0x1018)
          v31:Fixnum = GuardType v12, Fixnum recompile
          v32:Fixnum = GuardType v13, Fixnum
          v23:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_do_not_eliminate_fixnum_div() {
        eval("
            def test(a, b)
              a / b
              5
            end
            test(1, 2); test(3, 4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, /@0x1010, cme:0x1018)
          v31:Fixnum = GuardType v12, Fixnum recompile
          v32:Fixnum = GuardType v13, Fixnum
          v33:Integer = FixnumDiv v31, v32
          v23:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_do_not_eliminate_fixnum_mod() {
        eval("
            def test(a, b)
              a % b
              5
            end
            test(1, 2); test(3, 4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, %@0x1010, cme:0x1018)
          v31:Fixnum = GuardType v12, Fixnum recompile
          v32:Fixnum = GuardType v13, Fixnum
          v33:Fixnum = FixnumMod v31, v32
          v23:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_eliminate_fixnum_lt() {
        eval("
            def test(a, b)
              a < b
              5
            end
            test(1, 2); test(3, 4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, <@0x1010, cme:0x1018)
          v31:Fixnum = GuardType v12, Fixnum recompile
          v32:Fixnum = GuardType v13, Fixnum
          v23:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_eliminate_fixnum_le() {
        eval("
            def test(a, b)
              a <= b
              5
            end
            test(1, 2); test(3, 4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, <=@0x1010, cme:0x1018)
          v31:Fixnum = GuardType v12, Fixnum recompile
          v32:Fixnum = GuardType v13, Fixnum
          v23:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_eliminate_fixnum_gt() {
        eval("
            def test(a, b)
              a > b
              5
            end
            test(1, 2); test(3, 4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, >@0x1010, cme:0x1018)
          v31:Fixnum = GuardType v12, Fixnum recompile
          v32:Fixnum = GuardType v13, Fixnum
          v23:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_eliminate_fixnum_ge() {
        eval("
            def test(a, b)
              a >= b
              5
            end
            test(1, 2); test(3, 4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, >=@0x1010, cme:0x1018)
          v31:Fixnum = GuardType v12, Fixnum recompile
          v32:Fixnum = GuardType v13, Fixnum
          v23:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_eliminate_fixnum_eq() {
        eval("
            def test(a, b)
              a == b
              5
            end
            test(1, 2); test(3, 4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, ==@0x1010, cme:0x1018)
          v31:Fixnum = GuardType v12, Fixnum recompile
          v32:Fixnum = GuardType v13, Fixnum
          v23:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_eliminate_fixnum_neq() {
        eval("
            def test(a, b)
              a != b
              5
            end
            test(1, 2); test(3, 4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, !=@0x1010, cme:0x1018)
          v31:Fixnum = GuardType v12, Fixnum recompile
          PatchPoint BOPRedefined(INTEGER_REDEFINED_OP_FLAG, BOP_EQ)
          v33:Fixnum = GuardType v13, Fixnum
          v23:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_do_not_eliminate_get_constant_path() {
        eval("
            def test()
              C
              5
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:BasicObject = GetConstantPath 0x1000
          v13:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_do_not_eliminate_getconstant() {
        eval("
            def test(klass)
              klass::ARGV
              5
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :klass@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :klass@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:FalseClass = Const Value(false)
          v16:BasicObject = GetConstant v10, :ARGV, v14
          v20:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v20
        ");
    }

    #[test]
    fn kernel_itself_const() {
        eval("
            def test(x) = x.itself
            test(0) # profile
            test(1)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, itself@0x1010, cme:0x1018)
          v22:Fixnum = GuardType v10, Fixnum recompile
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn kernel_itself_known_type() {
        eval("
            def test = [].itself
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact = NewArray
          PatchPoint NoSingletonClass(Array@0x1000)
          PatchPoint MethodRedefined(Array@0x1000, itself@0x1008, cme:0x1010)
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn eliminate_kernel_itself() {
        eval("
            def test
              x = [].itself
              1
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:ArrayExact = NewArray
          PatchPoint NoSingletonClass(Array@0x1000)
          PatchPoint MethodRedefined(Array@0x1000, itself@0x1008, cme:0x1010)
          PatchPoint NoEPEscape(test)
          v20:Fixnum[1] = Const Value(1)
          CheckInterrupts
          Return v20
        ");
    }

    #[test]
    fn eliminate_module_name() {
        eval("
            module M; end
            def test
              x = M.name
              1
            end
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, M)
          v14:ModuleExact[M@0x1008] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(Module@0x1010)
          PatchPoint MethodRedefined(Module@0x1010, name@0x1018, cme:0x1020)
          v32:StringExact|NilClass = CCall v14, :Module#name@0x1048
          PatchPoint NoEPEscape(test)
          v22:Fixnum[1] = Const Value(1)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn eliminate_array_length() {
        eval("
            def test
              [].length
              5
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact = NewArray
          PatchPoint NoSingletonClass(Array@0x1000)
          PatchPoint MethodRedefined(Array@0x1000, length@0x1008, cme:0x1010)
          v16:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn normal_class_type_inference() {
        eval("
            class C; end
            def test = C
            test # Warm the constant cache
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, C)
          v11:ClassSubclass[C@0x1008] = Const Value(VALUE(0x1008))
          CheckInterrupts
          Return v11
        ");
    }

    #[test]
    fn core_classes_type_inference() {
        eval("
            def test = [String, Class, Module, BasicObject]
            test # Warm the constant cache
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, String)
          v11:ClassSubclass[String@0x1008] = Const Value(VALUE(0x1008))
          PatchPoint StableConstantNames(0x1010, Class)
          v15:ClassSubclass[Class@0x1018] = Const Value(VALUE(0x1018))
          PatchPoint StableConstantNames(0x1020, Module)
          v19:ClassSubclass[Module@0x1028] = Const Value(VALUE(0x1028))
          PatchPoint StableConstantNames(0x1030, BasicObject)
          v23:ClassSubclass[BasicObject@0x1038] = Const Value(VALUE(0x1038))
          v25:ArrayExact = NewArray v11, v15, v19, v23
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn module_instances_are_module_exact() {
        eval("
            def test = [Enumerable, Kernel]
            test # Warm the constant cache
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Enumerable)
          v11:ModuleExact[Enumerable@0x1008] = Const Value(VALUE(0x1008))
          PatchPoint StableConstantNames(0x1010, Kernel)
          v15:ModuleSubclass[Kernel@0x1018] = Const Value(VALUE(0x1018))
          v17:ArrayExact = NewArray v11, v15
          CheckInterrupts
          Return v17
        ");
    }

    #[test]
    fn module_subclasses_are_not_module_exact() {
        eval("
            class ModuleSubclass < Module; end
            MY_MODULE = ModuleSubclass.new
            def test = MY_MODULE
            test # Warm the constant cache
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, MY_MODULE)
          v11:ModuleSubclass[MY_MODULE@0x1008] = Const Value(VALUE(0x1008))
          CheckInterrupts
          Return v11
        ");
    }

    #[test]
    fn eliminate_array_size() {
        eval("
            def test
              [].size
              5
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact = NewArray
          PatchPoint NoSingletonClass(Array@0x1000)
          PatchPoint MethodRedefined(Array@0x1000, size@0x1008, cme:0x1010)
          v16:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn kernel_itself_argc_mismatch() {
        eval("
            def test = 1.itself(0)
            test rescue 0
            test rescue 0
        ");
        // Not specialized
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          v11:Fixnum[0] = Const Value(0)
          v13:BasicObject = Send v9, :itself, v11 # SendFallbackReason: Argument count does not match parameter count
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_inline_kernel_block_given_p() {
        eval("
            def test = block_given?
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, block_given?@0x1008, cme:0x1010)
          v18:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v19:CPtr = GetEP 0
          v20:RubyValue = LoadField v19, :VM_ENV_DATA_INDEX_SPECVAL@0x1038
          v21:BoolExact = IsBlockGiven v20
          CheckInterrupts
          Return v21
        ");
    }

    #[test]
    fn test_inline_kernel_block_given_p_in_block() {
        eval("
            TEST = proc { block_given? }
            TEST.call
        ");
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn block in <compiled>@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, block_given?@0x1008, cme:0x1010)
          v18:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v19:FalseClass = Const Value(false)
          CheckInterrupts
          Return v19
        ");
    }

    #[test]
    fn test_elide_kernel_block_given_p() {
        eval("
            def test
              block_given?
              5
            end
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, block_given?@0x1008, cme:0x1010)
          v22:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v14:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn const_send_direct_integer() {
        eval("
            def test(x) = 1.zero?
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, zero?@0x1010, cme:0x1018)
          v22:BoolExact = InvokeBuiltin leaf <inline_expr>, v13
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn class_known_send_direct_array() {
        eval("
            def test(x)
              a = [1,2,3]
              a.first
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:NilClass = Const Value(nil)
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:NilClass = Const Value(nil)
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:NilClass):
          v16:ArrayExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v17:ArrayExact = ArrayDup v16
          PatchPoint NoSingletonClass(Array@0x1010)
          PatchPoint MethodRedefined(Array@0x1010, first@0x1018, cme:0x1020)
          v30:BasicObject = InvokeBuiltin leaf <inline_expr>, v17
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn send_direct_to_module() {
        eval("
            module M; end
            def test = M.class
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, M)
          v11:ModuleExact[M@0x1008] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(Module@0x1010)
          PatchPoint MethodRedefined(Module@0x1010, class@0x1018, cme:0x1020)
          v22:ClassSubclass[Module@0x1010] = Const Value(VALUE(0x1010))
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_send_to_instance_method() {
        eval("
            class C
              def foo = []
            end

            def test(c) = c.foo
            c = C.new
            test c
            test c
        ");

        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :c@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :c@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          PushInlineFrame v22 (0x1040)
          v28:ArrayExact = NewArray
          CheckInterrupts
          PopInlineFrame
          Return v28
        ");
    }

    #[test]
    fn test_send_iseq_with_block() {
        let result = eval("
            def foo(a, b, &block) = block.call(a, b)
            def test = foo(1, 2) { |a, b| a + b }
            test
            test
        ");
        assert_eq!(VALUE::fixnum_from_usize(3), result);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v21:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v50:NilClass = Const Value(nil)
          PushInlineFrame v21 (0x1038), v10, v12
          v32:CPtr = GetEP 0
          v33:CUInt64 = LoadField v32, :VM_ENV_DATA_INDEX_FLAGS@0x1060
          v34:CBool = IsBlockParamModified v33
          CondBranch v34, bb6(), bb7()
        bb6():
          v36:BasicObject = LoadField v32, :block@0x1061
          Jump bb8(v36, v36)
        bb7():
          v38:CInt64 = LoadField v32, :VM_ENV_DATA_INDEX_SPECVAL@0x1062
          v39:CInt64 = GuardAnyBitSet v38, CUInt64(1) recompile
          v40:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1068))
          Jump bb8(v40, v50)
        bb8(v30:BasicObject, v31:BasicObject):
          v45:BasicObject = Send v30, :call, v10, v12 # SendFallbackReason: Send: unsupported optimized method type BlockCall
          CheckInterrupts
          PopInlineFrame
          Return v45
        ");
    }

    #[test]
    fn test_yield_no_args_inlines_invocation() {
        // `foo` is inlined into `test`, which passes a literal block, so PushInlineFrame writes
        // that exact block into the frame's EP from a compile-time constant. The yield's block
        // handler is therefore statically known: dispatch has no tag/iseq GuardBitEquals, just an
        // untag (IntAnd -4) feeding InvokeBlockIseqDirect.
        let result = eval("
            def foo = yield
            def test = foo { 42 }
            test
            test
        ");
        assert_eq!(VALUE::fixnum_from_usize(42), result);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v17 (0x1038)
          v23:CPtr = GetEP 0
          v24:CInt64 = LoadField v23, :VM_ENV_DATA_INDEX_SPECVAL@0x1060
          v25:CInt64[-4] = Const CInt64(-4)
          v26:CInt64 = IntAnd v24, v25
          v27:BasicObject = InvokeBlockIseqDirect (0x1068), v26
          CheckInterrupts
          PopInlineFrame
          Return v27
        ");
    }

    #[test]
    fn test_yield_live_stack_below_args_inlines_invocation() {
        // A live value sits on the stack below the yield args (base > 0): the no-receiver-slot
        // SP math must preserve it.
        let result = eval("
            def foo(x) = x + yield(1, 2)
            def test = foo(10) { |a, b| a + b }
            test
            test
        ");
        assert_eq!(VALUE::fixnum_from_usize(13), result);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[10] = Const Value(10)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v19:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v19 (0x1038), v10
          v27:Fixnum[1] = Const Value(1)
          v29:Fixnum[2] = Const Value(2)
          v31:CPtr = GetEP 0
          v32:CInt64 = LoadField v31, :VM_ENV_DATA_INDEX_SPECVAL@0x1060
          v33:CInt64[-4] = Const CInt64(-4)
          v34:CInt64 = IntAnd v32, v33
          v35:BasicObject = InvokeBlockIseqDirect (0x1068), v34, v27, v29
          PatchPoint MethodRedefined(Integer@0x1090, +@0x1098, cme:0x10a0)
          v50:Fixnum = GuardType v35, Fixnum
          v51:Fixnum = FixnumAdd v10, v50
          CheckInterrupts
          PopInlineFrame
          Return v51
        ");
    }

    #[test]
    fn test_yield_with_too_many_args_for_lir_falls_back() {
        // Captured self plus six args don't fit in C argument registers, so the profiled
        // invokeblock specialization must not emit InvokeBlockIseqDirect.
        let result = eval("
            def foo = yield(1, 2, 3, 4, 5, 6)
            def test = foo { |a, b, c, d, e, f| a + b + c + d + e + f }
            test
            test
        ");
        assert_eq!(VALUE::fixnum_from_usize(21), result);
        assert_snapshot!(hir_string("foo"), @"
        fn foo@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          v11:Fixnum[2] = Const Value(2)
          v13:Fixnum[3] = Const Value(3)
          v15:Fixnum[4] = Const Value(4)
          v17:Fixnum[5] = Const Value(5)
          v19:Fixnum[6] = Const Value(6)
          v21:BasicObject = InvokeBlock v9, v11, v13, v15, v17, v19 # SendFallbackReason: Too many arguments for LIR
          CheckInterrupts
          Return v21
        ");
    }

    #[test]
    fn test_inlined_yield_with_too_many_args_for_lir_falls_back() {
        // Same as test_yield_with_too_many_args_for_lir_falls_back, but for the guard-free
        // yield dispatch inside an inlined callee whose caller passes a literal block.
        let result = eval("
            def foo = yield(1, 2, 3, 4, 5, 6)
            def test = foo { |a, b, c, d, e, f| a + b + c + d + e + f }
            test
            test
        ");
        assert_eq!(VALUE::fixnum_from_usize(21), result);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v17 (0x1038)
          v23:Fixnum[1] = Const Value(1)
          v25:Fixnum[2] = Const Value(2)
          v27:Fixnum[3] = Const Value(3)
          v29:Fixnum[4] = Const Value(4)
          v31:Fixnum[5] = Const Value(5)
          v33:Fixnum[6] = Const Value(6)
          v35:BasicObject = InvokeBlock v23, v25, v27, v29, v31, v33 # SendFallbackReason: Too many arguments for LIR
          CheckInterrupts
          PopInlineFrame
          Return v35
        ");
    }

    #[test]
    fn test_yield_lambda_falls_back() {
        // A lambda passed via &l becomes a proc block handler (not imemo_iseq), so it never inlines invocation.
        // Compiles to Send.
        let result = eval("
            def foo = yield(5)
            def test(l) = foo(&l)
            l = ->(x) { x * 10 }
            test(l)
            test(l)
        ");
        assert_eq!(VALUE::fixnum_from_usize(50), result);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :l@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :l@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:BasicObject = Send v9, &block, :foo, v10 # SendFallbackReason: Send: block argument is not nil
          CheckInterrupts
          Return v15
        ");
    }

    #[test]
    fn reload_local_across_send() {
        eval("
            def foo(&block) = 1
            def test
              a = 1
              foo {|| a = 2 }
              a
            end
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v33:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v8, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v34:Fixnum[1] = Const Value(1)
          PatchPoint NoEPEscape(test)
          v20:CPtr = LoadSP
          v21:BasicObject = LoadField v20, :a@0x1038
          CheckInterrupts
          Return v21
        ");
    }

    #[test]
    fn reload_local_across_send_after_ep_escape() {
        eval("
            def foo(&block) = 1
            def test
              a = 1
              lambda { a }
              foo {|| a = 2 }
              a
            end
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          v7:CPtr = GetEP 0
          StoreField v7, :a@0x1000, v6
          Jump bb3(v5, v6)
        bb3(v10:BasicObject, v11:NilClass):
          v14:Fixnum[1] = Const Value(1)
          SetLocal :a, l0, EP@3, v14
          PatchPoint MethodRedefined(Object@0x1008, lambda@0x1010, cme:0x1018)
          v42:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v10, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          v43:BasicObject = CCallWithFrame v42, :Kernel#lambda@0x1040, block=0x1048
          v21:CPtr = GetEP 0
          v22:BasicObject = LoadField v21, :a@0x1000
          PatchPoint MethodRedefined(Object@0x1008, foo@0x1070, cme:0x1078)
          v33:CPtr = GetEP 0
          v34:BasicObject = LoadField v33, :a@0x1000
          CheckInterrupts
          Return v34
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_rest() {
        eval("
            def foo(*args) = args.length
            def test = foo 1, 2, 3
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[2] = Const Value(2)
          v14:Fixnum[3] = Const Value(3)
          v22:ArrayExact = NewArray v10, v12, v14
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v25:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v25 (0x1038), v22
          PatchPoint NoSingletonClass(Array@0x1060)
          PatchPoint MethodRedefined(Array@0x1060, length@0x1068, cme:0x1070)
          v47:CInt64 = ArrayLength v22
          v48:Fixnum = BoxFixnum v47
          CheckInterrupts
          PopInlineFrame
          Return v48
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_many_rest_arguments() {
        eval("
            def foo(*args) = args.length
            def test = foo 1, 2, 3, 4, 5, 6, 7
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[2] = Const Value(2)
          v14:Fixnum[3] = Const Value(3)
          v16:Fixnum[4] = Const Value(4)
          v18:Fixnum[5] = Const Value(5)
          v20:Fixnum[6] = Const Value(6)
          v22:Fixnum[7] = Const Value(7)
          v30:ArrayExact = NewArray v10, v12, v14, v16, v18, v20, v22
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v33:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v33 (0x1038), v30
          PatchPoint NoSingletonClass(Array@0x1060)
          PatchPoint MethodRedefined(Array@0x1060, length@0x1068, cme:0x1070)
          v55:CInt64 = ArrayLength v30
          v56:Fixnum = BoxFixnum v55
          CheckInterrupts
          PopInlineFrame
          Return v56
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_rest_and_block_literal() {
        eval("
            def foo(*args) = yield args.length
            def test = foo(1, 2, 3) { |n| n + 1 }
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[2] = Const Value(2)
          v14:Fixnum[3] = Const Value(3)
          v22:ArrayExact = NewArray v10, v12, v14
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v25:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v25 (0x1038), v22
          PatchPoint NoSingletonClass(Array@0x1060)
          PatchPoint MethodRedefined(Array@0x1060, length@0x1068, cme:0x1070)
          v53:CInt64 = ArrayLength v22
          v54:Fixnum = BoxFixnum v53
          v36:CPtr = GetEP 0
          v37:CInt64 = LoadField v36, :VM_ENV_DATA_INDEX_SPECVAL@0x1098
          v38:CInt64[-4] = Const CInt64(-4)
          v39:CInt64 = IntAnd v37, v38
          v40:BasicObject = InvokeBlockIseqDirect (0x10a0), v39, v54
          CheckInterrupts
          PopInlineFrame
          Return v40
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_rest_and_block_param() {
        eval("
            def foo(*args, &block) = block.call(args.length)
            def test = foo(1, 2, 3) { |n| n + 1 }
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[2] = Const Value(2)
          v14:Fixnum[3] = Const Value(3)
          v22:ArrayExact = NewArray v10, v12, v14
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v25:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v55:NilClass = Const Value(nil)
          PushInlineFrame v25 (0x1038), v22
          v35:CPtr = GetEP 0
          v36:CUInt64 = LoadField v35, :VM_ENV_DATA_INDEX_FLAGS@0x1060
          v37:CBool = IsBlockParamModified v36
          CondBranch v37, bb6(), bb7()
        bb6():
          v39:BasicObject = LoadField v35, :block@0x1061
          Jump bb8(v39, v39)
        bb7():
          v41:CInt64 = LoadField v35, :VM_ENV_DATA_INDEX_SPECVAL@0x1062
          v42:CInt64 = GuardAnyBitSet v41, CUInt64(1) recompile
          v43:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1068))
          Jump bb8(v43, v55)
        bb8(v33:BasicObject, v34:BasicObject):
          PatchPoint NoSingletonClass(Array@0x1070)
          PatchPoint MethodRedefined(Array@0x1070, length@0x1078, cme:0x1080)
          v64:CInt64 = ArrayLength v22
          v65:Fixnum = BoxFixnum v64
          v50:BasicObject = Send v33, :call, v65 # SendFallbackReason: Send: unsupported optimized method type BlockCall
          CheckInterrupts
          PopInlineFrame
          Return v50
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_rest_and_post() {
        eval("
            def foo(a, *args, z) = args.length + a + z
            def test = foo 1, 2, 3, 4
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[2] = Const Value(2)
          v14:Fixnum[3] = Const Value(3)
          v16:Fixnum[4] = Const Value(4)
          v24:ArrayExact = NewArray v12, v14
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v27:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v27 (0x1038), v10, v24, v16
          PatchPoint NoSingletonClass(Array@0x1060)
          PatchPoint MethodRedefined(Array@0x1060, length@0x1068, cme:0x1070)
          v59:CInt64 = ArrayLength v24
          v60:Fixnum = BoxFixnum v59
          PatchPoint MethodRedefined(Integer@0x1098, +@0x10a0, cme:0x10a8)
          v64:Fixnum = FixnumAdd v60, v10
          v68:Fixnum = FixnumAdd v64, v16
          CheckInterrupts
          PopInlineFrame
          Return v68
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_rest_and_keyword() {
        eval("
            def foo(*args, k:) = args.length + k
            def test = foo 1, 2, k: 40
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[2] = Const Value(2)
          v14:Fixnum[40] = Const Value(40)
          v22:ArrayExact = NewArray v10, v12
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v25:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v45:Fixnum[0] = Const Value(0)
          PushInlineFrame v25 (0x1038), v22, v14
          PatchPoint NoSingletonClass(Array@0x1060)
          PatchPoint MethodRedefined(Array@0x1060, length@0x1068, cme:0x1070)
          v54:CInt64 = ArrayLength v22
          v55:Fixnum = BoxFixnum v54
          PatchPoint MethodRedefined(Integer@0x1098, +@0x10a0, cme:0x10a8)
          v59:Fixnum = FixnumAdd v55, v14
          CheckInterrupts
          PopInlineFrame
          Return v59
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_rest_and_optional_keyword_default() {
        eval("
            def foo(*args, k: 40) = args.length + k
            def test = foo 1, 2
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[2] = Const Value(2)
          v20:Fixnum[40] = Const Value(40)
          v21:ArrayExact = NewArray v10, v12
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v24:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v44:Fixnum[0] = Const Value(0)
          PushInlineFrame v24 (0x1038), v21, v20
          PatchPoint NoSingletonClass(Array@0x1060)
          PatchPoint MethodRedefined(Array@0x1060, length@0x1068, cme:0x1070)
          v53:CInt64 = ArrayLength v21
          v54:Fixnum = BoxFixnum v53
          PatchPoint MethodRedefined(Integer@0x1098, +@0x10a0, cme:0x10a8)
          v58:Fixnum = FixnumAdd v54, v20
          CheckInterrupts
          PopInlineFrame
          Return v58
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_optional_and_rest() {
        eval("
            def foo(a, b = 1, *rest) = [a, b, rest]
            def test = foo(10, 20, 30, 40)
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[10] = Const Value(10)
          v12:Fixnum[20] = Const Value(20)
          v14:Fixnum[30] = Const Value(30)
          v16:Fixnum[40] = Const Value(40)
          v24:ArrayExact = NewArray v14, v16
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v27:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v27 (0x1038), v10, v12, v24
          v39:ArrayExact = NewArray v10, v12, v24
          CheckInterrupts
          PopInlineFrame
          Return v39
        ");
    }

    #[test]
    fn specialize_call_to_post_param_iseq() {
        eval("
            def foo(opt=80, post) = post
            def test = foo(10)
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[10] = Const Value(10)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v19:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v19 (0x1038), v10
          v26:Fixnum[80] = Const Value(80)
          CheckInterrupts
          PopInlineFrame
          Return v10
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_optional_between_required_params() {
        let result = eval("
            def foo(lead, opt=80, post) = lead + opt + post
            def test = foo(10, 20)
            test
            test
        ");
        assert_eq!(VALUE::fixnum_from_usize(110), result);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[10] = Const Value(10)
          v12:Fixnum[20] = Const Value(20)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v21:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v21 (0x1038), v10, v12
          v29:Fixnum[80] = Const Value(80)
          PatchPoint MethodRedefined(Integer@0x1060, +@0x1068, cme:0x1070)
          v64:Fixnum[110] = Const Value(110)
          CheckInterrupts
          PopInlineFrame
          Return v64
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_multiple_required_kw() {
        eval("
            def foo(a:, b:) = [a, b]
            def test = foo(a: 1, b: 2)
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v21:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v37:Fixnum[0] = Const Value(0)
          PushInlineFrame v21 (0x1038), v10, v12
          v32:ArrayExact = NewArray v10, v12
          CheckInterrupts
          PopInlineFrame
          Return v32
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_required_kw_reorder() {
        eval("
            def foo(a:, b:, c:) = [a, b, c]
            def test = foo(c: 3, a: 1, b: 2)
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[3] = Const Value(3)
          v12:Fixnum[1] = Const Value(1)
          v14:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v24:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v42:Fixnum[0] = Const Value(0)
          PushInlineFrame v24 (0x1038), v12, v14, v10
          v37:ArrayExact = NewArray v12, v14, v10
          CheckInterrupts
          PopInlineFrame
          Return v37
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_positional_and_required_kw_reorder() {
        eval("
            def foo(x, a:, b:) = [x, a, b]
            def test = foo(0, b: 2, a: 1)
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[0] = Const Value(0)
          v12:Fixnum[2] = Const Value(2)
          v14:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v24:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v42:Fixnum[0] = Const Value(0)
          PushInlineFrame v24 (0x1038), v10, v14, v12
          v37:ArrayExact = NewArray v10, v14, v12
          CheckInterrupts
          PopInlineFrame
          Return v37
        ");
    }

    #[test]
    fn specialize_call_with_positional_and_optional_kw() {
        eval("
            def foo(x, a: 1) = [x, a]
            def test = foo(0, a: 2)
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[0] = Const Value(0)
          v12:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v21:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v37:Fixnum[0] = Const Value(0)
          PushInlineFrame v21 (0x1038), v10, v12
          v32:ArrayExact = NewArray v10, v12
          CheckInterrupts
          PopInlineFrame
          Return v32
        ");
    }

    #[test]
    fn specialize_call_with_pos_optional_and_req_kw() {
        eval("
            def foo(r, x = 2, a:, b:) = [x, a]
            def test = [foo(1, a: 3, b: 4), foo(1, 2, b: 4, a: 3)] # with and without the optional, change kw order
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[3] = Const Value(3)
          v14:Fixnum[4] = Const Value(4)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v36:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v69:Fixnum[0] = Const Value(0)
          PushInlineFrame v36 (0x1038), v10, v12, v14
          v50:Fixnum[2] = Const Value(2)
          v63:ArrayExact = NewArray v50, v12
          CheckInterrupts
          PopInlineFrame
          v19:Fixnum[1] = Const Value(1)
          v21:Fixnum[2] = Const Value(2)
          v23:Fixnum[4] = Const Value(4)
          v25:Fixnum[3] = Const Value(3)
          v90:Fixnum[0] = Const Value(0)
          PushInlineFrame v36 (0x1038), v19, v21, v25, v23
          v85:ArrayExact = NewArray v21, v25
          PopInlineFrame
          v29:ArrayExact = NewArray v63, v85
          Return v29
        ");
    }

    #[test]
    fn specialize_call_with_pos_optional_and_kw_optional() {
        eval("
            def foo(r, x = 2, a:, b: 4) = [r, x, a, b]
            def test = [foo(1, a: 3), foo(1, 2, b: 40, a: 30)] # with and without the optionals
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[3] = Const Value(3)
          v33:Fixnum[4] = Const Value(4)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v36:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v71:Fixnum[0] = Const Value(0)
          PushInlineFrame v36 (0x1038), v10, v12, v33
          v50:Fixnum[2] = Const Value(2)
          v65:ArrayExact = NewArray v10, v50, v12, v33
          CheckInterrupts
          PopInlineFrame
          v17:Fixnum[1] = Const Value(1)
          v19:Fixnum[2] = Const Value(2)
          v21:Fixnum[40] = Const Value(40)
          v23:Fixnum[30] = Const Value(30)
          v94:Fixnum[0] = Const Value(0)
          PushInlineFrame v36 (0x1038), v17, v19, v23, v21
          v89:ArrayExact = NewArray v17, v19, v23, v21
          PopInlineFrame
          v27:ArrayExact = NewArray v65, v89
          Return v27
        ");
    }

    #[test]
    fn test_call_with_pos_optional_and_maybe_too_many_args() {
        eval("
            def target(a = 1, b = 2, c = 3, d = 4, e = 5, f:) = [a, b, c, d, e, f]
            def test = [target(f: 6), target(10, 20, 30, f: 6), target(10, 20, 30, 40, 50, f: 60)]
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[6] = Const Value(6)
          PatchPoint MethodRedefined(Object@0x1000, target@0x1008, cme:0x1010)
          v47:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v48:BasicObject = SendDirect v47, 0x0, :target (0x1038), v10
          v15:Fixnum[10] = Const Value(10)
          v17:Fixnum[20] = Const Value(20)
          v19:Fixnum[30] = Const Value(30)
          v21:Fixnum[6] = Const Value(6)
          PatchPoint MethodRedefined(Object@0x1000, target@0x1008, cme:0x1010)
          v51:BasicObject = SendDirect v47, 0x0, :target (0x1038), jit_entry_idx=3, v15, v17, v19, v21
          v26:Fixnum[10] = Const Value(10)
          v28:Fixnum[20] = Const Value(20)
          v30:Fixnum[30] = Const Value(30)
          v32:Fixnum[40] = Const Value(40)
          v34:Fixnum[50] = Const Value(50)
          v36:Fixnum[60] = Const Value(60)
          v38:BasicObject = Send v47, :target, v26, v28, v30, v32, v34, v36 # SendFallbackReason: Too many arguments for LIR
          v40:ArrayExact = NewArray v48, v51, v38
          CheckInterrupts
          Return v40
        ");
    }

    #[test]
    fn dont_specialize_call_to_rest_with_keyword_to_positional_hash() {
        enable_zjit_stats();
        eval("
            def foo(*args) = args
            def test = foo(k: 1)
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          IncrCounterPtr
          Jump bb3(v4)
        bb3(v7:BasicObject):
          IncrCounter zjit_insn_count
          IncrCounter zjit_insn_count
          v13:Fixnum[1] = Const Value(1)
          IncrCounter zjit_insn_count
          IncrCounter complex_arg_pass_keyword_to_positional_hash
          v16:BasicObject = Send v7, :foo, v13 # SendFallbackReason: Complex argument passing
          IncrCounter zjit_insn_count
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn dont_classify_keyword_to_positional_hash_argc_mismatch_as_complex_arg_pass() {
        eval("
            def foo(a, b) = a
            def test = foo(k: 1)
            begin; test; rescue ArgumentError; end
            begin; test; rescue ArgumentError; end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:BasicObject = Send v6, :foo, v10 # SendFallbackReason: Argument count does not match parameter count
          CheckInterrupts
          Return v12
        ");
    }

    #[test]
    fn test_send_call_to_iseq_with_optional_kw() {
        eval("
            def foo(a: 1) = a
            def test = foo(a: 2)
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v19:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v31:Fixnum[0] = Const Value(0)
          PushInlineFrame v19 (0x1038), v10
          CheckInterrupts
          PopInlineFrame
          Return v10
        ");
    }

    #[test]
    fn dont_specialize_call_to_iseq_with_kwrest() {
        enable_zjit_stats();
        eval("
            def foo(**args) = 1
            def test = foo(a: 1)
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          IncrCounterPtr
          Jump bb3(v4)
        bb3(v7:BasicObject):
          IncrCounter zjit_insn_count
          IncrCounter zjit_insn_count
          v13:Fixnum[1] = Const Value(1)
          IncrCounter zjit_insn_count
          IncrCounter complex_arg_pass_param_kwrest
          v16:BasicObject = Send v7, :foo, v13 # SendFallbackReason: Complex argument passing
          IncrCounter zjit_insn_count
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_send_hash_to_kwarg_only_method() {
        eval(r#"
            def callee(a:) = a
            def test = callee({a: 1})
            begin; test; rescue ArgumentError; end
            begin; test; rescue ArgumentError; end
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:HashExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v11:HashExact = HashDup v10
          v13:BasicObject = Send v6, :callee, v11 # SendFallbackReason: Argument count does not match parameter count
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_send_hash_to_optional_kwarg_only_method() {
        eval(r#"
            def callee(a: nil) = a
            def test = callee({a: 1})
            begin; test; rescue ArgumentError; end
            begin; test; rescue ArgumentError; end
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:HashExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v11:HashExact = HashDup v10
          v13:BasicObject = Send v6, :callee, v11 # SendFallbackReason: Argument count does not match parameter count
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn specialize_call_to_iseq_with_optional_param_kw_using_default() {
        eval("
            def foo(int: 1) = int + 1
            def test = foo
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v16:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v19:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v36:Fixnum[0] = Const Value(0)
          PushInlineFrame v19 (0x1038), v16
          v28:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1060, +@0x1068, cme:0x1070)
          v45:Fixnum[2] = Const Value(2)
          CheckInterrupts
          PopInlineFrame
          Return v45
        ");
    }

    #[test]
    fn dont_specialize_call_to_iseq_with_call_kwsplat() {
        enable_zjit_stats();
        eval("
            def foo(a:) = a
            def test = foo(**{a: 1})
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          IncrCounterPtr
          Jump bb3(v4)
        bb3(v7:BasicObject):
          IncrCounter zjit_insn_count
          IncrCounter zjit_insn_count
          v13:HashExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v14:HashExact = HashDup v13
          IncrCounter zjit_insn_count
          IncrCounter complex_arg_pass_caller_kw_splat
          v17:BasicObject = Send v7, :foo, v14 # SendFallbackReason: Complex argument passing
          IncrCounter zjit_insn_count
          CheckInterrupts
          Return v17
        ");
    }

    #[test]
    fn dont_specialize_call_to_iseq_with_param_kwrest() {
        enable_zjit_stats();
        eval("
            def foo(**kwargs) = kwargs.keys
            def test = foo
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          IncrCounterPtr
          Jump bb3(v4)
        bb3(v7:BasicObject):
          IncrCounter zjit_insn_count
          IncrCounter zjit_insn_count
          IncrCounter complex_arg_pass_param_kwrest
          v13:BasicObject = Send v7, :foo # SendFallbackReason: Complex argument passing
          IncrCounter zjit_insn_count
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn dont_optimize_ccall_with_kwarg() {
        eval("
            def test = sprintf('%s', a: 1)
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v11:StringExact = StringCopy v10
          v13:Fixnum[1] = Const Value(1)
          v15:BasicObject = Send v6, :sprintf, v11, v13 # SendFallbackReason: Complex argument passing
          CheckInterrupts
          Return v15
        ");
    }

    #[test]
    fn dont_optimize_ccall_with_block_and_kwarg() {
        eval("
            def test(s)
              a = []
              s.each_line(chomp: true) { |l| a << l }
              a
            end
            test %(a\nb\nc)
            test %()
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          v4:NilClass = Const Value(nil)
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :s@1
          v9:NilClass = Const Value(nil)
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:NilClass):
          v16:ArrayExact = NewArray
          v21:TrueClass = Const Value(true)
          v23:BasicObject = Send v12, 0x1008, :each_line, v21 # SendFallbackReason: Complex argument passing
          PatchPoint NoEPEscape(test)
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn dont_replace_get_constant_path_with_empty_ic() {
        eval("
            def test = Kernel
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:BasicObject = GetConstantPath 0x1000
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn dont_replace_get_constant_path_with_invalidated_ic() {
        eval("
            def test = Kernel
            test
            Kernel = 5
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:BasicObject = GetConstantPath 0x1000
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn replace_get_constant_path_with_const() {
        eval("
            def test = Kernel
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Kernel)
          v11:ModuleSubclass[Kernel@0x1008] = Const Value(VALUE(0x1008))
          CheckInterrupts
          Return v11
        ");
    }

    #[test]
    fn replace_nested_get_constant_path_with_const() {
        eval("
            module Foo
              module Bar
                class C
                end
              end
            end
            def test = Foo::Bar::C
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:8:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Foo::Bar::C)
          v11:ClassSubclass[Foo::Bar::C@0x1008] = Const Value(VALUE(0x1008))
          CheckInterrupts
          Return v11
        ");
    }

    #[test]
    fn test_opt_new_no_initialize() {
        eval("
            class C; end
            def test = C.new
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, C)
          v11:ClassSubclass[C@0x1008] = Const Value(VALUE(0x1008))
          v13:NilClass = Const Value(nil)
          PatchPoint MethodRedefined(C@0x1008, new@0x1009, cme:0x1010)
          v41:ObjectSubclass[class_exact:C] = ObjectAllocClass C:VALUE(0x1008)
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, initialize@0x1038, cme:0x1040)
          CheckInterrupts
          Return v41
        ");
    }

    #[test]
    fn test_opt_new_initialize() {
        eval("
            class C
              def initialize x
                @x = x
              end
            end
            def test = C.new 1
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, C)
          v11:ClassSubclass[C@0x1008] = Const Value(VALUE(0x1008))
          v13:NilClass = Const Value(nil)
          v16:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(C@0x1008, new@0x1009, cme:0x1010)
          v44:ObjectSubclass[class_exact:C] = ObjectAllocClass C:VALUE(0x1008)
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, initialize@0x1038, cme:0x1040)
          PushInlineFrame v44 (0x1068), v16
          v61:CShape = LoadField v44, :shape_id@0x1090
          v62:CShape[0x1091] = GuardBitEquals v61, CShape(0x1091) recompile
          StoreField v44, :@x@0x1092, v16
          WriteBarrier v44, v16
          v65:CShape[0x1093] = Const CShape(0x1093)
          StoreField v44, :shape_id@0x1090, v65
          CheckInterrupts
          PopInlineFrame
          Return v44
        ");
    }

    #[test]
    fn test_opt_new_object() {
        eval("
            def test = Object.new
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Object)
          v11:ClassSubclass[Object@0x1008] = Const Value(VALUE(0x1008))
          v13:NilClass = Const Value(nil)
          PatchPoint MethodRedefined(Object@0x1008, new@0x1009, cme:0x1010)
          v41:ObjectExact = ObjectAllocClass Object:VALUE(0x1008)
          PatchPoint NoSingletonClass(Object@0x1008)
          PatchPoint MethodRedefined(Object@0x1008, initialize@0x1038, cme:0x1040)
          CheckInterrupts
          Return v41
        ");
    }

    #[test]
    fn test_opt_new_basic_object() {
        eval("
            def test = BasicObject.new
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, BasicObject)
          v11:ClassSubclass[BasicObject@0x1008] = Const Value(VALUE(0x1008))
          v13:NilClass = Const Value(nil)
          PatchPoint MethodRedefined(BasicObject@0x1008, new@0x1009, cme:0x1010)
          v41:BasicObjectExact = ObjectAllocClass BasicObject:VALUE(0x1008)
          PatchPoint NoSingletonClass(BasicObject@0x1008)
          PatchPoint MethodRedefined(BasicObject@0x1008, initialize@0x1038, cme:0x1040)
          CheckInterrupts
          Return v41
        ");
    }

    #[test]
    fn test_opt_new_hash() {
        eval("
            def test = Hash.new
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Hash)
          v11:ClassSubclass[Hash@0x1008] = Const Value(VALUE(0x1008))
          v13:NilClass = Const Value(nil)
          PatchPoint MethodRedefined(Hash@0x1008, new@0x1009, cme:0x1010)
          v41:HashExact = ObjectAllocClass Hash:VALUE(0x1008)
          v42:Fixnum[0] = Const Value(0)
          PatchPoint NoSingletonClass(Hash@0x1008)
          PatchPoint MethodRedefined(Hash@0x1008, initialize@0x1038, cme:0x1040)
          v92:Fixnum[0] = Const Value(0)
          v93:NilClass = Const Value(nil)
          PushInlineFrame v41 (0x1068), v42
          v60:TrueClass = Const Value(true)
          v77:CPtr = GetEP 0
          v78:CUInt64 = LoadField v77, :VM_ENV_DATA_INDEX_FLAGS@0x1090
          v79:CBool = IsBlockParamModified v78
          CondBranch v79, bb11(), bb12()
        bb11():
          v81:BasicObject = LoadField v77, :block@0x1091
          Jump bb13(v81)
        bb12():
          v83:BasicObject = GetBlockParam :block, l0, EP@4
          Jump bb13(v83)
        bb13(v76:BasicObject):
          v86:BasicObject = InvokeBuiltin rb_hash_init, v41, v42, v60, v60, v76
          CheckInterrupts
          PopInlineFrame
          Return v41
        ");
        assert_snapshot!(inspect("test"), @"{}");
    }

    #[test]
    fn test_opt_new_array() {
        eval("
            def test = Array.new 1
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Array)
          v11:ClassSubclass[Array@0x1008] = Const Value(VALUE(0x1008))
          v13:NilClass = Const Value(nil)
          v16:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Array@0x1008, new@0x1009, cme:0x1010)
          PatchPoint MethodRedefined(Class@0x1038, new@0x1009, cme:0x1010)
          v52:BasicObject = CCallVariadic v11, :Array.new@0x1040, v16
          CheckInterrupts
          Return v52
        ");
    }

    #[test]
    fn test_opt_new_set() {
        eval("
            def test = Set.new
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Set)
          v11:ClassSubclass[Set@0x1008] = Const Value(VALUE(0x1008))
          v13:NilClass = Const Value(nil)
          PatchPoint MethodRedefined(Set@0x1008, new@0x1009, cme:0x1010)
          v18:HeapBasicObject = ObjectAlloc v11
          PatchPoint NoSingletonClass(Set@0x1008)
          PatchPoint MethodRedefined(Set@0x1008, initialize@0x1038, cme:0x1040)
          v44:SetExact = GuardType v18, SetExact recompile
          v45:BasicObject = CCallVariadic v44, :Set#initialize@0x1068
          CheckInterrupts
          Return v44
        ");
    }

    #[test]
    fn test_opt_new_string() {
        eval("
            def test = String.new
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, String)
          v11:ClassSubclass[String@0x1008] = Const Value(VALUE(0x1008))
          v13:NilClass = Const Value(nil)
          PatchPoint MethodRedefined(String@0x1008, new@0x1009, cme:0x1010)
          PatchPoint MethodRedefined(Class@0x1038, new@0x1009, cme:0x1010)
          v49:BasicObject = CCallVariadic v11, :String.new@0x1040
          CheckInterrupts
          Return v49
        ");
    }

    #[test]
    fn test_opt_new_regexp() {
        eval("
            def test = Regexp.new ''
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Regexp)
          v11:ClassSubclass[Regexp@0x1008] = Const Value(VALUE(0x1008))
          v13:NilClass = Const Value(nil)
          v16:StringExact[VALUE(0x1010)] = Const Value(VALUE(0x1010))
          v17:StringExact = StringCopy v16
          PatchPoint MethodRedefined(Regexp@0x1008, new@0x1018, cme:0x1020)
          v45:RegexpExact = ObjectAllocClass Regexp:VALUE(0x1008)
          PatchPoint NoSingletonClass(Regexp@0x1008)
          PatchPoint MethodRedefined(Regexp@0x1008, initialize@0x1048, cme:0x1050)
          v50:BasicObject = CCallVariadic v45, :Regexp#initialize@0x1078, v17
          CheckInterrupts
          Return v45
        ");
    }

    #[test]
    fn test_inline_class_allocate() {
        eval("
            class C; end
            def test = C.allocate
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, C)
          v11:ClassSubclass[C@0x1008] = Const Value(VALUE(0x1008))
          PatchPoint MethodRedefined(Class@0x1010, allocate@0x1018, cme:0x1020)
          v22:ObjectSubclass[class_exact:C] = ObjectAllocClass C:VALUE(0x1008)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_dont_inline_class_allocate_with_args() {
        eval("
            class C; end
            def test = C.allocate(1)
            test rescue 0
            test rescue 0
        ");
        // Not specialized
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, C)
          v11:ClassSubclass[C@0x1008] = Const Value(VALUE(0x1008))
          v13:Fixnum[1] = Const Value(1)
          v15:BasicObject = Send v11, :allocate, v13 # SendFallbackReason: Argument count does not match parameter count
          CheckInterrupts
          Return v15
        ");
    }

    #[test]
    fn test_dont_inline_class_allocate_with_singleton_class() {
        eval("
            class C; end
            SC = C.singleton_class
            def test = SC.allocate
            test rescue 0
        ");
        // Not specialized: singleton classes are not leaf allocators
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, SC)
          v11:ClassSubclass[Class@0x1008] = Const Value(VALUE(0x1008))
          PatchPoint MethodRedefined(Class@0x1010, allocate@0x1018, cme:0x1020)
          v22:BasicObject = CCallWithFrame v11, :Class.allocate@0x1048
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_opt_length() {
        eval("
            def test(a,b) = [a,b].length
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          v18:ArrayExact = NewArray v12, v13
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, length@0x1010, cme:0x1018)
          v30:CInt64 = ArrayLength v18
          v31:Fixnum = BoxFixnum v30
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_opt_size() {
        eval("
            def test(a,b) = [a,b].size
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          v18:ArrayExact = NewArray v12, v13
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, size@0x1010, cme:0x1018)
          v30:CInt64 = ArrayLength v18
          v31:Fixnum = BoxFixnum v30
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_getblockparamproxy() {
        eval("
            def test(&block) = tap(&block)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :block@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :block@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v16:CPtr = GetEP 0
          v17:CUInt64 = LoadField v16, :VM_ENV_DATA_INDEX_FLAGS@0x1001
          v18:CBool = IsBlockParamModified v17
          CondBranch v18, bb4(), bb5()
        bb4():
          v20:BasicObject = LoadField v16, :block@0x1002
          Jump bb6(v20, v20)
        bb5():
          v22:CInt64 = LoadField v16, :VM_ENV_DATA_INDEX_SPECVAL@0x1003
          v23:CInt64 = GuardAnyBitSet v22, CUInt64(1) recompile
          v24:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1008))
          Jump bb6(v24, v10)
        bb6(v14:BasicObject, v15:BasicObject):
          SideExit NoProfileSend recompile
        ");
    }

    #[test]
    fn test_getblockparamproxy_proc() {
        eval("
            val = proc { 1 }
            def test(&block)
              0.then(&block)
            end
            test(&val)
        ");
        assert_contains_opcode("test", YARVINSN_getblockparamproxy);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :block@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :block@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:Fixnum[0] = Const Value(0)
          v17:CPtr = GetEP 0
          v18:CUInt64 = LoadField v17, :VM_ENV_DATA_INDEX_FLAGS@0x1001
          v19:CBool = IsBlockParamModified v18
          CondBranch v19, bb4(), bb5()
        bb4():
          v21:BasicObject = LoadField v17, :block@0x1002
          Jump bb6(v21, v21)
        bb5():
          v23:BasicObject = LoadField v17, :VM_ENV_DATA_INDEX_SPECVAL@0x1003
          v24:BasicObject = CCall v23, :rb_obj_is_proc@0x1004
          v25:TrueClass = GuardBitEquals v24, Value(true) recompile
          Jump bb6(v23, v10)
        bb6(v15:BasicObject, v16:BasicObject):
          v28:BasicObject = Send v13, &block, :then, v15 # SendFallbackReason: Send: block argument is not nil
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_recompile_no_profile_getblockparamproxy() {
        eval("
            def test(flag, &block)
              if flag
                0.then(&block)
              else
                :skip
              end
            end
            test(false)
            test(false)
            test(true)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :flag@0x1000
          v4:BasicObject = LoadField v2, :block@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :flag@1
          v9:BasicObject = LoadArg :block@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          v17:CBool = Test v12
          v18:Falsy = RefineType v12, Falsy
          CondBranch v17, bb5(), bb4(v11, v18, v13)
        bb5():
          v20:Truthy = RefineType v12, Truthy
          v23:Fixnum[0] = Const Value(0)
          v27:CPtr = GetEP 0
          v28:CUInt64 = LoadField v27, :VM_ENV_DATA_INDEX_FLAGS@0x1002
          v29:CBool = IsBlockParamModified v28
          CondBranch v29, bb6(), bb7()
        bb6():
          v31:BasicObject = LoadField v27, :block@0x1003
          Jump bb8(v31, v31)
        bb7():
          v33:CInt64 = LoadField v27, :VM_ENV_DATA_INDEX_SPECVAL@0x1004
          v34:CInt64[0] = GuardBitEquals v33, CInt64(0) recompile
          v35:NilClass = Const Value(nil)
          Jump bb8(v35, v13)
        bb8(v25:BasicObject, v26:BasicObject):
          v54:NilClass = GuardBitEquals v25, Value(nil) recompile
          PatchPoint MethodRedefined(Integer@0x1008, then@0x1010, cme:0x1018)
          PushInlineFrame v23 (0x1040)
          v73:BasicObject = InvokeBuiltin <inline_expr>, v23
          CheckInterrupts
          PopInlineFrame
          Return v73
        bb4(v43:BasicObject, v44:Falsy, v45:BasicObject):
          v48:StaticSymbol[:skip] = Const Value(VALUE(0x1068))
          CheckInterrupts
          Return v48
        ");
    }

    #[test]
    fn test_getblockparamproxy_modified() {
        eval("
            def test(&block)
              b = block
              tap(&block)
            end
        ");
        assert_contains_opcode("test", YARVINSN_getblockparamproxy);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :block@0x1000
          v4:NilClass = Const Value(nil)
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :block@1
          v9:NilClass = Const Value(nil)
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:NilClass):
          v17:CPtr = GetEP 0
          v18:CUInt64 = LoadField v17, :VM_ENV_DATA_INDEX_FLAGS@0x1001
          v19:CBool = IsBlockParamModified v18
          CondBranch v19, bb4(), bb5()
        bb4():
          v21:BasicObject = LoadField v17, :block@0x1002
          Jump bb6(v21)
        bb5():
          v23:BasicObject = GetBlockParam :block, l0, EP@4
          Jump bb6(v23)
        bb6(v16:BasicObject):
          v31:CPtr = GetEP 0
          v32:CUInt64 = LoadField v31, :VM_ENV_DATA_INDEX_FLAGS@0x1001
          v33:CBool = IsBlockParamModified v32
          CondBranch v33, bb7(), bb8()
        bb7():
          v35:BasicObject = LoadField v31, :block@0x1002
          Jump bb9(v35, v35)
        bb8():
          v37:CInt64 = LoadField v31, :VM_ENV_DATA_INDEX_SPECVAL@0x1003
          v38:CInt64 = GuardAnyBitSet v37, CUInt64(1) recompile
          v39:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1008))
          Jump bb9(v39, v16)
        bb9(v29:BasicObject, v30:BasicObject):
          SideExit NoProfileSend recompile
        ");
    }

    #[test]
    fn test_getblockparamproxy_modified_nested_block() {
        eval("
            def test(&block)
              proc do
                b = block
                tap(&block)
              end
            end
        ");
        assert_snapshot!(hir_string_proc("test"), @"
        fn block in test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v13:CPtr = GetEP 1
          v14:CUInt64 = LoadField v13, :VM_ENV_DATA_INDEX_FLAGS@0x1000
          v15:CBool = IsBlockParamModified v14
          CondBranch v15, bb4(), bb5()
        bb4():
          v17:BasicObject = LoadField v13, :block@0x1001
          Jump bb6(v17)
        bb5():
          v19:BasicObject = GetBlockParam :block, l1, EP@3
          Jump bb6(v19)
        bb6(v12:BasicObject):
          v26:CPtr = GetEP 1
          v27:CUInt64 = LoadField v26, :VM_ENV_DATA_INDEX_FLAGS@0x1000
          v28:CBool = IsBlockParamModified v27
          CondBranch v28, bb7(), bb8()
        bb7():
          v30:BasicObject = LoadField v26, :block@0x1001
          Jump bb9(v30)
        bb8():
          v32:CInt64 = LoadField v26, :VM_ENV_DATA_INDEX_SPECVAL@0x1002
          v33:CInt64 = GuardAnyBitSet v32, CUInt64(1) recompile
          v34:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1008))
          Jump bb9(v34)
        bb9(v25:BasicObject):
          SideExit NoProfileSend recompile
        ");
    }

    #[test]
    fn test_getblockparamproxy_polymorphic_none_and_iseq() {
        set_call_threshold(3);
        eval("
            def test(&block)
              0.then(&block)
            end

            test
            test { 1 }
        ");
        assert_contains_opcode("test", YARVINSN_getblockparamproxy);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :block@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :block@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:Fixnum[0] = Const Value(0)
          v17:CPtr = GetEP 0
          v18:CUInt64 = LoadField v17, :VM_ENV_DATA_INDEX_FLAGS@0x1001
          v19:CBool = IsBlockParamModified v18
          CondBranch v19, bb4(), bb5()
        bb4():
          v21:BasicObject = LoadField v17, :block@0x1002
          Jump bb6(v21, v21)
        bb5():
          v23:CInt64 = LoadField v17, :VM_ENV_DATA_INDEX_SPECVAL@0x1003
          v24:CInt64[1] = Const CInt64(1)
          v25:CInt64 = IntAnd v23, v24
          v26:CBool = IsBitEqual v25, v24
          CondBranch v26, bb7(), bb9()
        bb7():
          v28:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1008))
          Jump bb6(v28, v10)
        bb9():
          v30:CInt64[0] = Const CInt64(0)
          v31:CBool = IsBitEqual v23, v30
          CondBranch v31, bb8(), bb10()
        bb8():
          v33:NilClass = Const Value(nil)
          Jump bb6(v33, v10)
        bb6(v15:BasicObject, v16:BasicObject):
          v37:BasicObject = Send v13, &block, :then, v15 # SendFallbackReason: Send: block argument is not nil
          CheckInterrupts
          Return v37
        bb10():
          SideExit BlockParamProxyProfileNotCovered
        ");
    }

    #[test]
    fn test_getblockparamproxy_polymorphic_none_and_iseq_and_proc() {
        set_call_threshold(4);
        eval("
            val = proc { 3 }
            def test(&block)
              0.then(&block)
            end
            test
            test { 1 }
            test(&val)
        ");
        assert_contains_opcode("test", YARVINSN_getblockparamproxy);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :block@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :block@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:Fixnum[0] = Const Value(0)
          v17:CPtr = GetEP 0
          v18:CUInt64 = LoadField v17, :VM_ENV_DATA_INDEX_FLAGS@0x1001
          v19:CBool = IsBlockParamModified v18
          CondBranch v19, bb4(), bb5()
        bb4():
          v21:BasicObject = LoadField v17, :block@0x1002
          Jump bb6(v21, v21)
        bb5():
          v23:CInt64 = LoadField v17, :VM_ENV_DATA_INDEX_SPECVAL@0x1003
          v25:BasicObject = LoadField v17, :VM_ENV_DATA_INDEX_SPECVAL@0x1003
          v26:BasicObject = CCall v25, :rb_obj_is_proc@0x1004
          v27:TrueClass = Const Value(true)
          v28:CBool = IsBitEqual v26, v27
          CondBranch v28, bb7(), bb11()
        bb7():
          Jump bb6(v25, v10)
        bb11():
          v31:CInt64[0] = Const CInt64(0)
          v32:CBool = IsBitEqual v23, v31
          CondBranch v32, bb8(), bb12()
        bb8():
          v34:NilClass = Const Value(nil)
          Jump bb6(v34, v10)
        bb12():
          v36:CInt64[1] = Const CInt64(1)
          v37:CInt64 = IntAnd v23, v36
          v38:CBool = IsBitEqual v37, v36
          CondBranch v38, bb9(), bb13()
        bb9():
          v40:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1008))
          Jump bb6(v40, v10)
        bb6(v15:BasicObject, v16:BasicObject):
          v44:BasicObject = Send v13, &block, :then, v15 # SendFallbackReason: Send: block argument is not nil
          CheckInterrupts
          Return v44
        bb13():
          SideExit BlockParamProxyProfileNotCovered
        ");
    }

    #[test]
    fn test_getblockparam() {
        eval("
            def test(&block) = block
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :block@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :block@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:CPtr = GetEP 0
          v15:CUInt64 = LoadField v14, :VM_ENV_DATA_INDEX_FLAGS@0x1001
          v16:CBool = IsBlockParamModified v15
          CondBranch v16, bb4(), bb5()
        bb4():
          v18:BasicObject = LoadField v14, :block@0x1002
          Jump bb6(v18)
        bb5():
          v20:BasicObject = GetBlockParam :block, l0, EP@3
          Jump bb6(v20)
        bb6(v13:BasicObject):
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_getblockparam_nested_block() {
        eval("
            def test(&block)
              proc do
                block
              end
            end
        ");
        assert_snapshot!(hir_string_proc("test"), @"
        fn block in test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:CPtr = GetEP 1
          v11:CUInt64 = LoadField v10, :VM_ENV_DATA_INDEX_FLAGS@0x1000
          v12:CBool = IsBlockParamModified v11
          CondBranch v12, bb4(), bb5()
        bb4():
          v14:BasicObject = LoadField v10, :block@0x1001
          Jump bb6(v14)
        bb5():
          v16:BasicObject = GetBlockParam :block, l1, EP@3
          Jump bb6(v16)
        bb6(v9:BasicObject):
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_setblockparam() {
        eval("
            def test(&block)
              block = nil
            end
        ");
        assert_contains_opcode("test", YARVINSN_setblockparam);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :block@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :block@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:NilClass = Const Value(nil)
          SetLocal :block, l0, EP@3, v13
          v17:CPtr = GetEP 0
          v18:CInt64 = LoadField v17, :VM_ENV_DATA_INDEX_FLAGS@0x1001
          v19:CInt64[512] = Const CInt64(512)
          v20:CInt64 = IntOr v18, v19
          StoreField v17, :VM_ENV_DATA_INDEX_FLAGS@0x1001, v20
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_setblockparam_nested_block() {
        eval("
            def test(&block)
              proc do
                block = nil
              end
            end
        ");
        assert_snapshot!(hir_string_proc("test"), @"
        fn block in test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:NilClass = Const Value(nil)
          SetLocal :block, l1, EP@3, v9
          v13:CPtr = GetEP 1
          v14:CInt64 = LoadField v13, :VM_ENV_DATA_INDEX_FLAGS@0x1000
          v15:CInt64[512] = Const CInt64(512)
          v16:CInt64 = IntOr v14, v15
          StoreField v13, :VM_ENV_DATA_INDEX_FLAGS@0x1000, v16
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_getinstancevariable() {
        eval("
            def test = @foo
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          SideExit NoProfileGetIvar recompile
        ");
    }

    #[test]
    fn test_setinstancevariable() {
        eval("
            def test = @foo = 1
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          PatchPoint SingleRactorMode
          v13:HeapBasicObject = GuardType v6, HeapBasicObject
          SideExit NoProfileSetIvar recompile
        ");
    }

    #[test]
    fn test_specialize_monomorphic_definedivar_true() {
        eval("
            @foo = 4
            def test = defined?(@foo)
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:HeapBasicObject = GuardType v6, HeapBasicObject
          v10:CShape = LoadField v9, :shape_id@0x1000
          v11:CShape[0x1001] = GuardBitEquals v10, CShape(0x1001) recompile
          v12:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          CheckInterrupts
          Return v12
        ");
    }

    #[test]
    fn test_specialize_monomorphic_definedivar_false() {
        eval("
            def test = defined?(@foo)
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:HeapBasicObject = GuardType v6, HeapBasicObject
          v10:CShape = LoadField v9, :shape_id@0x1000
          v11:CShape[0x1001] = GuardBitEquals v10, CShape(0x1001) recompile
          v12:NilClass = Const Value(nil)
          CheckInterrupts
          Return v12
        ");
    }

    #[test]
    fn test_specialize_proc_call() {
        eval("
            p = proc { |x| x + 1 }
            def test(p)
              p.call(1)
            end
            test p
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :p@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :p@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[1] = Const Value(1)
          PatchPoint NoSingletonClass(Proc@0x1008)
          PatchPoint MethodRedefined(Proc@0x1008, call@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact:Proc] = GuardType v10, ObjectSubclass[class_exact:Proc] recompile
          v25:BasicObject = InvokeProc v24, v14
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_specialize_proc_aref() {
        eval("
            p = proc { |x| x + 1 }
            def test(p)
              p[2]
            end
            test p
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :p@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :p@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[2] = Const Value(2)
          PatchPoint NoSingletonClass(Proc@0x1008)
          PatchPoint MethodRedefined(Proc@0x1008, []@0x1010, cme:0x1018)
          v25:ObjectSubclass[class_exact:Proc] = GuardType v10, ObjectSubclass[class_exact:Proc] recompile
          v26:BasicObject = InvokeProc v25, v14
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_specialize_proc_yield() {
        eval("
            p = proc { |x| x + 1 }
            def test(p)
              p.yield(3)
            end
            test p
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :p@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :p@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[3] = Const Value(3)
          PatchPoint NoSingletonClass(Proc@0x1008)
          PatchPoint MethodRedefined(Proc@0x1008, yield@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact:Proc] = GuardType v10, ObjectSubclass[class_exact:Proc] recompile
          v25:BasicObject = InvokeProc v24, v14
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_specialize_proc_eqq() {
        eval("
            p = proc { |x| x > 0 }
            def test(p)
              p === 1
            end
            test p
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :p@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :p@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[1] = Const Value(1)
          PatchPoint NoSingletonClass(Proc@0x1008)
          PatchPoint MethodRedefined(Proc@0x1008, ===@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact:Proc] = GuardType v10, ObjectSubclass[class_exact:Proc] recompile
          v25:BasicObject = InvokeProc v24, v14
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_dont_specialize_proc_call_splat() {
        eval("
            p = proc { }
            def test(p)
              empty = []
              p.call(*empty)
            end
            test p
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :p@0x1000
          v4:NilClass = Const Value(nil)
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :p@1
          v9:NilClass = Const Value(nil)
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:NilClass):
          v16:ArrayExact = NewArray
          v22:ArrayExact = ToArray v16
          v24:BasicObject = Send v12, :call, v22 # SendFallbackReason: Complex argument passing
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_dont_specialize_proc_call_kwarg() {
        eval("
            p = proc { |a:| a }
            def test(p)
              p.call(a: 1)
            end
            test p
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :p@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :p@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[1] = Const Value(1)
          v16:BasicObject = Send v10, :call, v14 # SendFallbackReason: Complex argument passing
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_dont_specialize_definedivar_with_immediate() {
        eval("
            module M
              def test = defined?(@a)
            end

            class Integer
              include M
            end

            1.test
            2.test
            TEST = M.instance_method(:test)
        ");
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact|NilClass = DefinedIvar v6, :@a
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_dont_specialize_definedivar_with_t_struct() {
        // Range is T_STRUCT (not T_OBJECT): falls back to DefinedIvar.
        eval("
            class C < Range
              def test = defined?(@a)
            end
            obj = C.new 0, 1
            obj.instance_variable_set(:@a, 1)
            obj.test
            TEST = C.instance_method(:test)
        ");
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact|NilClass = DefinedIvar v6, :@a
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_optimize_definedivar_polymorphic() {
        set_call_threshold(3);
        eval("
            class C
              def test = defined?(@a)
            end
            obj = C.new
            obj.instance_variable_set(:@a, 1)
            obj.test
            obj = C.new
            obj.instance_variable_set(:@b, 1)
            obj.test
            TEST = C.instance_method(:test)
        ");
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v9, :shape_id@0x1000
          v12:CShape[0x1001] = Const CShape(0x1001)
          v13:CBool = IsBitEqual v11, v12
          CondBranch v13, bb5(), bb6()
        bb5():
          v15:NilClass = Const Value(nil)
          Jump bb4(v15)
        bb6():
          v17:CShape = LoadField v9, :shape_id@0x1000
          v18:CShape[0x1002] = Const CShape(0x1002)
          v19:CBool = IsBitEqual v17, v18
          CondBranch v19, bb7(), bb8()
        bb7():
          v21:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          Jump bb4(v21)
        bb8():
          v23:StringExact|NilClass = DefinedIvar v9, :@a
          Jump bb4(v23)
        bb4(v10:StringExact|NilClass):
          CheckInterrupts
          Return v10
        ");
    }

    // Two consecutive polymorphic `defined?` on the same `self` must both get
    // inline shape branches. Specializing the first rewrites `self` to a GuardType
    // wrapper, so `polymorphic_summary` must peel it (`chase_insn`, not `find_const`)
    // to match the profile entry; otherwise the second falls back to a generic DefinedIvar.
    #[test]
    fn test_optimize_two_consecutive_definedivar_polymorphic() {
        set_call_threshold(3);
        eval("
            class C
              def test = [defined?(@a), defined?(@b)]
            end
            obj = C.new
            obj.instance_variable_set(:@a, 1)
            obj.instance_variable_set(:@b, 1)
            obj.test
            obj = C.new
            obj.instance_variable_set(:@x, 1)
            obj.instance_variable_set(:@a, 1)
            obj.instance_variable_set(:@b, 1)
            obj.test
            TEST = C.instance_method(:test)
        ");
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v9, :shape_id@0x1000
          v12:CShape[0x1001] = Const CShape(0x1001)
          v13:CBool = IsBitEqual v11, v12
          CondBranch v13, bb5(), bb6()
        bb5():
          v15:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          Jump bb4(v15)
        bb6():
          v17:CShape = LoadField v9, :shape_id@0x1000
          v18:CShape[0x1010] = Const CShape(0x1010)
          v19:CBool = IsBitEqual v17, v18
          CondBranch v19, bb7(), bb8()
        bb7():
          v21:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          Jump bb4(v21)
        bb8():
          v23:StringExact|NilClass = DefinedIvar v9, :@a
          Jump bb4(v23)
        bb4(v10:StringExact|NilClass):
          v28:CShape = LoadField v9, :shape_id@0x1000
          v29:CShape[0x1001] = Const CShape(0x1001)
          v30:CBool = IsBitEqual v28, v29
          CondBranch v30, bb10(), bb11()
        bb10():
          v32:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          Jump bb9(v32)
        bb11():
          v34:CShape = LoadField v9, :shape_id@0x1000
          v35:CShape[0x1010] = Const CShape(0x1010)
          v36:CBool = IsBitEqual v34, v35
          CondBranch v36, bb12(), bb13()
        bb12():
          v38:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          Jump bb9(v38)
        bb13():
          v40:StringExact|NilClass = DefinedIvar v9, :@b
          Jump bb9(v40)
        bb9(v27:StringExact|NilClass):
          v43:ArrayExact = NewArray v10, v27
          CheckInterrupts
          Return v43
        ");
    }

    #[test]
    fn test_optimize_definedivar_polymorphic_with_immediate() {
        set_call_threshold(3);
        eval(r#"
            module M
              def test = defined?(@a)
            end

            class C
              include M
            end

            class Integer
              include M
            end

            obj = C.new
            obj.instance_variable_set(:@a, 1)

            obj.test
            1.test
            TEST = M.instance_method(:test)
        "#);
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v9, :shape_id@0x1000
          v12:CShape[0x1001] = Const CShape(0x1001)
          v13:CBool = IsBitEqual v11, v12
          CondBranch v13, bb5(), bb6()
        bb5():
          v15:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          Jump bb4(v15)
        bb6():
          v17:StringExact|NilClass = DefinedIvar v9, :@a
          Jump bb4(v17)
        bb4(v10:StringExact|NilClass):
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_optimize_definedivar_polymorphic_with_t_struct() {
        set_call_threshold(3);
        eval(r#"
            module M
              def test = defined?(@a)
            end

            class C
              include M
            end

            class D < Range
              include M
            end

            obj = C.new
            obj.instance_variable_set(:@a, 1)

            range = D.new 0, 1
            range.instance_variable_set(:@a, 1)

            obj.test
            range.test
            TEST = M.instance_method(:test)
        "#);
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v9, :shape_id@0x1000
          v12:CShape[0x1001] = Const CShape(0x1001)
          v13:CBool = IsBitEqual v11, v12
          CondBranch v13, bb5(), bb6()
        bb5():
          v15:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          Jump bb4(v15)
        bb6():
          v17:StringExact|NilClass = DefinedIvar v9, :@a
          Jump bb4(v17)
        bb4(v10:StringExact|NilClass):
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_optimize_definedivar_polymorphic_with_complex_shape() {
        set_call_threshold(3);
        eval(r#"
            module M
              def test = defined?(@a)
            end

            class C
              include M
            end

            class D
              include M
            end

            obj = C.new
            obj.instance_variable_set(:@a, 1)

            complex = D.new
            (0..1000).each do |i|
              complex.instance_variable_set(:"@v#{i}", i)
            end
            (0..1000).each do |i|
              complex.remove_instance_variable(:"@v#{i}")
            end

            obj.test
            complex.test
            TEST = M.instance_method(:test)
        "#);
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v9, :shape_id@0x1000
          v12:CShape[0x1001] = Const CShape(0x1001)
          v13:CBool = IsBitEqual v11, v12
          CondBranch v13, bb5(), bb6()
        bb5():
          v15:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          Jump bb4(v15)
        bb6():
          v17:StringExact|NilClass = DefinedIvar v9, :@a
          Jump bb4(v17)
        bb4(v10:StringExact|NilClass):
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_dont_specialize_complex_shape_definedivar() {
        eval(r#"
            class C
              def test = defined?(@a)
            end
            obj = C.new
            (0..1000).each do |i|
              obj.instance_variable_set(:"@v#{i}", i)
            end
            (0..1000).each do |i|
              obj.remove_instance_variable(:"@v#{i}")
            end
            obj.test
            TEST = C.instance_method(:test)
        "#);
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact|NilClass = DefinedIvar v6, :@a
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_specialize_monomorphic_setivar_already_in_shape() {
        eval("
            @foo = 4
            def test = @foo = 5
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[5] = Const Value(5)
          PatchPoint SingleRactorMode
          v13:HeapBasicObject = GuardType v6, HeapBasicObject
          v14:CShape = LoadField v13, :shape_id@0x1000
          v15:CShape[0x1001] = GuardBitEquals v14, CShape(0x1001) recompile
          StoreField v13, :@foo@0x1002, v9
          WriteBarrier v13, v9
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_specialize_monomorphic_setivar_on_extended_robject() {
        let obj = eval(r#"
            class ExtendedSetIvar
              def test(value)
                @v0 = value
              end
            end

            OBJ = ExtendedSetIvar.new
            10.times { |i| OBJ.instance_variable_set(:"@v#{i}", i) }
            OBJ.test(10)
            TEST = ExtendedSetIvar.instance_method(:test)
            OBJ
        "#);
        assert!(obj.layout() == ShapeLayout::Extended);

        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :value@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :value@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint SingleRactorMode
          v16:HeapBasicObject = GuardType v9, HeapBasicObject
          v17:CShape = LoadField v16, :shape_id@0x1001
          v18:CShape[0x1002] = GuardBitEquals v17, CShape(0x1002) recompile
          v19:BasicObject = LoadField v16, :as_heap@0x1003
          StoreField v19, :@v0@0x1003, v10
          WriteBarrier v19, v10
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_specialize_monomorphic_setivar_with_shape_transition() {
        eval("
            def test = @foo = 5
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[5] = Const Value(5)
          PatchPoint SingleRactorMode
          v13:HeapBasicObject = GuardType v6, HeapBasicObject
          v14:CShape = LoadField v13, :shape_id@0x1000
          v15:CShape[0x1001] = GuardBitEquals v14, CShape(0x1001) recompile
          StoreField v13, :@foo@0x1002, v9
          WriteBarrier v13, v9
          v18:CShape[0x1003] = Const CShape(0x1003)
          StoreField v13, :shape_id@0x1000, v18
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_specialize_monomorphic_setivar_on_final_version() {
        set_max_versions(2);
        set_inline_threshold(0);
        eval("
            class FinalSetIvar
              def test(x)
                @foo = 5
                x + 1
              end
            end

            obj = FinalSetIvar.new
            30.times { obj.test(1) }
            30.times { obj.test(1.5) }
        ");

        let hir = hir_string_proc("FinalSetIvar.new.method(:test)");
        assert!(hir.contains("CondBranch"), "{hir}");
        assert!(hir.contains("StoreField"), "{hir}");
        assert!(hir.contains("SetIvar"), "{hir}");
        assert!(!hir.contains("GuardBitEquals"), "{hir}");
    }

    #[test]
    fn test_specialize_multiple_monomorphic_setivar_with_shape_transition() {
        eval(r#"
            klass = Class.new do
              def test
                @foo = 1
                @bar = 2
              end
            end

            # Grow class max_iv_count so fresh instances can keep both writes
            # on the embedded fast path.
            warm = klass.new
            warm.instance_variable_set(:@warm1, 1)
            warm.instance_variable_set(:@warm2, 2)

            obj = klass.new
            obj.test
            TEST = klass.instance_method(:test)
        "#);
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          PatchPoint SingleRactorMode
          v12:HeapBasicObject = GuardType v6, HeapBasicObject
          v13:CShape = LoadField v12, :shape_id@0x1000
          v14:CShape[0x1001] = GuardBitEquals v13, CShape(0x1001) recompile
          StoreField v12, :@foo@0x1002, v9
          WriteBarrier v12, v9
          v17:CShape[0x1003] = Const CShape(0x1003)
          StoreField v12, :shape_id@0x1000, v17
          v22:Fixnum[2] = Const Value(2)
          PatchPoint SingleRactorMode
          StoreField v12, :@bar@0x1004, v22
          WriteBarrier v12, v22
          v31:CShape[0x1005] = Const CShape(0x1005)
          StoreField v12, :shape_id@0x1000, v31
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_dont_specialize_setivar_with_t_data() {
        eval("
            class C < Range
              def test = @a = 5
            end
            obj = C.new 0, 1
            obj.instance_variable_set(:@a, 1)
            obj.test
            TEST = C.instance_method(:test)
        ");
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[5] = Const Value(5)
          PatchPoint SingleRactorMode
          SetIvar v6, :@a, v9
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_specialize_polymorphic_setivar() {
        set_call_threshold(3);
        eval("
            class C
              def test = @a = 5
            end
            obj = C.new
            obj.instance_variable_set(:@a, 1)
            obj.test
            obj = C.new
            obj.instance_variable_set(:@b, 1)
            obj.test
            TEST = C.instance_method(:test)
        ");
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[5] = Const Value(5)
          PatchPoint SingleRactorMode
          v13:HeapBasicObject = GuardType v6, HeapBasicObject
          v14:CShape = LoadField v13, :shape_id@0x1000
          v15:CShape[0x1001] = Const CShape(0x1001)
          v16:CBool = IsBitEqual v14, v15
          CondBranch v16, bb5(), bb6()
        bb5():
          StoreField v13, :@a@0x1002, v9
          WriteBarrier v13, v9
          v20:CShape[0x1003] = Const CShape(0x1003)
          StoreField v13, :shape_id@0x1000, v20
          Jump bb4()
        bb6():
          v23:CShape[0x1004] = GuardBitEquals v14, CShape(0x1004) recompile
          StoreField v13, :@a@0x1005, v9
          WriteBarrier v13, v9
          Jump bb4()
        bb4():
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_dont_specialize_complex_shape_setivar() {
        eval(r#"
            class C
              def test = @a = 5
            end
            obj = C.new
            (0..1000).each do |i|
              obj.instance_variable_set(:"@v#{i}", i)
            end
            (0..1000).each do |i|
              obj.remove_instance_variable(:"@v#{i}")
            end
            obj.test
            TEST = C.instance_method(:test)
        "#);
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[5] = Const Value(5)
          PatchPoint SingleRactorMode
          SetIvar v6, :@a, v9
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_dont_specialize_setivar_when_next_shape_is_complex() {
        eval(r#"
            class AboutToBeTooComplex
              def test = @abc = 5
            end
            SHAPE_MAX_VARIATIONS = 8  # see shape.h
            SHAPE_MAX_VARIATIONS.times do
              AboutToBeTooComplex.new.instance_variable_set(:"@a#{_1}", 1)
            end
            AboutToBeTooComplex.new.test
            TEST = AboutToBeTooComplex.instance_method(:test)
        "#);
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[5] = Const Value(5)
          PatchPoint SingleRactorMode
          SetIvar v6, :@abc, v9
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_elide_freeze_with_frozen_hash() {
        eval("
            def test = {}.freeze
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(HASH_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:HashExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_dont_optimize_hash_freeze_if_redefined() {
        eval("
            class Hash
              def freeze; end
            end
            def test = {}.freeze
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          SideExit PatchPoint(BOPRedefined(HASH_REDEFINED_OP_FLAG, BOP_FREEZE))
        ");
    }

    #[test]
    fn test_elide_freeze_with_refrozen_hash() {
        eval("
            def test = {}.freeze.freeze
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(HASH_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:HashExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_no_elide_freeze_with_unfrozen_hash() {
        eval("
            def test = {}.dup.freeze
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:HashExact = NewHash
          PatchPoint NoSingletonClass(Hash@0x1000)
          PatchPoint MethodRedefined(Hash@0x1000, dup@0x1008, cme:0x1010)
          v22:BasicObject = CCallWithFrame v9, :Kernel#dup@0x1038
          v13:BasicObject = Send v22, :freeze # SendFallbackReason: Uncategorized(opt_send_without_block)
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_no_elide_freeze_hash_with_args() {
        eval("
            def test = {}.freeze(nil)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:HashExact = NewHash
          v11:NilClass = Const Value(nil)
          v13:BasicObject = Send v9, :freeze, v11 # SendFallbackReason: Argument count does not match parameter count
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_elide_freeze_with_frozen_ary() {
        eval("
            def test = [].freeze
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(ARRAY_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_elide_freeze_with_refrozen_ary() {
        eval("
            def test = [].freeze.freeze
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(ARRAY_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_no_elide_freeze_with_unfrozen_ary() {
        eval("
            def test = [].dup.freeze
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact = NewArray
          PatchPoint NoSingletonClass(Array@0x1000)
          PatchPoint MethodRedefined(Array@0x1000, dup@0x1008, cme:0x1010)
          v22:BasicObject = CCallWithFrame v9, :Kernel#dup@0x1038
          v13:BasicObject = Send v22, :freeze # SendFallbackReason: Uncategorized(opt_send_without_block)
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_no_elide_freeze_ary_with_args() {
        eval("
            def test = [].freeze(nil)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact = NewArray
          v11:NilClass = Const Value(nil)
          v13:BasicObject = Send v9, :freeze, v11 # SendFallbackReason: Argument count does not match parameter count
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_elide_freeze_with_frozen_str() {
        eval("
            def test = ''.freeze
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(STRING_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_elide_freeze_with_refrozen_str() {
        eval("
            def test = ''.freeze.freeze
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(STRING_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_no_elide_freeze_with_unfrozen_str() {
        eval("
            def test = ''.dup.freeze
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v10:StringExact = StringCopy v9
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, dup@0x1010, cme:0x1018)
          v23:BasicObject = CCallWithFrame v10, :String#dup@0x1040
          v14:BasicObject = Send v23, :freeze # SendFallbackReason: Uncategorized(opt_send_without_block)
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn test_no_elide_freeze_str_with_args() {
        eval("
            def test = ''.freeze(nil)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v10:StringExact = StringCopy v9
          v12:NilClass = Const Value(nil)
          v14:BasicObject = Send v10, :freeze, v12 # SendFallbackReason: Argument count does not match parameter count
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn test_elide_uminus_with_frozen_str() {
        eval("
            def test = -''
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(STRING_REDEFINED_OP_FLAG, BOP_UMINUS)
          v10:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_elide_uminus_with_refrozen_str() {
        eval("
            def test = -''.freeze
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(STRING_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          PatchPoint BOPRedefined(STRING_REDEFINED_OP_FLAG, BOP_UMINUS)
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_no_elide_uminus_with_unfrozen_str() {
        eval("
            def test = -''.dup
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v10:StringExact = StringCopy v9
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, dup@0x1010, cme:0x1018)
          v23:BasicObject = CCallWithFrame v10, :String#dup@0x1040
          v14:BasicObject = Send v23, :-@ # SendFallbackReason: Uncategorized(opt_send_without_block)
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn test_objtostring_anytostring_string() {
        eval(r##"
            def test = "#{('foo')}"
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v12:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v13:StringExact = StringCopy v12
          v33:StringExact = StringConcat v9, v13
          CheckInterrupts
          Return v33
        ");
    }

    #[test]
    fn test_objtostring_anytostring_with_non_string() {
        eval(r##"
            def test = "#{1}"
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v11:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, to_s@0x1010, cme:0x1018)
          v39:StringExact = CCallVariadic v11, :Integer#to_s@0x1040
          v31:StringExact = StringConcat v9, v39
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_optimize_objtostring_anytostring_recv_profiled() {
        eval("
            def test(a)
              \"#{a}\"
            end
            test('foo'); test('foo')
        ");

        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(String@0x1010)
          v18:String = GuardType v10, String
          v28:StringExact = StringConcat v13, v18
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_optimize_objtostring_anytostring_recv_profiled_string_subclass() {
        eval("
            class MyString < String; end

            def test(a)
              \"#{a}\"
            end
            foo = MyString.new('foo')
            test(MyString.new(foo)); test(MyString.new(foo))
        ");

        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(MyString@0x1010)
          v18:String = GuardType v10, String
          v28:StringExact = StringConcat v13, v18
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_optimize_objtostring_profiled_nonstring_falls_back_to_send() {
        eval("
            def test(a)
              \"#{a}\"
            end
            test([1,2,3]); test([1,2,3]) # No fast path for array
        ");

        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v17:ArrayExact = GuardType v10, ArrayExact
          PatchPoint NoSingletonClass(Array@0x1010)
          PatchPoint MethodRedefined(Array@0x1010, to_s@0x1018, cme:0x1020)
          v38:BasicObject = CCallWithFrame v17, :Array#to_s@0x1048
          v20:CBool = HasType v38, String
          CondBranch v20, bb4(), bb5()
        bb4():
          v22:String = RefineType v38, String
          Jump bb6(v22)
        bb5():
          v24:StringExact = AnyToString v10
          Jump bb6(v24)
        bb6(v26:String):
          v28:StringExact = StringConcat v13, v26
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_branchnil_nil() {
        eval("
            def test
              x = nil
              x&.itself
            end
        ");

        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v19:NilClass = Const Value(nil)
          CheckInterrupts
          Return v19
        ");
    }

    #[test]
    fn test_branchnil_truthy() {
        eval("
            def test
              x = 1
              x&.itself
            end
        ");

        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1000, itself@0x1008, cme:0x1010)
          CheckInterrupts
          Return v12
        ");
    }

    #[test]
    fn test_dont_eliminate_load_from_non_frozen_array() {
        eval(r##"
            S = [4,5,6]
            def test = S[0]
            test
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, S)
          v11:ArrayExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v13:Fixnum[0] = Const Value(0)
          PatchPoint NoSingletonClass(Array@0x1010)
          PatchPoint MethodRedefined(Array@0x1010, []@0x1018, cme:0x1020)
          v33:CInt64[0] = Const CInt64(0)
          v27:CInt64 = ArrayLength v11
          v28:CInt64[0] = GuardLess v33, v27
          v32:BasicObject = ArrayAref v11, v28
          CheckInterrupts
          Return v32
        ");
       // TODO(max): Check the result of `S[0] = 5; test` using `inspect` to make sure that we
       // actually do the load at run-time.
    }

    #[test]
    fn test_eliminate_load_from_frozen_array_in_bounds() {
        eval(r##"
            def test = [4,5,6].freeze[1]
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(ARRAY_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v12:Fixnum[1] = Const Value(1)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          v33:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v33
        ");
    }

    #[test]
    fn test_eliminate_load_from_frozen_array_negative() {
        eval(r##"
            def test = [4,5,6].freeze[-3]
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(ARRAY_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v12:Fixnum[-3] = Const Value(-3)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          v31:CInt64[-3] = Const CInt64(-3)
          v32:CInt64[3] = Const CInt64(3)
          v27:CInt64 = AdjustBounds v31, v32
          v28:CInt64[0] = Const CInt64(0)
          v29:CInt64 = GuardGreaterEq v27, v28
          v30:BasicObject = ArrayAref v10, v29
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_eliminate_load_from_frozen_array_negative_out_of_bounds() {
        eval(r##"
            def test = [4,5,6].freeze[-10]
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(ARRAY_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v12:Fixnum[-10] = Const Value(-10)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          v31:CInt64[-10] = Const CInt64(-10)
          v32:CInt64[3] = Const CInt64(3)
          v27:CInt64 = AdjustBounds v31, v32
          v28:CInt64[0] = Const CInt64(0)
          v29:CInt64 = GuardGreaterEq v27, v28
          v30:BasicObject = ArrayAref v10, v29
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_eliminate_load_from_frozen_array_out_of_bounds() {
        eval(r##"
            def test = [4,5,6].freeze[10]
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(ARRAY_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v12:Fixnum[10] = Const Value(10)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          SideExit GuardLess
        ");
    }

    #[test]
    fn test_dont_optimize_array_aref_if_redefined() {
        eval(r##"
            class Array
              def [](index) = []
            end
            def test = [4,5,6].freeze[10]
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(ARRAY_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v12:Fixnum[10] = Const Value(10)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          PushInlineFrame v10 (0x1040), v12
          v29:ArrayExact = NewArray
          CheckInterrupts
          PopInlineFrame
          Return v29
        ");
    }

    #[test]
    fn test_dont_optimize_array_aset_if_redefined() {
        eval(r##"
            class Array
              def []=(*args); :redefined; end
            end

            def test(arr)
              arr[1] = 10
            end
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :arr@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v16:Fixnum[1] = Const Value(1)
          v18:Fixnum[10] = Const Value(10)
          SideExit NoProfileSend recompile
        ");
    }

    #[test]
    fn test_dont_optimize_array_max_if_redefined() {
        eval(r##"
            class Array
              def max = []
            end
            def test = [4,5,6].max
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v10:ArrayExact = ArrayDup v9
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, max@0x1010, cme:0x1018)
          PushInlineFrame v10 (0x1040)
          v25:ArrayExact = NewArray
          CheckInterrupts
          PopInlineFrame
          Return v25
        ");
    }

    #[test]
    fn test_optimize_array_max() {
        eval(r##"
            def test(a,b) = [a,b].max
        "##);
        assert_contains_opcode("test", YARVINSN_opt_newarray_send);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint BOPRedefined(ARRAY_REDEFINED_OP_FLAG, BOP_MAX)
          v19:BasicObject = ArrayMax v12, v13
          CheckInterrupts
          Return v19
        ");
    }

    #[test]
    fn test_optimize_array_min() {
        eval(r##"
            def test(a,b) = [a,b].min
        "##);
        assert_contains_opcode("test", YARVINSN_opt_newarray_send);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint BOPRedefined(ARRAY_REDEFINED_OP_FLAG, BOP_MIN)
          v19:BasicObject = ArrayMin v12, v13
          CheckInterrupts
          Return v19
        ");
    }

    #[test]
    fn test_dont_optimize_array_min_if_redefined() {
        eval(r##"
            class Array
              def min = []
            end
            def test = [4,5,6].min
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v10:ArrayExact = ArrayDup v9
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, min@0x1010, cme:0x1018)
          PushInlineFrame v10 (0x1040)
          v25:ArrayExact = NewArray
          CheckInterrupts
          PopInlineFrame
          Return v25
        ");
    }

    #[test]
    fn test_set_type_from_constant() {
        eval("
            MY_SET = Set.new

            def test = MY_SET

            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, MY_SET)
          v11:SetExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          CheckInterrupts
          Return v11
        ");
    }

    #[test]
    fn test_regexp_type() {
        eval("
            def test = /a/
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:RegexpExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_bmethod_send_direct() {
        eval("
            define_method(:zero) { :b }
            define_method(:one) { |arg| arg }

            def test = one(zero)
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint MethodRedefined(Object@0x1000, zero@0x1008, cme:0x1010)
          v21:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v22:StaticSymbol[:b] = Const Value(VALUE(0x1038))
          PatchPoint MethodRedefined(Object@0x1000, one@0x1040, cme:0x1048)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_symbol_block_bmethod() {
        eval("
            define_method(:identity, &:itself)
            def test = identity(100)
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[100] = Const Value(100)
          v12:BasicObject = Send v6, :identity, v10 # SendFallbackReason: Bmethod: Proc object is not defined by an ISEQ
          CheckInterrupts
          Return v12
        ");
    }

    #[test]
    fn test_call_bmethod_with_block() {
        eval("
            define_method(:bmethod) { :b }
            def test = (bmethod {})
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:BasicObject = Send v6, 0x1000, :bmethod # SendFallbackReason: Send: unsupported method type Bmethod
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_call_shareable_bmethod() {
        eval("
            class Foo
              class << self
                define_method(:identity, &(Ractor.make_shareable ->(val){val}))
              end
            end
            def test = Foo.identity(100)
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Foo)
          v11:ClassSubclass[Foo@0x1008] = Const Value(VALUE(0x1008))
          v13:Fixnum[100] = Const Value(100)
          PatchPoint MethodRedefined(Class@0x1010, identity@0x1018, cme:0x1020)
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_nil_nil_specialized_to_ccall() {
        eval("
            def test = nil.nil?
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:NilClass = Const Value(nil)
          PatchPoint MethodRedefined(NilClass@0x1000, nil?@0x1008, cme:0x1010)
          v20:TrueClass = Const Value(true)
          CheckInterrupts
          Return v20
        ");
    }

    #[test]
    fn test_eliminate_nil_nil_specialized_to_ccall() {
        eval("
            def test
              nil.nil?
              1
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:NilClass = Const Value(nil)
          PatchPoint MethodRedefined(NilClass@0x1000, nil?@0x1008, cme:0x1010)
          v16:Fixnum[1] = Const Value(1)
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_non_nil_nil_specialized_to_ccall() {
        eval("
            def test = 1.nil?
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1000, nil?@0x1008, cme:0x1010)
          v20:FalseClass = Const Value(false)
          CheckInterrupts
          Return v20
        ");
    }

    #[test]
    fn test_eliminate_non_nil_nil_specialized_to_ccall() {
        eval("
            def test
              1.nil?
              2
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1000, nil?@0x1008, cme:0x1010)
          v16:Fixnum[2] = Const Value(2)
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_guard_nil_for_nil_opt() {
        eval("
            def test(val) = val.nil?

            test(nil)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :val@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :val@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(NilClass@0x1008, nil?@0x1010, cme:0x1018)
          v23:NilClass = GuardType v10, NilClass recompile
          v24:TrueClass = Const Value(true)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_guard_false_for_nil_opt() {
        eval("
            def test(val) = val.nil?

            test(false)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :val@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :val@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(FalseClass@0x1008, nil?@0x1010, cme:0x1018)
          v23:FalseClass = GuardType v10, FalseClass recompile
          v24:FalseClass = Const Value(false)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_guard_true_for_nil_opt() {
        eval("
            def test(val) = val.nil?

            test(true)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :val@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :val@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(TrueClass@0x1008, nil?@0x1010, cme:0x1018)
          v23:TrueClass = GuardType v10, TrueClass recompile
          v24:FalseClass = Const Value(false)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_guard_symbol_for_nil_opt() {
        eval("
            def test(val) = val.nil?

            test(:foo)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :val@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :val@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Symbol@0x1008, nil?@0x1010, cme:0x1018)
          v23:StaticSymbol = GuardType v10, StaticSymbol recompile
          v24:FalseClass = Const Value(false)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_guard_fixnum_for_nil_opt() {
        eval("
            def test(val) = val.nil?

            test(1)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :val@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :val@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, nil?@0x1010, cme:0x1018)
          v23:Fixnum = GuardType v10, Fixnum recompile
          v24:FalseClass = Const Value(false)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_guard_float_for_nil_opt() {
        eval("
            def test(val) = val.nil?

            test(1.0)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :val@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :val@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, nil?@0x1010, cme:0x1018)
          v23:Flonum = GuardType v10, Flonum recompile
          v24:FalseClass = Const Value(false)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_guard_string_for_nil_opt() {
        eval("
            def test(val) = val.nil?

            test('foo')
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :val@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :val@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, nil?@0x1010, cme:0x1018)
          v24:StringExact = GuardType v10, StringExact recompile
          v25:FalseClass = Const Value(false)
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_specialize_basicobject_not_truthy() {
        eval("
            def test(a) = !a

            test([])
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, !@0x1010, cme:0x1018)
          v24:ArrayExact = GuardType v10, ArrayExact recompile
          v25:FalseClass = Const Value(false)
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_specialize_basicobject_not_false() {
        eval("
            def test(a) = !a

            test(false)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(FalseClass@0x1008, !@0x1010, cme:0x1018)
          v23:FalseClass = GuardType v10, FalseClass recompile
          v24:TrueClass = Const Value(true)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_specialize_basicobject_not_nil() {
        eval("
            def test(a) = !a

            test(nil)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(NilClass@0x1008, !@0x1010, cme:0x1018)
          v23:NilClass = GuardType v10, NilClass recompile
          v24:TrueClass = Const Value(true)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_specialize_basicobject_not_falsy() {
        eval("
            def test(a) = !(if a then false else nil end)

            # TODO(max): Make this not GuardType NilClass and instead just reason
            # statically
            test(false)
            test(true)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:CBool = Test v10
          v15:Falsy = RefineType v10, Falsy
          CondBranch v14, bb6(), bb4(v9, v15)
        bb6():
          v17:Truthy = RefineType v10, Truthy
          v19:FalseClass = Const Value(false)
          Jump bb5(v9, v17, v19)
        bb4(v22:BasicObject, v23:Falsy):
          v25:NilClass = Const Value(nil)
          Jump bb5(v22, v23, v25)
        bb5(v27:BasicObject, v28:BasicObject, v29:Falsy):
          v33:CBool = HasType v29, FalseClass
          CondBranch v33, bb8(), bb9()
        bb8():
          PatchPoint MethodRedefined(FalseClass@0x1008, !@0x1010, cme:0x1018)
          v54:TrueClass = Const Value(true)
          Jump bb7(v54)
        bb9():
          v39:CBool = HasType v29, NilClass
          CondBranch v39, bb10(), bb11()
        bb10():
          PatchPoint MethodRedefined(NilClass@0x1040, !@0x1010, cme:0x1018)
          v57:TrueClass = Const Value(true)
          Jump bb7(v57)
        bb11():
          v45:BasicObject = Send v29, :! # SendFallbackReason: Send: polymorphic call site
          Jump bb7(v45)
        bb7(v32:BasicObject):
          CheckInterrupts
          Return v32
        ");
    }

    #[test]
    fn test_specialize_array_empty_p() {
        eval("
            def test(a) = a.empty?

            test([])
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, empty?@0x1010, cme:0x1018)
          v24:ArrayExact = GuardType v10, ArrayExact recompile
          v25:CInt64 = ArrayLength v24
          v26:CInt64[0] = Const CInt64(0)
          v27:CBool = IsBitEqual v25, v26
          v28:BoolExact = BoxBool v27
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_specialize_hash_empty_p_to_ccall() {
        eval("
            def test(a) = a.empty?

            test({})
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(Hash@0x1008)
          PatchPoint MethodRedefined(Hash@0x1008, empty?@0x1010, cme:0x1018)
          v24:HashExact = GuardType v10, HashExact recompile
          v25:BoolExact = CCall v24, :Hash#empty?@0x1040
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_specialize_basic_object_eq_to_ccall() {
        eval("
            class C; end
            def test(a, b) = a == b

            test(C.new, C.new)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, ==@0x1010, cme:0x1018)
          v28:ObjectSubclass[class_exact:C] = GuardType v12, ObjectSubclass[class_exact:C] recompile
          v29:CBool = IsBitEqual v28, v13
          v30:BoolExact = BoxBool v29
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_guard_fixnum_and_fixnum() {
        eval("
            def test(x, y) = x & y

            test(1, 2)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, &@0x1010, cme:0x1018)
          v27:Fixnum = GuardType v12, Fixnum recompile
          v28:Fixnum = GuardType v13, Fixnum
          v29:Fixnum = FixnumAnd v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_guard_fixnum_or_fixnum() {
        eval("
            def test(x, y) = x | y

            test(1, 2)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, |@0x1010, cme:0x1018)
          v27:Fixnum = GuardType v12, Fixnum recompile
          v28:Fixnum = GuardType v13, Fixnum
          v29:Fixnum = FixnumOr v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_method_redefinition_patch_point_on_top_level_method() {
        eval("
            def foo; end
            def test = foo

            test; test
        ");

        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v18:NilClass = Const Value(nil)
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_optimize_getivar_embedded() {
        eval("
            class C
              attr_reader :foo
              def initialize
                @foo = 42
              end
            end

            O = C.new
            def test(o) = o.foo
            test O
            test O
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:10:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v24:CShape = LoadField v22, :shape_id@0x1040
          v25:CShape[0x1041] = GuardBitEquals v24, CShape(0x1041) recompile
          v26:BasicObject = LoadField v22, :@foo@0x1042
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_optimize_getivar_complex() {
        eval(r#"
            class C
              attr_reader :foo
              def initialize
                1000.times do |i|
                  instance_variable_set("@v#{i}", i)
                end
                @foo = 42
              end
            end

            O = C.new
            def test(o) = o.foo
            test O
            test O
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:13:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v23:BasicObject = GetIvar v22, :@foo
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_getivar_shape_guard_recompile() {
        // Call with one shape to compile, then call with a different shape to
        // trigger shape guard exits and recompilation. On the recompiled version,
        // GetIvar stays as a C call because iseq_to_hir handles polymorphic
        // branching at parse time for getinstancevariable.
        eval("
            class C
              def initialize(extra = false)
                @bar = 0 if extra  # changes the shape
                @foo = 42
              end
              def foo = @foo
            end

            c = C.new
            c.foo  # profile
            c.foo  # compile (version 1 with shape guard)
            d = C.new(true)  # same class, different shape
            100.times { d.foo }  # trigger shape guard exits -> recompile
            100.times { c.foo }  # run recompiled version (version 2)
        ");
        // After recompilation, iseq_to_hir generates polymorphic branches at
        // parse time using the exit-profiled shapes: two optimized LoadField
        // fast paths plus a GetIvar C call fallback.
        assert_snapshot!(hir_string_proc("C.new.method(:foo)"), @"
        fn foo@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:HeapBasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:HeapBasicObject):
          PatchPoint SingleRactorMode
          v11:CShape = LoadField v6, :shape_id@0x1000
          v13:CShape[0x1001] = Const CShape(0x1001)
          v14:CBool = IsBitEqual v11, v13
          CondBranch v14, bb5(), bb6()
        bb5():
          v16:BasicObject = LoadField v6, :@foo@0x1002
          Jump bb4(v16)
        bb6():
          v18:CShape[0x1003] = GuardBitEquals v11, CShape(0x1003) recompile
          v20:BasicObject = LoadField v6, :@foo@0x1004
          Jump bb4(v20)
        bb4(v12:BasicObject):
          CheckInterrupts
          Return v12
        ");
    }

    // The following tests pin down the soundness boundary of the `self:
    // HeapBasicObject` inference (see `iseq_self_is_heap_object`). A `def` method
    // gets `self: HeapBasicObject` only when its owning class can never produce an
    // immediate receiver. For each class below, `self` must stay `BasicObject`:
    // the six immediate classes have no default allocator, and Object/BasicObject/
    // Numeric use the default allocator but are ancestors of immediates (caught by
    // the Integer kind_of check). Each test reopens the class, compiles the method
    // (call threshold is 30), then checks the resulting `self` type.

    #[test]
    fn test_self_not_heap_object_owner_integer() {
        eval("
            class Integer
              def probe = @foo
            end
            100.times { 5.probe }
        ");
        assert_snapshot!(hir_string_proc("5.method(:probe)"), @"
        fn probe@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          SideExit NoProfileGetIvar recompile
        ");
    }

    #[test]
    fn test_self_not_heap_object_owner_symbol() {
        eval("
            class Symbol
              def probe = @foo
            end
            100.times { :sym.probe }
        ");
        assert_snapshot!(hir_string_proc(":sym.method(:probe)"), @"
        fn probe@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          SideExit NoProfileGetIvar recompile
        ");
    }

    #[test]
    fn test_self_not_heap_object_owner_float() {
        eval("
            class Float
              def probe = @foo
            end
            100.times { 1.5.probe }
        ");
        assert_snapshot!(hir_string_proc("1.5.method(:probe)"), @"
        fn probe@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          SideExit NoProfileGetIvar recompile
        ");
    }

    #[test]
    fn test_self_not_heap_object_owner_nil_class() {
        eval("
            class NilClass
              def probe = @foo
            end
            100.times { nil.probe }
        ");
        assert_snapshot!(hir_string_proc("nil.method(:probe)"), @"
        fn probe@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          SideExit NoProfileGetIvar recompile
        ");
    }

    #[test]
    fn test_self_not_heap_object_owner_true_class() {
        eval("
            class TrueClass
              def probe = @foo
            end
            100.times { true.probe }
        ");
        assert_snapshot!(hir_string_proc("true.method(:probe)"), @"
        fn probe@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          SideExit NoProfileGetIvar recompile
        ");
    }

    #[test]
    fn test_self_not_heap_object_owner_false_class() {
        eval("
            class FalseClass
              def probe = @foo
            end
            100.times { false.probe }
        ");
        assert_snapshot!(hir_string_proc("false.method(:probe)"), @"
        fn probe@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          SideExit NoProfileGetIvar recompile
        ");
    }

    #[test]
    fn test_self_not_heap_object_owner_object() {
        // Object uses the default allocator, but Integer (and every other immediate)
        // descends from it, so a method on Object can run with an immediate self.
        eval("
            class Object
              def probe = @foo
            end
            o = Object.new
            100.times { o.probe }
        ");
        assert_snapshot!(hir_string_proc("Object.new.method(:probe)"), @"
        fn probe@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v10, :shape_id@0x1000
          v12:CShape[0x1001] = GuardBitEquals v11, CShape(0x1001) recompile
          v13:NilClass = Const Value(nil)
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_self_not_heap_object_owner_basic_object() {
        // Same as Object: BasicObject has the default allocator but is the root of
        // the immediate classes' ancestry.
        eval("
            class BasicObject
              def probe = @foo
            end
            o = Object.new
            100.times { o.probe }
        ");
        assert_snapshot!(hir_string_proc("Object.new.method(:probe)"), @"
        fn probe@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v10, :shape_id@0x1000
          v12:CShape[0x1001] = GuardBitEquals v11, CShape(0x1001) recompile
          v13:NilClass = Const Value(nil)
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_self_not_heap_object_owner_numeric() {
        // Numeric has the default allocator but Integer/Float descend from it, so a
        // method on Numeric can run with an immediate self.
        eval("
            class Numeric
              def probe = @foo
            end
            100.times { 5.probe }
        ");
        assert_snapshot!(hir_string_proc("5.method(:probe)"), @"
        fn probe@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          SideExit NoProfileGetIvar recompile
        ");
    }

    #[test]
    fn test_definedivar_shape_guard_recompile() {
        // Call with one shape to compile, then call with a different shape to
        // trigger shape guard exits and recompilation. On the recompiled version,
        // DefinedIvar uses polymorphic fast paths plus a C call fallback.
        eval("
            class C
              def initialize(extra = false)
                @bar = 0 if extra  # changes the shape
                @foo = 42
              end
              def has_foo = defined?(@foo)
            end

            c = C.new
            c.has_foo  # profile
            c.has_foo  # compile (version 1 with shape guard)
            d = C.new(true)  # same class, different shape
            100.times { d.has_foo }  # trigger shape guard exits -> recompile
            100.times { c.has_foo }  # run recompiled version (version 2)
        ");
        assert_snapshot!(hir_string_proc("C.new.method(:has_foo)"), @"
        fn has_foo@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:HeapBasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:HeapBasicObject):
          v11:CShape = LoadField v6, :shape_id@0x1000
          v12:CShape[0x1001] = Const CShape(0x1001)
          v13:CBool = IsBitEqual v11, v12
          CondBranch v13, bb5(), bb6()
        bb5():
          v15:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          Jump bb4(v15)
        bb6():
          v17:CShape = LoadField v6, :shape_id@0x1000
          v18:CShape[0x1010] = Const CShape(0x1010)
          v19:CBool = IsBitEqual v17, v18
          CondBranch v19, bb7(), bb8()
        bb7():
          v21:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          Jump bb4(v21)
        bb8():
          v23:StringExact|NilClass = DefinedIvar v6, :@foo
          Jump bb4(v23)
        bb4(v10:StringExact|NilClass):
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_setivar_shape_guard_recompile() {
        set_max_versions(2);
        // Call with one shape to compile, then call with a different shape to
        // trigger shape guard exits and recompilation. The recompiled version
        // specializes both profiled shapes.
        eval("
            class C
              def initialize(extra = false)
                @bar = 0 if extra  # changes the shape
                @foo = 42
              end
              def foo = @foo = 5
            end

            c = C.new
            c.foo  # profile
            c.foo  # compile (version 1 with shape guard)
            d = C.new(true)  # same class, different shape
            100.times { d.foo }  # trigger shape guard exits -> recompile
            100.times { c.foo }  # run recompiled version (version 2)
        ");
        assert_snapshot!(hir_string_proc("C.new.method(:foo)"), @"
        fn foo@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:HeapBasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:HeapBasicObject):
          v9:Fixnum[5] = Const Value(5)
          PatchPoint SingleRactorMode
          v14:CShape = LoadField v6, :shape_id@0x1000
          v15:CShape[0x1001] = Const CShape(0x1001)
          v16:CBool = IsBitEqual v14, v15
          CondBranch v16, bb5(), bb6()
        bb5():
          StoreField v6, :@foo@0x1002, v9
          WriteBarrier v6, v9
          Jump bb4()
        bb6():
          v21:CShape[0x1003] = GuardBitEquals v14, CShape(0x1003) recompile
          StoreField v6, :@foo@0x1004, v9
          WriteBarrier v6, v9
          Jump bb4()
        bb4():
          CheckInterrupts
          Return v9
        ");
    }

    #[test]
    fn test_setivar_shape_guard_attr_writer_no_recompile() {
        // attr_writer SetIvar has no inline cache and may target a receiver
        // operand other than CFP self, so don't recompile here yet.
        eval("
            class C
              attr_writer :foo
              def initialize(extra = false)
                @bar = 0 if extra  # changes the shape
                @foo = 42
              end
            end

            class D
              def write(obj)
                obj.foo = 5
              end
            end

            c = C.new
            d = D.new
            d.write(c)  # profile
            d.write(c)  # compile (version 1 with shape guard)
            e = C.new(true)  # same class, different shape
            100.times { d.write(e) }  # shape guard exits, but no recompile
        ");
        assert_snapshot!(hir_string_proc("D.new.method(:write)"), @"
        fn write@<compiled>:12:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :obj@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:HeapBasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :obj@1
          Jump bb3(v6, v7)
        bb3(v9:HeapBasicObject, v10:BasicObject):
          v16:Fixnum[5] = Const Value(5)
          v20:CBool = HasType v10, ObjectSubclass[class_exact:C]
          CondBranch v20, bb5(), bb6()
        bb5():
          v23:ObjectSubclass[class_exact:C] = RefineType v10, ObjectSubclass[class_exact:C]
          PatchPoint MethodRedefined(C@0x1008, foo=@0x1010, cme:0x1018)
          SetIvar v23, :@foo, v16
          Jump bb4(v16)
        bb6():
          v26:BasicObject = Send v10, :foo=, v16 # SendFallbackReason: Send: polymorphic call site
          Jump bb4(v26)
        bb4(v19:BasicObject):
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_optimize_getivar_on_module_embedded() {
        eval("
            module M
              @foo = 42
              def self.test = @foo
            end
            M.test
        ");
        assert_snapshot!(hir_string_proc("M.method(:test)"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v10, :shape_id@0x1000
          v12:CShape[0x1001] = GuardBitEquals v11, CShape(0x1001) recompile
          v13:RubyValue = LoadField v10, :fields_obj@0x1002
          v14:BasicObject = LoadField v13, :@foo@0x1003
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn test_optimize_getivar_on_module_complex() {
        eval(r#"
            module M
              @foo = 42
              for i in 0...1000
                instance_variable_set("@v#{i}", i)
              end
              def self.test = @foo
            end
            M.test
        "#);
        assert_snapshot!(hir_string_proc("M.method(:test)"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          SideExit NoProfileGetIvar recompile
        ");
    }

    #[test]
    fn test_no_side_exit_assertion() {
        eval("
          def side_exit = ::RubyVM::ZJIT.induce_side_exit!
          side_exit
        ");
        std::panic::catch_unwind(|| assert_compiles("side_exit")).expect_err("Should panic because the program should side exit");
    }

    #[test]
    fn test_optimize_getivar_on_class_embedded() {
        eval("
            class C
              @foo = 42
              def self.test = @foo
            end
            C.test
        ");
        assert_snapshot!(assert_compiles("C.test"), @"42");
        assert_snapshot!(hir_string_proc("C.method(:test)"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v10, :shape_id@0x1000
          v12:CShape[0x1001] = GuardBitEquals v11, CShape(0x1001) recompile
          v13:RubyValue = LoadField v10, :fields_obj@0x1002
          v14:BasicObject = LoadField v13, :@foo@0x1003
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn test_optimize_getivar_on_class_complex() {
        eval(r#"
            class C
              @foo = 42
              for i in 0...1000
                instance_variable_set("@v#{i}", i)
              end
              def self.test = @foo
            end
            C.test
        "#);
        assert_snapshot!(hir_string_proc("C.method(:test)"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          SideExit NoProfileGetIvar recompile
        ");
    }

    #[test]
    fn test_optimize_getivar_on_t_struct() {
        // Range is T_STRUCT (not T_DATA): falls back to CCall
        eval("
            class C < Range
              def test = @a
            end
            obj = C.new 0, 1
            obj.instance_variable_set(:@a, 1)
            obj.test
            TEST = C.instance_method(:test)
        ");
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v10, :shape_id@0x1000
          v12:CShape[0x1001] = GuardBitEquals v11, CShape(0x1001) recompile
          v13:RubyValue = LoadField v10, :fields_obj@0x1002
          v14:BasicObject = LoadField v13, :@a@0x1002
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn test_optimize_getivar_on_t_data() {
        // T_DATA uses fields_obj for instance variables.
        eval("
            class C < Thread
              def test = @a
            end
            obj = C.new { }
            obj.join
            obj.instance_variable_set(:@a, 1)
            obj.test
            TEST = C.instance_method(:test)
        ");
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v10, :shape_id@0x1000
          v12:CShape[0x1001] = GuardBitEquals v11, CShape(0x1001) recompile
          v13:RubyValue = LoadField v10, :fields_obj@0x1002
          v14:BasicObject = LoadField v13, :@a@0x1002
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn test_optimize_getivar_on_t_data_complex_fields() {
        // T_DATA with enough ivars to force heap field storage
        eval("
            class C < Thread
              def test = @var1000
            end
            obj = C.new { }
            obj.join
            1000.times { |i| obj.instance_variable_set(:\"@var#{i}\", 1) }
            obj.instance_variable_set(:@var1000, 42)
            obj.test
            TEST = C.instance_method(:test)
        ");
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          SideExit NoProfileGetIvar recompile
        ");
    }

    #[test]
    fn test_optimize_getivar_on_module_multi_ractor() {
        eval("
            module M
              @foo = 42
              def self.test = @foo
            end
            Ractor.new {}.value
            M.test
        ");
        assert_snapshot!(hir_string_proc("M.method(:test)"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          SideExit UnhandledYARVInsn(getinstancevariable)
        ");
    }

    #[test]
    fn test_optimize_attr_reader_on_module_multi_ractor() {
        eval("
            module M
              @foo = 42
              class << self
                attr_reader :foo
              end
              def self.test = foo
            end
            Ractor.new {}.value
            M.test
        ");
        assert_snapshot!(hir_string_proc("M.method(:test)"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:BasicObject = Send v6, :foo # SendFallbackReason: Single-ractor mode required
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_optimize_getivar_polymorphic() {
        set_call_threshold(3);
        eval(r#"
            class C
              def foo_then_many
                @foo = 1
                10.times { |i| instance_variable_set(:"@v#{i}", i) }
                @bar = 2
              end

              def many_then_foo
                10.times { |i| instance_variable_set(:"@v#{i}", i) }
                @bar = 3
                @foo = 4
              end

              def foo = @foo + 1
            end

            O1 = C.new
            O1.foo_then_many
            O2 = C.new
            O2.many_then_foo
            O1.foo
            O2.foo
        "#);
        assert_snapshot!(hir_string_proc("C.instance_method(:foo)"), @"
        fn foo@<compiled>:15:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v10, :shape_id@0x1000
          v13:CShape[0x1001] = Const CShape(0x1001)
          v14:CBool = IsBitEqual v11, v13
          CondBranch v14, bb5(), bb6()
        bb5():
          v16:BasicObject = LoadField v10, :@foo@0x1002
          Jump bb4(v16)
        bb6():
          v18:CShape[0x1003] = GuardBitEquals v11, CShape(0x1003) recompile
          v20:RubyValue = LoadField v10, :fields_obj@0x1004
          v21:BasicObject = LoadField v20, :@foo@0x1004
          Jump bb4(v21)
        bb4(v12:BasicObject):
          v24:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v35:Fixnum = GuardType v12, Fixnum recompile
          v36:Fixnum = FixnumAdd v35, v24
          CheckInterrupts
          Return v36
        ");
    }

    #[test]
    fn test_optimize_getivar_skewed_polymorphic() {
        // Use threshold=6 so we get 5 profile samples.
        // 4 calls with shape A, 1 with shape B = 80% skew (>= 75% threshold).
        set_call_threshold(6);
        eval(r#"
            class C
              def foo_then_many
                @foo = 1
                100.times { |i| instance_variable_set(:"@v#{i}", i) }
                @bar = 2
              end

              def many_then_foo
                100.times { |i| instance_variable_set(:"@v#{i}", i) }
                @bar = 3
                @foo = 4
              end

              def foo = @foo + 1
            end

            O1 = C.new
            O1.foo_then_many
            O2 = C.new
            O2.many_then_foo
            O1.foo
            O1.foo
            O1.foo
            O1.foo
            O2.foo
        "#);
        assert_snapshot!(hir_string_proc("C.instance_method(:foo)"), @"
        fn foo@<compiled>:15:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v10, :shape_id@0x1000
          v13:CShape[0x1001] = Const CShape(0x1001)
          v14:CBool = IsBitEqual v11, v13
          CondBranch v14, bb5(), bb6()
        bb5():
          v16:RubyValue = LoadField v10, :fields_obj@0x1002
          v17:BasicObject = LoadField v16, :@foo@0x1002
          Jump bb4(v17)
        bb6():
          v19:CShape[0x1003] = GuardBitEquals v11, CShape(0x1003) recompile
          v21:BasicObject = LoadField v10, :@foo@0x1004
          Jump bb4(v21)
        bb4(v12:BasicObject):
          v24:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v35:Fixnum = GuardType v12, Fixnum recompile
          v36:Fixnum = FixnumAdd v35, v24
          CheckInterrupts
          Return v36
        ");
    }

    #[test]
    fn test_optimize_getivar_polymorphic_with_subclass() {
        set_call_threshold(3);
        eval(r#"
            class C
              def initialize
                @foo = 3
              end

              def foo = @foo + 1
            end

            class D < C
              def initialize
                super
                @bar = 4
              end
            end

            O1 = C.new
            O2 = D.new
            O1.foo
            O2.foo
        "#);
        assert_snapshot!(hir_string_proc("C.instance_method(:foo)"), @"
        fn foo@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v10, :shape_id@0x1000
          v13:CShape[0x1001] = Const CShape(0x1001)
          v14:CBool = IsBitEqual v11, v13
          CondBranch v14, bb5(), bb6()
        bb5():
          v16:BasicObject = LoadField v10, :@foo@0x1002
          Jump bb4(v16)
        bb6():
          v18:CShape[0x1003] = GuardBitEquals v11, CShape(0x1003) recompile
          v20:BasicObject = LoadField v10, :@foo@0x1002
          Jump bb4(v20)
        bb4(v12:BasicObject):
          v23:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v34:Fixnum = GuardType v12, Fixnum recompile
          v35:Fixnum = FixnumAdd v34, v23
          CheckInterrupts
          Return v35
        ");
    }

    #[test]
    fn test_getivar_polymorphic_t_class_and_t_data() {
        set_call_threshold(3);
        eval(r#"
          module Reader
            def test = @a
          end

          class A
            extend Reader
            @a = 0
          end

          ARGF.instance_eval do
            extend Reader
            @a = :a
          end

          A.test
          ARGF.test
        "#);
        assert_snapshot!(assert_compiles("[A.test, ARGF.test]"), @"[0, :a]");
        assert_snapshot!(hir_string_proc("Reader.instance_method(:test)"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          v10:HeapBasicObject = GuardType v6, HeapBasicObject
          v11:CShape = LoadField v10, :shape_id@0x1000
          v13:CShape[0x1001] = Const CShape(0x1001)
          v14:CBool = IsBitEqual v11, v13
          CondBranch v14, bb5(), bb6()
        bb5():
          v16:RubyValue = LoadField v10, :fields_obj@0x1002
          v17:BasicObject = LoadField v16, :@a@0x1002
          Jump bb4(v17)
        bb6():
          v19:CShape[0x1003] = GuardBitEquals v11, CShape(0x1003) recompile
          v21:RubyValue = LoadField v10, :fields_obj@0x1004
          v22:BasicObject = LoadField v21, :@a@0x1002
          Jump bb4(v22)
        bb4(v12:BasicObject):
          CheckInterrupts
          Return v12
        ");
    }

    #[test]
    fn test_dont_optimize_attr_accessor_polymorphic() {
        set_call_threshold(3);
        eval("
            class C
              attr_reader :foo, :bar

              def foo_then_bar
                @foo = 1
                @bar = 2
              end

              def bar_then_foo
                @bar = 3
                @foo = 4
              end
            end

            O1 = C.new
            O1.foo_then_bar
            O2 = C.new
            O2.bar_then_foo
            def test(o) = o.foo
            test O1
            test O2
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:20:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:CBool = HasType v10, ObjectSubclass[class_exact:C]
          CondBranch v15, bb5(), bb6()
        bb5():
          v18:ObjectSubclass[class_exact:C] = RefineType v10, ObjectSubclass[class_exact:C]
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo@0x1010, cme:0x1018)
          v30:BasicObject = GetIvar v18, :@foo
          Jump bb4(v30)
        bb6():
          v21:BasicObject = Send v10, :foo # SendFallbackReason: Send: polymorphic call site
          Jump bb4(v21)
        bb4(v14:BasicObject):
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn test_dont_optimize_getivar_with_complex_shape() {
        eval(r#"
            class C
              attr_accessor :foo
            end
            obj = C.new
            (0..1000).each do |i|
              obj.instance_variable_set(:"@v#{i}", i)
            end
            (0..1000).each do |i|
              obj.remove_instance_variable(:"@v#{i}")
            end
            def test(o) = o.foo
            test obj
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:12:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v23:BasicObject = GetIvar v22, :@foo
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_optimize_send_with_block() {
        eval(r#"
            def test = [1, 2, 3].map { |x| x * 2 }
            test; test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v10:ArrayExact = ArrayDup v9
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, map@0x1010, cme:0x1018)
          v21:BasicObject = SendDirect v10, 0x1040, :map (0x1068)
          CheckInterrupts
          Return v21
        ");
    }

    #[test]
    fn test_optimize_send_variadic_with_block() {
        eval(r#"
            A = [1, 2, 3]
            B = ["a", "b", "c"]

            def test
              result = []
              A.zip(B) { |x, y| result << [x, y] }
              result
            end

            test; test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:ArrayExact = NewArray
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, A)
          v18:ArrayExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint StableConstantNames(0x1010, B)
          v22:ArrayExact[VALUE(0x1018)] = Const Value(VALUE(0x1018))
          PatchPoint NoSingletonClass(Array@0x1020)
          PatchPoint MethodRedefined(Array@0x1020, zip@0x1028, cme:0x1030)
          v41:BasicObject = CCallVariadic v18, :Array#zip@0x1058, v22
          PatchPoint NoEPEscape(test)
          CheckInterrupts
          Return v12
        ");
    }

    #[test]
    fn test_do_not_optimize_send_with_block_forwarding() {
        eval(r#"
            def test(&block) = [].map(&block)
            test { |x| x }; test { |x| x }
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :block@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :block@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:ArrayExact = NewArray
          v17:CPtr = GetEP 0
          v18:CUInt64 = LoadField v17, :VM_ENV_DATA_INDEX_FLAGS@0x1001
          v19:CBool = IsBlockParamModified v18
          CondBranch v19, bb4(), bb5()
        bb4():
          v21:BasicObject = LoadField v17, :block@0x1002
          Jump bb6(v21, v21)
        bb5():
          v23:CInt64 = LoadField v17, :VM_ENV_DATA_INDEX_SPECVAL@0x1003
          v24:CInt64 = GuardAnyBitSet v23, CUInt64(1) recompile
          v25:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1008))
          Jump bb6(v25, v10)
        bb6(v15:BasicObject, v16:BasicObject):
          v28:BasicObject = Send v13, &block, :map, v15 # SendFallbackReason: Send: block argument is not nil
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_replace_block_param_proxy_with_nil() {
        eval(r#"
            def test(&block) = [].map(&block)
            test; test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :block@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :block@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:ArrayExact = NewArray
          v17:CPtr = GetEP 0
          v18:CUInt64 = LoadField v17, :VM_ENV_DATA_INDEX_FLAGS@0x1001
          v19:CBool = IsBlockParamModified v18
          CondBranch v19, bb4(), bb5()
        bb4():
          v21:BasicObject = LoadField v17, :block@0x1002
          Jump bb6(v21, v21)
        bb5():
          v23:CInt64 = LoadField v17, :VM_ENV_DATA_INDEX_SPECVAL@0x1003
          v24:CInt64[0] = GuardBitEquals v23, CInt64(0) recompile
          v25:NilClass = Const Value(nil)
          Jump bb6(v25, v10)
        bb6(v15:BasicObject, v16:BasicObject):
          v34:NilClass = GuardBitEquals v15, Value(nil) recompile
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, map@0x1010, cme:0x1018)
          v39:BasicObject = SendDirect v13, 0x0, :map (0x1040)
          CheckInterrupts
          Return v39
        ");
    }

    #[test]
    fn test_replace_block_param_proxy_with_nil_nested() {
        eval(r#"
            def test(&block)
              proc do
                [].map(&block)
              end
            end
            test; test
        "#);
        assert_snapshot!(hir_string_proc("test"), @"
        fn block in test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact = NewArray
          v12:CPtr = GetEP 1
          v13:CUInt64 = LoadField v12, :VM_ENV_DATA_INDEX_FLAGS@0x1000
          v14:CBool = IsBlockParamModified v13
          CondBranch v14, bb4(), bb5()
        bb4():
          v16:BasicObject = LoadField v12, :block@0x1001
          Jump bb6(v16)
        bb5():
          v18:CInt64 = LoadField v12, :VM_ENV_DATA_INDEX_SPECVAL@0x1002
          v19:CInt64 = GuardAnyBitSet v18, CUInt64(1) recompile
          v20:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1008))
          Jump bb6(v20)
        bb6(v11:BasicObject):
          v23:BasicObject = Send v9, &block, :map, v11 # SendFallbackReason: Send: block argument is not nil
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_send_iseq_with_block_no_callee_block_param() {
        let result = eval(r#"
            def foo
              yield 1
            end

            def test = foo { |x| x * 2 }
            test; test
        "#);
        assert_eq!(VALUE::fixnum_from_usize(2), result);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v17 (0x1038)
          v23:Fixnum[1] = Const Value(1)
          v25:CPtr = GetEP 0
          v26:CInt64 = LoadField v25, :VM_ENV_DATA_INDEX_SPECVAL@0x1060
          v27:CInt64[-4] = Const CInt64(-4)
          v28:CInt64 = IntAnd v26, v27
          v29:BasicObject = InvokeBlockIseqDirect (0x1068), v28, v23
          CheckInterrupts
          PopInlineFrame
          Return v29
        ");
    }

    #[test]
    fn test_send_iseq_with_block_param_no_block() {
        set_max_versions(2);
        let result = eval("
            def foo(&blk)
              blk ? blk.call : 42
            end
            def test = foo
            test
            test
        ");
        assert_eq!(VALUE::fixnum_from_usize(42), result);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v68:NilClass = Const Value(nil)
          PushInlineFrame v17 (0x1038)
          v26:CPtr = GetEP 0
          v27:CUInt64 = LoadField v26, :VM_ENV_DATA_INDEX_FLAGS@0x1060
          v28:CBool = IsBlockParamModified v27
          CondBranch v28, bb7(), bb8()
        bb7():
          v30:BasicObject = LoadField v26, :blk@0x1061
          Jump bb9(v30, v30)
        bb8():
          v32:CInt64 = LoadField v26, :VM_ENV_DATA_INDEX_SPECVAL@0x1062
          v33:CInt64[0] = GuardBitEquals v32, CInt64(0) recompile
          v34:NilClass = Const Value(nil)
          Jump bb9(v34, v68)
        bb9(v24:BasicObject, v25:BasicObject):
          v37:CBool = Test v24
          CondBranch v37, bb10(), bb6(v17, v25)
        bb10():
          v44:CPtr = GetEP 0
          v45:CUInt64 = LoadField v44, :VM_ENV_DATA_INDEX_FLAGS@0x1060
          v46:CBool = IsBlockParamModified v45
          CondBranch v46, bb11(), bb12()
        bb11():
          v48:BasicObject = LoadField v44, :blk@0x1061
          Jump bb13(v48, v48)
        bb12():
          v50:CInt64 = LoadField v44, :VM_ENV_DATA_INDEX_SPECVAL@0x1062
          v51:CInt64 = GuardAnyBitSet v50, CUInt64(1) recompile
          v52:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1068))
          Jump bb13(v52, v25)
        bb13(v42:BasicObject, v43:BasicObject):
          v55:BasicObject = Send v42, :call # SendFallbackReason: Send: no profile data available
          CheckInterrupts
          Jump bb4(v55)
        bb6(v60:ObjectSubclass[class_exact*:Object@VALUE(0x1000)], v61:BasicObject):
          v63:Fixnum[42] = Const Value(42)
          CheckInterrupts
          Jump bb4(v63)
        bb4(v69:BasicObject):
          PopInlineFrame
          CheckInterrupts
          Return v69
        ");
    }

    #[test]
    fn test_send_bmethod_with_block_param_no_block() {
        let result = eval("
            define_method(:foo) { |&blk|
              blk ? blk.call : 42
            }
            def test = foo
            test
            test
        ");
        assert_eq!(VALUE::fixnum_from_usize(42), result);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v18:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v45:NilClass = Const Value(nil)
          PushInlineFrame v18 (0x1038)
          v40:Fixnum[42] = Const Value(42)
          CheckInterrupts
          PopInlineFrame
          Return v40
        ");
    }

    #[test]
    fn test_send_with_non_nil_block_arg() {
        eval(r#"
            def foo = 42

            def test
              block = :to_s
              foo(&block)
            end
            test; test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:StaticSymbol[:to_s] = Const Value(VALUE(0x1000))
          v18:BasicObject = Send v8, &block, :foo, v12 # SendFallbackReason: Send: block argument is not nil
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_send_with_statically_nil_block_arg() {
        eval(r#"
            def foo = 42

            def test
              block = nil
              foo(&block)
            end
            test; test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:NilClass = Const Value(nil)
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v26:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v8, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v27:Fixnum[42] = Const Value(42)
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_send_with_monomorphically_nil_block_arg() {
        eval(r#"
            def foo = 42

            def test(&block)
              foo(&block)
            end
            test; test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :block@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :block@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v16:CPtr = GetEP 0
          v17:CUInt64 = LoadField v16, :VM_ENV_DATA_INDEX_FLAGS@0x1001
          v18:CBool = IsBlockParamModified v17
          CondBranch v18, bb4(), bb5()
        bb4():
          v20:BasicObject = LoadField v16, :block@0x1002
          Jump bb6(v20, v20)
        bb5():
          v22:CInt64 = LoadField v16, :VM_ENV_DATA_INDEX_SPECVAL@0x1003
          v23:CInt64[0] = GuardBitEquals v22, CInt64(0) recompile
          v24:NilClass = Const Value(nil)
          Jump bb6(v24, v10)
        bb6(v14:BasicObject, v15:BasicObject):
          v33:NilClass = GuardBitEquals v14, Value(nil) recompile
          PatchPoint MethodRedefined(Object@0x1008, foo@0x1010, cme:0x1018)
          v36:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          v37:Fixnum[42] = Const Value(42)
          CheckInterrupts
          Return v37
        ");
    }

    #[test]
    fn test_inline_attr_reader_constant() {
        eval("
            class C
              attr_reader :foo
            end

            O = C.new
            def test = O.foo
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, O)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(C@0x1010)
          PatchPoint MethodRedefined(C@0x1010, foo@0x1018, cme:0x1020)
          v23:CShape = LoadField v11, :shape_id@0x1048
          v24:CShape[0x1049] = GuardBitEquals v23, CShape(0x1049) recompile
          v25:NilClass = Const Value(nil)
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_inline_attr_accessor_constant() {
        eval("
            class C
              attr_accessor :foo
            end

            O = C.new
            def test = O.foo
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, O)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(C@0x1010)
          PatchPoint MethodRedefined(C@0x1010, foo@0x1018, cme:0x1020)
          v23:CShape = LoadField v11, :shape_id@0x1048
          v24:CShape[0x1049] = GuardBitEquals v23, CShape(0x1049) recompile
          v25:NilClass = Const Value(nil)
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_inline_attr_reader() {
        eval("
            class C
              attr_reader :foo
            end

            def test(o) = o.foo
            test C.new
            test C.new
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v24:CShape = LoadField v22, :shape_id@0x1040
          v25:CShape[0x1041] = GuardBitEquals v24, CShape(0x1041) recompile
          v26:NilClass = Const Value(nil)
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_inline_attr_accessor() {
        eval("
            class C
              attr_accessor :foo
            end

            def test(o) = o.foo
            test C.new
            test C.new
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v24:CShape = LoadField v22, :shape_id@0x1040
          v25:CShape[0x1041] = GuardBitEquals v24, CShape(0x1041) recompile
          v26:NilClass = Const Value(nil)
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_inline_attr_accessor_set() {
        eval("
            class C
              attr_accessor :foo
            end

            def test(o) = o.foo = 5
            test C.new
            test C.new
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v16:Fixnum[5] = Const Value(5)
          PatchPoint MethodRedefined(C@0x1008, foo=@0x1010, cme:0x1018)
          v27:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v29:CShape = LoadField v27, :shape_id@0x1040
          v30:CShape[0x1041] = GuardBitEquals v29, CShape(0x1041)
          StoreField v27, :@foo@0x1042, v16
          WriteBarrier v27, v16
          v33:CShape[0x1043] = Const CShape(0x1043)
          StoreField v27, :shape_id@0x1040, v33
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_inline_attr_writer_set() {
        eval("
            class C
              attr_writer :foo
            end

            def test(o) = o.foo = 5
            test C.new
            test C.new
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v16:Fixnum[5] = Const Value(5)
          PatchPoint MethodRedefined(C@0x1008, foo=@0x1010, cme:0x1018)
          v27:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v29:CShape = LoadField v27, :shape_id@0x1040
          v30:CShape[0x1041] = GuardBitEquals v29, CShape(0x1041)
          StoreField v27, :@foo@0x1042, v16
          WriteBarrier v27, v16
          v33:CShape[0x1043] = Const CShape(0x1043)
          StoreField v27, :shape_id@0x1040, v33
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_inline_struct_aref_embedded() {
        eval(r#"
            C = Struct.new(:foo)
            def test(o) = o.foo
            test C.new
            test C.new
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v23:BasicObject = LoadField v22, :foo@0x1040
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_inline_struct_aref_heap() {
        eval(r#"
            C = Struct.new(*(0..1000).map {|i| :"a#{i}"}, :foo)
            def test(o) = o.foo
            test C.new
            test C.new
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v23:CPtr = LoadField v22, :as_heap@0x1040
          v24:BasicObject = LoadField v23, :foo@0x1041
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_elide_struct_aref() {
        eval(r#"
            C = Struct.new(*(0..1000).map {|i| :"a#{i}"}, :foo)
            def test(o)
              o.foo
              5
            end
            test C.new
            test C.new
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo@0x1010, cme:0x1018)
          v26:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v18:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_inline_struct_aset_embedded() {
        eval(r#"
            C = Struct.new(:foo)
            def test(o, v) = o.foo = v
            value = Object.new
            test C.new, value
            test C.new, value
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          v4:BasicObject = LoadField v2, :v@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :o@1
          v9:BasicObject = LoadArg :v@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo=@0x1010, cme:0x1018)
          v30:ObjectSubclass[class_exact:C] = GuardType v12, ObjectSubclass[class_exact:C] recompile
          v31:CUInt64 = LoadField v30, :RBASIC_FLAGS@0x1040
          v32:CUInt64 = GuardNoBitsSet v31, RUBY_FL_FREEZE=CUInt64(2048)
          StoreField v30, :foo=@0x1041, v13
          WriteBarrier v30, v13
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_inline_struct_aset_heap() {
        eval(r#"
            C = Struct.new(*(0..1000).map {|i| :"a#{i}"}, :foo)
            def test(o, v) = o.foo = v
            value = Object.new
            test C.new, value
            test C.new, value
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          v4:BasicObject = LoadField v2, :v@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :o@1
          v9:BasicObject = LoadArg :v@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo=@0x1010, cme:0x1018)
          v30:ObjectSubclass[class_exact:C] = GuardType v12, ObjectSubclass[class_exact:C] recompile
          v31:CUInt64 = LoadField v30, :RBASIC_FLAGS@0x1040
          v32:CUInt64 = GuardNoBitsSet v31, RUBY_FL_FREEZE=CUInt64(2048)
          v33:CPtr = LoadField v30, :as_heap@0x1041
          StoreField v33, :foo=@0x1042, v13
          WriteBarrier v30, v13
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_array_reverse_returns_array() {
        eval(r#"
            def test = [].reverse
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact = NewArray
          PatchPoint NoSingletonClass(Array@0x1000)
          PatchPoint MethodRedefined(Array@0x1000, reverse@0x1008, cme:0x1010)
          v20:ArrayExact = CCallWithFrame v9, :Array#reverse@0x1038
          CheckInterrupts
          Return v20
        ");
    }

    #[test]
    fn test_array_reverse_is_elidable() {
        eval(r#"
            def test
              [].reverse
              5
            end
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact = NewArray
          PatchPoint NoSingletonClass(Array@0x1000)
          PatchPoint MethodRedefined(Array@0x1000, reverse@0x1008, cme:0x1010)
          v15:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v15
        ");
    }

    #[test]
    fn test_array_join_returns_string() {
        eval(r#"
            def test = [].join ","
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact = NewArray
          v11:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v12:StringExact = StringCopy v11
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, join@0x1010, cme:0x1018)
          v23:StringExact = CCallVariadic v9, :Array#join@0x1040, v12
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_string_to_s_returns_string() {
        eval(r#"
            def test = "".to_s
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v10:StringExact = StringCopy v9
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, to_s@0x1010, cme:0x1018)
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_inline_string_literal_to_s() {
        eval(r#"
            def test = "foo".to_s
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v10:StringExact = StringCopy v9
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, to_s@0x1010, cme:0x1018)
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_inline_profiled_string_to_s() {
        eval(r#"
            def test(o) = o.to_s
            test "foo"
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, to_s@0x1010, cme:0x1018)
          v23:StringExact = GuardType v10, StringExact recompile
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_fixnum_to_s_returns_string() {
        eval(r#"
            def test(x) = x.to_s
            test 5
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, to_s@0x1010, cme:0x1018)
          v22:Fixnum = GuardType v10, Fixnum recompile
          v23:StringExact = CCallVariadic v22, :Integer#to_s@0x1040
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_bignum_to_s_returns_string() {
        eval(r#"
            def test(x) = x.to_s
            test (2**65)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, to_s@0x1010, cme:0x1018)
          v22:Bignum = GuardType v10, Bignum recompile
          v23:StringExact = CCallVariadic v22, :Integer#to_s@0x1040
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_fold_any_to_string_with_known_string_exact() {
        eval(r##"
            def test(x) = "#{x}"
            test 123
        "##);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v17:Fixnum = GuardType v10, Fixnum
          PatchPoint MethodRedefined(Integer@0x1010, to_s@0x1018, cme:0x1020)
          v37:StringExact = CCallVariadic v17, :Integer#to_s@0x1048
          v28:StringExact = StringConcat v13, v37
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_array_aref_fixnum_literal() {
        eval("
            def test
              arr = [1, 2, 3]
              arr[0]
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v13:ArrayExact = ArrayDup v12
          v18:Fixnum[0] = Const Value(0)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          v37:CInt64[0] = Const CInt64(0)
          v31:CInt64 = ArrayLength v13
          v32:CInt64[0] = GuardLess v37, v31
          v36:BasicObject = ArrayAref v13, v32
          CheckInterrupts
          Return v36
        ");
    }

    #[test]
    fn test_array_aref_fixnum_profiled() {
        eval("
            def test(arr, idx)
              arr[idx]
            end
            test([1, 2, 3], 0)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          v4:BasicObject = LoadField v2, :idx@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :arr@1
          v9:BasicObject = LoadArg :idx@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          v28:ArrayExact = GuardType v12, ArrayExact recompile
          v29:Fixnum = GuardType v13, Fixnum
          v30:CInt64 = UnboxFixnum v29
          v31:CInt64 = ArrayLength v28
          v32:CInt64 = GuardLess v30, v31
          v33:CInt64 = AdjustBounds v32, v31
          v34:CInt64[0] = Const CInt64(0)
          v35:CInt64 = GuardGreaterEq v33, v34
          v36:BasicObject = ArrayAref v28, v35
          CheckInterrupts
          Return v36
        ");
    }

    #[test]
    fn test_array_aref_fixnum_array_subclass() {
        eval("
            class C < Array; end
            def test(arr, idx)
              arr[idx]
            end
            test(C.new([1, 2, 3]), 0)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          v4:BasicObject = LoadField v2, :idx@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :arr@1
          v9:BasicObject = LoadArg :idx@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, []@0x1010, cme:0x1018)
          v28:ArraySubclass[class_exact:C] = GuardType v12, ArraySubclass[class_exact:C] recompile
          v29:Fixnum = GuardType v13, Fixnum
          v30:CInt64 = UnboxFixnum v29
          v31:CInt64 = ArrayLength v28
          v32:CInt64 = GuardLess v30, v31
          v33:CInt64 = AdjustBounds v32, v31
          v34:CInt64[0] = Const CInt64(0)
          v35:CInt64 = GuardGreaterEq v33, v34
          v36:BasicObject = ArrayAref v28, v35
          CheckInterrupts
          Return v36
        ");
    }

    #[test]
    fn test_hash_aref_literal() {
        eval("
            def test
              arr = {1 => 3}
              arr[1]
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:HashExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v13:HashExact = HashDup v12
          v18:Fixnum[1] = Const Value(1)
          PatchPoint NoSingletonClass(Hash@0x1008)
          PatchPoint MethodRedefined(Hash@0x1008, []@0x1010, cme:0x1018)
          v30:BasicObject = HashAref v13, v18
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_hash_aref_profiled() {
        eval("
            def test(hash, key)
              hash[key]
            end
            test({1 => 3}, 1)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :hash@0x1000
          v4:BasicObject = LoadField v2, :key@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :hash@1
          v9:BasicObject = LoadArg :key@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(Hash@0x1008)
          PatchPoint MethodRedefined(Hash@0x1008, []@0x1010, cme:0x1018)
          v28:HashExact = GuardType v12, HashExact recompile
          v29:BasicObject = HashAref v28, v13
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_no_optimize_hash_aref_subclass() {
        eval("
            class C < Hash; end
            def test(hash, key)
              hash[key]
            end
            test(C.new({0 => 3}), 0)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :hash@0x1000
          v4:BasicObject = LoadField v2, :key@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :hash@1
          v9:BasicObject = LoadArg :key@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, []@0x1010, cme:0x1018)
          v28:HashSubclass[class_exact:C] = GuardType v12, HashSubclass[class_exact:C] recompile
          v29:BasicObject = CCallWithFrame v28, :Hash#[]@0x1040, v13
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_does_not_fold_hash_aref_with_frozen_hash() {
        eval("
            H = {a: 0}.freeze
            def test = H[:a]
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, H)
          v11:HashExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v13:StaticSymbol[:a] = Const Value(VALUE(0x1010))
          PatchPoint NoSingletonClass(Hash@0x1018)
          PatchPoint MethodRedefined(Hash@0x1018, []@0x1020, cme:0x1028)
          v26:BasicObject = HashAref v11, v13
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_hash_aset_literal() {
        eval("
            def test
              h = {}
              h[1] = 3
            end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:HashExact = NewHash
          PatchPoint NoEPEscape(test)
          v21:Fixnum[1] = Const Value(1)
          v23:Fixnum[3] = Const Value(3)
          PatchPoint NoSingletonClass(Hash@0x1000)
          PatchPoint MethodRedefined(Hash@0x1000, []=@0x1008, cme:0x1010)
          HashAset v12, v21, v23
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_hash_aset_profiled() {
        eval("
            def test(hash, key, val)
              hash[key] = val
            end
            test({}, 0, 1)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :hash@0x1000
          v4:BasicObject = LoadField v2, :key@0x1001
          v5:BasicObject = LoadField v2, :val@0x1002
          Jump bb3(v1, v3, v4, v5)
        bb2():
          EntryPoint JIT(0)
          v8:BasicObject = LoadArg :self@0
          v9:BasicObject = LoadArg :hash@1
          v10:BasicObject = LoadArg :key@2
          v11:BasicObject = LoadArg :val@3
          Jump bb3(v8, v9, v10, v11)
        bb3(v13:BasicObject, v14:BasicObject, v15:BasicObject, v16:BasicObject):
          PatchPoint NoSingletonClass(Hash@0x1008)
          PatchPoint MethodRedefined(Hash@0x1008, []=@0x1010, cme:0x1018)
          v36:HashExact = GuardType v14, HashExact recompile
          HashAset v36, v15, v16
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_no_optimize_hash_aset_subclass() {
        eval("
            class C < Hash; end
            def test(hash, key, val)
              hash[key] = val
            end
            test(C.new, 0, 1)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :hash@0x1000
          v4:BasicObject = LoadField v2, :key@0x1001
          v5:BasicObject = LoadField v2, :val@0x1002
          Jump bb3(v1, v3, v4, v5)
        bb2():
          EntryPoint JIT(0)
          v8:BasicObject = LoadArg :self@0
          v9:BasicObject = LoadArg :hash@1
          v10:BasicObject = LoadArg :key@2
          v11:BasicObject = LoadArg :val@3
          Jump bb3(v8, v9, v10, v11)
        bb3(v13:BasicObject, v14:BasicObject, v15:BasicObject, v16:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, []=@0x1010, cme:0x1018)
          v36:HashSubclass[class_exact:C] = GuardType v14, HashSubclass[class_exact:C] recompile
          v37:BasicObject = CCallWithFrame v36, :Hash#[]=@0x1040, v15, v16
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_optimize_thread_current() {
        eval("
            def test = Thread.current
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Thread)
          v11:ClassSubclass[Thread@0x1008] = Const Value(VALUE(0x1008))
          PatchPoint MethodRedefined(Class@0x1010, current@0x1018, cme:0x1020)
          v22:CPtr = LoadEC
          v23:CPtr = LoadField v22, :thread_ptr@0x1048
          v24:BasicObject = LoadField v23, :self@0x1049
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_optimize_array_aset_literal() {
        eval("
            def test(arr)
              arr[1] = 10
            end
            test([])
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :arr@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v16:Fixnum[1] = Const Value(1)
          v18:Fixnum[10] = Const Value(10)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []=@0x1010, cme:0x1018)
          v32:ArrayExact = GuardType v10, ArrayExact recompile
          v33:CUInt64 = LoadField v32, :RBASIC_FLAGS@0x1040
          v34:CUInt64 = GuardNoBitsSet v33, RUBY_FL_FREEZE=CUInt64(2048)
          v36:CUInt64 = GuardNoBitsSet v34, RUBY_ELTS_SHARED=CUInt64(4096)
          v45:CInt64[1] = Const CInt64(1)
          v38:CInt64 = ArrayLength v32
          v39:CInt64[1] = GuardLess v45, v38
          ArrayAset v32, v39, v18
          WriteBarrier v32, v18
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_optimize_array_aset_profiled() {
        eval("
            def test(arr, index, val)
              arr[index] = val
            end
            test([], 0, 1)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          v4:BasicObject = LoadField v2, :index@0x1001
          v5:BasicObject = LoadField v2, :val@0x1002
          Jump bb3(v1, v3, v4, v5)
        bb2():
          EntryPoint JIT(0)
          v8:BasicObject = LoadArg :self@0
          v9:BasicObject = LoadArg :arr@1
          v10:BasicObject = LoadArg :index@2
          v11:BasicObject = LoadArg :val@3
          Jump bb3(v8, v9, v10, v11)
        bb3(v13:BasicObject, v14:BasicObject, v15:BasicObject, v16:BasicObject):
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, []=@0x1010, cme:0x1018)
          v36:ArrayExact = GuardType v14, ArrayExact recompile
          v37:Fixnum = GuardType v15, Fixnum
          v38:CUInt64 = LoadField v36, :RBASIC_FLAGS@0x1040
          v39:CUInt64 = GuardNoBitsSet v38, RUBY_FL_FREEZE=CUInt64(2048)
          v41:CUInt64 = GuardNoBitsSet v39, RUBY_ELTS_SHARED=CUInt64(4096)
          v42:CInt64 = UnboxFixnum v37
          v43:CInt64 = ArrayLength v36
          v44:CInt64 = GuardLess v42, v43
          v45:CInt64 = AdjustBounds v44, v43
          v46:CInt64[0] = Const CInt64(0)
          v47:CInt64 = GuardGreaterEq v45, v46
          ArrayAset v36, v47, v16
          WriteBarrier v36, v16
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_optimize_array_aset_array_subclass() {
        eval("
            class MyArray < Array; end
            def test(arr, index, val)
              arr[index] = val
            end
            a = MyArray.new
            test(a, 0, 1)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          v4:BasicObject = LoadField v2, :index@0x1001
          v5:BasicObject = LoadField v2, :val@0x1002
          Jump bb3(v1, v3, v4, v5)
        bb2():
          EntryPoint JIT(0)
          v8:BasicObject = LoadArg :self@0
          v9:BasicObject = LoadArg :arr@1
          v10:BasicObject = LoadArg :index@2
          v11:BasicObject = LoadArg :val@3
          Jump bb3(v8, v9, v10, v11)
        bb3(v13:BasicObject, v14:BasicObject, v15:BasicObject, v16:BasicObject):
          PatchPoint NoSingletonClass(MyArray@0x1008)
          PatchPoint MethodRedefined(MyArray@0x1008, []=@0x1010, cme:0x1018)
          v36:ArraySubclass[class_exact:MyArray] = GuardType v14, ArraySubclass[class_exact:MyArray] recompile
          v37:BasicObject = CCallVariadic v36, :Array#[]=@0x1040, v15, v16
          CheckInterrupts
          Return v16
        ");
    }

    #[test]
    fn test_optimize_array_ltlt() {
        eval("
            def test(arr)
              arr << 1
            end
            test([])
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :arr@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[1] = Const Value(1)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, <<@0x1010, cme:0x1018)
          v26:ArrayExact = GuardType v10, ArrayExact recompile
          v27:CUInt64 = LoadField v26, :RBASIC_FLAGS@0x1040
          v28:CUInt64 = GuardNoBitsSet v27, RUBY_FL_FREEZE=CUInt64(2048)
          ArrayPush v26, v14
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_optimize_array_push_single_arg() {
        eval("
            def test(arr)
              arr.push(1)
            end
            test([])
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :arr@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[1] = Const Value(1)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, push@0x1010, cme:0x1018)
          v25:ArrayExact = GuardType v10, ArrayExact recompile
          v26:CUInt64 = LoadField v25, :RBASIC_FLAGS@0x1040
          v27:CUInt64 = GuardNoBitsSet v26, RUBY_FL_FREEZE=CUInt64(2048)
          ArrayPush v25, v14
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_do_not_optimize_array_push_multi_arg() {
        eval("
            def test(arr)
              arr.push(1,2,3)
            end
            test([])
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :arr@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[1] = Const Value(1)
          v16:Fixnum[2] = Const Value(2)
          v18:Fixnum[3] = Const Value(3)
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, push@0x1010, cme:0x1018)
          v29:ArrayExact = GuardType v10, ArrayExact recompile
          v30:BasicObject = CCallVariadic v29, :Array#push@0x1040, v14, v16, v18
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_optimize_array_push_with_array_subclass() {
        eval("
            class PushSubArray < Array
              def <<(val) = super
            end
            test = PushSubArray.new
            test << 1
        ");
        assert_snapshot!(hir_string_proc("PushSubArray.new.method(:<<)"), @"
        fn <<@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :val@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :val@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Array@0x1008, <<@0x1010, cme:0x1018)
          v22:CPtr = GetEP 0
          v23:RubyValue = LoadField v22, :VM_ENV_DATA_INDEX_ME_CREF@0x1040
          v24:CallableMethodEntry[VALUE(0x1048)] = GuardBitEquals v23, Value(VALUE(0x1048))
          v25:RubyValue = LoadField v22, :VM_ENV_DATA_INDEX_SPECVAL@0x1050
          v26:FalseClass = GuardBitEquals v25, Value(false)
          v27:Array = GuardType v9, Array
          v28:CUInt64 = LoadField v27, :RBASIC_FLAGS@0x1051
          v29:CUInt64 = GuardNoBitsSet v28, RUBY_FL_FREEZE=CUInt64(2048)
          ArrayPush v27, v10
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_optimize_array_pop_with_array_subclass() {
        eval("
            class PopSubArray < Array
              def pop = super
            end
            test = PopSubArray.new([1])
            test.pop
        ");
        assert_snapshot!(hir_string_proc("PopSubArray.new.method(:pop)"), @"
        fn pop@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Array@0x1000, pop@0x1008, cme:0x1010)
          v17:CPtr = GetEP 0
          v18:RubyValue = LoadField v17, :VM_ENV_DATA_INDEX_ME_CREF@0x1038
          v19:CallableMethodEntry[VALUE(0x1040)] = GuardBitEquals v18, Value(VALUE(0x1040))
          v20:RubyValue = LoadField v17, :VM_ENV_DATA_INDEX_SPECVAL@0x1048
          v21:FalseClass = GuardBitEquals v20, Value(false)
          v22:Array = GuardType v6, Array
          v23:CUInt64 = LoadField v22, :RBASIC_FLAGS@0x1049
          v24:CUInt64 = GuardNoBitsSet v23, RUBY_FL_FREEZE=CUInt64(2048)
          v26:CUInt64 = GuardNoBitsSet v24, RUBY_ELTS_SHARED=CUInt64(4096)
          v27:BasicObject = ArrayPop v22
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_optimize_array_aref_with_array_subclass_and_fixnum() {
        eval("
            class ArefSubArray < Array
              def [](idx) = super
            end
            test = ArefSubArray.new([1])
            test[0]
        ");
        assert_snapshot!(hir_string_proc("ArefSubArray.new.method(:[])"), @"
        fn []@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :idx@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :idx@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          v22:CPtr = GetEP 0
          v23:RubyValue = LoadField v22, :VM_ENV_DATA_INDEX_ME_CREF@0x1040
          v24:CallableMethodEntry[VALUE(0x1048)] = GuardBitEquals v23, Value(VALUE(0x1048))
          v25:RubyValue = LoadField v22, :VM_ENV_DATA_INDEX_SPECVAL@0x1050
          v26:FalseClass = GuardBitEquals v25, Value(false)
          v27:Array = GuardType v9, Array
          v28:Fixnum = GuardType v10, Fixnum
          v29:CInt64 = UnboxFixnum v28
          v30:CInt64 = ArrayLength v27
          v31:CInt64 = GuardLess v29, v30
          v32:CInt64 = AdjustBounds v31, v30
          v33:CInt64[0] = Const CInt64(0)
          v34:CInt64 = GuardGreaterEq v32, v33
          v35:BasicObject = ArrayAref v27, v34
          CheckInterrupts
          Return v35
        ");
    }

    #[test]
    fn test_dont_optimize_array_aref_with_array_subclass_and_non_fixnum() {
        eval("
            class ArefSubArrayRange < Array
              def [](idx) = super
            end
            test = ArefSubArrayRange.new([1, 2, 3])
            test[0..1]
        ");
        assert_snapshot!(hir_string_proc("ArefSubArrayRange.new.method(:[])"), @"
        fn []@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :idx@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :idx@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Array@0x1008, []@0x1010, cme:0x1018)
          v22:CPtr = GetEP 0
          v23:RubyValue = LoadField v22, :VM_ENV_DATA_INDEX_ME_CREF@0x1040
          v24:CallableMethodEntry[VALUE(0x1048)] = GuardBitEquals v23, Value(VALUE(0x1048))
          v25:RubyValue = LoadField v22, :VM_ENV_DATA_INDEX_SPECVAL@0x1050
          v26:FalseClass = GuardBitEquals v25, Value(false)
          v27:BasicObject = CCallVariadic v9, :Array#[]@0x1051, v10
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_optimize_array_length() {
        eval("
            def test(arr) = arr.length
            test([])
        ");
        assert_contains_opcode("test", YARVINSN_opt_length);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :arr@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, length@0x1010, cme:0x1018)
          v24:ArrayExact = GuardType v10, ArrayExact recompile
          v25:CInt64 = ArrayLength v24
          v26:Fixnum = BoxFixnum v25
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_optimize_array_size() {
        eval("
            def test(arr) = arr.size
            test([])
        ");
        assert_contains_opcode("test", YARVINSN_opt_size);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :arr@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :arr@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, size@0x1010, cme:0x1018)
          v24:ArrayExact = GuardType v10, ArrayExact recompile
          v25:CInt64 = ArrayLength v24
          v26:Fixnum = BoxFixnum v25
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_optimize_regexpmatch2() {
        eval(r#"
            def test(s) = s =~ /a/
            test("foo")
        "#);
        assert_contains_opcode("test", YARVINSN_opt_regexpmatch2);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :s@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:RegexpExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(String@0x1010)
          PatchPoint MethodRedefined(String@0x1010, =~@0x1018, cme:0x1020)
          v26:StringExact = GuardType v10, StringExact recompile
          v27:BasicObject = CCallWithFrame v26, :String#=~@0x1048, v14
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_optimize_string_getbyte_fixnum() {
        eval(r#"
            def test(s, i) = s.getbyte(i)
            test("foo", 0)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          v4:BasicObject = LoadField v2, :i@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :s@1
          v9:BasicObject = LoadArg :i@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, getbyte@0x1010, cme:0x1018)
          v27:StringExact = GuardType v12, StringExact recompile
          v28:Fixnum = GuardType v13, Fixnum
          v29:CInt64 = UnboxFixnum v28
          v30:CInt64 = LoadField v27, :len@0x1040
          v31:CInt64 = GuardLess v29, v30
          v32:CInt64 = AdjustBounds v31, v30
          v33:CInt64[0] = Const CInt64(0)
          v34:CInt64 = GuardGreaterEq v32, v33
          v35:Fixnum = StringGetbyte v27, v34
          CheckInterrupts
          Return v35
        ");
    }

    #[test]
    fn test_elide_string_getbyte_fixnum() {
        eval(r#"
            def test(s, i)
              s.getbyte(i)
              5
            end
            test("foo", 0)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          v4:BasicObject = LoadField v2, :i@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :s@1
          v9:BasicObject = LoadArg :i@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, getbyte@0x1010, cme:0x1018)
          v31:StringExact = GuardType v12, StringExact recompile
          v32:Fixnum = GuardType v13, Fixnum
          v33:CInt64 = UnboxFixnum v32
          v34:CInt64 = LoadField v31, :len@0x1040
          v35:CInt64 = GuardLess v33, v34
          v36:CInt64 = AdjustBounds v35, v34
          v37:CInt64[0] = Const CInt64(0)
          v38:CInt64 = GuardGreaterEq v36, v37
          v22:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_optimize_string_setbyte_fixnum() {
        eval(r#"
            def test(s, idx, val)
                s.setbyte(idx, val)
            end
            test("foo", 0, 127)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          v4:BasicObject = LoadField v2, :idx@0x1001
          v5:BasicObject = LoadField v2, :val@0x1002
          Jump bb3(v1, v3, v4, v5)
        bb2():
          EntryPoint JIT(0)
          v8:BasicObject = LoadArg :self@0
          v9:BasicObject = LoadArg :s@1
          v10:BasicObject = LoadArg :idx@2
          v11:BasicObject = LoadArg :val@3
          Jump bb3(v8, v9, v10, v11)
        bb3(v13:BasicObject, v14:BasicObject, v15:BasicObject, v16:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, setbyte@0x1010, cme:0x1018)
          v31:StringExact = GuardType v14, StringExact recompile
          v32:Fixnum = GuardType v15, Fixnum
          v33:Fixnum = GuardType v16, Fixnum
          v34:CInt64 = UnboxFixnum v32
          v35:CInt64 = LoadField v31, :len@0x1040
          v36:CInt64 = GuardLess v34, v35
          v37:CInt64 = AdjustBounds v36, v35
          v38:CInt64[0] = Const CInt64(0)
          v39:CInt64 = GuardGreaterEq v37, v38
          v40:CUInt64 = LoadField v31, :RBASIC_FLAGS@0x1041
          v41:CUInt64 = GuardNoBitsSet v40, RUBY_FL_FREEZE=CUInt64(2048)
          v42:Fixnum = StringSetbyteFixnum v31, v32, v33
          CheckInterrupts
          Return v33
        ");
    }

    #[test]
    fn test_optimize_string_subclass_setbyte_fixnum() {
        eval(r#"
            class MyString < String
            end
            def test(s, idx, val)
                s.setbyte(idx, val)
            end
            test(MyString.new('foo'), 0, 127)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          v4:BasicObject = LoadField v2, :idx@0x1001
          v5:BasicObject = LoadField v2, :val@0x1002
          Jump bb3(v1, v3, v4, v5)
        bb2():
          EntryPoint JIT(0)
          v8:BasicObject = LoadArg :self@0
          v9:BasicObject = LoadArg :s@1
          v10:BasicObject = LoadArg :idx@2
          v11:BasicObject = LoadArg :val@3
          Jump bb3(v8, v9, v10, v11)
        bb3(v13:BasicObject, v14:BasicObject, v15:BasicObject, v16:BasicObject):
          PatchPoint NoSingletonClass(MyString@0x1008)
          PatchPoint MethodRedefined(MyString@0x1008, setbyte@0x1010, cme:0x1018)
          v31:StringSubclass[class_exact:MyString] = GuardType v14, StringSubclass[class_exact:MyString] recompile
          v32:Fixnum = GuardType v15, Fixnum
          v33:Fixnum = GuardType v16, Fixnum
          v34:CInt64 = UnboxFixnum v32
          v35:CInt64 = LoadField v31, :len@0x1040
          v36:CInt64 = GuardLess v34, v35
          v37:CInt64 = AdjustBounds v36, v35
          v38:CInt64[0] = Const CInt64(0)
          v39:CInt64 = GuardGreaterEq v37, v38
          v40:CUInt64 = LoadField v31, :RBASIC_FLAGS@0x1041
          v41:CUInt64 = GuardNoBitsSet v40, RUBY_FL_FREEZE=CUInt64(2048)
          v42:Fixnum = StringSetbyteFixnum v31, v32, v33
          CheckInterrupts
          Return v33
        ");
    }

    #[test]
    fn test_do_not_optimize_string_setbyte_non_fixnum() {
        eval(r#"
            def test(s, idx, val)
                s.setbyte(idx, val)
            end
            test("foo", 0, 3.14)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          v4:BasicObject = LoadField v2, :idx@0x1001
          v5:BasicObject = LoadField v2, :val@0x1002
          Jump bb3(v1, v3, v4, v5)
        bb2():
          EntryPoint JIT(0)
          v8:BasicObject = LoadArg :self@0
          v9:BasicObject = LoadArg :s@1
          v10:BasicObject = LoadArg :idx@2
          v11:BasicObject = LoadArg :val@3
          Jump bb3(v8, v9, v10, v11)
        bb3(v13:BasicObject, v14:BasicObject, v15:BasicObject, v16:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, setbyte@0x1010, cme:0x1018)
          v31:StringExact = GuardType v14, StringExact recompile
          v32:BasicObject = CCallWithFrame v31, :String#setbyte@0x1040, v15, v16
          CheckInterrupts
          Return v32
        ");
    }

    #[test]
    fn test_specialize_string_empty() {
        eval(r#"
            def test(s)
              s.empty?
            end
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :s@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, empty?@0x1010, cme:0x1018)
          v24:StringExact = GuardType v10, StringExact recompile
          v25:CInt64 = LoadField v24, :len@0x1040
          v26:CInt64[0] = Const CInt64(0)
          v27:CBool = IsBitEqual v25, v26
          v28:BoolExact = BoxBool v27
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_eliminate_string_empty() {
        eval(r#"
            def test(s)
              s.empty?
              4
            end
            test("this should get removed")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :s@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, empty?@0x1010, cme:0x1018)
          v28:StringExact = GuardType v10, StringExact recompile
          v19:Fixnum[4] = Const Value(4)
          CheckInterrupts
          Return v19
        ");
    }

    #[test]
    fn test_inline_integer_succ_with_fixnum() {
        eval("
            def test(x) = x.succ
            test(4)
        ");
        assert_contains_opcode("test", YARVINSN_opt_succ);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, succ@0x1010, cme:0x1018)
          v23:Fixnum = GuardType v10, Fixnum recompile
          v24:Fixnum[1] = Const Value(1)
          v25:Fixnum = FixnumAdd v23, v24
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_dont_inline_integer_succ_with_bignum() {
        eval("
            def test(x) = x.succ
            test(4 << 70)
        ");
        assert_contains_opcode("test", YARVINSN_opt_succ);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, succ@0x1010, cme:0x1018)
          v23:Bignum = GuardType v10, Bignum recompile
          v24:BasicObject = CCallWithFrame v23, :Integer#succ@0x1040
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_inline_integer_ltlt_with_known_fixnum() {
        eval("
            def test(x) = x << 5
            test(4)
        ");
        assert_contains_opcode("test", YARVINSN_opt_ltlt);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[5] = Const Value(5)
          PatchPoint MethodRedefined(Integer@0x1008, <<@0x1010, cme:0x1018)
          v25:Fixnum = GuardType v10, Fixnum recompile
          v26:Fixnum = FixnumLShift v25, v14
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_dont_inline_integer_ltlt_with_negative() {
        eval("
            def test(x) = x << -5
            test(4)
        ");
        assert_contains_opcode("test", YARVINSN_opt_ltlt);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[-5] = Const Value(-5)
          PatchPoint MethodRedefined(Integer@0x1008, <<@0x1010, cme:0x1018)
          v25:Fixnum = GuardType v10, Fixnum recompile
          v26:BasicObject = CCallWithFrame v25, :Integer#<<@0x1040, v14
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_dont_inline_integer_ltlt_with_out_of_range() {
        eval("
            def test(x) = x << 64
            test(4)
        ");
        assert_contains_opcode("test", YARVINSN_opt_ltlt);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[64] = Const Value(64)
          PatchPoint MethodRedefined(Integer@0x1008, <<@0x1010, cme:0x1018)
          v25:Fixnum = GuardType v10, Fixnum recompile
          v26:BasicObject = CCallWithFrame v25, :Integer#<<@0x1040, v14
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_dont_inline_integer_ltlt_with_unknown_fixnum() {
        eval("
            def test(x, y) = x << y
            test(4, 5)
        ");
        assert_contains_opcode("test", YARVINSN_opt_ltlt);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, <<@0x1010, cme:0x1018)
          v27:Fixnum = GuardType v12, Fixnum recompile
          v28:BasicObject = CCallWithFrame v27, :Integer#<<@0x1040, v13
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_inline_integer_gtgt_with_known_fixnum() {
        eval("
            def test(x) = x >> 5
            test(4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[5] = Const Value(5)
          PatchPoint MethodRedefined(Integer@0x1008, >>@0x1010, cme:0x1018)
          v24:Fixnum = GuardType v10, Fixnum recompile
          v25:Fixnum = FixnumRShift v24, v14
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_dont_inline_integer_gtgt_with_negative() {
        eval("
            def test(x) = x >> -5
            test(4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[-5] = Const Value(-5)
          PatchPoint MethodRedefined(Integer@0x1008, >>@0x1010, cme:0x1018)
          v24:Fixnum = GuardType v10, Fixnum recompile
          v25:BasicObject = CCallWithFrame v24, :Integer#>>@0x1040, v14
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_dont_inline_integer_gtgt_with_out_of_range() {
        eval("
            def test(x) = x >> 64
            test(4)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[64] = Const Value(64)
          PatchPoint MethodRedefined(Integer@0x1008, >>@0x1010, cme:0x1018)
          v24:Fixnum = GuardType v10, Fixnum recompile
          v25:BasicObject = CCallWithFrame v24, :Integer#>>@0x1040, v14
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_dont_inline_integer_gtgt_with_unknown_fixnum() {
        eval("
            def test(x, y) = x >> y
            test(4, 5)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, >>@0x1010, cme:0x1018)
          v26:Fixnum = GuardType v12, Fixnum recompile
          v27:BasicObject = CCallWithFrame v26, :Integer#>>@0x1040, v13
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_optimize_string_append() {
        eval(r#"
            def test(x, y) = x << y
            test("iron", "fish")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, <<@0x1010, cme:0x1018)
          v28:StringExact = GuardType v12, StringExact recompile
          v29:String = GuardType v13, String
          v30:StringExact = StringAppend v28, v29
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_optimize_string_append_codepoint() {
        eval(r#"
            def test(x, y) = x << y
            test("iron", 4)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, <<@0x1010, cme:0x1018)
          v28:StringExact = GuardType v12, StringExact recompile
          v29:Fixnum = GuardType v13, Fixnum
          v30:StringExact = StringAppendCodepoint v28, v29
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_optimize_string_append_string_subclass() {
        eval(r#"
            class MyString < String
            end
            def test(x, y) = x << y
            test("iron", MyString.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, <<@0x1010, cme:0x1018)
          v28:StringExact = GuardType v12, StringExact recompile
          v29:String = GuardType v13, String
          v30:StringExact = StringAppend v28, v29
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_do_not_optimize_string_subclass_append_string() {
        eval(r#"
            class MyString < String
            end
            def test(x, y) = x << y
            test(MyString.new, "iron")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(MyString@0x1008)
          PatchPoint MethodRedefined(MyString@0x1008, <<@0x1010, cme:0x1018)
          v28:StringSubclass[class_exact:MyString] = GuardType v12, StringSubclass[class_exact:MyString] recompile
          v29:BasicObject = CCallWithFrame v28, :String#<<@0x1040, v13
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_dont_optimize_string_append_non_string() {
        eval(r#"
            def test = "iron" << :a
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v10:StringExact = StringCopy v9
          v12:StaticSymbol[:a] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(String@0x1010)
          PatchPoint MethodRedefined(String@0x1010, <<@0x1018, cme:0x1020)
          v24:BasicObject = CCallWithFrame v10, :String#<<@0x1048, v12
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_dont_optimize_when_passing_too_many_args() {
        eval(r#"
            public def foo(lead, opt=raise) = opt
            def test = 0.foo(3, 3, 3)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[0] = Const Value(0)
          v11:Fixnum[3] = Const Value(3)
          v13:Fixnum[3] = Const Value(3)
          v15:Fixnum[3] = Const Value(3)
          v17:BasicObject = Send v9, :foo, v11, v13, v15 # SendFallbackReason: Argument count does not match parameter count
          CheckInterrupts
          Return v17
        ");
    }

    #[test]
    fn test_optimize_string_ascii_only_p() {
        eval(r#"
            def test(x) = x.ascii_only?
            test("iron")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, ascii_only?@0x1010, cme:0x1018)
          v22:StringExact = GuardType v10, StringExact recompile
          v23:CUInt64 = LoadField v22, :RBASIC_FLAGS@0x1040
          v24:CUInt64[3145728] = Const CUInt64(3145728)
          v25:CInt64 = IntAnd v23, v24
          v26:CInt64[1048576] = Const CInt64(1048576)
          v27:CInt64 = GuardGreaterEq v25, v26
          v28:CInt64[1048576] = Const CInt64(1048576)
          v29:CBool = IsBitEqual v27, v28
          v30:BoolExact = BoxBool v29
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_dont_optimize_when_passing_too_few_args() {
        eval(r#"
            public def foo(lead, opt=raise) = opt
            def test = 0.foo
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[0] = Const Value(0)
          v11:BasicObject = Send v9, :foo # SendFallbackReason: Argument count does not match parameter count
          CheckInterrupts
          Return v11
        ");
    }

    #[test]
    fn test_dont_inline_integer_succ_with_args() {
        eval("
            def test = 4.succ 1
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[4] = Const Value(4)
          v11:Fixnum[1] = Const Value(1)
          v13:BasicObject = Send v9, :succ, v11 # SendFallbackReason: Argument count does not match parameter count
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_inline_integer_xor_with_fixnum() {
        eval("
            def test(x, y) = x ^ y
            test(1, 2)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, ^@0x1010, cme:0x1018)
          v26:Fixnum = GuardType v12, Fixnum recompile
          v27:Fixnum = GuardType v13, Fixnum
          v28:Fixnum = FixnumXor v26, v27
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_eliminate_integer_xor() {
        eval(r#"
            def test(x, y)
              x ^ y
              42
            end
            test(1, 2)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, ^@0x1010, cme:0x1018)
          v30:Fixnum = GuardType v12, Fixnum recompile
          v31:Fixnum = GuardType v13, Fixnum
          v22:Fixnum[42] = Const Value(42)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_dont_inline_integer_xor_with_bignum_lhs() {
        eval("
            def test(x, y) = x ^ y
            test(4 << 70, 1)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, ^@0x1010, cme:0x1018)
          v26:Bignum = GuardType v12, Bignum recompile
          v27:BasicObject = CCallWithFrame v26, :Integer#^@0x1040, v13
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_dont_inline_integer_xor_with_bignum_rhs() {
        eval("
            def test(x, y) = x ^ y
            test(1, 4 << 70)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, ^@0x1010, cme:0x1018)
          v26:Fixnum = GuardType v12, Fixnum recompile
          v27:BasicObject = CCallWithFrame v26, :Integer#^@0x1040, v13
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_dont_inline_integer_xor_with_boolean() {
        eval("
            def test(x, y) = x ^ y
            test(true, 0)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(TrueClass@0x1008, ^@0x1010, cme:0x1018)
          v26:TrueClass = GuardType v12, TrueClass recompile
          v27:BasicObject = CCallWithFrame v26, :TrueClass#^@0x1040, v13
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_dont_inline_integer_xor_with_args() {
        eval("
            def test(x, y) = x.^()
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:BasicObject = LoadField v2, :y@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:BasicObject = LoadArg :y@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          SideExit NoProfileSend recompile
        ");
    }

    #[test]
    fn test_specialize_hash_size() {
        eval("
            def test(hash) = hash.size
            test({foo: 3, bar: 1, baz: 4})
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :hash@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :hash@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(Hash@0x1008)
          PatchPoint MethodRedefined(Hash@0x1008, size@0x1010, cme:0x1018)
          v24:HashExact = GuardType v10, HashExact recompile
          v25:Fixnum = CCall v24, :Hash#size@0x1040
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_eliminate_hash_size() {
        eval("
            def test(hash)
                hash.size
                5
            end
            test({foo: 3, bar: 1, baz: 4})
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :hash@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :hash@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(Hash@0x1008)
          PatchPoint MethodRedefined(Hash@0x1008, size@0x1010, cme:0x1018)
          v28:HashExact = GuardType v10, HashExact recompile
          v19:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v19
        ");
    }

    #[test]
    fn test_optimize_respond_to_p_true() {
        eval(r#"
            class C
              def foo; end
            end
            def test(o) = o.respond_to?(:foo)
            test(C.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:StaticSymbol[:foo] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(C@0x1010)
          PatchPoint MethodRedefined(C@0x1010, respond_to?@0x1018, cme:0x1020)
          v25:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          PatchPoint MethodRedefined(C@0x1010, foo@0x1048, cme:0x1050)
          v29:TrueClass = Const Value(true)
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_optimize_respond_to_p_false_no_method() {
        eval(r#"
            class C
            end
            def test(o) = o.respond_to?(:foo)
            test(C.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:StaticSymbol[:foo] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(C@0x1010)
          PatchPoint MethodRedefined(C@0x1010, respond_to?@0x1018, cme:0x1020)
          v25:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          PatchPoint MethodRedefined(C@0x1010, respond_to_missing?@0x1048, cme:0x1050)
          PatchPoint MethodRedefined(C@0x1010, foo@0x1078, cme:0x1080)
          v31:FalseClass = Const Value(false)
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_optimize_respond_to_p_false_default_private() {
        eval(r#"
            class C
                private
                def foo; end
            end
            def test(o) = o.respond_to?(:foo)
            test(C.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:StaticSymbol[:foo] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(C@0x1010)
          PatchPoint MethodRedefined(C@0x1010, respond_to?@0x1018, cme:0x1020)
          v25:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          PatchPoint MethodRedefined(C@0x1010, foo@0x1048, cme:0x1050)
          v29:FalseClass = Const Value(false)
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_optimize_respond_to_p_false_private() {
        eval(r#"
            class C
                private
                def foo; end
            end
            def test(o) = o.respond_to?(:foo, false)
            test(C.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:StaticSymbol[:foo] = Const Value(VALUE(0x1008))
          v16:FalseClass = Const Value(false)
          PatchPoint NoSingletonClass(C@0x1010)
          PatchPoint MethodRedefined(C@0x1010, respond_to?@0x1018, cme:0x1020)
          v27:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          PatchPoint MethodRedefined(C@0x1010, foo@0x1048, cme:0x1050)
          v31:FalseClass = Const Value(false)
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_optimize_respond_to_p_falsy_private() {
        eval(r#"
            class C
                private
                def foo; end
            end
            def test(o) = o.respond_to?(:foo, nil)
            test(C.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:StaticSymbol[:foo] = Const Value(VALUE(0x1008))
          v16:NilClass = Const Value(nil)
          PatchPoint NoSingletonClass(C@0x1010)
          PatchPoint MethodRedefined(C@0x1010, respond_to?@0x1018, cme:0x1020)
          v27:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          PatchPoint MethodRedefined(C@0x1010, foo@0x1048, cme:0x1050)
          v31:FalseClass = Const Value(false)
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_optimize_respond_to_p_true_private() {
        eval(r#"
            class C
                private
                def foo; end
            end
            def test(o) = o.respond_to?(:foo, true)
            test(C.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:StaticSymbol[:foo] = Const Value(VALUE(0x1008))
          v16:TrueClass = Const Value(true)
          PatchPoint NoSingletonClass(C@0x1010)
          PatchPoint MethodRedefined(C@0x1010, respond_to?@0x1018, cme:0x1020)
          v27:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          PatchPoint MethodRedefined(C@0x1010, foo@0x1048, cme:0x1050)
          v31:TrueClass = Const Value(true)
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_optimize_respond_to_p_truthy() {
        eval(r#"
            class C
              def foo; end
            end
            def test(o) = o.respond_to?(:foo, 4)
            test(C.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:StaticSymbol[:foo] = Const Value(VALUE(0x1008))
          v16:Fixnum[4] = Const Value(4)
          PatchPoint NoSingletonClass(C@0x1010)
          PatchPoint MethodRedefined(C@0x1010, respond_to?@0x1018, cme:0x1020)
          v27:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          PatchPoint MethodRedefined(C@0x1010, foo@0x1048, cme:0x1050)
          v31:TrueClass = Const Value(true)
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_optimize_respond_to_p_falsy() {
        eval(r#"
            class C
              def foo; end
            end
            def test(o) = o.respond_to?(:foo, nil)
            test(C.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:StaticSymbol[:foo] = Const Value(VALUE(0x1008))
          v16:NilClass = Const Value(nil)
          PatchPoint NoSingletonClass(C@0x1010)
          PatchPoint MethodRedefined(C@0x1010, respond_to?@0x1018, cme:0x1020)
          v27:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          PatchPoint MethodRedefined(C@0x1010, foo@0x1048, cme:0x1050)
          v31:TrueClass = Const Value(true)
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_optimize_respond_to_missing() {
        eval(r#"
            class C
            end
            def test(o) = o.respond_to?(:foo)
            test(C.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:StaticSymbol[:foo] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(C@0x1010)
          PatchPoint MethodRedefined(C@0x1010, respond_to?@0x1018, cme:0x1020)
          v25:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          PatchPoint MethodRedefined(C@0x1010, respond_to_missing?@0x1048, cme:0x1050)
          PatchPoint MethodRedefined(C@0x1010, foo@0x1078, cme:0x1080)
          v31:FalseClass = Const Value(false)
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_do_not_optimize_redefined_respond_to_missing() {
        eval(r#"
            class C
                def respond_to_missing?(method, include_private = false)
                    true
                end
            end
            def test(o) = o.respond_to?(:foo)
            test(C.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:StaticSymbol[:foo] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(C@0x1010)
          PatchPoint MethodRedefined(C@0x1010, respond_to?@0x1018, cme:0x1020)
          v25:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v26:BasicObject = CCallVariadic v25, :Kernel#respond_to?@0x1048, v14
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_inline_send_without_block_direct_putself() {
        eval(r#"
            def callee = self
            def test = callee
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, callee@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          CheckInterrupts
          Return v17
        ");
    }

    #[test]
    fn test_inline_send_without_block_direct_putobject_string() {
        eval(r#"
            # frozen_string_literal: true
            def callee = "abc"
            def test = callee
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, callee@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v18:StringExact[VALUE(0x1038)] = Const Value(VALUE(0x1038))
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_inline_send_without_block_direct_putnil() {
        eval(r#"
            def callee = nil
            def test = callee
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, callee@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v18:NilClass = Const Value(nil)
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_inline_send_without_block_direct_putobject_true() {
        eval(r#"
            def callee = true
            def test = callee
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, callee@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v18:TrueClass = Const Value(true)
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_inline_send_without_block_direct_putobject_false() {
        eval(r#"
            def callee = false
            def test = callee
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, callee@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v18:FalseClass = Const Value(false)
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_inline_send_without_block_direct_putobject_zero() {
        eval(r#"
            def callee = 0
            def test = callee
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, callee@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v18:Fixnum[0] = Const Value(0)
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_inline_send_without_block_direct_putobject_one() {
        eval(r#"
            def callee = 1
            def test = callee
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, callee@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v18:Fixnum[1] = Const Value(1)
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_inline_send_without_block_direct_parameter() {
        eval(r#"
            def callee(x) = x
            def test = callee 3
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[3] = Const Value(3)
          PatchPoint MethodRedefined(Object@0x1000, callee@0x1008, cme:0x1010)
          v19:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_inline_send_without_block_direct_last_parameter() {
        eval(r#"
            def callee(x, y, z) = z
            def test = callee 1, 2, 3
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:Fixnum[2] = Const Value(2)
          v14:Fixnum[3] = Const Value(3)
          PatchPoint MethodRedefined(Object@0x1000, callee@0x1008, cme:0x1010)
          v23:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn test_splat() {
        eval("
            def foo = itself

            def test
              # Use a local to inhibit compile.c peephole optimization to ensure callsites have VM_CALL_ARGS_SPLAT
              empty = []
              foo(*empty)
              ''.display(*empty)
              itself(*empty)
            end
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:ArrayExact = NewArray
          v18:ArrayExact = ToArray v12
          v20:BasicObject = Send v8, :foo, v18 # SendFallbackReason: Complex argument passing
          v24:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v25:StringExact = StringCopy v24
          PatchPoint NoEPEscape(test)
          v30:ArrayExact = ToArray v12
          v32:BasicObject = Send v25, :display, v30 # SendFallbackReason: Complex argument passing
          PatchPoint NoEPEscape(test)
          v40:ArrayExact = ToArray v12
          v42:BasicObject = Send v8, :itself, v40 # SendFallbackReason: Complex argument passing
          CheckInterrupts
          Return v42
        ");
    }

    #[test]
    fn dont_specialize_call_to_iseq_with_monomorphic_caller_splat() {
        enable_zjit_stats();
        eval("
            def foo(*args) = args
            def test(args) = foo(*args)
            test([1])
            test([2])
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :args@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :args@1
          IncrCounterPtr
          Jump bb3(v6, v7)
        bb3(v10:BasicObject, v11:BasicObject):
          IncrCounter zjit_insn_count
          IncrCounter zjit_insn_count
          IncrCounter zjit_insn_count
          v19:ArrayExact = ToArray v11
          IncrCounter zjit_insn_count
          IncrCounter complex_arg_pass_caller_splat
          IncrCounter caller_splat_profile_monomorphic
          v22:BasicObject = Send v10, :foo, v19 # SendFallbackReason: Complex argument passing
          IncrCounter zjit_insn_count
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn dont_specialize_call_to_iseq_with_polymorphic_caller_splat() {
        enable_zjit_stats();
        set_call_threshold(3);
        eval("
            def foo(*args) = args
            def test(args) = foo(*args)
            test([1])
            test([1, 2])
            test([3])
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :args@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :args@1
          IncrCounterPtr
          Jump bb3(v6, v7)
        bb3(v10:BasicObject, v11:BasicObject):
          IncrCounter zjit_insn_count
          IncrCounter zjit_insn_count
          IncrCounter zjit_insn_count
          v19:ArrayExact = ToArray v11
          IncrCounter zjit_insn_count
          IncrCounter complex_arg_pass_caller_splat
          IncrCounter caller_splat_profile_polymorphic
          v22:BasicObject = Send v10, :foo, v19 # SendFallbackReason: Complex argument passing
          IncrCounter zjit_insn_count
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_inline_symbol_to_sym() {
        eval(r#"
            def test(o) = o.to_sym
            test :foo
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Symbol@0x1008, to_sym@0x1010, cme:0x1018)
          v21:StaticSymbol = GuardType v10, StaticSymbol recompile
          CheckInterrupts
          Return v21
        ");
    }

    #[test]
    fn test_inline_integer_to_i() {
        eval(r#"
            def test(o) = o.to_i
            test 5
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, to_i@0x1010, cme:0x1018)
          v21:Fixnum = GuardType v10, Fixnum recompile
          CheckInterrupts
          Return v21
        ");
    }

    #[test]
    fn test_inline_send_with_block_with_no_params() {
        // Passing a block to a method that doesn't use it falls back to the
        // interpreter so that unused block warnings are properly emitted.
        eval(r#"
            def callee = 123
            def test
              callee do
              end
            end
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:BasicObject = Send v6, 0x1000, :callee # SendFallbackReason: Complex argument passing
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_inline_send_with_block_with_one_param() {
        // Passing a block to a method that doesn't use it falls back to the
        // interpreter so that unused block warnings are properly emitted.
        eval(r#"
            def callee = 123
            def test
              callee do |_|
              end
            end
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:BasicObject = Send v6, 0x1000, :callee # SendFallbackReason: Complex argument passing
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_inline_send_with_block_with_multiple_params() {
        // Passing a block to a method that doesn't use it falls back to the
        // interpreter so that unused block warnings are properly emitted.
        eval(r#"
            def callee = 123
            def test
              callee do |_a, _b|
              end
            end
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:BasicObject = Send v6, 0x1000, :callee # SendFallbackReason: Complex argument passing
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_no_inline_send_with_symbol_block() {
        eval(r#"
            def callee = 123
            public def the_block = 456
            def test
              callee(&:the_block)
            end
            puts test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:StaticSymbol[:the_block] = Const Value(VALUE(0x1000))
          v12:BasicObject = Send v6, &block, :callee, v10 # SendFallbackReason: Send: block argument is not nil
          CheckInterrupts
          Return v12
        ");
    }

    #[test]
    fn test_profile_stack_skips_block_arg() {
        // Regression test: profile_stack must skip the &block arg on the stack when mapping
        // profiled operand types. Without the fix, the receiver type would be mapped to the
        // wrong stack slot, causing resolve_receiver_type to return NoProfile.
        // With the fix, the receiver type is correctly resolved and the send gets past type
        // resolution to hit the ARGS_BLOCKARG guard (ComplexArgPass) instead of NoProfile.
        eval("
            def test(&block) = [].map(&block)
            test { |x| x }; test { |x| x }
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :block@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :block@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:ArrayExact = NewArray
          v17:CPtr = GetEP 0
          v18:CUInt64 = LoadField v17, :VM_ENV_DATA_INDEX_FLAGS@0x1001
          v19:CBool = IsBlockParamModified v18
          CondBranch v19, bb4(), bb5()
        bb4():
          v21:BasicObject = LoadField v17, :block@0x1002
          Jump bb6(v21, v21)
        bb5():
          v23:CInt64 = LoadField v17, :VM_ENV_DATA_INDEX_SPECVAL@0x1003
          v24:CInt64 = GuardAnyBitSet v23, CUInt64(1) recompile
          v25:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1008))
          Jump bb6(v25, v10)
        bb6(v15:BasicObject, v16:BasicObject):
          v28:BasicObject = Send v13, &block, :map, v15 # SendFallbackReason: Send: block argument is not nil
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_optimize_stringexact_eq_stringexact() {
        eval(r#"
            def test(l, r) = l == r
            test("a", "b")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :l@0x1000
          v4:BasicObject = LoadField v2, :r@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :l@1
          v9:BasicObject = LoadArg :r@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, ==@0x1010, cme:0x1018)
          v28:StringExact = GuardType v12, StringExact recompile
          v29:String = GuardType v13, String
          v30:BoolExact = StringEqual v28, v29
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_optimize_string_eq_string() {
        eval(r#"
            class C < String
            end
            def test(l, r) = l == r
            test(C.new("a"), C.new("b"))
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :l@0x1000
          v4:BasicObject = LoadField v2, :r@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :l@1
          v9:BasicObject = LoadArg :r@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, ==@0x1010, cme:0x1018)
          v28:StringSubclass[class_exact:C] = GuardType v12, StringSubclass[class_exact:C] recompile
          v29:String = GuardType v13, String
          v30:BoolExact = StringEqual v28, v29
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_optimize_stringexact_eq_string() {
        eval(r#"
            class C < String
            end
            def test(l, r) = l == r
            test("a", C.new("b"))
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :l@0x1000
          v4:BasicObject = LoadField v2, :r@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :l@1
          v9:BasicObject = LoadArg :r@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, ==@0x1010, cme:0x1018)
          v28:StringExact = GuardType v12, StringExact recompile
          v29:String = GuardType v13, String
          v30:BoolExact = StringEqual v28, v29
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_optimize_stringexact_eqq_stringexact() {
        eval(r#"
            def test(l, r) = l === r
            test("a", "b")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :l@0x1000
          v4:BasicObject = LoadField v2, :r@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :l@1
          v9:BasicObject = LoadArg :r@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, ===@0x1010, cme:0x1018)
          v27:StringExact = GuardType v12, StringExact recompile
          v28:String = GuardType v13, String
          v29:BoolExact = StringEqual v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_optimize_string_eqq_string() {
        eval(r#"
            class C < String
            end
            def test(l, r) = l === r
            test(C.new("a"), C.new("b"))
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :l@0x1000
          v4:BasicObject = LoadField v2, :r@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :l@1
          v9:BasicObject = LoadArg :r@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, ===@0x1010, cme:0x1018)
          v27:StringSubclass[class_exact:C] = GuardType v12, StringSubclass[class_exact:C] recompile
          v28:String = GuardType v13, String
          v29:BoolExact = StringEqual v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_optimize_stringexact_eqq_string() {
        eval(r#"
            class C < String
            end
            def test(l, r) = l === r
            test("a", C.new("b"))
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :l@0x1000
          v4:BasicObject = LoadField v2, :r@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :l@1
          v9:BasicObject = LoadArg :r@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, ===@0x1010, cme:0x1018)
          v27:StringExact = GuardType v12, StringExact recompile
          v28:String = GuardType v13, String
          v29:BoolExact = StringEqual v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_fold_string_equal_same_operand_true() {
        eval(r#"
            def test(s) = s == s
            test("x")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :s@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, ==@0x1010, cme:0x1018)
          v25:StringExact = GuardType v10, StringExact recompile
          v28:TrueClass = Const Value(true)
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_fold_string_eqq_same_operand_true() {
        eval(r#"
            def test(s) = s === s
            test("x")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :s@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, ===@0x1010, cme:0x1018)
          v24:StringExact = GuardType v10, StringExact recompile
          v27:TrueClass = Const Value(true)
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_fold_string_equal_frozen_local_same_operand_true() {
        eval(r#"
            def test
              str = "a".freeze
              str == str
            end

            test
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          PatchPoint BOPRedefined(STRING_REDEFINED_OP_FLAG, BOP_FREEZE)
          v13:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, ==@0x1010, cme:0x1018)
          v31:TrueClass = Const Value(true)
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_fold_string_equal_frozen_distinct_literals_false() {
        eval(r#"
            def test
              "a".freeze == "b".freeze
            end

            test
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint BOPRedefined(STRING_REDEFINED_OP_FLAG, BOP_FREEZE)
          v10:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v13:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(String@0x1010)
          PatchPoint MethodRedefined(String@0x1010, ==@0x1018, cme:0x1020)
          v27:FalseClass = Const Value(false)
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_not_fold_string_equal_true_without_pragma() {
        eval(r#"
            def test
              "a" == "a"
            end

            test
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v10:StringExact = StringCopy v9
          v12:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v13:StringExact = StringCopy v12
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, ==@0x1010, cme:0x1018)
          v26:BoolExact = StringEqual v10, v13
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_not_fold_string_equal_false_without_pragma() {
        eval(r#"
            def test
              "a" == "b"
            end

            test
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v10:StringExact = StringCopy v9
          v12:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v13:StringExact = StringCopy v12
          PatchPoint NoSingletonClass(String@0x1010)
          PatchPoint MethodRedefined(String@0x1010, ==@0x1018, cme:0x1020)
          v26:BoolExact = StringEqual v10, v13
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_fold_string_equal_true_with_pragma() {
        eval(r#"
            # frozen_string_literal: true
            def test
              "a" == "a"
            end

            test
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v11:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, ==@0x1010, cme:0x1018)
          v25:TrueClass = Const Value(true)
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_fold_string_equal_false_with_pragma() {
        eval(r#"
            # frozen_string_literal: true
            def test
              "a" == "b"
            end

            test
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v11:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(String@0x1010)
          PatchPoint MethodRedefined(String@0x1010, ==@0x1018, cme:0x1020)
          v25:FalseClass = Const Value(false)
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_not_fold_string_equal_after_string_append_mutation() {
        eval(r#"
            def test
              a = "a"
              b = "a"
              a << "a"
              a == b
            end

            test
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          v3:NilClass = Const Value(nil)
          Jump bb3(v1, v2, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:NilClass = Const Value(nil)
          v8:NilClass = Const Value(nil)
          Jump bb3(v6, v7, v8)
        bb3(v10:BasicObject, v11:NilClass, v12:NilClass):
          v15:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v16:StringExact = StringCopy v15
          v20:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v21:StringExact = StringCopy v20
          v26:StringExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v27:StringExact = StringCopy v26
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, <<@0x1010, cme:0x1018)
          v49:StringExact = StringAppend v16, v27
          PatchPoint NoEPEscape(test)
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, ==@0x1040, cme:0x1048)
          v54:BoolExact = StringEqual v16, v21
          CheckInterrupts
          Return v54
        ");
    }

    #[test]
    fn test_not_fold_string_equal_distinct_objects() {
        eval(r#"
            def test(s, t) = s == t
            test("x", "x")
            test("x", "x")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          v4:BasicObject = LoadField v2, :t@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :s@1
          v9:BasicObject = LoadArg :t@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, ==@0x1010, cme:0x1018)
          v28:StringExact = GuardType v12, StringExact recompile
          v29:String = GuardType v13, String
          v30:BoolExact = StringEqual v28, v29
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_not_fold_string_equal_one_side_known_literal() {
        eval(r#"
            def test(s) = "a" == s
            test("a")
            test("a")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :s@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v13:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v14:StringExact = StringCopy v13
          PatchPoint NoSingletonClass(String@0x1010)
          PatchPoint MethodRedefined(String@0x1010, ==@0x1018, cme:0x1020)
          v28:String = GuardType v10, String
          v29:BoolExact = StringEqual v14, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn opt_neq_string_nil_falls_back_to_basic_object_neq() {
        eval(r#"
            def test(str)
              str != nil
            end

            test("x")
            test("x")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :str@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :str@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:NilClass = Const Value(nil)
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, !=@0x1010, cme:0x1018)
          v26:StringExact = GuardType v10, StringExact recompile
          v27:BoolExact = CCallWithFrame v26, :BasicObject#!=@0x1040, v14
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_inline_string_not_equal_distinct_objects() {
        eval(r#"
            def test(s, t) = s != t
            test("x", "x")
            test("x", "x")
        "#);
        assert_contains_opcode("test", YARVINSN_opt_neq);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          v4:BasicObject = LoadField v2, :t@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :s@1
          v9:BasicObject = LoadArg :t@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, !=@0x1010, cme:0x1018)
          v28:StringExact = GuardType v12, StringExact recompile
          PatchPoint MethodRedefined(String@0x1008, ==@0x1040, cme:0x1048)
          v32:String = GuardType v13, String
          v33:BoolExact = StringEqual v28, v32
          v34:TrueClass = Const Value(true)
          v35:CBool = IsBitNotEqual v33, v34
          v36:BoolExact = BoxBool v35
          CheckInterrupts
          Return v36
        ");
    }

    #[test]
    fn test_fold_string_not_equal_same_operand_false() {
        eval(r#"
            def test(s) = s != s
            test("x")
            test("x")
        "#);
        assert_contains_opcode("test", YARVINSN_opt_neq);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :s@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, !=@0x1010, cme:0x1018)
          v25:StringExact = GuardType v10, StringExact recompile
          PatchPoint MethodRedefined(String@0x1008, ==@0x1040, cme:0x1048)
          v34:TrueClass = Const Value(true)
          v31:TrueClass = Const Value(true)
          v32:CBool = IsBitNotEqual v34, v31
          v33:BoolExact = BoxBool v32
          CheckInterrupts
          Return v33
        ");
    }

    #[test]
    fn test_specialize_string_size() {
        eval(r#"
            def test(s)
              s.size
            end
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :s@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, size@0x1010, cme:0x1018)
          v24:StringExact = GuardType v10, StringExact recompile
          v25:Fixnum = CCall v24, :String#size@0x1040
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_elide_string_size() {
         eval(r#"
            def test(s)
              s.size
              5
            end
            test("asdf")
        "#);
       assert_snapshot!(hir_string("test"), @"
       fn test@<compiled>:3:
       bb1():
         EntryPoint interpreter
         v1:BasicObject = LoadSelf
         v2:CPtr = LoadSP
         v3:BasicObject = LoadField v2, :s@0x1000
         Jump bb3(v1, v3)
       bb2():
         EntryPoint JIT(0)
         v6:BasicObject = LoadArg :self@0
         v7:BasicObject = LoadArg :s@1
         Jump bb3(v6, v7)
       bb3(v9:BasicObject, v10:BasicObject):
         PatchPoint NoSingletonClass(String@0x1008)
         PatchPoint MethodRedefined(String@0x1008, size@0x1010, cme:0x1018)
         v28:StringExact = GuardType v10, StringExact recompile
         v19:Fixnum[5] = Const Value(5)
         CheckInterrupts
         Return v19
       ");
    }

    #[test]
    fn test_inline_string_bytesize() {
        eval(r#"
            def test(s)
              s.bytesize
            end
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :s@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, bytesize@0x1010, cme:0x1018)
          v23:StringExact = GuardType v10, StringExact recompile
          v24:CInt64 = LoadField v23, :len@0x1040
          v25:Fixnum = BoxFixnum v24
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_elide_string_bytesize() {
        eval(r#"
            def test(s)
              s.bytesize
              5
            end
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :s@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, bytesize@0x1010, cme:0x1018)
          v27:StringExact = GuardType v10, StringExact recompile
          v18:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_specialize_string_length() {
        eval(r#"
            def test(s)
              s.length
            end
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :s@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, length@0x1010, cme:0x1018)
          v24:StringExact = GuardType v10, StringExact recompile
          v25:Fixnum = CCall v24, :String#length@0x1040
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_specialize_class_eqq() {
        eval(r#"
            def test(o) = String === o
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1008, String)
          v15:ClassSubclass[String@0x1010] = Const Value(VALUE(0x1010))
          PatchPoint NoEPEscape(test)
          PatchPoint MethodRedefined(Class@0x1018, ===@0x1020, cme:0x1028)
          v29:BoolExact = IsA v10, v15
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_dont_specialize_module_eqq() {
        eval(r#"
            def test(o) = Kernel === o
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1008, Kernel)
          v15:ModuleSubclass[Kernel@0x1010] = Const Value(VALUE(0x1010))
          PatchPoint NoEPEscape(test)
          PatchPoint MethodRedefined(Module@0x1018, ===@0x1020, cme:0x1028)
          v29:BoolExact = CCall v15, :Module#===@0x1050, v10
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_specialize_is_a_class() {
        eval(r#"
            def test(o) = o.is_a?(String)
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1008, String)
          v16:ClassSubclass[String@0x1010] = Const Value(VALUE(0x1010))
          PatchPoint NoSingletonClass(String@0x1010)
          PatchPoint MethodRedefined(String@0x1010, is_a?@0x1011, cme:0x1018)
          v27:StringExact = GuardType v10, StringExact recompile
          v28:BoolExact = IsA v27, v16
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_dont_specialize_is_a_module() {
        eval(r#"
            def test(o) = o.is_a?(Kernel)
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1008, Kernel)
          v16:ModuleSubclass[Kernel@0x1010] = Const Value(VALUE(0x1010))
          PatchPoint NoSingletonClass(String@0x1018)
          PatchPoint MethodRedefined(String@0x1018, is_a?@0x1020, cme:0x1028)
          v27:StringExact = GuardType v10, StringExact recompile
          v28:BasicObject = CCallWithFrame v27, :Kernel#is_a?@0x1050, v16
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_elide_is_a() {
        eval(r#"
            def test(o)
              o.is_a?(Integer)
              5
            end
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1008, Integer)
          v16:ClassSubclass[Integer@0x1010] = Const Value(VALUE(0x1010))
          PatchPoint NoSingletonClass(String@0x1018)
          PatchPoint MethodRedefined(String@0x1018, is_a?@0x1020, cme:0x1028)
          v31:StringExact = GuardType v10, StringExact recompile
          v22:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_elide_class_eqq() {
        eval(r#"
            def test(o)
              Integer === o
              5
            end
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1008, Integer)
          v15:ClassSubclass[Integer@0x1010] = Const Value(VALUE(0x1010))
          PatchPoint NoEPEscape(test)
          PatchPoint MethodRedefined(Class@0x1018, ===@0x1020, cme:0x1028)
          v24:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_specialize_kind_of_class() {
        eval(r#"
            def test(o) = o.kind_of?(String)
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1008, String)
          v16:ClassSubclass[String@0x1010] = Const Value(VALUE(0x1010))
          PatchPoint NoSingletonClass(String@0x1010)
          PatchPoint MethodRedefined(String@0x1010, kind_of?@0x1011, cme:0x1018)
          v27:StringExact = GuardType v10, StringExact recompile
          v28:BoolExact = IsA v27, v16
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_dont_specialize_kind_of_module() {
        eval(r#"
            def test(o) = o.kind_of?(Kernel)
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1008, Kernel)
          v16:ModuleSubclass[Kernel@0x1010] = Const Value(VALUE(0x1010))
          PatchPoint NoSingletonClass(String@0x1018)
          PatchPoint MethodRedefined(String@0x1018, kind_of?@0x1020, cme:0x1028)
          v27:StringExact = GuardType v10, StringExact recompile
          v28:BasicObject = CCallWithFrame v27, :Kernel#kind_of?@0x1050, v16
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_elide_kind_of() {
        eval(r#"
            def test(o)
              o.kind_of?(Integer)
              5
            end
            test("asdf")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1008, Integer)
          v16:ClassSubclass[Integer@0x1010] = Const Value(VALUE(0x1010))
          PatchPoint NoSingletonClass(String@0x1018)
          PatchPoint MethodRedefined(String@0x1018, kind_of?@0x1020, cme:0x1028)
          v31:StringExact = GuardType v10, StringExact recompile
          v22:Fixnum[5] = Const Value(5)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_fold_is_a_true() {
        eval(r#"
            def test = 5.is_a?(Integer)
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[5] = Const Value(5)
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Integer)
          v13:ClassSubclass[Integer@0x1008] = Const Value(VALUE(0x1008))
          PatchPoint MethodRedefined(Integer@0x1008, is_a?@0x1009, cme:0x1010)
          v25:TrueClass = Const Value(true)
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_fold_is_a_false() {
        eval(r#"
            def test = 5.is_a?(String)
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[5] = Const Value(5)
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, String)
          v13:ClassSubclass[String@0x1008] = Const Value(VALUE(0x1008))
          PatchPoint MethodRedefined(Integer@0x1010, is_a?@0x1018, cme:0x1020)
          v25:FalseClass = Const Value(false)
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_is_a_array_subclass_folds_to_true() {
        eval(r#"
            class C < Array; end
            O = C.new
            def test = O.is_a?(Array)
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, O)
          v11:ArraySubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint StableConstantNames(0x1010, Array)
          v15:ClassSubclass[Array@0x1018] = Const Value(VALUE(0x1018))
          PatchPoint NoSingletonClass(C@0x1020)
          PatchPoint MethodRedefined(C@0x1020, is_a?@0x1028, cme:0x1030)
          v28:TrueClass = Const Value(true)
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_is_a_user_defined_class_folds_to_true() {
        eval(r#"
            class C; end
            O = C.new
            def test = O.is_a?(C)
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, O)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint StableConstantNames(0x1010, C)
          v15:ClassSubclass[C@0x1018] = Const Value(VALUE(0x1018))
          PatchPoint NoSingletonClass(C@0x1018)
          PatchPoint MethodRedefined(C@0x1018, is_a?@0x1019, cme:0x1020)
          v28:TrueClass = Const Value(true)
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_is_a_symbol_folds_to_true() {
        eval(r#"
            O = :my_static_symbol
            def test = O.is_a?(Symbol)
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, O)
          v11:StaticSymbol[:my_static_symbol] = Const Value(VALUE(0x1008))
          PatchPoint StableConstantNames(0x1010, Symbol)
          v15:ClassSubclass[Symbol@0x1018] = Const Value(VALUE(0x1018))
          PatchPoint MethodRedefined(Symbol@0x1018, is_a?@0x1019, cme:0x1020)
          v27:TrueClass = Const Value(true)
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn counting_complex_feature_use_for_fallback() {
        eval("
            define_method(:fancy) { |_a, *_b, kw: 100, **kw_rest, &block| }
            def test = fancy(1)
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          v12:BasicObject = Send v6, :fancy, v10 # SendFallbackReason: Complex argument passing
          CheckInterrupts
          Return v12
        ");
    }

    #[test]
    fn call_method_forwardable_param() {
        eval("
           def forwardable(...) = itself(...)
           def call_forwardable = forwardable
           call_forwardable
        ");
        assert_snapshot!(hir_string("call_forwardable"), @"
        fn call_forwardable@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:BasicObject = Send v6, :forwardable # SendFallbackReason: Complex argument passing
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_elide_string_length() {
        eval(r#"
            def test(s)
              s.length
              4
            end
            test("this should get removed")
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :s@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(String@0x1008)
          PatchPoint MethodRedefined(String@0x1008, length@0x1010, cme:0x1018)
          v28:StringExact = GuardType v10, StringExact recompile
          v19:Fixnum[4] = Const Value(4)
          CheckInterrupts
          Return v19
        ");
    }

    #[test]
    fn test_fold_self_class_respond_to_true() {
        eval(r#"
            class C
              class << self
                attr_accessor :_lex_actions
                private :_lex_actions, :_lex_actions=
              end
              self._lex_actions = [1, 2, 3]
              def initialize
                if self.class.respond_to?(:_lex_actions, true)
                  :CORRECT
                else
                  :oh_no_wrong
                end
              end
            end
            C.new  # warm up
            TEST = C.instance_method(:initialize)
        "#);
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn initialize@<compiled>:9:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint NoSingletonClass(C@0x1000)
          PatchPoint MethodRedefined(C@0x1000, class@0x1008, cme:0x1010)
          v40:ObjectSubclass[class_exact:C] = GuardType v6, ObjectSubclass[class_exact:C] recompile
          v41:ClassSubclass[C@0x1000] = Const Value(VALUE(0x1000))
          v12:StaticSymbol[:_lex_actions] = Const Value(VALUE(0x1038))
          v14:TrueClass = Const Value(true)
          PatchPoint MethodRedefined(Class@0x1040, respond_to?@0x1048, cme:0x1050)
          PatchPoint MethodRedefined(Class@0x1040, _lex_actions@0x1078, cme:0x1080)
          v24:StaticSymbol[:CORRECT] = Const Value(VALUE(0x10a8))
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn test_fold_self_class_name() {
        eval(r#"
            class C; end
            def test(o) = o.class.name
            test(C.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, class@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v25:ClassSubclass[C@0x1008] = Const Value(VALUE(0x1008))
          PatchPoint MethodRedefined(Class@0x1040, name@0x1048, cme:0x1050)
          v29:StringExact|NilClass = CCall v25, :Module#name@0x1078
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_fold_kernel_class() {
        eval(r#"
            class C; end
            def test(o) = o.class
            test(C.new)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, class@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact:C] = GuardType v10, ObjectSubclass[class_exact:C] recompile
          v23:ClassSubclass[C@0x1008] = Const Value(VALUE(0x1008))
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_fold_fixnum_class() {
        eval(r#"
            def test = 5.class
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[5] = Const Value(5)
          PatchPoint MethodRedefined(Integer@0x1000, class@0x1008, cme:0x1010)
          v19:ClassSubclass[Integer@0x1000] = Const Value(VALUE(0x1000))
          CheckInterrupts
          Return v19
        ");
    }

    #[test]
    fn test_fold_singleton_class() {
        eval(r#"
            def test = self.class
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, class@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v18:ClassSubclass[Object@0x1038] = Const Value(VALUE(0x1038))
          CheckInterrupts
          Return v18
        ");
    }

    #[test]
    fn test_print_nil_module_name() {
        eval(r#"
            X = [Module.new].freeze
            def test = X[0]
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, X)
          v11:ArrayExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v13:Fixnum[0] = Const Value(0)
          PatchPoint NoSingletonClass(Array@0x1010)
          PatchPoint MethodRedefined(Array@0x1010, []@0x1018, cme:0x1020)
          v35:ModuleExact[VALUE(0x1048)] = Const Value(VALUE(0x1048))
          CheckInterrupts
          Return v35
        ");
    }

    #[test]
    fn no_load_from_ep_right_after_entrypoint() {
      let formatted = eval("
          def read_nil_local(a, _b, _c)
            formatted ||= a
            @formatted = formatted
            -> { formatted } # the environment escapes
          end

          def call
            puts [], [], [], []     # fill VM stack with junk
            read_nil_local(true, 1, 1) # expected direct send
          end

          call # profile
          call # compile
          @formatted
       ");
       assert_eq!(Qtrue, formatted, "{}", formatted.obj_info());
       assert_snapshot!(hir_string("read_nil_local"), @"
       fn read_nil_local@<compiled>:3:
       bb1():
         EntryPoint interpreter
         v1:BasicObject = LoadSelf
         v2:CPtr = LoadSP
         v3:BasicObject = LoadField v2, :a@0x1000
         v4:BasicObject = LoadField v2, :_b@0x1001
         v5:BasicObject = LoadField v2, :_c@0x1002
         v6:NilClass = Const Value(nil)
         Jump bb3(v1, v3, v4, v5, v6)
       bb2():
         EntryPoint JIT(0)
         v9:BasicObject = LoadArg :self@0
         v10:BasicObject = LoadArg :a@1
         v11:CPtr = GetEP 0
         StoreField v11, :a@0x1001, v10
         v13:BasicObject = LoadArg :_b@2
         StoreField v11, :_b@0x1002, v13
         v15:BasicObject = LoadArg :_c@3
         StoreField v11, :_c@0x1003, v15
         v17:NilClass = Const Value(nil)
         StoreField v11, :formatted@0x1004, v17
         Jump bb3(v9, v10, v13, v15, v17)
       bb3(v20:BasicObject, v21:BasicObject, v22:BasicObject, v23:BasicObject, v24:NilClass):
         SetLocal :formatted, l0, EP@3, v21
         PatchPoint SingleRactorMode
         v45:HeapBasicObject = GuardType v20, HeapBasicObject
         v46:CShape = LoadField v45, :shape_id@0x1005
         v47:CShape[0x1006] = Const CShape(0x1006)
         v48:CBool = IsBitEqual v46, v47
         CondBranch v48, bb7(), bb8()
       bb7():
         StoreField v45, :@formatted@0x1007, v21
         WriteBarrier v45, v21
         Jump bb6()
       bb8():
         v53:CShape[0x1008] = GuardBitEquals v46, CShape(0x1008) recompile
         StoreField v45, :@formatted@0x1007, v21
         WriteBarrier v45, v21
         v57:CShape[0x1006] = Const CShape(0x1006)
         StoreField v45, :shape_id@0x1005, v57
         Jump bb6()
       bb6():
         v63:ClassSubclass[VMFrozenCore] = Const Value(VALUE(0x1010))
         PatchPoint MethodRedefined(Class@0x1018, lambda@0x1020, cme:0x1028)
         v79:BasicObject = CCallWithFrame v63, :RubyVM::FrozenCore.lambda@0x1050, block=0x1058
         v66:CPtr = GetEP 0
         v67:BasicObject = LoadField v66, :a@0x1001
         v68:BasicObject = LoadField v66, :_b@0x1002
         v69:BasicObject = LoadField v66, :_c@0x1003
         v70:BasicObject = LoadField v66, :formatted@0x1004
         CheckInterrupts
         Return v79
       ");
    }

    #[test]
    fn test_fold_load_field_frozen_constant_object() {
        // Basic case: frozen constant object with attr_accessor
        eval("
            class TestFrozen
              attr_accessor :a
              def initialize
                @a = 1
              end
            end

            FROZEN_OBJ = TestFrozen.new.freeze

            def test = FROZEN_OBJ.a
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:11:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, FROZEN_OBJ)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(TestFrozen@0x1010)
          PatchPoint MethodRedefined(TestFrozen@0x1010, a@0x1018, cme:0x1020)
          v27:Fixnum[1] = Const Value(1)
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_fold_load_field_frozen_multiple_ivars() {
        // Frozen object with multiple instance variables
        eval("
            class TestMultiIvars
              attr_accessor :a, :b, :c
              def initialize
                @a = 10
                @b = 20
                @c = 30
              end
            end

            MULTI_FROZEN = TestMultiIvars.new.freeze

            def test = MULTI_FROZEN.b
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:13:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, MULTI_FROZEN)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(TestMultiIvars@0x1010)
          PatchPoint MethodRedefined(TestMultiIvars@0x1010, b@0x1018, cme:0x1020)
          v27:Fixnum[20] = Const Value(20)
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_fold_load_field_frozen_string_value() {
        // Frozen object with a string ivar
        eval(r#"
            class TestFrozenStr
              attr_accessor :name
              def initialize
                @name = "hello"
              end
            end

            FROZEN_STR = TestFrozenStr.new.freeze

            def test = FROZEN_STR.name
            test
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:11:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, FROZEN_STR)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(TestFrozenStr@0x1010)
          PatchPoint MethodRedefined(TestFrozenStr@0x1010, name@0x1018, cme:0x1020)
          v27:StringExact[VALUE(0x1048)] = Const Value(VALUE(0x1048))
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_fold_load_field_frozen_nil_value() {
        // Frozen object with nil ivar
        eval("
            class TestFrozenNil
              attr_accessor :value
              def initialize
                @value = nil
              end
            end

            FROZEN_NIL = TestFrozenNil.new.freeze

            def test = FROZEN_NIL.value
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:11:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, FROZEN_NIL)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(TestFrozenNil@0x1010)
          PatchPoint MethodRedefined(TestFrozenNil@0x1010, value@0x1018, cme:0x1020)
          v27:NilClass = Const Value(nil)
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_no_fold_load_field_unfrozen_object() {
        // Non-frozen object should NOT be folded
        eval("
            class TestUnfrozen
              attr_accessor :a
              def initialize
                @a = 1
              end
            end

            UNFROZEN_OBJ = TestUnfrozen.new

            def test = UNFROZEN_OBJ.a
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:11:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, UNFROZEN_OBJ)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(TestUnfrozen@0x1010)
          PatchPoint MethodRedefined(TestUnfrozen@0x1010, a@0x1018, cme:0x1020)
          v23:CShape = LoadField v11, :shape_id@0x1048
          v24:CShape[0x1049] = GuardBitEquals v23, CShape(0x1049) recompile
          v25:BasicObject = LoadField v11, :@a@0x104a
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_fold_load_field_frozen_with_attr_reader() {
        // Using attr_reader instead of attr_accessor
        eval("
            class TestAttrReader
              attr_reader :value
              def initialize(v)
                @value = v
              end
            end

            FROZEN_READER = TestAttrReader.new(42).freeze

            def test = FROZEN_READER.value
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:11:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, FROZEN_READER)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(TestAttrReader@0x1010)
          PatchPoint MethodRedefined(TestAttrReader@0x1010, value@0x1018, cme:0x1020)
          v27:Fixnum[42] = Const Value(42)
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_fold_load_field_frozen_symbol_value() {
        // Frozen object with a symbol ivar
        eval("
            class TestFrozenSym
              attr_accessor :sym
              def initialize
                @sym = :hello
              end
            end

            FROZEN_SYM = TestFrozenSym.new.freeze

            def test = FROZEN_SYM.sym
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:11:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, FROZEN_SYM)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(TestFrozenSym@0x1010)
          PatchPoint MethodRedefined(TestFrozenSym@0x1010, sym@0x1018, cme:0x1020)
          v27:StaticSymbol[:hello] = Const Value(VALUE(0x1048))
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_fold_load_field_frozen_true_false() {
        // Frozen object with boolean ivars
        eval("
            class TestFrozenBool
              attr_accessor :flag
              def initialize
                @flag = true
              end
            end

            FROZEN_TRUE = TestFrozenBool.new.freeze

            def test = FROZEN_TRUE.flag
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:11:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, FROZEN_TRUE)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(TestFrozenBool@0x1010)
          PatchPoint MethodRedefined(TestFrozenBool@0x1010, flag@0x1018, cme:0x1020)
          v27:TrueClass = Const Value(true)
          CheckInterrupts
          Return v27
        ");
    }

    #[test]
    fn test_no_fold_load_field_dynamic_receiver() {
        // Dynamic receiver (not a constant) should NOT be folded even if object is frozen
        eval("
            class TestDynamic
              attr_accessor :val
              def initialize
                @val = 99
              end
            end

            def test(obj) = obj.val
            o = TestDynamic.new.freeze
            test o
            test o
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:9:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :obj@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :obj@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(TestDynamic@0x1008)
          PatchPoint MethodRedefined(TestDynamic@0x1008, val@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact:TestDynamic] = GuardType v10, ObjectSubclass[class_exact:TestDynamic] recompile
          v24:CShape = LoadField v22, :shape_id@0x1040
          v25:CShape[0x1041] = GuardBitEquals v24, CShape(0x1041) recompile
          v26:BasicObject = LoadField v22, :@val@0x1042
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_fold_load_field_frozen_nested_access() {
        // Accessing multiple fields from frozen constant in sequence
        eval("
            class TestNestedAccess
              attr_accessor :x, :y
              def initialize
                @x = 100
                @y = 200
              end
            end

            NESTED_FROZEN = TestNestedAccess.new.freeze

            def test = NESTED_FROZEN.x + NESTED_FROZEN.y
            test
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:12:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, NESTED_FROZEN)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(TestNestedAccess@0x1010)
          PatchPoint MethodRedefined(TestNestedAccess@0x1010, x@0x1018, cme:0x1020)
          v48:Fixnum[100] = Const Value(100)
          PatchPoint StableConstantNames(0x1048, NESTED_FROZEN)
          v17:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint MethodRedefined(TestNestedAccess@0x1010, y@0x1050, cme:0x1058)
          v50:Fixnum[200] = Const Value(200)
          PatchPoint MethodRedefined(Integer@0x1080, +@0x1088, cme:0x1090)
          v51:Fixnum[300] = Const Value(300)
          CheckInterrupts
          Return v51
        ");
    }

    #[test]
    fn test_dont_fold_load_field_with_primitive_return_type() {
        eval(r#"
            S = "abc".freeze
            def test = S.bytesize
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, S)
          v11:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(String@0x1010)
          PatchPoint MethodRedefined(String@0x1010, bytesize@0x1018, cme:0x1020)
          v23:CInt64 = LoadField v11, :len@0x1048
          v24:Fixnum = BoxFixnum v23
          CheckInterrupts
          Return v24
        ");
    }

    #[test]
    fn optimize_call_to_private_method_iseq_with_fcall() {
        eval(r#"
            class C
              def callprivate = secret
              private def secret = 42
            end
            C.new.callprivate
        "#);
        assert_snapshot!(hir_string_proc("C.instance_method(:callprivate)"), @"
        fn callprivate@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint NoSingletonClass(C@0x1000)
          PatchPoint MethodRedefined(C@0x1000, secret@0x1008, cme:0x1010)
          v18:ObjectSubclass[class_exact:C] = GuardType v6, ObjectSubclass[class_exact:C] recompile
          v19:Fixnum[42] = Const Value(42)
          CheckInterrupts
          Return v19
        ");
    }

    #[test]
    fn dont_optimize_call_to_private_method_iseq() {
        eval(r#"
            class C
              private def secret = 42
            end
            Obj = C.new
            def test = Obj.secret rescue $!
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Obj)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v13:BasicObject = Send v11, :secret # SendFallbackReason: Send: method private or protected and no FCALL
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn optimize_call_to_private_method_cfunc_with_fcall() {
        eval(r#"
            class BasicObject
              def callprivate = initialize rescue $!
            end
            Obj = BasicObject.new.callprivate
        "#);
        assert_snapshot!(hir_string_proc("BasicObject.instance_method(:callprivate)"), @"
        fn callprivate@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint NoSingletonClass(BasicObject@0x1000)
          PatchPoint MethodRedefined(BasicObject@0x1000, initialize@0x1008, cme:0x1010)
          v20:BasicObjectExact = GuardType v6, BasicObjectExact recompile
          v21:NilClass = Const Value(nil)
          CheckInterrupts
          Return v21
        ");
    }

    #[test]
    fn dont_optimize_call_to_private_method_cfunc() {
        eval(r#"
            Obj = BasicObject.new
            def test = Obj.initialize rescue $!
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Obj)
          v11:BasicObjectExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v13:BasicObject = Send v11, :initialize # SendFallbackReason: Send: method private or protected and no FCALL
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn dont_optimize_call_to_private_top_level_method() {
        eval(r#"
            def toplevel_method = :OK
            Obj = Object.new
            def test = Obj.toplevel_method rescue $!
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Obj)
          v11:ObjectExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v13:BasicObject = Send v11, :toplevel_method # SendFallbackReason: Send: method private or protected and no FCALL
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn optimize_call_to_protected_method_iseq_with_fcall() {
        eval(r#"
            class C
              def callprotected = secret
              protected def secret = 42
            end
            C.new.callprotected
        "#);
        assert_snapshot!(hir_string_proc("C.instance_method(:callprotected)"), @"
        fn callprotected@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint NoSingletonClass(C@0x1000)
          PatchPoint MethodRedefined(C@0x1000, secret@0x1008, cme:0x1010)
          v18:ObjectSubclass[class_exact:C] = GuardType v6, ObjectSubclass[class_exact:C] recompile
          v19:Fixnum[42] = Const Value(42)
          CheckInterrupts
          Return v19
        ");
    }

    #[test]
    fn dont_optimize_call_to_protected_method_iseq() {
        eval(r#"
            class C
              protected def secret = 42
            end
            Obj = C.new
            def test = Obj.secret rescue $!
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Obj)
          v11:ObjectSubclass[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v13:BasicObject = Send v11, :secret # SendFallbackReason: Send: method private or protected and no FCALL
          CheckInterrupts
          Return v13
        ");
    }

    // Test that when a singleton class has been seen for a class, we skip the
    // NoSingletonClass optimization to avoid an invalidation loop.
    #[test]
    fn test_skip_optimization_after_singleton_class_seen() {
        // First, compile a function that uses the NoSingletonClass assumption
        eval(r#"
            def test(s, proc)
              s.length
              proc.call
              s.length
            end
            test("hi", -> {})
            test("hi", -> {})
        "#);
        let hir = hir_string("test");
        assert!(hir.contains("NoSingletonClass(String"), "{hir}");

        // Now we break the assumption by defining a singleton method on a string.
        eval(r#"
            special_string = +""
            test(special_string, -> { def special_string.length = -1 })
        "#);

        // The output should NOT have NoSingletonClass patchpoint for String, and should
        // fall back to SendWithoutBlock instead of the optimized CCall path.
        let hir = hir_string("test");
        assert!(! hir.contains("NoSingletonClass(String"), "{hir}");
        assert_snapshot!(hir, @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :s@0x1000
          v4:BasicObject = LoadField v2, :proc@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :s@1
          v9:BasicObject = LoadArg :proc@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          v18:BasicObject = Send v12, :length # SendFallbackReason: Singleton class previously created for receiver class
          PatchPoint NoSingletonClass(Proc@0x1008)
          PatchPoint MethodRedefined(Proc@0x1008, call@0x1010, cme:0x1018)
          v39:ObjectSubclass[class_exact:Proc] = GuardType v13, ObjectSubclass[class_exact:Proc] recompile
          v40:BasicObject = InvokeProc v39
          PatchPoint NoEPEscape(test)
          v31:BasicObject = Send v12, :length # SendFallbackReason: Singleton class previously created for receiver class
          CheckInterrupts
          Return v31
        ");
    }

    #[test]
    fn test_no_singleton_class_busts_isolated_per_iseq() {
        // First, compile a function that uses the NoSingletonClass assumption
        eval(r#"
            def will_bust(s, proc)
              s.length
              proc.call
              s.length
            end

            def call_length(s) = s.length

            will_bust("hi", -> {})
            will_bust("hi", -> {})
        "#);
        let hir = hir_string("will_bust");
        assert!(hir.contains("NoSingletonClass(String"), "{hir}");

        // Now we break the assumption by defining a singleton method on a string.
        eval(r#"
            special_string = +""
            will_bust(special_string, -> { def special_string.length = -1 })
        "#);
        let hir = hir_string("will_bust");
        assert!(! hir.contains("NoSingletonClas(String"), "{hir}");

        // But, the unrelated call_length() should still use NoSingletonClass
        eval("call_length('profile')");
        let hir = hir_string("call_length");
        assert!(hir.contains("NoSingletonClass"), "{hir}");
    }

    #[test]
    fn test_invokesuper_to_iseq_optimizes() {
        eval("
            class A
              def foo
                'A'
              end
            end

            class B < A
              def foo
                super
              end
            end

            B.new.foo; B.new.foo
        ");

        // A Ruby method as the target of `super` should optimize provided no block is given.
        let hir = hir_string_proc("B.new.method(:foo)");
        assert!(!hir.contains("InvokeSuper "), "InvokeSuper should optimize to SendDirect but got:\n{hir}");

        assert_snapshot!(hir, @"
        fn foo@<compiled>:10:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:HeapBasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:HeapBasicObject):
          PatchPoint MethodRedefined(A@0x1000, foo@0x1008, cme:0x1010)
          v17:CPtr = GetEP 0
          v18:RubyValue = LoadField v17, :VM_ENV_DATA_INDEX_ME_CREF@0x1038
          v19:CallableMethodEntry[VALUE(0x1040)] = GuardBitEquals v18, Value(VALUE(0x1040))
          v20:RubyValue = LoadField v17, :VM_ENV_DATA_INDEX_SPECVAL@0x1048
          v21:FalseClass = GuardBitEquals v20, Value(false)
          PushInlineFrame v6 (0x1050)
          v27:StringExact[VALUE(0x1078)] = Const Value(VALUE(0x1078))
          v28:StringExact = StringCopy v27
          CheckInterrupts
          PopInlineFrame
          Return v28
        ");
    }

    #[test]
    fn test_invokesuper_from_a_block() {
        _ = eval("
            define_method(:itself) { super() }
            itself
        ");

        assert_snapshot!(hir_string("itself"), @"
        fn block in <compiled>@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:BasicObject = InvokeSuper v6, 0x1000 # SendFallbackReason: super: call from within a block
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_invokesuper_with_positional_args_optimizes() {
        eval("
            class A
              def foo(x)
                x * 2
              end
            end

            class B < A
              def foo(x)
                super(x) + 1
              end
            end

            B.new.foo(5); B.new.foo(5)
        ");

        let hir = hir_string_proc("B.new.method(:foo)");
        assert!(!hir.contains("InvokeSuper "), "InvokeSuper should optimize to SendDirect but got:\n{hir}");

        assert_snapshot!(hir, @"
        fn foo@<compiled>:10:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:HeapBasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:HeapBasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(A@0x1008, foo@0x1010, cme:0x1018)
          v27:CPtr = GetEP 0
          v28:RubyValue = LoadField v27, :VM_ENV_DATA_INDEX_ME_CREF@0x1040
          v29:CallableMethodEntry[VALUE(0x1048)] = GuardBitEquals v28, Value(VALUE(0x1048))
          v30:RubyValue = LoadField v27, :VM_ENV_DATA_INDEX_SPECVAL@0x1050
          v31:FalseClass = GuardBitEquals v30, Value(false)
          PushInlineFrame v9 (0x1058), v10
          v43:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1080, *@0x1088, cme:0x1090)
          v57:Fixnum = GuardType v10, Fixnum recompile
          v58:Fixnum = FixnumMult v57, v43
          CheckInterrupts
          PopInlineFrame
          v17:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1080, +@0x10b8, cme:0x10c0)
          v36:Fixnum = FixnumAdd v58, v17
          Return v36
        ");
    }

    #[test]
    fn test_invokesuper_with_forwarded_splat_args_remains_invokesuper() {
        eval("
            class A
              def foo(x)
                x * 2
              end
            end

            class B < A
              def foo(*x)
                super
              end
            end

            B.new.foo(5); B.new.foo(5)
        ");

        let hir = hir_string_proc("B.new.method(:foo)");
        assert!(hir.contains("InvokeSuper "), "Expected unoptimized InvokeSuper but got:\n{hir}");
        assert!(!hir.contains("SendDirect"), "Should not optimize to SendDirect for explicit blockarg:\n{hir}");

        assert_snapshot!(hir, @"
        fn foo@<compiled>:10:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:ArrayExact = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:HeapBasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:HeapBasicObject, v10:BasicObject):
          v15:ArrayExact = ToArray v10
          v17:BasicObject = InvokeSuper v9, 0x1008, v15 # SendFallbackReason: super: complex argument passing to `super` call
          CheckInterrupts
          Return v17
        ");
    }

    #[test]
    fn test_invokesuper_with_block_literal_remains_invokesuper() {
        eval("
            class A
              def foo
                block_given? ? yield : 'no block'
              end
            end

            class B < A
              def foo
                super { 'from subclass' }
              end
            end

            B.new.foo; B.new.foo
        ");

        let hir = hir_string_proc("B.new.method(:foo)");
        assert!(hir.contains("InvokeSuper "), "Expected unoptimized InvokeSuper but got:\n{hir}");
        assert!(!hir.contains("SendDirect"), "Should not optimize to SendDirect for block literal:\n{hir}");

        // With a block, we don't optimize to SendDirect
        assert_snapshot!(hir, @"
        fn foo@<compiled>:10:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:HeapBasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:HeapBasicObject):
          v10:BasicObject = InvokeSuper v6, 0x1000 # SendFallbackReason: super: call made with a block
          CheckInterrupts
          Return v10
        ");
    }

    #[test]
    fn test_invokesuper_to_cfunc_optimizes_to_ccall() {
        eval("
            class C < Hash
              def size
                super
              end
            end

            C.new.size
        ");

        let hir = hir_string_proc("C.new.method(:size)");
        assert!(!hir.contains("InvokeSuper "), "Expected unoptimized InvokeSuper but got:\n{hir}");

        assert_snapshot!(hir, @"
        fn size@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Hash@0x1000, size@0x1008, cme:0x1010)
          v17:CPtr = GetEP 0
          v18:RubyValue = LoadField v17, :VM_ENV_DATA_INDEX_ME_CREF@0x1038
          v19:CallableMethodEntry[VALUE(0x1040)] = GuardBitEquals v18, Value(VALUE(0x1040))
          v20:RubyValue = LoadField v17, :VM_ENV_DATA_INDEX_SPECVAL@0x1048
          v21:FalseClass = GuardBitEquals v20, Value(false)
          v22:Fixnum = CCall v6, :Hash#size@0x1049
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_invokesuper_to_nonleaf_cfunc_preserves_return_type() {
        // super resolving to a non-leaf cfunc (Array#reverse: leaf but allocates,
        // so it goes through CCallWithFrame) must keep the annotated return type
        // (ArrayExact) instead of widening it to BasicObject.
        eval("
            class MyArray < Array
              def reverse
                super
              end
            end

            MyArray.new.reverse; MyArray.new.reverse
        ");

        assert_snapshot!(hir_string_proc("MyArray.instance_method(:reverse)"), @"
        fn reverse@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Array@0x1000, reverse@0x1008, cme:0x1010)
          v17:CPtr = GetEP 0
          v18:RubyValue = LoadField v17, :VM_ENV_DATA_INDEX_ME_CREF@0x1038
          v19:CallableMethodEntry[VALUE(0x1040)] = GuardBitEquals v18, Value(VALUE(0x1040))
          v20:RubyValue = LoadField v17, :VM_ENV_DATA_INDEX_SPECVAL@0x1048
          v21:FalseClass = GuardBitEquals v20, Value(false)
          v22:ArrayExact = CCallWithFrame v6, :Array#reverse@0x1049
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_invokesuper_to_nonleaf_variadic_cfunc_preserves_return_type() {
        // super resolving to a non-leaf variadic cfunc (Array#join: StringExact)
        // must keep the annotated return type instead of widening to BasicObject.
        eval("
            class MyArray < Array
              def join(sep = nil)
                super
              end
            end

            MyArray.new([1, 2]).join(','); MyArray.new([1, 2]).join(',')
        ");

        assert_snapshot!(hir_string_proc("MyArray.instance_method(:join)"), @"
        fn join@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :sep@0x1000
          v4:CPtr = LoadPC
          v5:CPtr[CPtr(0x1001)] = Const CPtr(0x1001)
          v6:CBool = IsBitEqual v4, v5
          CondBranch v6, bb3(v1, v3), bb6()
        bb6():
          Jump bb5(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v10:BasicObject = LoadArg :self@0
          v11:NilClass = Const Value(nil)
          Jump bb3(v10, v11)
        bb3(v17:BasicObject, v18:BasicObject):
          v20:NilClass = Const Value(nil)
          Jump bb5(v17, v20)
        bb4():
          EntryPoint JIT(1)
          v14:BasicObject = LoadArg :self@0
          v15:BasicObject = LoadArg :sep@1
          Jump bb5(v14, v15)
        bb5(v23:BasicObject, v24:BasicObject):
          PatchPoint MethodRedefined(Array@0x1008, join@0x1010, cme:0x1018)
          v36:CPtr = GetEP 0
          v37:RubyValue = LoadField v36, :VM_ENV_DATA_INDEX_ME_CREF@0x1040
          v38:CallableMethodEntry[VALUE(0x1048)] = GuardBitEquals v37, Value(VALUE(0x1048))
          v39:RubyValue = LoadField v36, :VM_ENV_DATA_INDEX_SPECVAL@0x1050
          v40:FalseClass = GuardBitEquals v39, Value(false)
          v41:StringExact = CCallVariadic v23, :Array#join@0x1051, v24
          CheckInterrupts
          Return v41
        ");
    }

    #[test]
    fn test_invokesuper_to_nonleaf_cfunc_preserves_elidable() {
        // an elidable non-leaf cfunc reached via super (Array#reverse) whose
        // result is unused must be removed by DCE. If elidable were widened to false,
        // the dead CCallWithFrame would remain.
        eval("
            class MyArray < Array
              def reverse
                super
                self
              end
            end

            MyArray.new.reverse; MyArray.new.reverse
        ");

        assert_snapshot!(hir_string_proc("MyArray.instance_method(:reverse)"), @"
        fn reverse@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Array@0x1000, reverse@0x1008, cme:0x1010)
          v20:CPtr = GetEP 0
          v21:RubyValue = LoadField v20, :VM_ENV_DATA_INDEX_ME_CREF@0x1038
          v22:CallableMethodEntry[VALUE(0x1040)] = GuardBitEquals v21, Value(VALUE(0x1040))
          v23:RubyValue = LoadField v20, :VM_ENV_DATA_INDEX_SPECVAL@0x1048
          v24:FalseClass = GuardBitEquals v23, Value(false)
          CheckInterrupts
          Return v6
        ");
    }

    #[test]
    fn test_inline_invokesuper_to_basicobject_initialize() {
        eval("
            class C
              def initialize
                super
              end
            end

            C.new
        ");
        assert_snapshot!(hir_string_proc("C.instance_method(:initialize)"), @"
        fn initialize@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(BasicObject@0x1000, initialize@0x1008, cme:0x1010)
          v17:CPtr = GetEP 0
          v18:RubyValue = LoadField v17, :VM_ENV_DATA_INDEX_ME_CREF@0x1038
          v19:CallableMethodEntry[VALUE(0x1040)] = GuardBitEquals v18, Value(VALUE(0x1040))
          v20:RubyValue = LoadField v17, :VM_ENV_DATA_INDEX_SPECVAL@0x1048
          v21:FalseClass = GuardBitEquals v20, Value(false)
          v22:NilClass = Const Value(nil)
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_invokesuper_to_variadic_cfunc_optimizes_to_ccall() {
        eval("
            class MyString < String
              def byteindex(needle, offset = 0)
                super(needle, offset)
              end
            end

            MyString.new('hello world').byteindex('world', 0); MyString.new('hello world').byteindex('world', 0)
        ");

        let hir = hir_string_proc("MyString.new('hello world').method(:byteindex)");
        assert!(!hir.contains("InvokeSuper "), "InvokeSuper should optimize to CCallVariadic but got:\n{hir}");
        assert!(hir.contains("CCallVariadic"), "Should optimize to CCallVariadic for variadic cfunc:\n{hir}");

        assert_snapshot!(hir, @"
        fn byteindex@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :needle@0x1000
          v4:BasicObject = LoadField v2, :offset@0x1001
          v5:CPtr = LoadPC
          v6:CPtr[CPtr(0x1002)] = Const CPtr(0x1002)
          v7:CBool = IsBitEqual v5, v6
          CondBranch v7, bb3(v1, v3, v4), bb6()
        bb6():
          Jump bb5(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v11:BasicObject = LoadArg :self@0
          v12:BasicObject = LoadArg :needle@1
          v13:NilClass = Const Value(nil)
          Jump bb3(v11, v12, v13)
        bb3(v20:BasicObject, v21:BasicObject, v22:BasicObject):
          v24:Fixnum[0] = Const Value(0)
          Jump bb5(v20, v21, v24)
        bb4():
          EntryPoint JIT(1)
          v16:BasicObject = LoadArg :self@0
          v17:BasicObject = LoadArg :needle@1
          v18:BasicObject = LoadArg :offset@2
          Jump bb5(v16, v17, v18)
        bb5(v27:BasicObject, v28:BasicObject, v29:BasicObject):
          PatchPoint MethodRedefined(String@0x1008, byteindex@0x1010, cme:0x1018)
          v42:CPtr = GetEP 0
          v43:RubyValue = LoadField v42, :VM_ENV_DATA_INDEX_ME_CREF@0x1040
          v44:CallableMethodEntry[VALUE(0x1048)] = GuardBitEquals v43, Value(VALUE(0x1048))
          v45:RubyValue = LoadField v42, :VM_ENV_DATA_INDEX_SPECVAL@0x1050
          v46:FalseClass = GuardBitEquals v45, Value(false)
          v47:BasicObject = CCallVariadic v27, :String#byteindex@0x1051, v28, v29
          CheckInterrupts
          Return v47
        ");
    }

    #[test]
    fn test_invokesuper_with_blockarg_remains_invokesuper() {
        eval("
            class A
              def foo
                block_given? ? yield : 'no block'
              end
            end

            class B < A
              def foo(&blk)
                other_block = proc { 'different block' }
                super(&other_block)
              end
            end

            B.new.foo { 'passed' }; B.new.foo { 'passed' }
        ");

        let hir = hir_string_proc("B.new.method(:foo)");
        assert!(hir.contains("InvokeSuper "), "Expected unoptimized InvokeSuper but got:\n{hir}");
        assert!(!hir.contains("SendDirect"), "Should not optimize to SendDirect for explicit blockarg:\n{hir}");

        assert_snapshot!(hir, @"
        fn foo@<compiled>:10:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :blk@0x1000
          v4:NilClass = Const Value(nil)
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:HeapBasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :blk@1
          v9:CPtr = GetEP 0
          StoreField v9, :blk@0x1001, v8
          v11:NilClass = Const Value(nil)
          StoreField v9, :other_block@0x1002, v11
          Jump bb3(v7, v8, v11)
        bb3(v14:HeapBasicObject, v15:BasicObject, v16:NilClass):
          PatchPoint NoSingletonClass(B@0x1008)
          PatchPoint MethodRedefined(B@0x1008, proc@0x1010, cme:0x1018)
          v41:ObjectSubclass[class_exact:B] = GuardType v14, ObjectSubclass[class_exact:B] recompile
          v42:BasicObject = CCallWithFrame v41, :Kernel#proc@0x1040, block=0x1048
          v21:CPtr = GetEP 0
          v22:BasicObject = LoadField v21, :blk@0x1001
          v23:BasicObject = LoadField v21, :other_block@0x1002
          SetLocal :other_block, l0, EP@3, v42
          v29:CPtr = GetEP 0
          v30:BasicObject = LoadField v29, :other_block@0x1002
          v32:BasicObject = InvokeSuper v41, 0x1070, v30 # SendFallbackReason: super: complex argument passing to `super` call
          CheckInterrupts
          Return v32
        ");
    }

    #[test]
    fn test_invokesuper_with_symbol_to_proc_remains_invokesuper() {
        eval("
            class A
              def foo(items, &blk)
                items.map(&blk)
              end
            end

            class B < A
              def foo(items)
                super(items, &:succ)
              end
            end

            B.new.foo([1, 2, 3]); B.new.foo([1, 2, 3])
        ");

        let hir = hir_string_proc("B.new.method(:foo)");
        assert!(hir.contains("InvokeSuper "), "Expected unoptimized InvokeSuper but got:\n{hir}");
        assert!(!hir.contains("SendDirect"), "Should not optimize to SendDirect for symbol-to-proc:\n{hir}");

        assert_snapshot!(hir, @"
        fn foo@<compiled>:10:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :items@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:HeapBasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :items@1
          Jump bb3(v6, v7)
        bb3(v9:HeapBasicObject, v10:BasicObject):
          v15:StaticSymbol[:succ] = Const Value(VALUE(0x1008))
          v17:BasicObject = InvokeSuper v9, 0x1010, v10, v15 # SendFallbackReason: super: complex argument passing to `super` call
          CheckInterrupts
          Return v17
        ");
    }

    #[test]
    fn test_invokesuper_with_keyword_args_remains_invokesuper() {
        eval("
          class A
            def foo(attributes = {})
              @attributes = attributes
            end
          end

          class B < A
            def foo(content = '')
              super(content: content)
            end
          end

          B.new.foo('image data'); B.new.foo('image data')
        ");

        let hir = hir_string_proc("B.new.method(:foo)");
        assert!(hir.contains("InvokeSuper "), "Expected unoptimized InvokeSuper but got:\n{hir}");

        assert_snapshot!(hir, @"
        fn foo@<compiled>:9:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :content@0x1000
          v4:CPtr = LoadPC
          v5:CPtr[CPtr(0x1001)] = Const CPtr(0x1001)
          v6:CBool = IsBitEqual v4, v5
          CondBranch v6, bb3(v1, v3), bb6()
        bb6():
          Jump bb5(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v10:HeapBasicObject = LoadArg :self@0
          v11:NilClass = Const Value(nil)
          Jump bb3(v10, v11)
        bb3(v17:HeapBasicObject, v18:BasicObject):
          v20:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v21:StringExact = StringCopy v20
          Jump bb5(v17, v21)
        bb4():
          EntryPoint JIT(1)
          v14:HeapBasicObject = LoadArg :self@0
          v15:BasicObject = LoadArg :content@1
          Jump bb5(v14, v15)
        bb5(v24:HeapBasicObject, v25:BasicObject):
          v30:BasicObject = InvokeSuper v24, 0x1010, v25 # SendFallbackReason: super: complex argument passing to `super` call
          CheckInterrupts
          Return v30
        ");
    }

    #[test]
    fn test_infer_truthiness_from_branch() {
        eval("
        def test(x)
          if x
            if x
              if x
                3
              else
                4
              end
            else
              5
            end
          else
            6
          end
        end
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:CBool = Test v10
          v15:Falsy = RefineType v10, Falsy
          CondBranch v14, bb7(), bb6(v9, v15)
        bb7():
          v17:Truthy = RefineType v10, Truthy
          v34:Fixnum[3] = Const Value(3)
          CheckInterrupts
          Return v34
        bb6(v39:BasicObject, v40:Falsy):
          v43:Fixnum[6] = Const Value(6)
          CheckInterrupts
          Return v43
        ");
    }

    #[test]
    fn specialize_polymorphic_send_iseq() {
        set_call_threshold(4);
        eval("
        class C
          def foo = 3
        end

        class D
          def foo = 4
        end

        def test o
          o.foo + 2
        end

        test C.new; test D.new; test C.new; test D.new
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:11:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:CBool = HasType v10, ObjectSubclass[class_exact:C]
          CondBranch v15, bb5(), bb6()
        bb5():
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo@0x1010, cme:0x1018)
          v41:Fixnum[3] = Const Value(3)
          Jump bb4(v41)
        bb6():
          v21:CBool = HasType v10, ObjectSubclass[class_exact:D]
          CondBranch v21, bb7(), bb8()
        bb7():
          PatchPoint NoSingletonClass(D@0x1040)
          PatchPoint MethodRedefined(D@0x1040, foo@0x1010, cme:0x1048)
          v44:Fixnum[4] = Const Value(4)
          Jump bb4(v44)
        bb8():
          v27:BasicObject = Send v10, :foo # SendFallbackReason: Send: polymorphic call site
          Jump bb4(v27)
        bb4(v14:BasicObject):
          v30:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1070, +@0x1078, cme:0x1080)
          v47:Fixnum = GuardType v14, Fixnum recompile
          v48:Fixnum = FixnumAdd v47, v30
          CheckInterrupts
          Return v48
        ");
    }

    #[test]
    fn specialize_polymorphic_send_with_immediate() {
        set_call_threshold(4);
        eval("
        class C; end

        def test o
          o.itself
        end

        test C.new; test 3; test C.new; test 4
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:CBool = HasType v10, ObjectSubclass[class_exact:C]
          CondBranch v15, bb5(), bb6()
        bb5():
          v18:ObjectSubclass[class_exact:C] = RefineType v10, ObjectSubclass[class_exact:C]
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, itself@0x1010, cme:0x1018)
          Jump bb4(v18)
        bb6():
          v21:CBool = HasType v10, Fixnum
          CondBranch v21, bb7(), bb8()
        bb7():
          v24:Fixnum = RefineType v10, Fixnum
          PatchPoint MethodRedefined(Integer@0x1040, itself@0x1010, cme:0x1018)
          Jump bb4(v24)
        bb8():
          v27:BasicObject = Send v10, :itself # SendFallbackReason: Send: polymorphic call site
          Jump bb4(v27)
        bb4(v14:BasicObject):
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn specialize_polymorphic_send_fixnum_and_bignum() {
        // Fixnum and Bignum both have class Integer, but they should be
        // treated as different types for polymorphic dispatch because
        // Fixnum is an immediate and Bignum is a heap object.
        set_call_threshold(4);
        eval("
        def test x
          x.to_s
        end

        fixnum = 1
        bignum = 10**100
        test(fixnum)
        test(bignum)
        test(fixnum)
        test(bignum)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:CBool = HasType v10, Fixnum
          CondBranch v15, bb5(), bb6()
        bb5():
          v18:Fixnum = RefineType v10, Fixnum
          PatchPoint MethodRedefined(Integer@0x1008, to_s@0x1010, cme:0x1018)
          v36:StringExact = CCallVariadic v18, :Integer#to_s@0x1040
          Jump bb4(v36)
        bb6():
          v21:CBool = HasType v10, Bignum
          CondBranch v21, bb7(), bb8()
        bb7():
          v24:Bignum = RefineType v10, Bignum
          PatchPoint MethodRedefined(Integer@0x1008, to_s@0x1010, cme:0x1018)
          v39:StringExact = CCallVariadic v24, :Integer#to_s@0x1040
          Jump bb4(v39)
        bb8():
          v27:BasicObject = Send v10, :to_s # SendFallbackReason: Send: polymorphic call site
          Jump bb4(v27)
        bb4(v14:BasicObject):
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn specialize_polymorphic_send_flonum_and_heap_float() {
        set_call_threshold(4);
        eval("
        def test x
          x.to_s
        end

        flonum = 1.5
        heap_float = 1.7976931348623157e+308
        test(flonum)
        test(heap_float)
        test(flonum)
        test(heap_float)
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:CBool = HasType v10, Flonum
          CondBranch v15, bb5(), bb6()
        bb5():
          v18:Flonum = RefineType v10, Flonum
          PatchPoint MethodRedefined(Float@0x1008, to_s@0x1010, cme:0x1018)
          v36:BasicObject = CCallWithFrame v18, :Float#to_s@0x1040
          Jump bb4(v36)
        bb6():
          v21:CBool = HasType v10, HeapFloat
          CondBranch v21, bb7(), bb8()
        bb7():
          v24:HeapFloat = RefineType v10, HeapFloat
          PatchPoint MethodRedefined(Float@0x1008, to_s@0x1010, cme:0x1018)
          v39:BasicObject = CCallWithFrame v24, :Float#to_s@0x1040
          Jump bb4(v39)
        bb8():
          v27:BasicObject = Send v10, :to_s # SendFallbackReason: Send: polymorphic call site
          Jump bb4(v27)
        bb4(v14:BasicObject):
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn specialize_polymorphic_send_static_and_dynamic_symbol() {
        set_call_threshold(4);
        eval("
        def test x
          x.to_s
        end

        static_sym = :foo
        dynamic_sym = (\"zjit_dynamic_\" + Object.new.object_id.to_s).to_sym
        test static_sym
        test dynamic_sym
        test static_sym
        test dynamic_sym
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:CBool = HasType v10, StaticSymbol
          CondBranch v15, bb5(), bb6()
        bb5():
          v18:StaticSymbol = RefineType v10, StaticSymbol
          PatchPoint MethodRedefined(Symbol@0x1008, to_s@0x1010, cme:0x1018)
          v35:StringExact = InvokeBuiltin leaf <inline_expr>, v18
          Jump bb4(v35)
        bb6():
          v21:CBool = HasType v10, DynamicSymbol
          CondBranch v21, bb7(), bb8()
        bb7():
          v24:DynamicSymbol = RefineType v10, DynamicSymbol
          PatchPoint MethodRedefined(Symbol@0x1008, to_s@0x1010, cme:0x1018)
          v37:StringExact = InvokeBuiltin leaf <inline_expr>, v24
          Jump bb4(v37)
        bb8():
          v27:BasicObject = Send v10, :to_s # SendFallbackReason: Send: polymorphic call site
          Jump bb4(v27)
        bb4(v14:BasicObject):
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn specialize_polymorphic_send_iseq_duplicate_class_profiles() {
        set_call_threshold(4);
        eval("
        class C
          def foo = 3
        end

        O1 = C.new
        O1.instance_variable_set(:@foo, 1)
        O2 = C.new
        O2.instance_variable_set(:@bar, 2)

        def test o
          o.foo
        end

        test O1; test O2; test O1; test O2
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:12:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :o@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :o@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:CBool = HasType v10, ObjectSubclass[class_exact:C]
          CondBranch v15, bb5(), bb6()
        bb5():
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, foo@0x1010, cme:0x1018)
          v30:Fixnum[3] = Const Value(3)
          Jump bb4(v30)
        bb6():
          v21:BasicObject = Send v10, :foo # SendFallbackReason: Send: polymorphic call site
          Jump bb4(v21)
        bb4(v14:BasicObject):
          CheckInterrupts
          Return v14
        ");
    }

    #[test]
    fn upgrade_self_type_to_heap_after_setivar() {
        // Snapshot the overflow path only when this build naturally keeps five
        // ivars embedded and overflows on the next write.
        let obj = eval(r#"
            klass = Class.new do
              def initialize
                @v0 = 0
                @v1 = 1
                @v2 = 2
                @v3 = 3
                @v4 = 4
              end

              def test
                @overflow = 1
                @after = 2
              end
            end

            TEST = klass.instance_method(:test)
            OBJ = klass.new
            OBJ
        "#);
        // Skip builds where five ivars already force heap-backed storage.
        if obj.layout() != ShapeLayout::RObject {
            return;
        }

        // Make sure the next write is the one that overflows into heap-backed
        // storage, so this snapshot still exercises the self-type upgrade path.
        let probe = eval(r#"
            probe = OBJ.class.new
            probe.instance_variable_set(:@overflow, 1)
            probe
        "#);
        if probe.layout() == ShapeLayout::RObject {
            return;
        }
        eval("OBJ.test");
        assert_snapshot!(hir_string_proc("TEST"), @"
        fn test@<compiled>:12:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[1] = Const Value(1)
          PatchPoint SingleRactorMode
          SetIvar v6, :@overflow, v10
          v14:HeapBasicObject = RefineType v6, HeapBasicObject
          v17:Fixnum[2] = Const Value(2)
          PatchPoint SingleRactorMode
          v29:CShape = LoadField v14, :shape_id@0x1000
          v30:CShape[0x1001] = GuardBitEquals v29, CShape(0x1001)
          v31:CPtr = LoadField v14, :as_heap@0x1002
          StoreField v31, :@after@0x1003, v17
          WriteBarrier v14, v17
          v34:CShape[0x1004] = Const CShape(0x1004)
          StoreField v14, :shape_id@0x1000, v34
          CheckInterrupts
          Return v17
        ");
    }

    #[test]
    fn recompile_after_ep_escape_uses_ep_locals() {
        // When a method creates a lambda, EP escapes to the heap. After
        // invalidation and recompilation, the compiler must use EP-based
        // locals (SetLocal/GetLocal) instead of SSA locals, because the
        // spill target (stack) and the read target (heap EP) diverge.
        eval("
            CONST = {}.freeze
            def test_ep_escape(list, sep=nil, iter_method=:each)
                sep ||= lambda { }
                kwsplat = CONST
                list.__send__(iter_method) {|*v| yield(*v) }
            end

            test_ep_escape({a: 1}, nil, :each_pair) { |k, v|
                test_ep_escape([1], lambda { }) { |x| }
            }
            test_ep_escape({a: 1}, nil, :each_pair) { |k, v|
                test_ep_escape([1], lambda { }) { |x| }
            }
        ");
        assert_snapshot!(hir_string("test_ep_escape"), @"
        fn test_ep_escape@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :list@0x1000
          v4:BasicObject = LoadField v2, :sep@0x1001
          v5:BasicObject = LoadField v2, :iter_method@0x1002
          v6:NilClass = Const Value(nil)
          v7:CPtr = LoadPC
          v8:CPtr[CPtr(0x1003)] = Const CPtr(0x1003)
          v9:CBool = IsBitEqual v7, v8
          CondBranch v9, bb3(v1, v3, v4, v5, v6), bb9()
        bb9():
          v11:CPtr[CPtr(0x1004)] = Const CPtr(0x1004)
          v12:CBool = IsBitEqual v7, v11
          CondBranch v12, bb5(v1, v3, v4, v5, v6), bb10()
        bb10():
          Jump bb7(v1, v3, v4, v5, v6)
        bb2():
          EntryPoint JIT(0)
          v16:BasicObject = LoadArg :self@0
          v17:BasicObject = LoadArg :list@1
          v18:CPtr = GetEP 0
          StoreField v18, :list@0x1001, v17
          v20:NilClass = Const Value(nil)
          StoreField v18, :sep@0x1002, v20
          v22:NilClass = Const Value(nil)
          StoreField v18, :iter_method@0x1005, v22
          v24:NilClass = Const Value(nil)
          StoreField v18, :kwsplat@0x1006, v24
          Jump bb3(v16, v17, v20, v22, v24)
        bb3(v51:BasicObject, v52:BasicObject, v53:BasicObject, v54:BasicObject, v55:NilClass):
          v57:NilClass = Const Value(nil)
          SetLocal :sep, l0, EP@5, v57
          Jump bb5(v51, v52, v57, v54, v55)
        bb4():
          EntryPoint JIT(1)
          v28:BasicObject = LoadArg :self@0
          v29:BasicObject = LoadArg :list@1
          v30:CPtr = GetEP 0
          StoreField v30, :list@0x1001, v29
          v32:BasicObject = LoadArg :sep@2
          StoreField v30, :sep@0x1002, v32
          v34:NilClass = Const Value(nil)
          StoreField v30, :iter_method@0x1005, v34
          v36:NilClass = Const Value(nil)
          StoreField v30, :kwsplat@0x1006, v36
          Jump bb5(v28, v29, v32, v34, v36)
        bb5(v61:BasicObject, v62:BasicObject, v63:BasicObject, v64:BasicObject, v65:NilClass):
          v67:StaticSymbol[:each] = Const Value(VALUE(0x1008))
          SetLocal :iter_method, l0, EP@4, v67
          Jump bb7(v61, v62, v63, v67, v65)
        bb6():
          EntryPoint JIT(2)
          v40:BasicObject = LoadArg :self@0
          v41:BasicObject = LoadArg :list@1
          v42:CPtr = GetEP 0
          StoreField v42, :list@0x1001, v41
          v44:BasicObject = LoadArg :sep@2
          StoreField v42, :sep@0x1002, v44
          v46:BasicObject = LoadArg :iter_method@3
          StoreField v42, :iter_method@0x1005, v46
          v48:NilClass = Const Value(nil)
          StoreField v42, :kwsplat@0x1006, v48
          Jump bb7(v40, v41, v44, v46, v48)
        bb7(v71:BasicObject, v72:BasicObject, v73:BasicObject, v74:BasicObject, v75:NilClass):
          v79:CBool = Test v73
          v80:Truthy = RefineType v73, Truthy
          CondBranch v79, bb8(v71, v72, v80, v74, v75), bb11()
        bb11():
          v82:Falsy = RefineType v73, Falsy
          PatchPoint MethodRedefined(Object@0x1010, lambda@0x1018, cme:0x1020)
          v127:ObjectSubclass[class_exact*:Object@VALUE(0x1010)] = GuardType v71, ObjectSubclass[class_exact*:Object@VALUE(0x1010)] recompile
          v128:BasicObject = CCallWithFrame v127, :Kernel#lambda@0x1048, block=0x1050
          v86:CPtr = GetEP 0
          v87:BasicObject = LoadField v86, :list@0x1001
          v88:BasicObject = LoadField v86, :sep@0x1002
          v89:BasicObject = LoadField v86, :iter_method@0x1005
          v90:BasicObject = LoadField v86, :kwsplat@0x1006
          SetLocal :sep, l0, EP@5, v128
          Jump bb8(v127, v87, v128, v89, v90)
        bb8(v94:BasicObject, v95:BasicObject, v96:BasicObject, v97:BasicObject, v98:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1078, CONST)
          v103:HashExact[VALUE(0x1080)] = Const Value(VALUE(0x1080))
          SetLocal :kwsplat, l0, EP@3, v103
          v108:CPtr = GetEP 0
          v109:BasicObject = LoadField v108, :list@0x1001
          v111:CPtr = GetEP 0
          v112:BasicObject = LoadField v111, :iter_method@0x1005
          v114:BasicObject = Send v109, 0x1088, :__send__, v112 # SendFallbackReason: Send: unsupported method type Optimized
          v115:CPtr = GetEP 0
          v116:BasicObject = LoadField v115, :list@0x1001
          v117:BasicObject = LoadField v115, :sep@0x1002
          v118:BasicObject = LoadField v115, :iter_method@0x1005
          v119:BasicObject = LoadField v115, :kwsplat@0x1006
          CheckInterrupts
          Return v114
        ");
    }

    #[test]
    fn test_array_each() {
        eval("[1, 2, 3].each { |x| x }");
        assert_snapshot!(hir_string_proc("Array.instance_method(:each)"), @"
        fn each@<internal:array>:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:NilClass = Const Value(nil)
          v14:TrueClass|NilClass = Defined yield, v12
          v16:CBool = Test v14
          CondBranch v16, bb9(), bb4(v8, v9)
        bb9():
          v32:Fixnum[0] = Const Value(0)
          Jump bb8(v8, v32)
        bb8(v44:BasicObject, v45:Fixnum):
          v47:Array = RefineType v44, Array
          v48:CInt64 = ArrayLength v47
          v49:Fixnum = BoxFixnum v48
          v50:BoolExact = FixnumGe v45, v49
          v52:CBool = Test v50
          CondBranch v52, bb11(), bb7(v44, v45)
        bb11():
          CheckInterrupts
          Return v44
        bb7(v65:BasicObject, v66:Fixnum):
          v69:Array = RefineType v65, Array
          v70:CInt64 = UnboxFixnum v66
          v71:BasicObject = ArrayAref v69, v70
          v73:CPtr = GetEP 0
          v74:CInt64 = LoadField v73, :VM_ENV_DATA_INDEX_SPECVAL@0x1000
          v75:CInt64[3] = Const CInt64(3)
          v76:CInt64 = IntAnd v74, v75
          v77:CInt64[1] = GuardBitEquals v76, CInt64(1) recompile
          v78:CInt64[-4] = Const CInt64(-4)
          v79:CInt64 = IntAnd v74, v78
          v80:CPtr = LoadField v79, :code_iseq@0x1001
          v81:CPtr[CPtr(0x1002)] = GuardBitEquals v80, CPtr(0x1002) recompile
          v82:BasicObject = InvokeBlockIseqDirect (0x1002), v79, v71
          v86:Fixnum[1] = Const Value(1)
          v87:Fixnum = FixnumAdd v66, v86
          PatchPoint NoEPEscape(each)
          Jump bb8(v65, v87)
        bb4(v22:BasicObject, v23:NilClass):
          v26:BasicObject = InvokeBuiltin <inline_expr>, v22
          CheckInterrupts
          Return v26
        ");
    }

    #[test]
    fn test_inline_with_block_folds_defined_yield() {
        eval(r"
            def foo = defined?(yield)
            def test = foo { |x| x }
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v17 (0x1038)
          v25:StringExact[VALUE(0x1060)] = Const Value(VALUE(0x1060))
          CheckInterrupts
          PopInlineFrame
          Return v25
        ");
    }

    #[test]
    fn test_inline_without_block_folds_defined_yield() {
        eval(r"
            def foo = defined?(yield)
            def test = foo
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, foo@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v17 (0x1038)
          v25:NilClass = Const Value(nil)
          CheckInterrupts
          PopInlineFrame
          Return v25
        ");
    }

    #[test]
    fn test_inline_array_each_with_block_folds_defined_yield() {
        set_inline_threshold(100);
        eval(r"
            def test = [1, 2, 3].each { |x| x }
            test
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:ArrayExact[VALUE(0x1000)] = Const Value(VALUE(0x1000))
          v10:ArrayExact = ArrayDup v9
          PatchPoint NoSingletonClass(Array@0x1008)
          PatchPoint MethodRedefined(Array@0x1008, each@0x1010, cme:0x1018)
          PushInlineFrame v10 (0x1040)
          v47:Fixnum[0] = Const Value(0)
          Jump bb10(v10, v47)
        bb10(v59:ArrayExact, v60:Fixnum):
          v63:CInt64 = ArrayLength v59
          v64:Fixnum = BoxFixnum v63
          v65:BoolExact = FixnumGe v60, v64
          v67:CBool = Test v65
          CondBranch v67, bb13(), bb9(v59, v60)
        bb13():
          CheckInterrupts
          PopInlineFrame
          Return v59
        bb9(v80:ArrayExact, v81:Fixnum):
          v85:CInt64 = UnboxFixnum v81
          v86:BasicObject = ArrayAref v80, v85
          v88:CPtr = GetEP 0
          v89:CInt64 = LoadField v88, :VM_ENV_DATA_INDEX_SPECVAL@0x1068
          v90:CInt64[-4] = Const CInt64(-4)
          v91:CInt64 = IntAnd v89, v90
          v92:BasicObject = InvokeBlockIseqDirect (0x1070), v91, v86
          v96:Fixnum[1] = Const Value(1)
          v97:Fixnum = FixnumAdd v81, v96
          PatchPoint NoEPEscape(each)
          Jump bb10(v80, v97)
        ");
    }

    #[test]
    fn test_delete_duplicate_store() {
        eval("
            class C
              def initialize
                a = 1
                @a = a
                @a = a
              end
            end

            C.new
        ");
        assert_snapshot!(hir_string_proc("C.instance_method(:initialize)"), @"
        fn initialize@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:Fixnum[1] = Const Value(1)
          PatchPoint SingleRactorMode
          v18:HeapBasicObject = GuardType v8, HeapBasicObject
          v19:CShape = LoadField v18, :shape_id@0x1000
          v20:CShape[0x1001] = GuardBitEquals v19, CShape(0x1001) recompile
          StoreField v18, :@a@0x1002, v12
          WriteBarrier v18, v12
          v23:CShape[0x1003] = Const CShape(0x1003)
          StoreField v18, :shape_id@0x1000, v23
          PatchPoint NoEPEscape(initialize)
          PatchPoint SingleRactorMode
          WriteBarrier v18, v12
          CheckInterrupts
          Return v12
        ");
    }

    #[test]
    fn test_remove_duplicate_store_with_non_effectful_insns_between() {
        eval("
            class C
              def initialize
                a = 1
                @a = a
                b = 5
                b += a
                @a = a
              end
            end

            C.new
        ");
        assert_snapshot!(hir_string_proc("C.instance_method(:initialize)"), @"
        fn initialize@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          v3:NilClass = Const Value(nil)
          Jump bb3(v1, v2, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:NilClass = Const Value(nil)
          v8:NilClass = Const Value(nil)
          Jump bb3(v6, v7, v8)
        bb3(v10:BasicObject, v11:NilClass, v12:NilClass):
          v15:Fixnum[1] = Const Value(1)
          PatchPoint SingleRactorMode
          v21:HeapBasicObject = GuardType v10, HeapBasicObject
          v22:CShape = LoadField v21, :shape_id@0x1000
          v23:CShape[0x1001] = GuardBitEquals v22, CShape(0x1001) recompile
          StoreField v21, :@a@0x1002, v15
          WriteBarrier v21, v15
          v26:CShape[0x1003] = Const CShape(0x1003)
          StoreField v21, :shape_id@0x1000, v26
          v31:Fixnum[5] = Const Value(5)
          PatchPoint NoEPEscape(initialize)
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v62:Fixnum[6] = Const Value(6)
          PatchPoint SingleRactorMode
          WriteBarrier v21, v15
          CheckInterrupts
          Return v15
        ");
    }

    #[test]
    fn test_remove_two_stores() {
        eval("
            class C
              def initialize
                a = 1
                @a = a
                @a = a
                @a = a
              end
            end

            C.new
        ");
        assert_snapshot!(hir_string_proc("C.instance_method(:initialize)"), @"
        fn initialize@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:BasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:BasicObject, v9:NilClass):
          v12:Fixnum[1] = Const Value(1)
          PatchPoint SingleRactorMode
          v18:HeapBasicObject = GuardType v8, HeapBasicObject
          v19:CShape = LoadField v18, :shape_id@0x1000
          v20:CShape[0x1001] = GuardBitEquals v19, CShape(0x1001) recompile
          StoreField v18, :@a@0x1002, v12
          WriteBarrier v18, v12
          v23:CShape[0x1003] = Const CShape(0x1003)
          StoreField v18, :shape_id@0x1000, v23
          PatchPoint NoEPEscape(initialize)
          PatchPoint SingleRactorMode
          WriteBarrier v18, v12
          WriteBarrier v18, v12
          CheckInterrupts
          Return v12
        ");
    }

    #[test]
    fn test_exit_from_function_stub_for_opt_keyword_callee() {
        // We have a SendDirect to a callee that fails to compile,
        // so the function stub has to take care of exiting to
        // interpreter.
        eval("
            def target(a = binding.local_variable_get(:a), b: nil)
              ::RubyVM::ZJIT.induce_compile_failure!
              [a, b]
            end

            def entry = target(b: -1)

            raise 'wrong' unless [nil, -1] == entry
            raise 'wrong' unless [nil, -1] == entry
        ");

        crate::hir::tests::hir_build_tests::assert_compile_fails("target", ParseError::DirectiveInduced);
        let hir = hir_string("entry");
        assert!(hir.contains("SendDirect"), "{hir}");
    }

    #[test]
    fn test_exit_from_function_stub_for_lead_opt() {
        // We have a SendDirect to a callee that fails to compile,
        // so the function stub has to take care of exiting to
        // interpreter.
        let result = eval("
            def target(_required, a = a, b = b)
              ::RubyVM::ZJIT.induce_compile_failure!
              a
            end

            def entry = target(1)

            entry
            entry
        ");
        assert_eq!(Qnil, result);

        crate::hir::tests::hir_build_tests::assert_compile_fails("target", ParseError::DirectiveInduced);
        let hir = hir_string("entry");
        assert!(hir.contains("SendDirect"), "{hir}");
    }

    #[test]
    fn test_recompile_no_profile_send() {
        // Test the SideExit -> recompile flow: a no-profile send becomes a SideExit,
        // the exit profiles the send, triggers recompilation, and the new version
        // optimizes it to SendDirect.
        eval("
            def greet_recompile(x) = x.to_s
            def test_no_profile_recompile(flag)
              if flag
                greet_recompile(42)
              else
                'hello'
              end
            end
        ");

        // With call_threshold=2, num_profiles=1:
        //   1st call profiles (flag=false, so greet is never reached)
        //   2nd call compiles (greet has no profile data -> SideExit recompile)
        eval("test_no_profile_recompile(false); test_no_profile_recompile(false)");

        // Now call with flag=true. This hits the SideExit, which profiles
        // the send and invalidates the ISEQ for recompilation.
        eval("test_no_profile_recompile(true)");

        // After profiling via the side exit, rebuilding HIR should now
        // have a SendDirect for greet_recompile instead of SideExit.
        assert_snapshot!(hir_string("test_no_profile_recompile"), @"
        fn test_no_profile_recompile@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :flag@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :flag@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:CBool = Test v10
          v15:Falsy = RefineType v10, Falsy
          CondBranch v14, bb5(), bb4(v9, v15)
        bb5():
          v17:Truthy = RefineType v10, Truthy
          v21:Fixnum[42] = Const Value(42)
          PatchPoint MethodRedefined(Object@0x1008, greet_recompile@0x1010, cme:0x1018)
          v40:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v40 (0x1040), v21
          PatchPoint MethodRedefined(Integer@0x1068, to_s@0x1070, cme:0x1078)
          v60:StringExact = CCallVariadic v21, :Integer#to_s@0x10a0
          CheckInterrupts
          PopInlineFrame
          Return v60
        bb4(v28:BasicObject, v29:Falsy):
          v32:StringExact[VALUE(0x10a8)] = Const Value(VALUE(0x10a8))
          v33:StringExact = StringCopy v32
          CheckInterrupts
          Return v33
        ");
    }

    #[test]
    fn test_recompile_no_profile_send_with_blockarg() {
        // Test that no-profile send recompilation profiles explicit blockargs.
        // The call remains a Send fallback because &block is still complex, but
        // it should no longer be a NoProfileSend side exit after recompilation.
        eval("
            def passthrough_recompile_blockarg(x, &block)
              block.call(x)
            end

            def test(flag, block)
              if flag
                passthrough_recompile_blockarg(42, &block)
              else
                'hello'
              end
            end
        ");

        // With call_threshold=2, num_profiles=1, the send is not profiled
        // during initial profiling because flag=false skips that branch.
        eval("
            block = proc { |x| x }
            test(false, block)
            test(false, block)
        ");

        // This hits the NoProfileSend side exit, profiles the send including
        // its explicit blockarg, and invalidates the ISEQ for recompilation.
        eval("
            block = proc { |x| x }
            test(true, block)
        ");

        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :flag@0x1000
          v4:BasicObject = LoadField v2, :block@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :flag@1
          v9:BasicObject = LoadArg :block@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          v17:CBool = Test v12
          v18:Falsy = RefineType v12, Falsy
          CondBranch v17, bb5(), bb4(v11, v18, v13)
        bb5():
          v20:Truthy = RefineType v12, Truthy
          v24:Fixnum[42] = Const Value(42)
          v27:BasicObject = Send v11, &block, :passthrough_recompile_blockarg, v24, v13 # SendFallbackReason: Send: block argument is not nil
          CheckInterrupts
          Return v27
        bb4(v32:BasicObject, v33:Falsy, v34:BasicObject):
          v37:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v38:StringExact = StringCopy v37
          CheckInterrupts
          Return v38
        ");
    }

    #[test]
    fn test_no_profile_send_on_final_version() {
        // On the final ISEQ version (MAX_ISEQ_VERSIONS reached), no-profile sends should
        // remain as Send fallbacks instead of being converted to SideExits, since recompilation
        // is no longer possible and SideExits would fire every time without benefit.
        //
        // Use call_threshold=3 to ensure the method is auto-compiled before hir_string() builds
        // the HIR. The auto-compile creates version 1, and hir_string() creates version 2
        // (= MAX_ISEQ_VERSIONS), so this is the final version.
        set_call_threshold(3);
        set_max_versions(2);
        set_inline_threshold(0);

        eval("
            def greet_final(x) = x.to_s
            def test_final_version(flag)
              if flag
                greet_final(42)
              else
                'hello'
              end
            end
        ");
        // Call enough times to trigger auto-compilation. flag=false so greet_final is never
        // reached and has no profile data.
        eval("3.times { test_final_version(false) }");

        // On the final version, greet_final should be a Send fallback, not a SideExit.
        assert_snapshot!(hir_string("test_final_version"), @"
        fn test_final_version@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :flag@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :flag@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:CBool = Test v10
          v15:Falsy = RefineType v10, Falsy
          CondBranch v14, bb5(), bb4(v9, v15)
        bb5():
          v17:Truthy = RefineType v10, Truthy
          v21:Fixnum[42] = Const Value(42)
          v23:BasicObject = Send v9, :greet_final, v21 # SendFallbackReason: Send: no profile data available
          CheckInterrupts
          Return v23
        bb4(v28:BasicObject, v29:Falsy):
          v32:StringExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          v33:StringExact = StringCopy v32
          CheckInterrupts
          Return v33
        ");
    }

    #[test]
    fn test_invokeblock_ifunc() {
        eval("
            class IFuncTestList
              include Enumerable
              def each
                yield 1
                yield 2
              end
            end
            IFuncTestList.new.map { |x| x }
        ");
        assert_snapshot!(hir_string_proc("IFuncTestList.instance_method(:each)"), @"
        fn each@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          v11:CPtr = GetEP 0
          v12:CInt64 = LoadField v11, :VM_ENV_DATA_INDEX_SPECVAL@0x1000
          v13:CInt64[3] = Const CInt64(3)
          v14:CInt64 = IntAnd v12, v13
          v15:CInt64[3] = Const CInt64(3)
          v16:CBool = IsBitEqual v14, v15
          CondBranch v16, bb5(), bb6()
        bb5():
          v19:BasicObject = InvokeBlockIfunc v12, v9
          Jump bb4(v19)
        bb6():
          v21:BasicObject = InvokeBlock v9 # SendFallbackReason: InvokeBlock: not yet specialized
          Jump bb4(v21)
        bb4(v17:BasicObject):
          v26:Fixnum[2] = Const Value(2)
          v28:CPtr = GetEP 0
          v29:CInt64 = LoadField v28, :VM_ENV_DATA_INDEX_SPECVAL@0x1000
          v30:CInt64[3] = Const CInt64(3)
          v31:CInt64 = IntAnd v29, v30
          v32:CInt64[3] = Const CInt64(3)
          v33:CBool = IsBitEqual v31, v32
          CondBranch v33, bb8(), bb9()
        bb8():
          v36:BasicObject = InvokeBlockIfunc v29, v26
          Jump bb7(v36)
        bb9():
          v38:BasicObject = InvokeBlock v26 # SendFallbackReason: InvokeBlock: not yet specialized
          Jump bb7(v38)
        bb7(v34:BasicObject):
          CheckInterrupts
          Return v34
        ");
    }

    #[test]
    fn test_invokeblock_ifunc_kwarg() {
        eval("
            def foo
              yield 1, a: 2
            end
            def test = enum_for(:foo).to_a
            test
        ");
        assert_snapshot!(hir_string("foo"), @"
        fn foo@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v9:Fixnum[1] = Const Value(1)
          v11:Fixnum[2] = Const Value(2)
          v13:BasicObject = InvokeBlock v9, v11 # SendFallbackReason: InvokeBlock: not yet specialized
          CheckInterrupts
          Return v13
        ");
    }

    #[test]
    fn test_dedup_guard_type() {
        // Two subtractions on the same Fixnum operand `n` each require a
        // GuardType n, Fixnum.  The second guard is redundant and should be
        // eliminated by fold_constants.
        eval("
            def test(n)
              (n - 1) + (n - 2)
            end
            test 1; test 2
        ");
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v14:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, -@0x1010, cme:0x1018)
          v34:Fixnum = GuardType v10, Fixnum recompile
          v35:Fixnum = FixnumSub v34, v14
          v20:Fixnum[2] = Const Value(2)
          v39:Fixnum = FixnumSub v34, v20
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1040, cme:0x1048)
          v43:Fixnum = FixnumAdd v35, v39
          CheckInterrupts
          Return v43
        ");
    }

    #[test]
    fn test_dedup_guard_type_across_cfg_join() {
        eval("
            def test(n, cond)
              if cond
                a = n + 1
              else
                a = n + 2
              end
              n + a
            end
            test(1, true); test(1, false)
        ");
        let hir = hir_string("test");
        let guard_count = hir.matches("GuardType").count();
        assert_eq!(
            guard_count, 2,
            "expected 2 GuardType instructions after cross-block dedup, found {guard_count}\n\nHIR:\n{hir}"
        );
    }

    #[test]
    fn test_forward_guard_through_conditional_branch() {
        eval("
            def test(n, a, b)
              if a
                if b
                  n + 1
                else
                  n + 2
                end
              else
                n + 3
              end
            end
            test(1, true, true); test(1, true, false); test(1, false, false)
        ");
        let hir = hir_string("test");
        let guard_count = hir.matches("GuardType").count();
        assert!(
            guard_count <= 3,
            "expected at most 3 GuardType instructions (one per leaf branch) after forwarding through conditional branches, found {guard_count}\n\nHIR:\n{hir}"
        );
    }

    #[test]
    fn test_no_forward_when_no_guard_in_branches() {
        let src = "
            def test(n, cond)
              a = if cond then 1 else 2 end
              n + a
            end
            test(1, true); test(1, false)
        ";
        eval(src);
        let hir = hir_string("test");
        let guard_count = hir.matches("GuardType").count();
        assert_eq!(
            guard_count, 1,
            "expected 1 GuardType (merge block only), found {guard_count}\n\nHIR:\n{hir}"
        );
    }

    #[test]
    fn test_infer_types_across_non_maximal_basic_blocks() {
        // Previous worklist-based type inference only worked for maximal SSA. This is a regression
        // test for hanging.
        eval("
            class TheClass
              def set_value_loop
                i = 0
                while i < 10
                  @levar ||= i
                  i += 1
                end
              end
            end
            3.times do |i|
              TheClass.new.set_value_loop
            end
        ");
        assert_snapshot!(hir_string_proc("TheClass.instance_method(:set_value_loop)"), @"
        fn set_value_loop@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          v2:NilClass = Const Value(nil)
          Jump bb3(v1, v2)
        bb2():
          EntryPoint JIT(0)
          v5:HeapBasicObject = LoadArg :self@0
          v6:NilClass = Const Value(nil)
          Jump bb3(v5, v6)
        bb3(v8:HeapBasicObject, v9:NilClass):
          v12:Fixnum[0] = Const Value(0)
          Jump bb6(v8, v12)
        bb6(v17:HeapBasicObject, v18:Fixnum):
          v21:Fixnum[10] = Const Value(10)
          PatchPoint MethodRedefined(Integer@0x1000, <@0x1008, cme:0x1010)
          v90:BoolExact = FixnumLt v18, v21
          CheckInterrupts
          v27:CBool = Test v90
          CondBranch v27, bb4(v17, v18), bb7()
        bb4(v37:HeapBasicObject, v38:Fixnum):
          PatchPoint SingleRactorMode
          v43:CShape = LoadField v37, :shape_id@0x1038
          v45:CShape[0x1039] = Const CShape(0x1039)
          v46:CBool = IsBitEqual v43, v45
          CondBranch v46, bb9(), bb10()
        bb9():
          v48:BasicObject = LoadField v37, :@levar@0x103a
          Jump bb8(v48)
        bb10():
          v50:CShape[0x103b] = GuardBitEquals v43, CShape(0x103b) recompile
          v52:NilClass = Const Value(nil)
          Jump bb8(v52)
        bb8(v44:BasicObject):
          v55:CBool = Test v44
          CondBranch v55, bb5(v37, v38), bb12()
        bb12():
          PatchPoint NoEPEscape(set_value_loop)
          PatchPoint SingleRactorMode
          v65:CShape = LoadField v37, :shape_id@0x1038
          v66:CShape[0x103b] = GuardBitEquals v65, CShape(0x103b) recompile
          StoreField v37, :@levar@0x103a, v38
          WriteBarrier v37, v38
          v69:CShape[0x1039] = Const CShape(0x1039)
          StoreField v37, :shape_id@0x1038, v69
          Jump bb5(v37, v38)
        bb5(v73:HeapBasicObject, v74:Fixnum):
          PatchPoint NoEPEscape(set_value_loop)
          v80:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1000, +@0x103c, cme:0x1040)
          v94:Fixnum = FixnumAdd v74, v80
          Jump bb6(v73, v94)
        bb7():
          v32:NilClass = Const Value(nil)
          CheckInterrupts
          Return v32
        ");
    }

    #[test]
    fn test_float_nan_p_annotation() {
        eval(r#"
            def test(x) = x.nan?
            test(1.0)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, nan?@0x1010, cme:0x1018)
          v22:Flonum = GuardType v10, Flonum recompile
          v23:BoolExact = CCall v22, :Float#nan?@0x1040
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_float_finite_p_annotation() {
        eval(r#"
            def test(x) = x.finite?
            test(1.0)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, finite?@0x1010, cme:0x1018)
          v22:Flonum = GuardType v10, Flonum recompile
          v23:BoolExact = CCall v22, :Float#finite?@0x1040
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_float_infinite_p_annotation() {
        eval(r#"
            def test(x) = x.infinite?
            test(1.0)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, infinite?@0x1010, cme:0x1018)
          v22:Flonum = GuardType v10, Flonum recompile
          v23:NilClass|Fixnum = CCall v22, :Float#infinite?@0x1040
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_integer_even_p_annotation() {
        eval(r#"
            def test(x) = x.even?
            test(2)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, even?@0x1010, cme:0x1018)
          v21:Fixnum = GuardType v10, Fixnum recompile
          v22:BoolExact = InvokeBuiltin leaf <inline_expr>, v21
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_integer_odd_p_annotation() {
        eval(r#"
            def test(x) = x.odd?
            test(3)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Integer@0x1008, odd?@0x1010, cme:0x1018)
          v21:Fixnum = GuardType v10, Fixnum recompile
          v22:BoolExact = InvokeBuiltin leaf <inline_expr>, v21
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_float_zero_p_annotation() {
        eval(r#"
            def test(x) = x.zero?
            test(1.0)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, zero?@0x1010, cme:0x1018)
          v21:Flonum = GuardType v10, Flonum recompile
          v22:BoolExact = InvokeBuiltin leaf <inline_expr>, v21
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_float_positive_p_annotation() {
        eval(r#"
            def test(x) = x.positive?
            test(1.0)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, positive?@0x1010, cme:0x1018)
          v21:Flonum = GuardType v10, Flonum recompile
          v22:BoolExact = InvokeBuiltin leaf <inline_expr>, v21
          CheckInterrupts
          Return v22
        ");
    }

    #[test]
    fn test_float_negative_p_annotation() {
        eval(r#"
            def test(x) = x.negative?
            test(-1.0)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :x@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, negative?@0x1010, cme:0x1018)
          v21:Flonum = GuardType v10, Flonum recompile
          v22:BoolExact = InvokeBuiltin leaf <inline_expr>, v21
          CheckInterrupts
          Return v22
        ");
    }
    #[test]
    fn test_float_add_inline() {
        eval(r#"
            def test(a, b) = a + b
            test(1.0, 2.0)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, +@0x1010, cme:0x1018)
          v27:Flonum = GuardType v12, Flonum recompile
          v28:Flonum = GuardType v13, Flonum recompile
          v29:Float = FloatAdd v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_float_mul_inline() {
        eval(r#"
            def test(a, b) = a * b
            test(1.5, 2.5)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, *@0x1010, cme:0x1018)
          v27:Flonum = GuardType v12, Flonum recompile
          v28:Flonum = GuardType v13, Flonum recompile
          v29:Float = FloatMul v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_float_sub_inline() {
        eval(r#"
            def test(a, b) = a - b
            test(5.0, 3.0)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, -@0x1010, cme:0x1018)
          v27:Flonum = GuardType v12, Flonum recompile
          v28:Flonum = GuardType v13, Flonum recompile
          v29:Float = FloatSub v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_float_div_inline() {
        eval(r#"
            def test(a, b) = a / b
            test(10.0, 3.0)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, /@0x1010, cme:0x1018)
          v27:Flonum = GuardType v12, Flonum recompile
          v28:Flonum = GuardType v13, Flonum recompile
          v29:Float = FloatDiv v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_float_to_i_inline() {
        eval(r#"
            def test(a) = a.to_i
            test(3.7)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :a@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, to_i@0x1010, cme:0x1018)
          v22:Flonum = GuardType v10, Flonum recompile
          v23:Integer = FloatToInt v22
          CheckInterrupts
          Return v23
        ");
    }

    #[test]
    fn test_float_mul_fixnum_inline() {
        eval(r#"
            def test(a, b) = a * b
            test(1.5, 3)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, *@0x1010, cme:0x1018)
          v27:Flonum = GuardType v12, Flonum recompile
          v28:Fixnum = GuardType v13, Fixnum recompile
          v29:Float = FloatMul v27, v28
          CheckInterrupts
          Return v29
        ");
    }

    #[test]
    fn test_float_mul_recompile_stops_inlining_heap_float() {
        set_max_versions(2);
        eval(r#"
            def test_float_mul_recompile(a, b) = a * b

            30.times { test_float_mul_recompile(1.5, 2.5) }
        "#);

        let intermediate_hir = hir_string("test_float_mul_recompile");
        assert!(intermediate_hir.contains("FloatMul"), "{intermediate_hir}");

        eval(r#"
            30.times { test_float_mul_recompile(1.5, -0.0) }
        "#);

        let final_hir = hir_string("test_float_mul_recompile");
        assert!(final_hir.contains("CCallWithFrame"), "{final_hir}");
        assert!(!final_hir.contains("FloatMul"), "{final_hir}");
        assert_snapshot!(format!("{intermediate_hir}\n{final_hir}"), @"
        fn test_float_mul_recompile@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, *@0x1010, cme:0x1018)
          v27:Flonum = GuardType v12, Flonum recompile
          v28:Flonum = GuardType v13, Flonum recompile
          v29:Float = FloatMul v27, v28
          CheckInterrupts
          Return v29

        fn test_float_mul_recompile@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, *@0x1010, cme:0x1018)
          v27:Flonum = GuardType v12, Flonum recompile
          v28:BasicObject = CCallWithFrame v27, :Float#*@0x1040, v13
          CheckInterrupts
          Return v28
        ");
    }

    #[test]
    fn test_float_mul_recompile_stops_inlining_heap_float_receiver() {
        set_max_versions(2);
        eval(r#"
            def test_float_mul_recompile(a, b) = a * b

            30.times { test_float_mul_recompile(1.5, 2.5) }
        "#);

        let intermediate_hir = hir_string("test_float_mul_recompile");
        assert!(intermediate_hir.contains("FloatMul"), "{intermediate_hir}");

        eval(r#"
            30.times { test_float_mul_recompile(-0.0, 1.5) }
        "#);

        let final_hir = hir_string("test_float_mul_recompile");
        assert!(final_hir.contains("CCallWithFrame"), "{final_hir}");
        assert!(!final_hir.contains("FloatMul"), "{final_hir}");
        assert_snapshot!(format!("{intermediate_hir}\n{final_hir}"), @"
        fn test_float_mul_recompile@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Float@0x1008, *@0x1010, cme:0x1018)
          v27:Flonum = GuardType v12, Flonum recompile
          v28:Flonum = GuardType v13, Flonum recompile
          v29:Float = FloatMul v27, v28
          CheckInterrupts
          Return v29

        fn test_float_mul_recompile@<compiled>:2:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          v20:CBool = HasType v12, HeapFloat
          CondBranch v20, bb5(), bb6()
        bb5():
          v23:HeapFloat = RefineType v12, HeapFloat
          PatchPoint MethodRedefined(Float@0x1008, *@0x1010, cme:0x1018)
          v41:BasicObject = CCallWithFrame v23, :Float#*@0x1040, v13
          Jump bb4(v41)
        bb6():
          v26:CBool = HasType v12, Flonum
          CondBranch v26, bb7(), bb8()
        bb7():
          v29:Flonum = RefineType v12, Flonum
          PatchPoint MethodRedefined(Float@0x1008, *@0x1010, cme:0x1018)
          v44:BasicObject = CCallWithFrame v29, :Float#*@0x1040, v13
          Jump bb4(v44)
        bb8():
          v32:BasicObject = Send v12, :*, v13 # SendFallbackReason: Send: polymorphic call site
          Jump bb4(v32)
        bb4(v19:BasicObject):
          CheckInterrupts
          Return v19
        ");
    }

    #[test]
    fn test_elide_repeated_heap_object_guards() {
        eval(r#"
            C = Struct.new(:var)
            def test(obj)
              sum = 0
              sum += obj.var
              sum += obj.var
              sum += obj.var
              sum += obj.var
              sum += obj.var
              sum += obj.var
              sum += obj.var
              sum += obj.var
              sum += obj.var
              sum += obj.var
              sum
            end
            test(C.new(3))
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :obj@0x1000
          v4:NilClass = Const Value(nil)
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :obj@1
          v9:NilClass = Const Value(nil)
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:NilClass):
          v16:Fixnum[0] = Const Value(0)
          PatchPoint NoSingletonClass(C@0x1008)
          PatchPoint MethodRedefined(C@0x1008, var@0x1010, cme:0x1018)
          v137:ObjectSubclass[class_exact:C] = GuardType v12, ObjectSubclass[class_exact:C] recompile
          v138:BasicObject = LoadField v137, :var@0x1040
          PatchPoint MethodRedefined(Integer@0x1048, +@0x1050, cme:0x1058)
          v142:Fixnum = GuardType v138, Fixnum
          PatchPoint NoEPEscape(test)
          v152:Fixnum = FixnumAdd v142, v142
          v161:Fixnum = FixnumAdd v152, v142
          v170:Fixnum = FixnumAdd v161, v142
          v179:Fixnum = FixnumAdd v170, v142
          v188:Fixnum = FixnumAdd v179, v142
          v197:Fixnum = FixnumAdd v188, v142
          v206:Fixnum = FixnumAdd v197, v142
          v215:Fixnum = FixnumAdd v206, v142
          v224:Fixnum = FixnumAdd v215, v142
          CheckInterrupts
          Return v224
        ");
    }

    #[test]
    fn test_dont_fold_array_length() {
        eval(r#"
            A = [1, 2, 3, 4]
            def test = A.length
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, A)
          v11:ArrayExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(Array@0x1010)
          PatchPoint MethodRedefined(Array@0x1010, length@0x1018, cme:0x1020)
          v24:CInt64 = ArrayLength v11
          v25:Fixnum = BoxFixnum v24
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_fold_frozen_array_length() {
        eval(r#"
            A = [1, 2, 3, 4].freeze
            def test = A.length
            test
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, A)
          v11:ArrayExact[VALUE(0x1008)] = Const Value(VALUE(0x1008))
          PatchPoint NoSingletonClass(Array@0x1010)
          PatchPoint MethodRedefined(Array@0x1010, length@0x1018, cme:0x1020)
          v26:CInt64[4] = Const CInt64(4)
          v25:Fixnum = BoxFixnum v26
          CheckInterrupts
          Return v25
        ");
    }

    #[test]
    fn test_elide_test_of_box_bool() {
        eval(r#"
            def test(a, b)
              if a == b
                3
              else
                4
              end
            end
            test(:a, :b)
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :a@0x1000
          v4:BasicObject = LoadField v2, :b@0x1001
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:BasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :a@1
          v9:BasicObject = LoadArg :b@2
          Jump bb3(v7, v8, v9)
        bb3(v11:BasicObject, v12:BasicObject, v13:BasicObject):
          PatchPoint MethodRedefined(Symbol@0x1008, ==@0x1010, cme:0x1018)
          v45:StaticSymbol = GuardType v12, StaticSymbol recompile
          v46:CBool = IsBitEqual v45, v13
          CondBranch v46, bb5(), bb4(v11, v45, v13)
        bb5():
          v27:Fixnum[3] = Const Value(3)
          CheckInterrupts
          Return v27
        bb4(v32:BasicObject, v33:StaticSymbol, v34:BasicObject):
          v37:Fixnum[4] = Const Value(4)
          CheckInterrupts
          Return v37
        ");
    }

    #[test]
    fn test_trigger_guard_type_recompilation() {
        set_max_versions(2);
        set_inline_threshold(0);
        eval("
            class C
              def f(x)
                @a = 1
                y = x + 1
                @a = y
              end
            end

            # As of 06/04/2026, zjit/src/options.rs uses 5 as the default number of profiles
            # Let's pick a number that is reasonably larger to ensure compilation, even if
            # the default value changes a bit
            num_to_compile = 30

            c = C.new

            # Repeatedly call an integer until this fast path gets JITed
            num_to_compile.times { c.f(1) }

        ");

        let intermediate_hir = hir_string_proc("C.new.method(:f)");

        eval("
            # Supposed to be the same as the earlier Ruby method in this test
            num_to_compile = 30
            c = C.new
            # Call this with a float in order to trigger a guard failure
            # Do this enough times to cause a recompilation
            num_to_compile.times { c.f(1.5) }
        ");
        let final_hir = hir_string_proc("C.new.method(:f)");

        assert_snapshot!(format!("{intermediate_hir}\n{final_hir}"), @"
        fn f@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:NilClass = Const Value(nil)
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:HeapBasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:NilClass = Const Value(nil)
          Jump bb3(v7, v8, v9)
        bb3(v11:HeapBasicObject, v12:BasicObject, v13:NilClass):
          v16:Fixnum[1] = Const Value(1)
          PatchPoint SingleRactorMode
          v20:CShape = LoadField v11, :shape_id@0x1001
          v21:CShape[0x1002] = Const CShape(0x1002)
          v22:CBool = IsBitEqual v20, v21
          CondBranch v22, bb5(), bb6()
        bb5():
          StoreField v11, :@a@0x1003, v16
          WriteBarrier v11, v16
          Jump bb4()
        bb6():
          v27:CShape[0x1004] = Const CShape(0x1004)
          v28:CBool = IsBitEqual v20, v27
          CondBranch v28, bb7(), bb8()
        bb7():
          StoreField v11, :@a@0x1003, v16
          WriteBarrier v11, v16
          v34:CShape[0x1002] = Const CShape(0x1002)
          StoreField v11, :shape_id@0x1001, v34
          Jump bb4()
        bb8():
          SetIvar v11, :@a, v16
          Jump bb4()
        bb4():
          PatchPoint NoEPEscape(f)
          v43:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v71:Fixnum = GuardType v12, Fixnum recompile
          v72:Fixnum = FixnumAdd v71, v43
          PatchPoint SingleRactorMode
          v54:CShape = LoadField v11, :shape_id@0x1001
          v55:CShape[0x1002] = Const CShape(0x1002)
          v56:CBool = IsBitEqual v54, v55
          CondBranch v56, bb10(), bb11()
        bb10():
          StoreField v11, :@a@0x1003, v72
          WriteBarrier v11, v72
          Jump bb9()
        bb11():
          SetIvar v11, :@a, v72
          Jump bb9()
        bb9():
          CheckInterrupts
          Return v72

        fn f@<compiled>:4:
        bb1():
          EntryPoint interpreter
          v1:HeapBasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :x@0x1000
          v4:NilClass = Const Value(nil)
          Jump bb3(v1, v3, v4)
        bb2():
          EntryPoint JIT(0)
          v7:HeapBasicObject = LoadArg :self@0
          v8:BasicObject = LoadArg :x@1
          v9:NilClass = Const Value(nil)
          Jump bb3(v7, v8, v9)
        bb3(v11:HeapBasicObject, v12:BasicObject, v13:NilClass):
          v16:Fixnum[1] = Const Value(1)
          PatchPoint SingleRactorMode
          v20:CShape = LoadField v11, :shape_id@0x1001
          v21:CShape[0x1002] = Const CShape(0x1002)
          v22:CBool = IsBitEqual v20, v21
          CondBranch v22, bb5(), bb6()
        bb5():
          StoreField v11, :@a@0x1003, v16
          WriteBarrier v11, v16
          Jump bb4()
        bb6():
          v27:CShape[0x1004] = Const CShape(0x1004)
          v28:CBool = IsBitEqual v20, v27
          CondBranch v28, bb7(), bb8()
        bb7():
          StoreField v11, :@a@0x1003, v16
          WriteBarrier v11, v16
          v34:CShape[0x1002] = Const CShape(0x1002)
          StoreField v11, :shape_id@0x1001, v34
          Jump bb4()
        bb8():
          SetIvar v11, :@a, v16
          Jump bb4()
        bb4():
          PatchPoint NoEPEscape(f)
          v43:Fixnum[1] = Const Value(1)
          v47:CBool = HasType v12, Fixnum
          CondBranch v47, bb10(), bb11()
        bb10():
          v50:Fixnum = RefineType v12, Fixnum
          PatchPoint MethodRedefined(Integer@0x1008, +@0x1010, cme:0x1018)
          v85:Fixnum = FixnumAdd v50, v43
          Jump bb9(v85)
        bb11():
          v53:CBool = HasType v12, Flonum
          CondBranch v53, bb12(), bb13()
        bb12():
          v56:Flonum = RefineType v12, Flonum
          PatchPoint MethodRedefined(Float@0x1040, +@0x1010, cme:0x1048)
          v88:Float = FloatAdd v56, v43
          Jump bb9(v88)
        bb13():
          v59:BasicObject = Send v12, :+, v43 # SendFallbackReason: Send: polymorphic call site
          Jump bb9(v59)
        bb9(v46:BasicObject):
          PatchPoint SingleRactorMode
          v68:CShape = LoadField v11, :shape_id@0x1001
          v69:CShape[0x1002] = Const CShape(0x1002)
          v70:CBool = IsBitEqual v68, v69
          CondBranch v70, bb15(), bb16()
        bb15():
          StoreField v11, :@a@0x1003, v46
          WriteBarrier v11, v46
          Jump bb14()
        bb16():
          SetIvar v11, :@a, v46
          Jump bb14()
        bb14():
          CheckInterrupts
          Return v46
        ");
    }

    // Helper that compiles with inlining enabled. Temporarily sets the inline
    // threshold, compiles and optimizes, then restores the original value.
    #[track_caller]
    fn hir_string_with_inlining(method: &str) -> String {
        let old_threshold = get_option!(inline_threshold);
        unsafe { OPTIONS.as_mut().unwrap().inline_threshold = 30; }
        let result = hir_string(method);
        unsafe { OPTIONS.as_mut().unwrap().inline_threshold = old_threshold; }
        result
    }

    #[test]
    fn test_inline_method_with_send() {
        // The callee-internal `x + x` Send gets specialized to FixnumAdd because the callee's
        // profile entries are merged into the caller's ProfileOracle during inlining.
        eval("
            def double(x)
              x + x
            end
            def test(n)
              double(n)
            end
            test(1)
            test(1)
        ");
        assert_snapshot!(hir_string_with_inlining("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Object@0x1008, double@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v22 (0x1040), v10
          PatchPoint MethodRedefined(Integer@0x1068, +@0x1070, cme:0x1078)
          v43:Fixnum = GuardType v10, Fixnum recompile
          v45:Fixnum = FixnumAdd v43, v43
          CheckInterrupts
          PopInlineFrame
          Return v45
        ");
    }

    #[test]
    fn test_inline_method_with_multiple_returns() {
        // `clamp_nonneg` has two Return instructions (one from the early `return 0 if ...`,
        // one from the implicit trailing `x`). Inlining rewrites each Return to a Jump into
        // the continuation block, whose single Param merges the return values.
        eval("
            def clamp_nonneg(x)
              return 0 if x < 0
              x
            end
            def test(n)
              clamp_nonneg(n)
            end
            test(1)
            test(1)
        ");
        assert_snapshot!(hir_string_with_inlining("test"), @"
        fn test@<compiled>:7:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Object@0x1008, clamp_nonneg@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v22 (0x1040), v10
          v30:Fixnum[0] = Const Value(0)
          PatchPoint MethodRedefined(Integer@0x1068, <@0x1070, cme:0x1078)
          v59:Fixnum = GuardType v10, Fixnum recompile
          v60:BoolExact = FixnumLt v59, v30
          v35:CBool = Test v60
          CondBranch v35, bb7(), bb6(v22, v59)
        bb7():
          v40:Fixnum[0] = Const Value(0)
          CheckInterrupts
          Jump bb4(v40)
        bb6(v45:ObjectSubclass[class_exact*:Object@VALUE(0x1008)], v46:Fixnum):
          CheckInterrupts
          Jump bb4(v46)
        bb4(v53:Fixnum):
          PopInlineFrame
          CheckInterrupts
          Return v53
        ");
    }

    #[test]
    fn test_inline_arithmetic_method() {
        eval("
            def add_one(x)
              x + 1
            end
            def test(n)
              add_one(n)
            end
            test(1)
            test(1)
        ");
        assert_snapshot!(hir_string_with_inlining("test"), @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Object@0x1008, add_one@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v22 (0x1040), v10
          v30:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1068, +@0x1070, cme:0x1078)
          v44:Fixnum = GuardType v10, Fixnum recompile
          v45:Fixnum = FixnumAdd v44, v30
          CheckInterrupts
          PopInlineFrame
          Return v45
        ");
    }

    #[test]
    fn test_final_inline_iteration_specializes_inlined_iseq_send() {
        eval("
            def inner(x)
              x + 1
            end
            def outer(x)
              inner(x)
            end
            def test(n)
              outer(n)
            end
            test(1)
            test(1)
        ");

        let old_threshold = get_option!(inline_threshold);
        let old_max_iterations = get_option!(inline_max_iterations);
        unsafe {
            OPTIONS.as_mut().unwrap().inline_threshold = 30;
            OPTIONS.as_mut().unwrap().inline_max_iterations = 1;
        }
        let result = hir_string("test");
        unsafe {
            OPTIONS.as_mut().unwrap().inline_threshold = old_threshold;
            OPTIONS.as_mut().unwrap().inline_max_iterations = old_max_iterations;
        }

        assert!(result.contains("PushInlineFrame"),
            "Expected outer to be inlined with inline_max_iterations=1:\n{result}");
        assert!(result.contains(" = SendDirect "),
            "Expected the Send inside the final inlined body to be specialized to SendDirect:\n{result}");
        assert!(!result.contains(" = Send "),
            "Expected no unspecialized Send after the final specialization round:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:9:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Object@0x1008, outer@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v22 (0x1040), v10
          PatchPoint MethodRedefined(Object@0x1008, inner@0x1068, cme:0x1070)
          v42:BasicObject = SendDirect v22, 0x0, :inner (0x1098), v10
          CheckInterrupts
          PopInlineFrame
          Return v42
        ");
    }

    #[test]
    fn test_inline_budget_rejects_when_exceeded() {
        // The same workload as test_inline_arithmetic_method, which we know inlines
        // successfully under the default settings (budget=500, threshold=30). Setting
        // the budget to 1 forces should_inline to bail on the budget check before
        // reaching any other rejection reason. To verify the budget specifically is
        // what blocked the inline (not e.g. the size threshold or a parameter-shape
        // check), we read the inline_reject_budget_exceeded counter and confirm it
        // incremented while inline_method_count did not.
        eval("
            def add_one(x)
              x + 1
            end
            def test(n)
              add_one(n)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let budget_rejects_before = counters.inline_reject_budget_exceeded;
        let inline_count_before = counters.inline_method_count;

        let old_threshold = get_option!(inline_threshold);
        let old_budget = get_option!(inline_budget);
        unsafe {
            OPTIONS.as_mut().unwrap().inline_threshold = 30;
            OPTIONS.as_mut().unwrap().inline_budget = 1;
        }
        let result = hir_string("test");
        unsafe {
            OPTIONS.as_mut().unwrap().inline_threshold = old_threshold;
            OPTIONS.as_mut().unwrap().inline_budget = old_budget;
        }

        let budget_rejects_after = counters.inline_reject_budget_exceeded;
        let inline_count_after = counters.inline_method_count;

        assert!(budget_rejects_after > budget_rejects_before,
            "Expected inline_reject_budget_exceeded to increment, but it stayed at {budget_rejects_before}");
        assert_eq!(inline_count_after, inline_count_before,
            "Expected no successful inlines under budget=1, but inline_method_count went from {inline_count_before} to {inline_count_after}");

        // Belt-and-braces: the resulting HIR also reflects no inlining took place.
        assert!(result.contains("SendDirect"),
            "Expected SendDirect to remain in HIR when budget is exceeded:\n{result}");
        assert!(!result.contains("PushInlineFrame"),
            "Expected no PushInlineFrame in HIR when budget is exceeded:\n{result}");
    }

    #[test]
    fn test_inline_method_with_all_optionals_omitted() {
        // Caller fills 0 optionals: both `b` and `c` defaults must run inside the inlined
        // body. We pick `jit_entry_blocks[0]` so the body's default-init chain executes
        // and assigns `b = 10`, `c = 100` before the post-default body adds them.
        eval("
            def add_opts(a, b = 10, c = 100)
              a + b + c
            end
            def test(n)
              add_opts(n)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected add_opts to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Object@0x1008, add_opts@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v22 (0x1040), v10
          v30:Fixnum[10] = Const Value(10)
          v38:Fixnum[100] = Const Value(100)
          PatchPoint MethodRedefined(Integer@0x1068, +@0x1070, cme:0x1078)
          v67:Fixnum = GuardType v10, Fixnum recompile
          v68:Fixnum = FixnumAdd v67, v30
          v72:Fixnum = FixnumAdd v68, v38
          CheckInterrupts
          PopInlineFrame
          Return v72
        ");
    }

    #[test]
    fn test_inline_method_with_some_optionals_supplied() {
        // Caller fills 1 of 2 optionals: only `c`'s default should run. We pick
        // `jit_entry_blocks[1]`, whose target enters the body just before the `c`
        // default-init code so `b` is taken from the caller and `c` is filled in.
        eval("
            def add_opts(a, b = 10, c = 100)
              a + b + c
            end
            def test(n)
              add_opts(n, 20)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected add_opts to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:Fixnum[20] = Const Value(20)
          PatchPoint MethodRedefined(Object@0x1008, add_opts@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v24 (0x1040), v10, v15
          v32:Fixnum[100] = Const Value(100)
          PatchPoint MethodRedefined(Integer@0x1068, +@0x1070, cme:0x1078)
          v60:Fixnum = GuardType v10, Fixnum recompile
          v61:Fixnum = FixnumAdd v60, v15
          v65:Fixnum = FixnumAdd v61, v32
          CheckInterrupts
          PopInlineFrame
          Return v65
        ");
    }

    #[test]
    fn test_inline_method_with_omitted_optional_return_default() {
        // With the optional omitted, the general inliner enters entry 0 and
        // runs the default expression path.
        eval("
            def callee(arg = nil || (return :default))
              arg
            end
            def test = callee
            test
            test
        ");
        assert_snapshot!(hir_string_with_inlining("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint MethodRedefined(Object@0x1000, callee@0x1008, cme:0x1010)
          v17:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          v37:NilClass = Const Value(nil)
          PushInlineFrame v17 (0x1038)
          v24:StaticSymbol[:default] = Const Value(VALUE(0x1060))
          CheckInterrupts
          PopInlineFrame
          Return v24
        ");
    }

    #[test]
    fn test_inline_method_with_supplied_optional_return_default() {
        // With the optional supplied, the general inliner uses the selected
        // optional entry instead of entry 0, so the default-expression return
        // is not inlined.
        eval("
            def callee(arg = nil || (return :default))
              arg
            end
            def test = callee(3)
            test
            test
        ");
        assert_snapshot!(hir_string_with_inlining("test"), @"
        fn test@<compiled>:5:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          v10:Fixnum[3] = Const Value(3)
          PatchPoint MethodRedefined(Object@0x1000, callee@0x1008, cme:0x1010)
          v19:ObjectSubclass[class_exact*:Object@VALUE(0x1000)] = GuardType v6, ObjectSubclass[class_exact*:Object@VALUE(0x1000)] recompile
          PushInlineFrame v19 (0x1038), v10
          CheckInterrupts
          PopInlineFrame
          Return v10
        ");
    }

    #[test]
    fn test_inline_method_with_rescue_handler() {
        eval("
            def maybe_rescue(x)
              begin
                x + 1
              rescue StandardError
                0
              end
            end
            def test(n)
              maybe_rescue(n)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected maybe_rescue to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:10:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Object@0x1008, maybe_rescue@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v22 (0x1040), v10
          v30:Fixnum[1] = Const Value(1)
          PatchPoint MethodRedefined(Integer@0x1068, +@0x1070, cme:0x1078)
          v45:Fixnum = GuardType v10, Fixnum recompile
          v46:Fixnum = FixnumAdd v45, v30
          CheckInterrupts
          PopInlineFrame
          Return v46
        ");
    }

    #[test]
    fn test_inline_rejects_callees_on_deny_list() {
        // The `--zjit-inline-deny=...` knob lists qualified method names that
        // should_inline must refuse to inline, regardless of any other heuristic
        // outcome. The match runs before size/parameter/budget checks so the
        // signal is unambiguous when reading stats. The counter check pins the
        // rejection cause to the deny list specifically; an HIR-only check could
        // pass for any number of unrelated reasons that also leave SendDirect
        // in place.
        eval("
            def add_one(x)
              x + 1
            end
            def test(n)
              add_one(n)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let denied_rejects_before = counters.inline_reject_denied;
        let inline_count_before = counters.inline_method_count;

        let old_deny = get_option!(inline_deny).clone();
        unsafe {
            OPTIONS.as_mut().unwrap().inline_deny.insert("Object#add_one".to_string());
        }
        let result = hir_string_with_inlining("test");
        unsafe {
            OPTIONS.as_mut().unwrap().inline_deny = old_deny;
        }

        let denied_rejects_after = counters.inline_reject_denied;
        let inline_count_after = counters.inline_method_count;

        assert!(denied_rejects_after > denied_rejects_before,
            "Expected inline_reject_denied to increment for Object#add_one, but it stayed at {denied_rejects_before}");
        assert_eq!(inline_count_after, inline_count_before,
            "Expected no inlines for Object#add_one when on the deny list, but inline_method_count went from {inline_count_before} to {inline_count_after}");

        assert!(result.contains("SendDirect"),
            "Expected SendDirect to remain in HIR when callee is on the deny list:\n{result}");
        assert!(!result.contains("PushInlineFrame"),
            "Expected no PushInlineFrame in HIR when callee is on the deny list:\n{result}");
    }

    #[test]
    fn test_inline_method_with_invokesuper() {
        eval("
            class Parent
              def greet = 'hi'
            end
            class Child < Parent
              def greet = super + '!'
            end
            child = Child.new
            def test(c) = c.greet
            test(child)
            test(child)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected Child#greet to be inlined, but inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in HIR when inlining a super-containing callee:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:9:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :c@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :c@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint NoSingletonClass(Child@0x1008)
          PatchPoint MethodRedefined(Child@0x1008, greet@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact:Child] = GuardType v10, ObjectSubclass[class_exact:Child] recompile
          PushInlineFrame v22 (0x1040)
          PatchPoint MethodRedefined(Parent@0x1068, greet@0x1010, cme:0x1070)
          v45:CPtr = GetEP 0
          v46:RubyValue = LoadField v45, :VM_ENV_DATA_INDEX_ME_CREF@0x1098
          v47:CallableMethodEntry[VALUE(0x1018)] = GuardBitEquals v46, Value(VALUE(0x1018))
          v48:RubyValue = LoadField v45, :VM_ENV_DATA_INDEX_SPECVAL@0x1099
          v49:FalseClass = GuardBitEquals v48, Value(false)
          PushInlineFrame v22 (0x10a0)
          v60:StringExact[VALUE(0x10c8)] = Const Value(VALUE(0x10c8))
          v61:StringExact = StringCopy v60
          CheckInterrupts
          PopInlineFrame
          v31:StringExact[VALUE(0x10d0)] = Const Value(VALUE(0x10d0))
          v32:StringExact = StringCopy v31
          PatchPoint NoSingletonClass(String@0x10d8)
          PatchPoint MethodRedefined(String@0x10d8, +@0x10e0, cme:0x10e8)
          v55:BasicObject = CCallWithFrame v61, :String#+@0x1110, v32
          CheckInterrupts
          PopInlineFrame
          Return v55
        ");
    }

    #[test]
    fn test_inline_method_with_all_optionals_supplied() {
        // Caller fills every optional: no default-init code runs. We pick the last
        // `jit_entry_blocks` entry, which lands directly in the post-default body.
        eval("
            def add_opts(a, b = 10, c = 100)
              a + b + c
            end
            def test(n)
              add_opts(n, 20, 200)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected add_opts to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:Fixnum[20] = Const Value(20)
          v17:Fixnum[200] = Const Value(200)
          PatchPoint MethodRedefined(Object@0x1008, add_opts@0x1010, cme:0x1018)
          v26:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v26 (0x1040), v10, v15, v17
          PatchPoint MethodRedefined(Integer@0x1068, +@0x1070, cme:0x1078)
          v53:Fixnum = GuardType v10, Fixnum recompile
          v54:Fixnum = FixnumAdd v53, v15
          v58:Fixnum = FixnumAdd v54, v17
          CheckInterrupts
          PopInlineFrame
          Return v58
        ");
    }

    #[test]
    fn test_inline_method_with_leading_optional_post_required() {
        // Callee shape `def m(a = 10, b)` has lead_num=0, opt_num=1, post_num=1.
        // The caller passes one positional, so the optional `a` falls through to
        // its default and `b` takes the lone caller arg. The inliner must shift
        // the post-required arg index past the gap of the unfilled optional.
        eval("
            def add_opt_post(a = 10, b)
              a + b
            end
            def test(n)
              add_opt_post(n)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected add_opt_post to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Object@0x1008, add_opt_post@0x1010, cme:0x1018)
          v22:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v22 (0x1040), v10
          v29:Fixnum[10] = Const Value(10)
          PatchPoint MethodRedefined(Integer@0x1068, +@0x1070, cme:0x1078)
          v53:Fixnum = GuardType v10, Fixnum
          v54:Fixnum = FixnumAdd v29, v53
          CheckInterrupts
          PopInlineFrame
          Return v54
        ");
    }

    #[test]
    fn test_inline_method_with_required_optional_post_all_omitted() {
        // Callee shape `def m(a, b = 10, c)` has lead_num=1, opt_num=1, post_num=1.
        // Calling with two positionals fills `a` and `c`; `b` falls through to its
        // default. The inliner must enter the body via jit_entry_blocks[0] so the
        // default-init code for `b` runs, and shift `c`'s arg index past the gap.
        eval("
            def add_lead_opt_post(a, b = 10, c)
              a + b + c
            end
            def test(n)
              add_lead_opt_post(n, 200)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected add_lead_opt_post to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:Fixnum[200] = Const Value(200)
          PatchPoint MethodRedefined(Object@0x1008, add_lead_opt_post@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v24 (0x1040), v10, v15
          v32:Fixnum[10] = Const Value(10)
          PatchPoint MethodRedefined(Integer@0x1068, +@0x1070, cme:0x1078)
          v60:Fixnum = GuardType v10, Fixnum recompile
          v61:Fixnum = FixnumAdd v60, v32
          v65:Fixnum = FixnumAdd v61, v15
          CheckInterrupts
          PopInlineFrame
          Return v65
        ");
    }

    #[test]
    fn test_inline_method_with_required_keyword() {
        eval("
            def add_kw(a, b:)
              a + b
            end
            def test(n)
              add_kw(n, b: 5)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected add_kw to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:Fixnum[5] = Const Value(5)
          PatchPoint MethodRedefined(Object@0x1008, add_kw@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          v41:Fixnum[0] = Const Value(0)
          PushInlineFrame v24 (0x1040), v10, v15
          PatchPoint MethodRedefined(Integer@0x1068, +@0x1070, cme:0x1078)
          v48:Fixnum = GuardType v10, Fixnum recompile
          v49:Fixnum = FixnumAdd v48, v15
          CheckInterrupts
          PopInlineFrame
          Return v49
        ");
    }

    #[test]
    fn test_inline_method_with_optional_keyword_supplied() {
        eval("
            def add_optkw(a, b: 10)
              a + b
            end
            def test(n)
              add_optkw(n, b: 50)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected add_optkw to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:Fixnum[50] = Const Value(50)
          PatchPoint MethodRedefined(Object@0x1008, add_optkw@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          v41:Fixnum[0] = Const Value(0)
          PushInlineFrame v24 (0x1040), v10, v15
          PatchPoint MethodRedefined(Integer@0x1068, +@0x1070, cme:0x1078)
          v48:Fixnum = GuardType v10, Fixnum recompile
          v49:Fixnum = FixnumAdd v48, v15
          CheckInterrupts
          PopInlineFrame
          Return v49
        ");
    }

    #[test]
    fn test_inline_method_with_optional_keyword_omitted_constant_default() {
        eval("
            def add_optkw(a, b: 10)
              a + b
            end
            def test(n)
              add_optkw(n)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected add_optkw to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v21:Fixnum[10] = Const Value(10)
          PatchPoint MethodRedefined(Object@0x1008, add_optkw@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          v41:Fixnum[0] = Const Value(0)
          PushInlineFrame v24 (0x1040), v10, v21
          PatchPoint MethodRedefined(Integer@0x1068, +@0x1070, cme:0x1078)
          v48:Fixnum = GuardType v10, Fixnum recompile
          v49:Fixnum = FixnumAdd v48, v21
          CheckInterrupts
          PopInlineFrame
          Return v49
        ");
    }

    #[test]
    fn test_inline_method_with_keywords_reordered() {
        // Caller passes keywords in an order that doesn't match the callee's declaration.
        eval("
            def add_kws(a, b:, c:)
              a * 100 + b * 10 + c
            end
            def test(n)
              add_kws(n, c: 3, b: 2)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected add_kws to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:Fixnum[3] = Const Value(3)
          v17:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Object@0x1008, add_kws@0x1010, cme:0x1018)
          v27:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          v59:Fixnum[0] = Const Value(0)
          PushInlineFrame v27 (0x1040), v10, v17, v15
          v38:Fixnum[100] = Const Value(100)
          PatchPoint MethodRedefined(Integer@0x1068, *@0x1070, cme:0x1078)
          v66:Fixnum = GuardType v10, Fixnum recompile
          v67:Fixnum = FixnumMult v66, v38
          v80:Fixnum[20] = Const Value(20)
          PatchPoint MethodRedefined(Integer@0x1068, +@0x10a0, cme:0x10a8)
          v75:Fixnum = FixnumAdd v67, v80
          v79:Fixnum = FixnumAdd v75, v15
          CheckInterrupts
          PopInlineFrame
          Return v79
        ");
    }

    #[test]
    fn test_inline_method_with_optional_keyword_omitted_nonconstant_default() {
        // Optional keyword with a non-constant default expression (`b: a * 2`) omitted by the caller.
        eval("
            def add_optkw_dyn(a, b: a * 2)
              a + b
            end
            def test(n)
              add_optkw_dyn(n)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected add_optkw_dyn to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v21:NilClass = Const Value(nil)
          PatchPoint MethodRedefined(Object@0x1008, add_optkw_dyn@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          v60:Fixnum[1] = Const Value(1)
          PushInlineFrame v24 (0x1040), v10, v21
          v32:BoolExact = FixnumBitCheck v60, 0
          v34:CBool = Test v32
          CondBranch v34, bb6(v24, v10, v21, v60), bb7()
        bb7():
          v40:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Integer@0x1068, *@0x1070, cme:0x1078)
          v67:Fixnum = GuardType v10, Fixnum recompile
          v68:Fixnum = FixnumMult v67, v40
          Jump bb6(v24, v67, v68, v60)
        bb6(v46:ObjectSubclass[class_exact*:Object@VALUE(0x1008)], v47:BasicObject, v48:NilClass|Fixnum, v49:Fixnum[1]):
          PatchPoint MethodRedefined(Integer@0x1068, +@0x10a0, cme:0x10a8)
          v71:Fixnum = GuardType v47, Fixnum recompile
          v72:Fixnum = GuardType v48, Fixnum
          v73:Fixnum = FixnumAdd v71, v72
          CheckInterrupts
          PopInlineFrame
          Return v73
        ");
    }

    #[test]
    fn test_inline_method_with_required_optional_post_all_supplied() {
        // Same callee shape as above (lead+opt+post) but the caller fills the
        // optional explicitly. We pick jit_entry_blocks[1] so no default-init code
        // runs and every local takes a caller arg directly.
        eval("
            def add_lead_opt_post(a, b = 10, c)
              a + b + c
            end
            def test(n)
              add_lead_opt_post(n, 20, 300)
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected add_lead_opt_post to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          v15:Fixnum[20] = Const Value(20)
          v17:Fixnum[300] = Const Value(300)
          PatchPoint MethodRedefined(Object@0x1008, add_lead_opt_post@0x1010, cme:0x1018)
          v26:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v26 (0x1040), v10, v15, v17
          PatchPoint MethodRedefined(Integer@0x1068, +@0x1070, cme:0x1078)
          v53:Fixnum = GuardType v10, Fixnum recompile
          v54:Fixnum = FixnumAdd v53, v15
          v58:Fixnum = FixnumAdd v54, v17
          CheckInterrupts
          PopInlineFrame
          Return v58
        ");
    }

    #[test]
    fn test_inline_method_with_invokeblock() {
        // The callee dispatches to the caller-supplied literal block via `yield`.
        // The block handler is established by the SPECVAL written into the inlined
        // frame by PushInlineFrame, and the `yield` lowers to an InvokeBlock that
        // reads it off the live CFP at runtime.
        eval("
            def with_yield(x)
              yield x
            end
            def test(n)
              with_yield(n) { |x| x + 2 }
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected with_yield to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Object@0x1008, with_yield@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          PushInlineFrame v24 (0x1040), v10
          v32:CPtr = GetEP 0
          v33:CInt64 = LoadField v32, :VM_ENV_DATA_INDEX_SPECVAL@0x1068
          v34:CInt64[-4] = Const CInt64(-4)
          v35:CInt64 = IntAnd v33, v34
          v36:BasicObject = InvokeBlockIseqDirect (0x1070), v35, v10
          CheckInterrupts
          PopInlineFrame
          PatchPoint NoEPEscape(test)
          Return v36
        ");
    }

    #[test]
    fn test_inline_method_with_block_param() {
        // The callee captures the caller-supplied literal block in a `&block`
        // parameter and invokes it with `block.call`. Inlining must preserve the
        // block handler so the reified Proc dispatches to the right block.
        eval("
            def with_block_param(x, &block)
              block.call(x)
            end
            def test(n)
              with_block_param(n) { |x| x + 2 }
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected with_block_param to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:6:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Object@0x1008, with_block_param@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          v51:NilClass = Const Value(nil)
          PushInlineFrame v24 (0x1040), v10
          v34:CPtr = GetEP 0
          v35:CUInt64 = LoadField v34, :VM_ENV_DATA_INDEX_FLAGS@0x1068
          v36:CBool = IsBlockParamModified v35
          CondBranch v36, bb6(), bb7()
        bb6():
          v38:BasicObject = LoadField v34, :block@0x1069
          Jump bb8(v38, v38)
        bb7():
          v40:CInt64 = LoadField v34, :VM_ENV_DATA_INDEX_SPECVAL@0x106a
          v41:CInt64 = GuardAnyBitSet v40, CUInt64(1) recompile
          v42:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1070))
          Jump bb8(v42, v51)
        bb8(v32:BasicObject, v33:BasicObject):
          v46:BasicObject = Send v32, :call, v10 # SendFallbackReason: Send: unsupported optimized method type BlockCall
          CheckInterrupts
          PopInlineFrame
          PatchPoint NoEPEscape(test)
          Return v46
        ");
    }

    #[test]
    fn test_inline_method_that_forwards_block_arg() {
        eval("
            def inner(x)
              yield x
            end
            def callee(x, &block)
              inner(x, &block)
            end
            def test(n)
              callee(n) { |x| x + 2 }
            end
            test(1)
            test(1)
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected callee to be inlined despite forwarding its block.\nHIR:\n{result}");
        assert_eq!(result.matches("PushInlineFrame").count(), 1,
            "Expected only `callee` to be inlined, not `inner`:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:9:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :n@0x1000
          Jump bb3(v1, v3)
        bb2():
          EntryPoint JIT(0)
          v6:BasicObject = LoadArg :self@0
          v7:BasicObject = LoadArg :n@1
          Jump bb3(v6, v7)
        bb3(v9:BasicObject, v10:BasicObject):
          PatchPoint MethodRedefined(Object@0x1008, callee@0x1010, cme:0x1018)
          v24:ObjectSubclass[class_exact*:Object@VALUE(0x1008)] = GuardType v9, ObjectSubclass[class_exact*:Object@VALUE(0x1008)] recompile
          v52:NilClass = Const Value(nil)
          PushInlineFrame v24 (0x1040), v10
          v36:CPtr = GetEP 0
          v37:CUInt64 = LoadField v36, :VM_ENV_DATA_INDEX_FLAGS@0x1068
          v38:CBool = IsBlockParamModified v37
          CondBranch v38, bb6(), bb7()
        bb6():
          v40:BasicObject = LoadField v36, :block@0x1069
          Jump bb8(v40, v40)
        bb7():
          v42:CInt64 = LoadField v36, :VM_ENV_DATA_INDEX_SPECVAL@0x106a
          v43:CInt64 = GuardAnyBitSet v42, CUInt64(1) recompile
          v44:ObjectSubclass[BlockParamProxy] = Const Value(VALUE(0x1070))
          Jump bb8(v44, v52)
        bb8(v34:BasicObject, v35:BasicObject):
          v47:BasicObject = Send v24, &block, :inner, v10, v34 # SendFallbackReason: Send: block argument is not nil
          CheckInterrupts
          PopInlineFrame
          PatchPoint NoEPEscape(test)
          Return v47
        ");
    }

    #[test]
    fn test_inline_object_new_no_escape() {
        // Mirrors the object-new-no-escape benchmark from ruby-bench.
        eval("
            class Point
              attr_reader :x, :y
              def initialize(x, y)
                @x = x
                @y = y
              end

              def ==(other)
                @x == other.x && @y == other.y
              end
            end

            def test
              Point.new(1, 2) == Point.new(1, 2)
            end
            test
            test
        ");
        let counters = crate::state::ZJITState::get_counters();
        let inline_count_before = counters.inline_method_count;

        let result = hir_string_with_inlining("test");

        assert!(counters.inline_method_count > inline_count_before,
            "Expected Point#initialize / Point#== to be inlined, inline_method_count did not increment.\nHIR:\n{result}");
        assert!(result.contains("PushInlineFrame"),
            "Expected PushInlineFrame in inlined HIR:\n{result}");
        assert!(!result.contains("SendDirect"),
            "Expected SendDirect to be replaced after inlining:\n{result}");

        assert_snapshot!(result, @"
        fn test@<compiled>:15:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          Jump bb3(v1)
        bb2():
          EntryPoint JIT(0)
          v4:BasicObject = LoadArg :self@0
          Jump bb3(v4)
        bb3(v6:BasicObject):
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1000, Point)
          v11:ClassSubclass[Point@0x1008] = Const Value(VALUE(0x1008))
          v13:NilClass = Const Value(nil)
          v16:Fixnum[1] = Const Value(1)
          v18:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Point@0x1008, new@0x1009, cme:0x1010)
          v84:ObjectSubclass[class_exact:Point] = ObjectAllocClass Point:VALUE(0x1008)
          PatchPoint NoSingletonClass(Point@0x1008)
          PatchPoint MethodRedefined(Point@0x1008, initialize@0x1038, cme:0x1040)
          PushInlineFrame v84 (0x1068), v16, v18
          v115:CShape = LoadField v84, :shape_id@0x1090
          v116:CShape[0x1091] = GuardBitEquals v115, CShape(0x1091) recompile
          StoreField v84, :@x@0x1092, v16
          WriteBarrier v84, v16
          v119:CShape[0x1093] = Const CShape(0x1093)
          StoreField v84, :shape_id@0x1090, v119
          PatchPoint NoEPEscape(initialize)
          PatchPoint SingleRactorMode
          StoreField v84, :@y@0x1094, v18
          WriteBarrier v84, v18
          v134:CShape[0x1095] = Const CShape(0x1095)
          StoreField v84, :shape_id@0x1090, v134
          CheckInterrupts
          PopInlineFrame
          PatchPoint SingleRactorMode
          PatchPoint StableConstantNames(0x1098, Point)
          v43:ClassSubclass[Point@0x1008] = Const Value(VALUE(0x1008))
          v45:NilClass = Const Value(nil)
          v48:Fixnum[1] = Const Value(1)
          v50:Fixnum[2] = Const Value(2)
          PatchPoint MethodRedefined(Point@0x1008, new@0x1009, cme:0x1010)
          v94:ObjectSubclass[class_exact:Point] = ObjectAllocClass Point:VALUE(0x1008)
          PatchPoint NoSingletonClass(Point@0x1008)
          PatchPoint MethodRedefined(Point@0x1008, initialize@0x1038, cme:0x1040)
          PushInlineFrame v94 (0x1068), v48, v50
          v154:CShape = LoadField v94, :shape_id@0x1090
          v155:CShape[0x1091] = GuardBitEquals v154, CShape(0x1091) recompile
          StoreField v94, :@x@0x1092, v48
          WriteBarrier v94, v48
          v158:CShape[0x1093] = Const CShape(0x1093)
          StoreField v94, :shape_id@0x1090, v158
          PatchPoint NoEPEscape(initialize)
          PatchPoint SingleRactorMode
          StoreField v94, :@y@0x1094, v50
          WriteBarrier v94, v50
          v173:CShape[0x1095] = Const CShape(0x1095)
          StoreField v94, :shape_id@0x1090, v173
          CheckInterrupts
          PopInlineFrame
          PatchPoint NoSingletonClass(Point@0x1008)
          PatchPoint MethodRedefined(Point@0x1008, ==@0x10a0, cme:0x10a8)
          PushInlineFrame v84 (0x10d0), v94
          PatchPoint SingleRactorMode
          v191:CShape = LoadField v84, :shape_id@0x1090
          v192:CShape[0x1095] = GuardBitEquals v191, CShape(0x1095) recompile
          v193:BasicObject = LoadField v84, :@x@0x1092
          PatchPoint NoEPEscape(==)
          PatchPoint MethodRedefined(Point@0x1008, x@0x10f8, cme:0x1100)
          PatchPoint MethodRedefined(Integer@0x1128, ==@0x10a0, cme:0x1130)
          v246:Fixnum = GuardType v193, Fixnum recompile
          v248:BoolExact = FixnumEq v246, v48
          v204:CBool = Test v248
          v205:FalseClass = RefineType v248, Falsy
          CondBranch v204, bb19(), bb18(v84, v94, v205)
        bb19():
          PatchPoint SingleRactorMode
          v212:CShape = LoadField v84, :shape_id@0x1090
          v213:CShape[0x1095] = GuardBitEquals v212, CShape(0x1095) recompile
          v214:BasicObject = LoadField v84, :@y@0x1094
          PatchPoint NoEPEscape(==)
          PatchPoint NoSingletonClass(Point@0x1008)
          PatchPoint MethodRedefined(Point@0x1008, y@0x1158, cme:0x1160)
          v253:CShape = LoadField v94, :shape_id@0x1090
          v254:CShape[0x1095] = GuardBitEquals v253, CShape(0x1095) recompile
          v255:BasicObject = LoadField v94, :@y@0x1094
          PatchPoint MethodRedefined(Integer@0x1128, ==@0x10a0, cme:0x1130)
          v258:Fixnum = GuardType v214, Fixnum recompile
          v259:Fixnum = GuardType v255, Fixnum
          v260:BoolExact = FixnumEq v258, v259
          Jump bb18(v84, v94, v260)
        bb18(v224:ObjectSubclass[class_exact:Point], v225:ObjectSubclass[class_exact:Point], v226:BoolExact):
          CheckInterrupts
          PopInlineFrame
          Return v226
        ");
    }

    #[test]
    fn test_ccall_with_frame_too_many_args_result_used_in_later_block() {
        unsafe extern "C" fn test_seven_args(
            _self: VALUE,
            a: VALUE,
            b: VALUE,
            c: VALUE,
            d: VALUE,
            e: VALUE,
            f: VALUE,
            g: VALUE,
        ) -> VALUE {
            unsafe { rb_ary_new_from_args(7, a, b, c, d, e, f, g) }
        }

        with_rubyvm(|| {
            let klass = define_class("ZJITSevenArgs", unsafe { rb_cObject });
            unsafe {
                rb_define_method(
                    klass,
                    c"seven".as_ptr(),
                    Some(std::mem::transmute::<
                        unsafe extern "C" fn(VALUE, VALUE, VALUE, VALUE, VALUE, VALUE, VALUE, VALUE) -> VALUE,
                        unsafe extern "C" fn(VALUE) -> VALUE,
                    >(test_seven_args)),
                    7,
                );
            }
        });

        eval(r#"
            def test(obj, flag)
              priceable = obj.seven(1, 2, 3, 4, 5, 6, 7)
              if flag
                priceable
              else
                nil
              end
            end

            obj = ZJITSevenArgs.new
            test(obj, true)  # profile receiver class
        "#);
        assert_snapshot!(hir_string("test"), @"
        fn test@<compiled>:3:
        bb1():
          EntryPoint interpreter
          v1:BasicObject = LoadSelf
          v2:CPtr = LoadSP
          v3:BasicObject = LoadField v2, :obj@0x1000
          v4:BasicObject = LoadField v2, :flag@0x1001
          v5:NilClass = Const Value(nil)
          Jump bb3(v1, v3, v4, v5)
        bb2():
          EntryPoint JIT(0)
          v8:BasicObject = LoadArg :self@0
          v9:BasicObject = LoadArg :obj@1
          v10:BasicObject = LoadArg :flag@2
          v11:NilClass = Const Value(nil)
          Jump bb3(v8, v9, v10, v11)
        bb3(v13:BasicObject, v14:BasicObject, v15:BasicObject, v16:NilClass):
          v20:Fixnum[1] = Const Value(1)
          v22:Fixnum[2] = Const Value(2)
          v24:Fixnum[3] = Const Value(3)
          v26:Fixnum[4] = Const Value(4)
          v28:Fixnum[5] = Const Value(5)
          v30:Fixnum[6] = Const Value(6)
          v32:Fixnum[7] = Const Value(7)
          v34:BasicObject = Send v14, :seven, v20, v22, v24, v26, v28, v30, v32 # SendFallbackReason: Too many arguments for LIR
          PatchPoint NoEPEscape(test)
          v41:CBool = Test v15
          v42:Falsy = RefineType v15, Falsy
          CondBranch v41, bb5(), bb4(v13, v14, v42, v34)
        bb5():
          v44:Truthy = RefineType v15, Truthy
          CheckInterrupts
          Return v34
        bb4(v51:BasicObject, v52:BasicObject, v53:Falsy, v54:BasicObject):
          v57:NilClass = Const Value(nil)
          CheckInterrupts
          Return v57
        ");
    }
}
