# frozen_string_literal: true

# mini_zjit.rb — A single-file demo of ZJIT's HIR compilation pipeline.
# Takes a RubyVM::InstructionSequence and produces typed SSA in HIR form,
# then runs optimization passes over it.
#
# Requires Ruby 3.x+ (for RubyVM::InstructionSequence and endless methods)
#
# Usage:
#   ruby mini_zjit.rb              # run built-in tests
#   ruby mini_zjit.rb --demo       # run interactive demo

require "set"

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

  RUBY_TYPES = %i[Fixnum Float String Array NilClass TrueClass FalseClass Object].freeze

  class Type
    attr_reader :name, :const_val

    def initialize(name, const_val = :none)
      @name = name
      @const_val = const_val
    end

    def with_const(val) = Type.new(@name, val)
    def has_const?      = @const_val != :none
    def fixnum?         = @name == :Fixnum
    def nilclass?       = @name == :NilClass
    def empty?          = @name == :Empty
    def any?            = @name == :Any
    def cbool?          = @name == :CBool
    def basic_object?   = @name == :BasicObject

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
      s += "[#{@const_val.inspect}]" if has_const?
      s
    end

    def ==(other) = other.is_a?(Type) && @name == other.name && @const_val == other.const_val
    def eql?(other) = self == other
    def hash = [@name, @const_val].hash
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
    Param = Struct.new(:idx) do
      def operands = []
      def effects  = Effects.new(Eff::Empty, Eff::Empty)
    end

    Const = Struct.new(:val) do
      def operands = []
      def effects  = Effects.new(Eff::Empty, Eff::Empty)
    end

    Snapshot = Struct.new(:locals, :stack) do
      def operands = locals.values.compact + stack.compact
      def effects  = Effects.new(Eff::Empty, Eff::Empty)
    end

    GuardType = Struct.new(:val, :guard_type, :state) do
      def operands = [val, state].compact
      def effects  = Effects.new(Eff::Empty, Eff::Control)
    end

    RefineType = Struct.new(:val, :new_type) do
      def operands = [val]
      def effects  = Effects.new(Eff::Empty, Eff::Empty)
    end

    Test = Struct.new(:val) do
      def operands = [val]
      def effects  = Effects.new(Eff::Empty, Eff::Empty)
    end

    FixnumAdd = Struct.new(:left, :right, :state) do
      def operands = [left, right, state].compact
      def effects  = Effects.new(Eff::Empty, Eff::Control)
    end

    FixnumSub = Struct.new(:left, :right, :state) do
      def operands = [left, right, state].compact
      def effects  = Effects.new(Eff::Empty, Eff::Control)
    end

    FixnumMult = Struct.new(:left, :right, :state) do
      def operands = [left, right, state].compact
      def effects  = Effects.new(Eff::Empty, Eff::Control)
    end

    FixnumLt = Struct.new(:left, :right) do
      def operands = [left, right]
      def effects  = Effects.new(Eff::Empty, Eff::Empty)
    end

    FixnumEq = Struct.new(:left, :right) do
      def operands = [left, right]
      def effects  = Effects.new(Eff::Empty, Eff::Empty)
    end

    FixnumGt = Struct.new(:left, :right) do
      def operands = [left, right]
      def effects  = Effects.new(Eff::Empty, Eff::Empty)
    end

    Send = Struct.new(:recv, :method_name, :args, :state) do
      def operands = [recv, *args, state].compact
      def effects  = Effects.new(Eff::Any, Eff::Any)
    end

    Return = Struct.new(:val) do
      def operands = [val]
      def effects  = Effects.new(Eff::Empty, Eff::Control)
    end

    Jump = Struct.new(:target) do
      def operands = target.args.dup
      def effects  = Effects.new(Eff::Empty, Eff::Control)
    end

    IfTrue = Struct.new(:val, :target) do
      def operands = [val, *target.args]
      def effects  = Effects.new(Eff::Empty, Eff::Control)
    end

    IfFalse = Struct.new(:val, :target) do
      def operands = [val, *target.args]
      def effects  = Effects.new(Eff::Empty, Eff::Control)
    end

    PutSelf = Struct.new(:placeholder) do
      def operands = []
      def effects  = Effects.new(Eff::Empty, Eff::Control)
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
    def block_for(insn_id) = @insn_block[insn_id.id]

    def set_type(insn_id, type)
      @types[insn_id.id] = type
    end

    def replace_insn(insn_id, new_insn)
      @insns[insn_id.id] = new_insn
    end

    # Replace all uses of `old_id` with `new_id` across the entire function
    def replace_uses(old_id, new_id)
      @insns.each do |insn|
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
          next unless insn
          next if insn.is_a?(Insn::Snapshot)
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
      when Insn::Param     then "#{prefix}Param[#{insn.idx}]"
      when Insn::Const     then "#{prefix}Const #{insn.val.inspect}"
      when Insn::PutSelf   then "#{prefix}PutSelf"
      when Insn::GuardType then "#{prefix}GuardType #{insn.val}, #{insn.guard_type}"
      when Insn::RefineType then "#{prefix}RefineType #{insn.val}, #{insn.new_type}"
      when Insn::Test       then "#{prefix}Test #{insn.val}"
      when Insn::FixnumAdd  then "#{prefix}FixnumAdd #{insn.left}, #{insn.right}"
      when Insn::FixnumSub  then "#{prefix}FixnumSub #{insn.left}, #{insn.right}"
      when Insn::FixnumMult then "#{prefix}FixnumMult #{insn.left}, #{insn.right}"
      when Insn::FixnumLt   then "#{prefix}FixnumLt #{insn.left}, #{insn.right}"
      when Insn::FixnumEq   then "#{prefix}FixnumEq #{insn.left}, #{insn.right}"
      when Insn::FixnumGt   then "#{prefix}FixnumGt #{insn.left}, #{insn.right}"
      when Insn::Send
        args_s = insn.args.map(&:to_s).join(", ")
        "#{prefix}Send #{insn.recv}, :#{insn.method_name}#{args_s.empty? ? "" : ", #{args_s}"}"
      when Insn::Return  then "Return #{insn.val}"
      when Insn::Jump    then "Jump #{insn.target}"
      when Insn::IfTrue  then "IfTrue #{insn.val}, #{insn.target}"
      when Insn::IfFalse then "IfFalse #{insn.val}, #{insn.target}"
      else "#{prefix}Unknown"
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
    def compile(iseq)
      body = iseq.to_a
      name = body[5].to_s
      yarv = body[13]  # instruction list

      fun = Function.new(name)

      # Pre-scan: find labels and jump targets
      label_positions = {}  # label_sym -> position in yarv array
      yarv.each_with_index do |insn, idx|
        if insn.is_a?(Symbol) && insn.to_s.start_with?("label_")
          label_positions[insn] = idx
        end
      end

      # Find all branch targets to determine block boundaries
      branch_targets = Set.new
      yarv.each do |insn|
        next unless insn.is_a?(::Array)
        case insn[0]
        when :branchif, :branchunless, :jump
          branch_targets << insn[1]
        end
      end

      # Create entry block
      entry_block = fun.new_block

      # Create blocks for branch targets
      label_to_block = {}
      branch_targets.each do |label|
        label_to_block[label] = fun.new_block
      end

      # Also need fall-through blocks after conditional branches
      need_fallthrough = {}
      yarv.each_with_index do |insn, idx|
        next unless insn.is_a?(::Array)
        case insn[0]
        when :branchif, :branchunless
          # Find next real instruction position after the branch
          nxt = idx + 1
          nxt += 1 while nxt < yarv.size && !yarv[nxt].is_a?(::Array) && !(yarv[nxt].is_a?(Symbol) && label_to_block[yarv[nxt]])
          # Check if next thing is already a label with a block
          if nxt < yarv.size && yarv[nxt].is_a?(Symbol) && label_to_block[yarv[nxt]]
            need_fallthrough[idx] = yarv[nxt]
          else
            ft_label = :"_ft_#{idx}"
            label_to_block[ft_label] = fun.new_block
            need_fallthrough[idx] = ft_label
          end
        end
      end

      # ── Emit instructions ──

      current_block = entry_block
      stack = []
      locals = {}
      param_count = body[4][:arg_size]
      local_table = body[10] || []

      # Self
      self_val = fun.push_insn(current_block, Insn::PutSelf.new(nil), Types::BasicObject)

      # Method parameters
      param_count.times do |i|
        p = fun.push_insn(current_block, Insn::Param.new(i), Types::BasicObject)
        locals[local_table.size - 1 - i] = p
      end

      terminated = false  # has current block been terminated?

      yarv.each_with_index do |raw, yarv_idx|
        # ── Handle labels ──
        if raw.is_a?(Symbol) && raw.to_s.start_with?("label_")
          if label_to_block[raw]
            target_block = label_to_block[raw]
            unless terminated
              # Fall through: jump to the target block with current state
              args = build_args(fun, stack, locals, local_table, self_val)
              fun.push_insn(current_block, Insn::Jump.new(BranchEdge.new(target_block, args)))
            end
            # Receive state in new block
            self_val, locals, stack = receive_params(fun, target_block, stack, locals, local_table)
            current_block = target_block
            terminated = false
          end
          next
        end

        next unless raw.is_a?(::Array)
        next if terminated
        op = raw[0]

        case op

        # ── Constants / Stack ──

        when :putnil
          id = fun.push_insn(current_block, Insn::Const.new(nil), Types::NilClass.with_const(nil))
          stack.push(id)

        when :putobject
          val = raw[1]
          id = fun.push_insn(current_block, Insn::Const.new(val), type_for_value(val))
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
          id = fun.push_insn(current_block, Insn::Const.new(raw[1]), Types::String)
          stack.push(id)

        when :dup
          stack.push(stack.last) if stack.last

        when :pop
          stack.pop

        when :swap
          if stack.size >= 2
            a = stack.pop
            b = stack.pop
            stack.push(a)
            stack.push(b)
          end

        when :nop, :trace
          # skip

        # ── Locals ──

        when :getlocal_WC_0
          local_idx = raw[1]
          val = locals[local_idx]
          unless val
            val = fun.push_insn(current_block, Insn::Const.new(nil), Types::NilClass.with_const(nil))
            locals[local_idx] = val
          end
          stack.push(val)

        when :setlocal_WC_0
          local_idx = raw[1]
          locals[local_idx] = stack.pop

        # ── Arithmetic / Comparison ──

        when :opt_plus, :opt_minus, :opt_mult, :opt_lt, :opt_eq, :opt_gt
          right = stack.pop
          left = stack.pop
          next unless left && right
          snap = fun.push_insn(current_block, Insn::Snapshot.new(locals.dup, stack.dup), Types::Any)

          ltype = fun.type_of(left)
          rtype = fun.type_of(right)

          result = if ltype.fixnum? && rtype.fixnum?
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
            method = { opt_plus: :+, opt_minus: :-, opt_mult: :*, opt_lt: :<, opt_eq: :==, opt_gt: :> }[op]
            fun.push_insn(current_block, Insn::Send.new(left, method, [right], snap), Types::BasicObject)
          end
          stack.push(result)

        when :opt_length, :opt_size, :opt_not, :opt_succ, :opt_empty_p
          recv = stack.pop
          next unless recv
          mid_map = { opt_length: :length, opt_size: :size, opt_not: :!, opt_succ: :succ, opt_empty_p: :empty? }
          snap = fun.push_insn(current_block, Insn::Snapshot.new(locals.dup, stack.dup), Types::Any)
          result = fun.push_insn(current_block, Insn::Send.new(recv, mid_map[op], [], snap), Types::BasicObject)
          stack.push(result)

        when :opt_send_without_block
          ci = raw[1]
          mid = ci[:mid]
          argc = ci[:orig_argc]
          args = argc.times.map { stack.pop }.reverse
          recv = stack.pop
          next unless recv
          snap = fun.push_insn(current_block, Insn::Snapshot.new(locals.dup, stack.dup), Types::Any)
          result = fun.push_insn(current_block, Insn::Send.new(recv, mid, args, snap), Types::BasicObject)
          stack.push(result)

        # ── Control Flow ──

        when :leave
          val = stack.pop
          if val
            fun.push_insn(current_block, Insn::Return.new(val))
          end
          terminated = true

        when :jump
          label = raw[1]
          target_block = label_to_block[label]
          if target_block
            args = build_args(fun, stack, locals, local_table, self_val)
            fun.push_insn(current_block, Insn::Jump.new(BranchEdge.new(target_block, args)))
          end
          terminated = true

        when :branchif, :branchunless
          label = raw[1]
          val = stack.pop
          next unless val

          test_id = fun.push_insn(current_block, Insn::Test.new(val), Types::CBool)

          target_block = label_to_block[label]
          target_args = build_args(fun, stack, locals, local_table, self_val)

          ft_label = need_fallthrough[yarv_idx]
          ft_block = label_to_block[ft_label]
          ft_args = build_args(fun, stack, locals, local_table, self_val)

          if op == :branchif
            fun.push_insn(current_block, Insn::IfTrue.new(test_id, BranchEdge.new(target_block, target_args)))
          else
            fun.push_insn(current_block, Insn::IfFalse.new(test_id, BranchEdge.new(target_block, target_args)))
          end

          if ft_block && ft_block != target_block
            fun.push_insn(current_block, Insn::Jump.new(BranchEdge.new(ft_block, ft_args)))
            self_val, locals, stack = receive_params(fun, ft_block, stack, locals, local_table)
            current_block = ft_block
            terminated = false
          end
        end
      end

      fun
    end

    private

    def type_for_value(val)
      case val
      when Integer   then Types::Fixnum.with_const(val)
      when ::Float   then Types::Float
      when NilClass  then Types::NilClass.with_const(nil)
      when TrueClass then Types::TrueClass
      when FalseClass then Types::FalseClass
      when ::String  then Types::String
      else Types::BasicObject
      end
    end

    def build_args(fun, stack, locals, local_table, self_val)
      args = [self_val]
      # Pass all live locals sorted by key for deterministic ordering
      locals.keys.sort.each { |k| args << locals[k] if locals[k] }
      args.concat(stack)
      args.compact
    end

    def receive_params(fun, block, stack, locals, local_table)
      blk = fun.blocks[block.id]
      unless blk.params.empty?
        # Already received — reconstruct state from existing params
        params = blk.params
        new_self = params[0]
        new_locals = {}
        pi = 1
        locals.keys.sort.each do |k|
          if locals[k] && pi < params.size
            new_locals[k] = params[pi]
            pi += 1
          end
        end
        new_stack = []
        while pi < params.size
          new_stack << params[pi]
          pi += 1
        end
        return [new_self, new_locals, new_stack]
      end

      # Create fresh params
      new_self = fun.push_insn(block, Insn::Param.new(:self), Types::BasicObject)
      new_locals = {}
      locals.keys.sort.each do |k|
        if locals[k]
          new_locals[k] = fun.push_insn(block, Insn::Param.new(k), fun.type_of(locals[k]))
        end
      end
      new_stack = stack.map.with_index do |s, idx|
        fun.push_insn(block, Insn::Param.new(:"stack_#{idx}"), fun.type_of(s))
      end
      [new_self, new_locals, new_stack]
    end
  end

  # ═══════════════════════════════════════════════════════════════════
  # Optimization Passes
  # ═══════════════════════════════════════════════════════════════════

  module Passes

    # ── Constant Folding ─────────────────────────────────────────────
    # If both operands of an arithmetic/comparison insn are constants,
    # replace with a Const.

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

    def self.propagate_types(fun)
      changed = false
      fun.each_block_rpo do |bid|
        block = fun.blocks[bid.id]
        block.insns.each do |iid|
          insn = fun.insn_for(iid)
          case insn
          when Insn::GuardType
            src_type = fun.type_of(insn.val)
            narrowed = src_type & insn.guard_type
            if fun.type_of(iid) != narrowed
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
          end
        end
      end
      changed
    end

    # ── Eliminate Redundant Guards ───────────────────────────────────
    # If we already know a value has the guarded type, replace with
    # a forwarding RefineType and rewrite uses.

    def self.eliminate_redundant_guards(fun)
      changed = false
      fun.each_block_rpo do |bid|
        block = fun.blocks[bid.id]
        block.insns.each do |iid|
          insn = fun.insn_for(iid)
          next unless insn.is_a?(Insn::GuardType)
          src_type = fun.type_of(insn.val)
          if src_type <= insn.guard_type
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
    # Remove instructions whose results are unused and have no side effects.

    def self.eliminate_dead_code(fun)
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
            fun.replace_insn(iid, nil)
            changed = true
            true
          else
            false
          end
        end
      end
      changed
    end

    # ── Run All Passes (fixpoint) ────────────────────────────────────

    def self.optimize!(fun)
      10.times do
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
    Compiler.new.compile(iseq)
  end

  def self.compile_and_optimize(iseq)
    fun = compile(iseq)
    Passes.optimize!(fun)
    fun
  end

  # Convenience: compile a string of Ruby source
  def self.hir(code, optimize: true)
    iseq = RubyVM::InstructionSequence.compile(code)
    fun = compile(iseq)
    Passes.optimize!(fun) if optimize
    fun.to_s
  end
