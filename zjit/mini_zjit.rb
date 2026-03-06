#!/usr/bin/env ruby
# frozen_string_literal: true

# mini_zjit.rb — A single-file demo of ZJIT's HIR compilation pipeline.
# Takes a RubyVM::InstructionSequence and produces typed SSA in HIR form,
# then runs optimization passes over it.
#
# Usage:
#   ruby mini_zjit.rb              # run built-in tests
#   ruby mini_zjit.rb --demo       # run interactive demo

module MiniZJIT

  # ═══════════════════════════════════════════════════════════════════
  # Type Lattice
  #
  #   Any (top)
  #   ├── BasicObject
  #   │   ├── Fixnum  (may carry a constant: Fixnum[3])
  #   │   ├── Float
  #   │   ├── String
  #   │   ├── Array
  #   │   ├── NilClass
  #   │   ├── TrueClass
  #   │   ├── FalseClass
  #   │   └── Object
  #   ├── CBool   (internal C boolean from Test/IsNil)
  #   └── Empty   (bottom — unreachable)
  # ═══════════════════════════════════════════════════════════════════

  class Type
    attr_reader :name, :const_val

    def initialize(name, const_val = :none)
      @name = name
      @const_val = const_val
    end

    def with_const(val)  = Type.new(@name, val)
    def has_const?       = @const_val != :none
    def fixnum?          = @name == :Fixnum
    def nilclass?        = @name == :NilClass
    def empty?           = @name == :Empty
    def any?             = @name == :Any
    def cbool?           = @name == :CBool
    def basic_object?    = @name == :BasicObject

    # Subtype check (simplified lattice)
    def <=(other)
      return true if other.any?
      return true if self.empty?
      return true if @name == other.name
      return true if other.name == :BasicObject && RUBY_TYPES.include?(@name)
      false
    end

    # Meet (intersection) — used by RefineType
    def &(other)
      return self  if self <= other
      return other if other <= self
      Types::Empty
    end

    def to_s
      s = @name.to_s
      s += "[#{@const_val}]" if has_const?
      s
    end

    def ==(other) = other.is_a?(Type) && @name == other.name && @const_val == other.const_val
    def eql?(other) = self == other
    def hash = [@name, @const_val].hash

    RUBY_TYPES = %i[Fixnum Float String Array NilClass TrueClass FalseClass Object].freeze
  end

  module Types
    Any         = Type.new(:Any)
    BasicObject = Type.new(:BasicObject)
    Fixnum      = Type.new(:Fixnum)
    Float       = Type.new(:Float)
    String      = Type.new(:String)
    Array       = Type.new(:Array)
    NilClass    = Type.new(:NilClass)
    TrueClass   = Type.new(:TrueClass)
    FalseClass  = Type.new(:FalseClass)
    Object      = Type.new(:Object)
    CBool       = Type.new(:CBool)
    Empty       = Type.new(:Empty)
  end

  # ═══════════════════════════════════════════════════════════════════
  # Effects — abstract heap read/write categories
  #
  #   Any = Control | Memory
  #   Memory = Locals | Stack | Other
  #   Control — prevents DCE but has no data dependency
  #   Empty — pure / no side effects
  # ═══════════════════════════════════════════════════════════════════

  module Eff
    Empty   = 0
    Control = 1 << 0
    Locals  = 1 << 1
    Stack   = 1 << 2
    Other   = 1 << 3
    Memory  = Locals | Stack | Other
    Any     = Control | Memory

    def self.name_of(bits)
      return "Empty" if bits == 0
      parts = []
      parts << "Control" if bits & Control != 0
      parts << "Locals"  if bits & Locals  != 0
      parts << "Stack"   if bits & Stack   != 0
      parts << "Other"   if bits & Other   != 0
      parts.join("|")
    end
  end

  Effects = Struct.new(:read, :write) do
    def elidable? = read == Eff::Empty && write == Eff::Empty
    def pure?     = elidable?

    def to_s = "(read: #{Eff.name_of(read)}, write: #{Eff.name_of(write)})"
  end

  # ═══════════════════════════════════════════════════════════════════
  # SSA Values, Instructions, Blocks, and the Function container
  # ═══════════════════════════════════════════════════════════════════

  class InsnId
    attr_reader :id
    def initialize(id) = @id = id
    def to_s = "v#{@id}"
    def ==(other) = other.is_a?(InsnId) && @id == other.id
    def eql?(other) = self == other
    def hash = @id.hash
  end

  class BlockId
    attr_reader :id
    def initialize(id) = @id = id
    def to_s = "bb#{@id}"
    def ==(other) = other.is_a?(BlockId) && @id == other.id
    def eql?(other) = self == other
    def hash = @id.hash
  end

  class BranchEdge
    attr_accessor :target, :args
    def initialize(target, args = [])
      @target = target
      @args = args
    end

    def to_s
      if @args.empty?
        @target.to_s
      else
        "#{@target}(#{@args.join(", ")})"
      end
    end
  end

  # Every instruction in the HIR
  module Insn
    # Block parameter (SSA phi-like)
    Param = Struct.new(:idx) do
      def operands = []
      def effects = Effects.new(Eff::Empty, Eff::Empty)
    end

    # Ruby constant value
    Const = Struct.new(:val) do
      def operands = []
      def effects = Effects.new(Eff::Empty, Eff::Empty)
    end

    # Snapshot of interpreter state for deoptimization
    Snapshot = Struct.new(:locals, :stack) do
      def operands = locals.compact + stack.compact
      def effects = Effects.new(Eff::Empty, Eff::Empty)
    end

    # Type guard — side-exits if val doesn't match guard_type
    GuardType = Struct.new(:val, :guard_type, :state) do
      def operands = [val, state].compact
      def effects = Effects.new(Eff::Empty, Eff::Control)
    end

    # Intersect a value's type with new_type (no runtime cost)
    RefineType = Struct.new(:val, :new_type) do
      def operands = [val]
      def effects = Effects.new(Eff::Empty, Eff::Empty)
    end

    # Test if value is truthy (returns CBool)
    Test = Struct.new(:val) do
      def operands = [val]
      def effects = Effects.new(Eff::Empty, Eff::Empty)
    end

    # Fixnum arithmetic
    FixnumAdd = Struct.new(:left, :right, :state) do
      def operands = [left, right, state].compact
      def effects = Effects.new(Eff::Empty, Eff::Control) # overflow side-exit
    end

    FixnumSub = Struct.new(:left, :right, :state) do
      def operands = [left, right, state].compact
      def effects = Effects.new(Eff::Empty, Eff::Control)
    end

    FixnumMult = Struct.new(:left, :right, :state) do
      def operands = [left, right, state].compact
      def effects = Effects.new(Eff::Empty, Eff::Control)
    end

    # Fixnum comparisons (pure — no overflow possible)
    FixnumLt = Struct.new(:left, :right) do
      def operands = [left, right]
      def effects = Effects.new(Eff::Empty, Eff::Empty)
    end

    FixnumEq = Struct.new(:left, :right) do
      def operands = [left, right]
      def effects = Effects.new(Eff::Empty, Eff::Empty)
    end

    FixnumGt = Struct.new(:left, :right) do
      def operands = [left, right]
      def effects = Effects.new(Eff::Empty, Eff::Empty)
    end

    # Generic method send (unspecialized)
    Send = Struct.new(:recv, :method_name, :args, :state) do
      def operands = [recv, *args, state].compact
      def effects = Effects.new(Eff::Any, Eff::Any)
    end

    # Return from function
    Return = Struct.new(:val) do
      def operands = [val]
      def effects = Effects.new(Eff::Empty, Eff::Control)
    end

    # Unconditional jump
    Jump = Struct.new(:target) do
      def operands = target.args.dup
      def effects = Effects.new(Eff::Empty, Eff::Control)
    end

    # Conditional branches
    IfTrue = Struct.new(:val, :target) do
      def operands = [val, *target.args]
      def effects = Effects.new(Eff::Empty, Eff::Control)
    end

    IfFalse = Struct.new(:val, :target) do
      def operands = [val, *target.args]
      def effects = Effects.new(Eff::Empty, Eff::Control)
    end

    # Putself — load self
    PutSelf = Struct.new(:placeholder) do
      def operands = []
      def effects = Effects.new(Eff::Empty, Eff::Empty)
    end
  end

  # ─── Basic Block ───────────────────────────────────────────────────

  class Block
    attr_reader :id, :insns, :params

    def initialize(id)
      @id = id
      @insns = []   # array of InsnId
      @params = []  # array of InsnId (block parameters)
    end

    def add_param(insn_id)
      @params << insn_id
    end

    def push(insn_id)
      @insns << insn_id
    end
  end

  # ─── Function (the whole HIR graph) ────────────────────────────────

  class Function
    attr_reader :blocks, :insns, :types, :name

    def initialize(name = "test")
      @name = name
      @blocks = []
      @insns = []       # flat array: InsnId.id -> Insn struct
      @types = []       # flat array: InsnId.id -> Type
      @insn_block = []  # flat array: InsnId.id -> BlockId
    end

    def new_block
      id = BlockId.new(@blocks.size)
      block = Block.new(id)
      @blocks << block
      id
    end

    def push_insn(block_id, insn, type = Types::Any)
      id = InsnId.new(@insns.size)
      @insns << insn
      @types << type
      @insn_block << block_id
      block = @blocks[block_id.id]
      if insn.is_a?(Insn::Param)
        block.add_param(id)
      else
        block.push(id)
      end
      id
    end

    def insn_for(insn_id)  = @insns[insn_id.id]
    def type_of(insn_id)   = @types[insn_id.id]
    def set_type(insn_id, type) = @types[insn_id.id] = type
    def block_for(insn_id) = @insn_block[insn_id.id]

    def replace_insn(insn_id, new_insn)
      @insns[insn_id.id] = new_insn
    end

    # Replace all uses of `old_id` with `new_id` across the entire function
    def replace_uses(old_id, new_id)
      @insns.each_with_index do |insn, idx|
        next unless insn
        replace_operands(insn, old_id, new_id)
      end
    end

    # Walk all reachable blocks in RPO-ish order starting from bb0
    def each_block_rpo(&blk)
      visited = Set.new
      worklist = [@blocks[0]&.id].compact
      order = []
      while (bid = worklist.shift)
        next if visited.include?(bid)
        visited << bid
        order << bid
        block = @blocks[bid.id]
        block.insns.each do |iid|
          insn = insn_for(iid)
          case insn
          when Insn::Jump    then worklist << insn.target.target
          when Insn::IfTrue  then worklist << insn.target.target
          when Insn::IfFalse then worklist << insn.target.target
          end
        end
      end
      order.each(&blk)
    end

    # ─── Printer ───────────────────────────────────────────────────

    def to_s
      out = +"fn #{@name}:\n"
      each_block_rpo do |bid|
        block = @blocks[bid.id]
        params_str = if block.params.empty?
          ""
        else
          "(#{block.params.map { |p| "#{p}:#{type_of(p)}" }.join(", ")})"
        end
        out << "#{bid}#{params_str}:\n"
        block.insns.each do |iid|
          insn = insn_for(iid)
          next if insn.is_a?(Insn::Snapshot) # hide snapshots like real ZJIT does in clean output
          line = format_insn(iid, insn)
          out << "  #{line}\n"
        end
      end
      out
    end

    private

    def format_insn(iid, insn)
      type = type_of(iid)
      prefix = "#{iid}:#{type} = "

      case insn
      when Insn::Param
        "#{prefix}Param[#{insn.idx}]"
      when Insn::Const
        "#{prefix}Const #{insn.val.inspect}"
      when Insn::PutSelf
        "#{prefix}PutSelf"
      when Insn::GuardType
        "#{prefix}GuardType #{insn.val}, #{insn.guard_type}"
      when Insn::RefineType
        "#{prefix}RefineType #{insn.val}, #{insn.new_type}"
      when Insn::Test
        "#{prefix}Test #{insn.val}"
      when Insn::FixnumAdd
        "#{prefix}FixnumAdd #{insn.left}, #{insn.right}"
      when Insn::FixnumSub
        "#{prefix}FixnumSub #{insn.left}, #{insn.right}"
      when Insn::FixnumMult
        "#{prefix}FixnumMult #{insn.left}, #{insn.right}"
      when Insn::FixnumLt
        "#{prefix}FixnumLt #{insn.left}, #{insn.right}"
      when Insn::FixnumEq
        "#{prefix}FixnumEq #{insn.left}, #{insn.right}"
      when Insn::FixnumGt
        "#{prefix}FixnumGt #{insn.left}, #{insn.right}"
      when Insn::Send
        args_s = insn.args.map(&:to_s).join(", ")
        "#{prefix}Send #{insn.recv}, :#{insn.method_name}#{args_s.empty? ? "" : ", #{args_s}"}"
      when Insn::Return
        "Return #{insn.val}"
      when Insn::Jump
        "Jump #{insn.target}"
      when Insn::IfTrue
        "IfTrue #{insn.val}, #{insn.target}"
      when Insn::IfFalse
        "IfFalse #{insn.val}, #{insn.target}"
      else
        "#{prefix}Unknown"
      end
    end

    def replace_operands(insn, old_id, new_id)
      case insn
      when Insn::GuardType
        insn.val = new_id if insn.val == old_id
        insn.state = new_id if insn.state == old_id
      when Insn::RefineType
        insn.val = new_id if insn.val == old_id
      when Insn::Test
        insn.val = new_id if insn.val == old_id
      when Insn::FixnumAdd, Insn::FixnumSub, Insn::FixnumMult
        insn.left = new_id if insn.left == old_id
        insn.right = new_id if insn.right == old_id
        insn.state = new_id if insn.state == old_id
      when Insn::FixnumLt, Insn::FixnumEq, Insn::FixnumGt
        insn.left = new_id if insn.left == old_id
        insn.right = new_id if insn.right == old_id
      when Insn::Send
        insn.recv = new_id if insn.recv == old_id
        insn.args.map! { |a| a == old_id ? new_id : a }
        insn.state = new_id if insn.state == old_id
      when Insn::Return
        insn.val = new_id if insn.val == old_id
      when Insn::Jump
        insn.target.args.map! { |a| a == old_id ? new_id : a }
      when Insn::IfTrue, Insn::IfFalse
        insn.val = new_id if insn.val == old_id
        insn.target.args.map! { |a| a == old_id ? new_id : a }
      end
    end
  end

  # ═══════════════════════════════════════════════════════════════════
  # ISeq → HIR Compiler
  # ═══════════════════════════════════════════════════════════════════

  class Compiler
    # Profile data: maps method+arg_index to observed type
    # In real ZJIT this comes from runtime profiling; here we simulate it.
    attr_accessor :profiles

    def initialize
      @profiles = {}  # { [method_name, arg_idx] => Type }
    end

    def compile(iseq)
      body = iseq.to_a
      name = body[5] # method name
      insns = body[13]  # instruction list

      fun = Function.new(name.to_s)

      # Pre-scan: find jump targets to determine block boundaries
      targets = Set.new
      label_map = {} # label -> index in insns array
      insns.each_with_index do |insn, idx|
        if insn.is_a?(Symbol) && insn.to_s.start_with?("label_")
          label_map[insn] = idx
        end
        if insn.is_a?(::Array)
          case insn[0]
          when :branchif, :branchunless, :jump
            targets << insn[1]
          end
        end
      end

      # Create blocks: bb0 is the entry block
      entry_block = fun.new_block

      # Map labels to block IDs
      block_map = {}
      targets.each do |label|
        block_map[label] = fun.new_block
      end

      # Also create blocks for fall-through after branches
      fallthrough_labels = {}
      insns.each_with_index do |insn, idx|
        next unless insn.is_a?(::Array)
        case insn[0]
        when :branchif, :branchunless
          # The instruction after the branch needs a block if it's not already a label
          next_idx = idx + 1
          while next_idx < insns.size && (insns[next_idx].is_a?(Symbol) || insns[next_idx].is_a?(Integer))
            next_idx += 1
          end
          # Synthesize a fallthrough label
          ft_key = :"fallthrough_#{next_idx}"
          unless block_map[ft_key] || insns[idx + 1].is_a?(Symbol) && block_map[insns[idx + 1]]
            block_map[ft_key] = fun.new_block
            fallthrough_labels[next_idx] = ft_key
          end
        end
      end

      # ── Compile instructions ──

      current_block = entry_block
      stack = []       # operand stack of InsnId
      locals = {}      # local_idx => InsnId
      param_count = iseq.to_a[4][:arg_size]

      # Create self param
      self_val = fun.push_insn(current_block, Insn::PutSelf.new(nil), Types::BasicObject)

      # Create method params as block params
      local_table = body[10] # local variable table
      params = []
      param_count.times do |i|
        p = fun.push_insn(current_block, Insn::Param.new(i), Types::BasicObject)
        locals[local_table.size - 1 - i] = p
        params << p
      end

      # Create initial snapshot
      snap = fun.push_insn(current_block, Insn::Snapshot.new(locals.dup, stack.dup), Types::Any)

      insn_index = 0
      while insn_index < insns.size
        raw = insns[insn_index]
        insn_index += 1

        # Handle labels
        if raw.is_a?(Symbol) && raw.to_s.start_with?("label_")
          if block_map[raw] && block_map[raw] != current_block
            target_block = block_map[raw]
            # Transfer state via block params
            args = transfer_state(fun, target_block, stack, locals, local_table, self_val)
            fun.push_insn(current_block, Insn::Jump.new(BranchEdge.new(target_block, args)))
            # Set up receiving params in target block
            stack, locals, self_val = receive_state(fun, target_block, stack, locals, local_table)
            current_block = target_block
            snap = fun.push_insn(current_block, Insn::Snapshot.new(locals.dup, stack.dup), Types::Any)
          end
          next
        end

        # Handle fall-through labels
        if fallthrough_labels[insn_index - 1]
          ft = fallthrough_labels[insn_index - 1]
          if block_map[ft] && block_map[ft] != current_block
            # Already jumped, set up new block
          end
        end

        next unless raw.is_a?(::Array)
        op = raw[0]

        case op
        when :putnil
          id = fun.push_insn(current_block, Insn::Const.new(nil), Types::NilClass.with_const(nil))
          stack.push(id)

        when :putobject
          val = raw[1]
          type = type_for_value(val)
          id = fun.push_insn(current_block, Insn::Const.new(val), type)
          stack.push(id)

        when :putobject_INT2FIX_0_
          id = fun.push_insn(current_block, Insn::Const.new(0), Types::Fixnum.with_const(0))
          stack.push(id)

        when :putobject_INT2FIX_1_
          id = fun.push_insn(current_block, Insn::Const.new(1), Types::Fixnum.with_const(1))
          stack.push(id)

        when :putself
          stack.push(self_val)

        when :putstring, :putchilledstring
          val = raw[1]
          id = fun.push_insn(current_block, Insn::Const.new(val), Types::String)
          stack.push(id)

        when :getlocal_WC_0
          local_idx = raw[1]
          val = locals[local_idx] || fun.push_insn(current_block, Insn::Const.new(nil), Types::NilClass.with_const(nil))
          stack.push(val)

        when :setlocal_WC_0
          local_idx = raw[1]
          locals[local_idx] = stack.pop

        when :dup
          stack.push(stack.last)

        when :pop
          stack.pop

        when :swap
          a, b = stack.pop, stack.pop
          stack.push(a)
          stack.push(b)

        when :opt_plus, :opt_minus, :opt_mult, :opt_lt, :opt_eq, :opt_gt
          right = stack.pop
          left = stack.pop
          snap = fun.push_insn(current_block, Insn::Snapshot.new(locals.dup, stack.dup), Types::Any)

          rtype = fun.type_of(right)
          ltype = fun.type_of(left)

          result = if ltype.fixnum? && rtype.fixnum?
            # Specialize to fixnum operations
            gl = fun.push_insn(current_block, Insn::GuardType.new(left, Types::Fixnum, snap), Types::Fixnum)
            gr = fun.push_insn(current_block, Insn::GuardType.new(right, Types::Fixnum, snap), Types::Fixnum)

            case op
            when :opt_plus  then fun.push_insn(current_block, Insn::FixnumAdd.new(gl, gr, snap), Types::Fixnum)
            when :opt_minus then fun.push_insn(current_block, Insn::FixnumSub.new(gl, gr, snap), Types::Fixnum)
            when :opt_mult  then fun.push_insn(current_block, Insn::FixnumMult.new(gl, gr, snap), Types::Fixnum)
            when :opt_lt    then fun.push_insn(current_block, Insn::FixnumLt.new(gl, gr), Types::CBool)
            when :opt_eq    then fun.push_insn(current_block, Insn::FixnumEq.new(gl, gr), Types::CBool)
            when :opt_gt    then fun.push_insn(current_block, Insn::FixnumGt.new(gl, gr), Types::CBool)
            end
          else
            # Generic send
            method = { opt_plus: :+, opt_minus: :-, opt_mult: :*, opt_lt: :<, opt_eq: :==, opt_gt: :> }[op]
            fun.push_insn(current_block, Insn::Send.new(left, method, [right], snap), Types::BasicObject)
          end
          stack.push(result)

        when :opt_send_without_block
          ci = raw[1]
          mid = ci[:mid]
          argc = ci[:orig_argc]
          args = argc.times.map { stack.pop }.reverse
          recv = stack.pop
          snap = fun.push_insn(current_block, Insn::Snapshot.new(locals.dup, stack.dup), Types::Any)
          result = fun.push_insn(current_block, Insn::Send.new(recv, mid, args, snap), Types::BasicObject)
          stack.push(result)

        when :leave
          val = stack.pop
          fun.push_insn(current_block, Insn::Return.new(val))
          break if insn_index >= insns.size

        when :jump
          label = raw[1]
          target_block = block_map[label]
          args = transfer_state(fun, target_block, stack, locals, local_table, self_val)
          fun.push_insn(current_block, Insn::Jump.new(BranchEdge.new(target_block, args)))
          stack, locals, self_val = receive_state(fun, target_block, stack, locals, local_table)
          # Find next non-label instruction's block
          current_block = find_next_block(insns, insn_index, block_map, fallthrough_labels) || current_block
          snap = fun.push_insn(current_block, Insn::Snapshot.new(locals.dup, stack.dup), Types::Any)

        when :branchif, :branchunless
          label = raw[1]
          val = stack.pop
          test_id = fun.push_insn(current_block, Insn::Test.new(val), Types::CBool)

          target_block = block_map[label]
          target_args = transfer_state(fun, target_block, stack, locals, local_table, self_val)

          # Create fall-through block
          ft_key = fallthrough_labels[insn_index]
          ft_block = ft_key ? block_map[ft_key] : fun.new_block
          ft_args = transfer_state(fun, ft_block, stack, locals, local_table, self_val)

          if op == :branchif
            fun.push_insn(current_block, Insn::IfTrue.new(test_id, BranchEdge.new(target_block, target_args)))
          else
            fun.push_insn(current_block, Insn::IfFalse.new(test_id, BranchEdge.new(target_block, target_args)))
          end

          fun.push_insn(current_block, Insn::Jump.new(BranchEdge.new(ft_block, ft_args)))

          # Set up receiving state for fall-through
          stack, locals, self_val = receive_state(fun, ft_block, stack, locals, local_table)
          current_block = ft_block
          snap = fun.push_insn(current_block, Insn::Snapshot.new(locals.dup, stack.dup), Types::Any)

          # Also set up receiving for the target block (it will be filled when we reach the label)
          receive_state_if_needed(fun, target_block)

        when :nop, :trace
          # skip

        else
          # Unhandled opcode — emit as a comment-like const
          id = fun.push_insn(current_block, Insn::Const.new(:"unhandled_#{op}"), Types::Any)
          # Manage stack effect conservatively
        end
      end

      fun
    end

    private

    def type_for_value(val)
      case val
      when Integer then Types::Fixnum.with_const(val)
      when Float   then Types::Float
      when NilClass then Types::NilClass.with_const(nil)
      when TrueClass then Types::TrueClass
      when FalseClass then Types::FalseClass
      when String then Types::String
      when Symbol then Types::Object
      else Types::BasicObject
      end
    end

    def transfer_state(fun, target_block, stack, locals, local_table, self_val)
      # Build args: [self, *locals_in_order, *stack]
      args = [self_val]
      local_table.size.times { |i| args << (locals[i] || nil) }
      args.concat(stack)
      args.compact
    end

    def receive_state(fun, target_block, stack, locals, local_table)
      block = fun.blocks[target_block.id]
      return [stack, locals, nil] unless block.params.empty?

      # Create params for the incoming state
      new_self = fun.push_insn(target_block, Insn::Param.new(:self), Types::BasicObject)
      new_locals = {}
      local_table.size.times do |i|
        if locals[i]
          type = fun.type_of(locals[i])
          new_locals[i] = fun.push_insn(target_block, Insn::Param.new(i), type)
        end
      end
      new_stack = stack.map.with_index do |s, i|
        fun.push_insn(target_block, Insn::Param.new(:"stack_#{i}"), fun.type_of(s))
      end
      [new_stack, new_locals, new_self]
    end

    def receive_state_if_needed(fun, target_block)
      block = fun.blocks[target_block.id]
      # Already has params, skip
    end

    def find_next_block(insns, idx, block_map, fallthrough_labels)
      while idx < insns.size
        if insns[idx].is_a?(Symbol) && block_map[insns[idx]]
          return block_map[insns[idx]]
        end
        if fallthrough_labels[idx]
          return block_map[fallthrough_labels[idx]]
        end
        idx += 1
      end
      nil
    end
  end

  # ═══════════════════════════════════════════════════════════════════
  # Optimization Passes
  # ═══════════════════════════════════════════════════════════════════

  module Passes
    # ── Constant Folding ─────────────────────────────────────────────
    # If both operands are constants, compute the result at compile time.

    def self.fold_constants(fun)
      changed = false
      fun.each_block_rpo do |bid|
        block = fun.blocks[bid.id]
        block.insns.each do |iid|
          insn = fun.insn_for(iid)
          case insn
          when Insn::FixnumAdd, Insn::FixnumSub, Insn::FixnumMult
            lt = fun.type_of(insn.left)
            rt = fun.type_of(insn.right)
            if lt.has_const? && rt.has_const? && lt.fixnum? && rt.fixnum?
              result = case insn
                       when Insn::FixnumAdd  then lt.const_val + rt.const_val
                       when Insn::FixnumSub  then lt.const_val - rt.const_val
                       when Insn::FixnumMult then lt.const_val * rt.const_val
                       end
              fun.replace_insn(iid, Insn::Const.new(result))
              fun.set_type(iid, Types::Fixnum.with_const(result))
              changed = true
            end
          when Insn::FixnumLt, Insn::FixnumEq, Insn::FixnumGt
            lt = fun.type_of(insn.left)
            rt = fun.type_of(insn.right)
            if lt.has_const? && rt.has_const? && lt.fixnum? && rt.fixnum?
              result = case insn
                       when Insn::FixnumLt then lt.const_val < rt.const_val
                       when Insn::FixnumEq then lt.const_val == rt.const_val
                       when Insn::FixnumGt then lt.const_val > rt.const_val
                       end
              # Fold to a Ruby boolean constant
              type = result ? Types::TrueClass : Types::FalseClass
              fun.replace_insn(iid, Insn::Const.new(result))
              fun.set_type(iid, type)
              changed = true
            end
          end
        end
      end
      changed
    end

    # ── Type Propagation ─────────────────────────────────────────────
    # After GuardType, narrow the type of the guarded value.
    # After RefineType, intersect types.

    def self.propagate_types(fun)
      changed = false
      fun.each_block_rpo do |bid|
        block = fun.blocks[bid.id]
        block.insns.each do |iid|
          insn = fun.insn_for(iid)
          case insn
          when Insn::GuardType
            # The output of GuardType is at least as narrow as the guard
            src_type = fun.type_of(insn.val)
            guard = insn.guard_type
            narrowed = src_type & guard
            if fun.type_of(iid) != narrowed
              fun.set_type(iid, narrowed)
              changed = true
            end
            # If source is already the guarded type, the guard is redundant
            if src_type <= guard
              fun.replace_insn(iid, Insn::RefineType.new(insn.val, guard))
              fun.set_type(iid, narrowed)
              changed = true
            end
          when Insn::RefineType
            src_type = fun.type_of(insn.val)
            narrowed = src_type & insn.new_type
            if fun.type_of(iid) != narrowed
              fun.set_type(iid, narrowed)
              changed = true
            end
          when Insn::FixnumAdd, Insn::FixnumSub, Insn::FixnumMult
            if fun.type_of(iid) != Types::Fixnum
              fun.set_type(iid, Types::Fixnum)
              changed = true
            end
          end
        end
      end
      changed
    end

    # ── Eliminate Redundant Guards ───────────────────────────────────
    # If we already know a value is Fixnum, don't guard it again.

    def self.eliminate_redundant_guards(fun)
      changed = false
      fun.each_block_rpo do |bid|
        block = fun.blocks[bid.id]
        block.insns.each do |iid|
          insn = fun.insn_for(iid)
          next unless insn.is_a?(Insn::GuardType)

          src_type = fun.type_of(insn.val)
          if src_type <= insn.guard_type
            # Guard is redundant — replace with a no-op RefineType
            fun.replace_insn(iid, Insn::RefineType.new(insn.val, insn.guard_type))
            fun.set_type(iid, src_type)
            fun.replace_uses(iid, insn.val)
            changed = true
          end
        end
      end
      changed
    end

    # ── Dead Code Elimination ────────────────────────────────────────
    # Remove instructions whose results are unused and that have no side effects.

    def self.eliminate_dead_code(fun)
      # Build use counts
      used = Hash.new(0)
      fun.blocks.each do |block|
        (block.params + block.insns).each do |iid|
          insn = fun.insn_for(iid)
          next unless insn
          insn.operands.each { |op| used[op] += 1 if op.is_a?(InsnId) }
        end
      end

      changed = false
      fun.blocks.each do |block|
        block.insns.reject! do |iid|
          insn = fun.insn_for(iid)
          next false unless insn
          if used[iid] == 0 && insn.effects.elidable?
            fun.replace_insn(iid.id, nil) if iid.id.is_a?(Integer)
            changed = true
            true
          else
            false
          end
        end
      end
      changed
    end

    # ── Run All Passes ───────────────────────────────────────────────

    def self.optimize!(fun)
      max_iters = 10
      max_iters.times do
        changed = false
        changed |= propagate_types(fun)
        changed |= fold_constants(fun)
        changed |= eliminate_redundant_guards(fun)
        changed |= eliminate_dead_code(fun)
        break unless changed
      end
      fun
    end
  end

  # ═══════════════════════════════════════════════════════════════════
  # Public API
  # ═══════════════════════════════════════════════════════════════════

  def self.compile(iseq)
    compiler = Compiler.new
    compiler.compile(iseq)
  end

  def self.compile_and_optimize(iseq)
    fun = compile(iseq)
    Passes.optimize!(fun)
    fun
  end

  # Convenience: compile a string of Ruby code
  def self.hir(code, optimize: true)
    iseq = RubyVM::InstructionSequence.compile(code)
    fun = compile(iseq)
    Passes.optimize!(fun) if optimize
    fun.to_s
  end
