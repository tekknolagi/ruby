class Range
  with_jit do
    if Primitive.rb_builtin_basic_definition_p(:each)
      undef :each

      def each # :nodoc:
        Primitive.attr! :inline_block, :c_trace, :without_interrupts

        unless defined?(yield)
          return Primitive.cexpr! 'SIZED_ENUMERATOR(self, 0, 0, range_enum_size)'
        end

        # Fixnum ranges are iterated here so that the JIT can inline both the loop and the block.
        # Everything else (bignums, endless ranges, strings, symbols, #succ) uses the C implementation.
        lim = Primitive.range_each_fixnum_limit
        if lim
          i = Primitive.cexpr! 'RANGE_BEG(self)'
          while Primitive.rb_builtin_fixnum_lt(i, lim)
            yield i
            i = Primitive.rb_builtin_fixnum_inc(i)
          end
          self
        else
          Primitive.range_each_generic
        end
      end
    end
  end
end