end

# ═══════════════════════════════════════════════════════════════════════
# Tests & Demo
# ═══════════════════════════════════════════════════════════════════════

if $0 == __FILE__
  if ARGV.include?("--demo")
    puts "═══ MiniZJIT Demo ═══\n\n"

    puts "── Constant folding: 2 * 3 + 4 ──"
    puts MiniZJIT.hir("2 * 3 + 4")

    puts "── Before optimization: 1 + 2 ──"
    puts MiniZJIT.hir("1 + 2", optimize: false)

    puts "── After optimization: 1 + 2 ──"
    puts MiniZJIT.hir("1 + 2", optimize: true)

    puts "── Comparison: 1 < 2 ──"
    puts MiniZJIT.hir("1 < 2")

    puts "── Branching: if true then 1 else 2 end ──"
    puts MiniZJIT.hir("if true then 1 else 2 end", optimize: false)

    exit
  end

  require "minitest/autorun"

  class TypeTest < Minitest::Test
    include MiniZJIT

    def test_subtype_fixnum_under_basic_object
      assert Types::Fixnum <= Types::BasicObject
    end

    def test_subtype_fixnum_under_any
      assert Types::Fixnum <= Types::Any
    end

    def test_empty_is_bottom
      assert Types::Empty <= Types::Fixnum
      assert Types::Empty <= Types::Any
    end

    def test_basic_object_not_subtype_of_fixnum
      refute Types::BasicObject <= Types::Fixnum
    end

    def test_meet_narrows
      assert_equal Types::Fixnum, (Types::Fixnum & Types::BasicObject)
    end

    def test_type_display_with_const
      assert_equal "Fixnum[42]", Types::Fixnum.with_const(42).to_s
    end

    def test_type_display_without_const
      assert_equal "Fixnum", Types::Fixnum.to_s
    end
  end

  class EffectsTest < Minitest::Test
    include MiniZJIT

    def test_const_is_pure
      c = Insn::Const.new(1)
      assert c.effects.pure?
    end

    def test_guard_is_not_elidable
      g = Insn::GuardType.new(nil, nil, nil)
      refute g.effects.elidable?
    end

    def test_send_has_any_effects
      s = Insn::Send.new(nil, :foo, [], nil)
      assert_equal Eff::Any, s.effects.read
      assert_equal Eff::Any, s.effects.write
    end

    def test_fixnum_lt_is_pure
      assert Insn::FixnumLt.new(nil, nil).effects.pure?
    end
  end

  class HIRCompileTest < Minitest::Test
    # Assert HIR output matches expected string (like snapshot tests in Rust ZJIT)
    def assert_hir(code, expected, optimize: true)
      actual = MiniZJIT.hir(code, optimize: optimize).strip
      expected = expected.gsub(/^ {8}/, "").strip
      assert_equal expected, actual, "HIR mismatch for: #{code}"
    end

    def test_constant
      assert_hir "42", <<~HIR, optimize: false
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v1:Fixnum[42] = Const 42
          Return v1
      HIR
    end

    def test_nil
      assert_hir "nil", <<~HIR, optimize: false
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v1:NilClass[nil] = Const nil
          Return v1
      HIR
    end

    def test_addition_unoptimized
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

    def test_subtraction_unoptimized
      assert_hir "5 - 3", <<~HIR, optimize: false
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v1:Fixnum[5] = Const 5
          v2:Fixnum[3] = Const 3
          v4:Fixnum = GuardType v1, Fixnum
          v5:Fixnum = GuardType v2, Fixnum
          v6:Fixnum = FixnumSub v4, v5
          Return v6
      HIR
    end

    def test_comparison_unoptimized
      assert_hir "1 < 2", <<~HIR, optimize: false
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v2:Fixnum[2] = Const 2
          v4:Fixnum = GuardType v1, Fixnum
          v5:Fixnum = GuardType v2, Fixnum
          v6:CBool = FixnumLt v4, v5
          Return v6
      HIR
    end
  end

  class ConstantFoldingTest < Minitest::Test
    def assert_hir(code, expected, optimize: true)
      actual = MiniZJIT.hir(code, optimize: optimize).strip
      expected = expected.gsub(/^ {8}/, "").strip
      assert_equal expected, actual, "HIR mismatch for: #{code}"
    end

    def test_fold_addition
      assert_hir "1 + 2", <<~HIR
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v6:Fixnum[3] = Const 3
          Return v6
      HIR
    end

    def test_fold_subtraction
      assert_hir "10 - 3", <<~HIR
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v6:Fixnum[7] = Const 7
          Return v6
      HIR
    end

    def test_fold_multiplication
      assert_hir "3 * 4", <<~HIR
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v6:Fixnum[12] = Const 12
          Return v6
      HIR
    end

    def test_fold_comparison_true
      assert_hir "1 < 2", <<~HIR
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v6:TrueClass = Const true
          Return v6
      HIR
    end

    def test_fold_comparison_false
      assert_hir "3 < 1", <<~HIR
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v6:FalseClass = Const false
          Return v6
      HIR
    end

    def test_fold_nested_arithmetic
      # 2*3 + 4 => 6 + 4 => 10
      assert_hir "2 * 3 + 4", <<~HIR
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v12:Fixnum[10] = Const 10
          Return v12
      HIR
    end

    def test_fold_chain
      # (1 + 2) * (3 + 4) => 3 * 7 => 21
      assert_hir "(1 + 2) * (3 + 4)", <<~HIR
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v18:Fixnum[21] = Const 21
          Return v18
      HIR
    end
  end

  class GuardEliminationTest < Minitest::Test
    def assert_hir(code, expected, optimize: true)
      actual = MiniZJIT.hir(code, optimize: optimize).strip
      expected = expected.gsub(/^ {8}/, "").strip
      assert_equal expected, actual, "HIR mismatch for: #{code}"
    end

    def test_guards_eliminated_when_types_known
      # Constants are already Fixnum, so guards should be eliminated
      hir = MiniZJIT.hir("1 + 2")
      refute_includes hir, "GuardType", "Guards on known-Fixnum constants should be eliminated"
    end

    def test_guards_present_when_types_unknown
      # Parameters have BasicObject type — guards are needed
      hir = MiniZJIT.hir("1 + 2", optimize: false)
      assert_includes hir, "GuardType", "Guards should be present before optimization"
    end
  end

  class DeadCodeEliminationTest < Minitest::Test
    def test_dead_consts_removed
      before = MiniZJIT.hir("1 + 2", optimize: false)
      after  = MiniZJIT.hir("1 + 2", optimize: true)
      # Optimized should have fewer instructions
      assert after.lines.size < before.lines.size,
        "DCE should remove dead instructions\nBefore:\n#{before}\nAfter:\n#{after}"
    end

    def test_putself_kept_even_if_unused
      # PutSelf is pure but should survive because we don't track its usage perfectly
      hir = MiniZJIT.hir("42", optimize: true)
      assert_includes hir, "PutSelf"
    end
  end

  class BranchTest < Minitest::Test
    def test_branch_compiles
      hir = MiniZJIT.hir("if true then 1 else 2 end", optimize: false)
      # Should have multiple blocks and branch instructions
      assert_match(/bb\d+/, hir)
      assert(hir.include?("IfTrue") || hir.include?("IfFalse"),
        "Branch should produce IfTrue or IfFalse:\n#{hir}")
    end

    def test_branch_has_multiple_blocks
      hir = MiniZJIT.hir("if true then 1 else 2 end", optimize: false)
      blocks = hir.scan(/^bb\d+/).uniq
      assert blocks.size >= 2, "Branch should create multiple blocks:\n#{hir}"
    end
  end

  class LocalVariableTest < Minitest::Test
    def assert_hir(code, expected, optimize: true)
      actual = MiniZJIT.hir(code, optimize: optimize).strip
      expected = expected.gsub(/^ {8}/, "").strip
      assert_equal expected, actual, "HIR mismatch for: #{code}"
    end

    def test_local_assignment_and_use
      # x = 1; x + 2 should compile and fold
      assert_hir "x = 1; x + 2", <<~HIR
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v7:Fixnum[3] = Const 3
          Return v7
      HIR
    end

    def test_local_forwarding
      # x = 42; x should just return the constant
      assert_hir "x = 42; x", <<~HIR
        fn <compiled>:
        bb0():
          v0:BasicObject = PutSelf
          v1:Fixnum[42] = Const 42
          Return v1
      HIR
    end
  end

  class SendTest < Minitest::Test
    def test_send_emitted_for_unknown_types
      # String + String can't be specialized to fixnum ops
      hir = MiniZJIT.hir('"hello".length', optimize: false)
      assert_includes hir, "Send"
    end
  end
end