end

# ═══════════════════════════════════════════════════════════════════════
# Tests
# ═══════════════════════════════════════════════════════════════════════

if $0 == __FILE__
  if ARGV.include?("--demo")
    puts "═══ MiniZJIT Demo ═══\n\n"

    code = "def add(a, b) = a + b"
    puts "Source: #{code}"
    iseq = RubyVM::InstructionSequence.compile(code + "; add(1, 2)")
    # Compile just the method
    method_iseq = RubyVM::InstructionSequence.compile("def add(a, b); a + b; end").to_a[13]
      .find { |i| i.is_a?(::Array) && i[0] == :definemethod }&.then { |i| i[2] }

    if method_iseq
      puts "\n── Before optimization ──"
      fun = MiniZJIT.compile(method_iseq)
      puts fun

      puts "── After optimization ──"
      MiniZJIT::Passes.optimize!(fun)
      puts fun
    else
      # Fallback: compile the expression directly
      puts "\n── Compiling expression: 1 + 2 ──"
      puts MiniZJIT.hir("1 + 2")
    end

    puts "\n── Branching: if x > 0 then x else -x end ──"
    puts MiniZJIT.hir("x = 1; if x > 0 then x else 0 - x end")

    puts "\n── Constant folding: 2 * 3 + 4 ──"
    puts MiniZJIT.hir("2 * 3 + 4")

    exit
  end

  require "minitest/autorun"

  class MiniZJITTest < Minitest::Test
    # ── Type Lattice Tests ──────────────────────────────────────────

    def test_type_subtype
      assert MiniZJIT::Types::Fixnum <= MiniZJIT::Types::BasicObject
      assert MiniZJIT::Types::Fixnum <= MiniZJIT::Types::Any
      assert MiniZJIT::Types::Empty <= MiniZJIT::Types::Fixnum
      refute MiniZJIT::Types::BasicObject <= MiniZJIT::Types::Fixnum
    end

    def test_type_meet
      t = MiniZJIT::Types::Fixnum & MiniZJIT::Types::BasicObject
      assert_equal MiniZJIT::Types::Fixnum, t
    end

    def test_type_const_display
      t = MiniZJIT::Types::Fixnum.with_const(42)
      assert_equal "Fixnum[42]", t.to_s
    end

    # ── Effects Tests ───────────────────────────────────────────────

    def test_const_is_pure
      c = MiniZJIT::Insn::Const.new(1)
      assert c.effects.pure?
      assert c.effects.elidable?
    end

    def test_guard_is_not_elidable
      g = MiniZJIT::Insn::GuardType.new(nil, nil, nil)
      refute g.effects.elidable?
    end

    def test_send_has_any_effects
      s = MiniZJIT::Insn::Send.new(nil, :foo, [], nil)
      assert_equal MiniZJIT::Eff::Any, s.effects.read
      assert_equal MiniZJIT::Eff::Any, s.effects.write
    end

    # ── HIR Compilation Tests ───────────────────────────────────────

    def test_compile_constant
      hir = MiniZJIT.hir("1", optimize: false)
      assert_includes hir, "Const 1"
      assert_includes hir, "Fixnum[1]"
    end

    def test_compile_nil
      hir = MiniZJIT.hir("nil", optimize: false)
      assert_includes hir, "Const nil"
      assert_includes hir, "NilClass"
    end

    def test_compile_addition
      hir = MiniZJIT.hir("1 + 2", optimize: false)
      assert_includes hir, "Const 1"
      assert_includes hir, "Const 2"
      # Should specialize since both are fixnum constants
      assert_includes hir, "GuardType"
      assert_includes hir, "FixnumAdd"
    end

    def test_compile_subtraction
      hir = MiniZJIT.hir("5 - 3", optimize: false)
      assert_includes hir, "FixnumSub"
    end

    def test_compile_return
      hir = MiniZJIT.hir("42", optimize: false)
      assert_includes hir, "Return"
    end

    def test_compile_comparison
      hir = MiniZJIT.hir("1 < 2", optimize: false)
      assert_includes hir, "FixnumLt"
    end

    # ── Optimization: Constant Folding ──────────────────────────────

    def test_fold_addition
      hir = MiniZJIT.hir("1 + 2", optimize: true)
      assert_includes hir, "Fixnum[3]"
      assert_includes hir, "Const 3"
      # The FixnumAdd should be folded away
      refute_includes hir, "FixnumAdd"
    end

    def test_fold_multiplication
      hir = MiniZJIT.hir("3 * 4", optimize: true)
      assert_includes hir, "Fixnum[12]"
      refute_includes hir, "FixnumMult"
    end

    def test_fold_comparison
      hir = MiniZJIT.hir("1 < 2", optimize: true)
      assert_includes hir, "Const true"
      refute_includes hir, "FixnumLt"
    end

    def test_fold_nested_arithmetic
      hir = MiniZJIT.hir("2 * 3 + 4", optimize: true)
      # 2*3=6, 6+4=10
      assert_includes hir, "Fixnum[10]"
      assert_includes hir, "Const 10"
    end

    # ── Optimization: Guard Elimination ─────────────────────────────

    def test_redundant_guard_eliminated
      hir = MiniZJIT.hir("1 + 2", optimize: true)
      # After folding, the guards on constants should be eliminated
      # (constants already have Fixnum type)
      refute_includes hir, "GuardType"
    end

    # ── Optimization: Dead Code Elimination ─────────────────────────

    def test_dead_const_eliminated
      # When constant folding replaces FixnumAdd, the original
      # Const operands become dead if only used by the folded insn
      hir_before = MiniZJIT.hir("1 + 2", optimize: false)
      hir_after  = MiniZJIT.hir("1 + 2", optimize: true)
      # Optimized version should be shorter
      assert hir_after.lines.size <= hir_before.lines.size,
        "Expected optimized HIR to be no longer than unoptimized"
    end

    # ── Snapshot-style assertions (like real ZJIT) ──────────────────

    def assert_hir(code, expected, optimize: true)
      actual = MiniZJIT.hir(code, optimize: optimize)
      expected = expected.gsub(/^ +/, "").strip
      actual = actual.strip
      assert_equal expected, actual, "HIR mismatch for: #{code}"
    end

    def test_snapshot_constant_fold
      assert_hir "1 + 2", <<~HIR, optimize: true
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v5:Fixnum[3] = Const 3
          Return v5
      HIR
    end

    def test_snapshot_multiply_fold
      assert_hir "3 * 4", <<~HIR, optimize: true
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v5:Fixnum[12] = Const 12
          Return v5
      HIR
    end

    def test_snapshot_nested_fold
      assert_hir "2 * 3 + 4", <<~HIR, optimize: true
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v10:Fixnum[10] = Const 10
          Return v10
      HIR
    end

    def test_snapshot_comparison_fold
      assert_hir "1 < 2", <<~HIR, optimize: true
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v5:TrueClass = Const true
          Return v5
      HIR
    end

    def test_snapshot_no_optimize
      assert_hir "1 + 2", <<~HIR, optimize: false
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v2:Fixnum[2] = Const 2
          v4:Fixnum = GuardType v1, Fixnum
          v5:Fixnum = GuardType v2, Fixnum
          v6:Fixnum = FixnumAdd v4, v5
          Return v6
      HIR
    end
  end
end
