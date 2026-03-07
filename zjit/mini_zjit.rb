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
  # Type Lattice — bitset-based, like real ZJIT
  #
  # Each leaf type gets one bit. Composite types are bitwise OR of their
  # children. Subtype check is (a.bits & b.bits) == a.bits. Intersection
  # is bitwise AND. An optional Specialization carries constant info.
  #
  # Bit layout (leaf types):
  #   0: Fixnum    4: NilClass    8:  Array     12: CBool
  #   1: Flonum    5: TrueClass   9:  Hash
  #   2: Bignum    6: FalseClass  10: Symbol
  #   3: String    7: Object      11: Undef
  #
  # Composite types (bitwise OR):
  #   Integer      = Fixnum | Bignum
  #   Float        = Flonum
  #   Numeric      = Integer | Float
  #   Immediate    = Fixnum | Flonum | NilClass | TrueClass | FalseClass | Symbol
  #   Falsy        = NilClass | FalseClass
  #   BasicObject  = Fixnum | Flonum | Bignum | String | NilClass | TrueClass
  #                  | FalseClass | Object | Array | Hash | Symbol | Undef
  #   CValue       = CBool
  #   RubyValue    = BasicObject | Undef
  #   Any          = RubyValue | CValue
  #   Empty        = 0 (bottom)
  # ═══════════════════════════════════════════════════════════════════

  module Bits
    Fixnum    = 1 << 0
    Flonum    = 1 << 1
    Bignum    = 1 << 2
    String    = 1 << 3
    NilClass  = 1 << 4
    TrueClass = 1 << 5
    FalseClass= 1 << 6
    Object    = 1 << 7
    Array     = 1 << 8
    Hash      = 1 << 9
    Symbol    = 1 << 10
    Undef     = 1 << 11
    CBool     = 1 << 12

    Integer     = Fixnum | Bignum
    Float       = Flonum
    Numeric     = Integer | Float
    Immediate   = Fixnum | Flonum | NilClass | TrueClass | FalseClass | Symbol
    Falsy       = NilClass | FalseClass
    BasicObject = Fixnum | Flonum | Bignum | String | NilClass | TrueClass |
                  FalseClass | Object | Array | Hash | Symbol | Undef
    CValue      = CBool
    RubyValue   = BasicObject | Undef
    Any         = RubyValue | CValue
    Empty       = 0

    # Map from single-bit leaf → display name (sorted by bit position)
    LEAF_NAMES = {
      Fixnum => "Fixnum", Flonum => "Flonum", Bignum => "Bignum",
      String => "String", NilClass => "NilClass", TrueClass => "TrueClass",
      FalseClass => "FalseClass", Object => "Object", Array => "Array",
      Hash => "Hash", Symbol => "Symbol", Undef => "Undef", CBool => "CBool",
    }.freeze

    # Named composite patterns for display (checked in order, most specific first)
    NAMED_COMPOSITES = [
      [Integer, "Integer"], [Float, "Float"], [Numeric, "Numeric"],
      [Immediate, "Immediate"], [Falsy, "Falsy"],
      [BasicObject, "BasicObject"], [CValue, "CValue"],
      [RubyValue, "RubyValue"], [Any, "Any"],
    ].freeze
  end

  # Specialization — optional constant/object info attached to a Type.
  # Like real ZJIT: bits tell you the set of possible types, spec tells
  # you "we know exactly this value/class".
  module Spec
    NONE  = :none   # No specialization (like Specialization::Any)
    # Otherwise, the spec is the Ruby constant value itself (like Specialization::Object)
  end

  class Type
    attr_reader :bits, :spec

    def initialize(bits, spec = Spec::NONE)
      @bits = bits
      @spec = spec
    end

    # ── Constructors ──

    def with_const(val) = Type.new(@bits, val)

    # ── Predicates ──

    def has_const?      = @spec != Spec::NONE
    def fixnum?         = (@bits & ~Bits::Fixnum) == 0 && @bits != 0
    def nilclass?       = (@bits & ~Bits::NilClass) == 0 && @bits != 0
    def empty?          = @bits == Bits::Empty
    def any?            = @bits == Bits::Any
    def cbool?          = (@bits & ~Bits::CBool) == 0 && @bits != 0
    def basic_object?   = (@bits & ~Bits::BasicObject) == 0 && @bits != 0
    def const_val       = @spec

    # ── Lattice operations ──

    # Subtype: self's bits are a subset of other's bits, and specs are compatible
    def <=(other)
      return true if @bits == Bits::Empty
      return true if (@bits & other.bits) == @bits && spec_compatible?(other)
      false
    end

    # Intersection (meet): bitwise AND of bits, keep more specific spec
    def &(other)
      new_bits = @bits & other.bits
      return Types::Empty if new_bits == Bits::Empty
      if self <= other
        Type.new(new_bits, @spec)
      elsif other <= self
        Type.new(new_bits, other.spec)
      else
        Type.new(new_bits)
      end
    end

    # Union (join): bitwise OR of bits, keep spec only if identical
    def |(other)
      new_bits = @bits | other.bits
      new_spec = (@spec == other.spec) ? @spec : Spec::NONE
      Type.new(new_bits, new_spec)
    end

    # ── Display ──

    def to_s
      return "Empty" if @bits == Bits::Empty

      # Try exact match against leaf types first (most specific)
      Bits::LEAF_NAMES.each do |pattern, name|
        if @bits == pattern
          s = name
          s += "[#{@spec.inspect}]" if has_const?
          return s
        end
      end

      # Try exact match against named composites
      Bits::NAMED_COMPOSITES.each do |pattern, name|
        if @bits == pattern
          s = name
          s += "[#{@spec.inspect}]" if has_const?
          return s
        end
      end

      # Decompose into union of smallest named parts
      remaining = @bits
      parts = []
      Bits::LEAF_NAMES.each do |pattern, name|
        if @bits == pattern
          s = name
          s += "[#{@spec.inspect}]" if has_const?
          return s
        end
      end

      # Decompose into union of smallest named parts
      remaining = @bits
      parts = []
      Bits::LEAF_NAMES.each do |pattern, name|
        if (remaining & pattern) == pattern
          parts << name
          remaining &= ~pattern
        end
      end
      parts.join("|")
    end

    def ==(other) = other.is_a?(Type) && @bits == other.bits && @spec == other.spec
    def eql?(other) = self == other
    def hash = [@bits, @spec].hash

    private

    def spec_compatible?(other)
      return true if other.spec == Spec::NONE   # other is unspecialized — supertype of all specs
      return true if @spec == other.spec         # identical specialization
      false                                      # unspecialized self is NOT subtype of specialized other
    end
  end

  module Types
    Any         = Type.new(Bits::Any)
    BasicObject = Type.new(Bits::BasicObject)
    Fixnum      = Type.new(Bits::Fixnum)
    Float       = Type.new(Bits::Float)
    Integer     = Type.new(Bits::Integer)
    Numeric     = Type.new(Bits::Numeric)
    String      = Type.new(Bits::String)
    Array       = Type.new(Bits::Array)
    Hash        = Type.new(Bits::Hash)
    Symbol      = Type.new(Bits::Symbol)
    NilClass    = Type.new(Bits::NilClass)
    TrueClass   = Type.new(Bits::TrueClass)
    FalseClass  = Type.new(Bits::FalseClass)
    Falsy       = Type.new(Bits::Falsy)
    Object      = Type.new(Bits::Object)
    CBool       = Type.new(Bits::CBool)
    Empty       = Type.new(Bits::Empty)
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
    attr_accessor :id, :type, :block, :forwarded

    def initialize(type = Types::Any)
      @id = nil       # assigned by Function#push_insn
      @type = type
      @block = nil    # back-pointer to owning Block
      @forwarded = self  # union-find: points to self (fixpoint) or canonical rep
    end

    def to_s = "v#{find.id}"

    # ── Union-Find (per-instruction) ──
    # Each insn carries its own forwarding pointer. find() follows the
    # chain with path compression. Fixpoint: insn.forwarded == insn.

    def find
      # Walk to root
      root = self
      while !root.forwarded.equal?(root)
        root = root.forwarded
      end
      # Path compression
      current = self
      while !current.equal?(root)
        nxt = current.forwarded
        current.forwarded = root
        current = nxt
      end
      root
    end

    def make_equal_to(other)
      find.forwarded = other.find
    end

    # Override in subclasses
    def operands = []
    def effects  = Effects.new(Eff::Empty, Eff::Empty)

    # GVN key: [class, *canonical_operand_ids]. Two instructions with the
    # same key compute the same value. Returns nil if not numberable
    # (side-effecting, control, etc). Subclasses override as needed.
    def value_key = nil
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
    def value_key = [:Const, @val]
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
    def value_key = [:Test, val.find.id]
  end

  class FixnumAdd < Insn
    attr_accessor :left, :right, :state
    def initialize(left, right, state)
      super(Types::Fixnum)
      @left = left; @right = right; @state = state
    end
    def operands = [left, right, state].compact
    def effects  = Effects.new(Eff::Empty, Eff::Control)
    def value_key = [:FixnumAdd, left.find.id, right.find.id]
  end

  class FixnumSub < Insn
    attr_accessor :left, :right, :state
    def initialize(left, right, state)
      super(Types::Fixnum)
      @left = left; @right = right; @state = state
    end
    def operands = [left, right, state].compact
    def effects  = Effects.new(Eff::Empty, Eff::Control)
    def value_key = [:FixnumSub, left.find.id, right.find.id]
  end

  class FixnumMult < Insn
    attr_accessor :left, :right, :state
    def initialize(left, right, state)
      super(Types::Fixnum)
      @left = left; @right = right; @state = state
    end
    def operands = [left, right, state].compact
    def effects  = Effects.new(Eff::Empty, Eff::Control)
    def value_key = [:FixnumMult, left.find.id, right.find.id]
  end

  class FixnumLt < Insn
    attr_accessor :left, :right
    def initialize(left, right)
      super(Types::CBool)
      @left = left; @right = right
    end
    def operands = [left, right]
    def value_key = [:FixnumLt, left.find.id, right.find.id]
  end

  class FixnumEq < Insn
    attr_accessor :left, :right
    def initialize(left, right)
      super(Types::CBool)
      @left = left; @right = right
    end
    def operands = [left, right]
    def value_key = [:FixnumEq, left.find.id, right.find.id]
  end

  class FixnumGt < Insn
    attr_accessor :left, :right
    def initialize(left, right)
      super(Types::CBool)
      @left = left; @right = right
    end
    def operands = [left, right]
    def value_key = [:FixnumGt, left.find.id, right.find.id]
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

  # Read a field from an object at a given offset/name
  class LoadField < Insn
    attr_accessor :recv, :field_name
    def initialize(recv, field_name, type = Types::BasicObject)
      super(type)
      @recv = recv; @field_name = field_name
    end
    def operands = [recv]
    def effects  = Effects.new(Eff::Other, Eff::Empty)
    def value_key = [:LoadField, recv.find.id, @field_name]
  end

  # Write a field on an object
  class StoreField < Insn
    attr_accessor :recv, :field_name, :val
    def initialize(recv, field_name, val)
      super(Types::Empty)
      @recv = recv; @field_name = field_name; @val = val
    end
    def operands = [recv, val]
    def effects  = Effects.new(Eff::Empty, Eff::Other)
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

  # ─── Dominators (Cooper/Harvey/Kennedy) ─────────────────────────────
  # Computes the immediate dominator (idom) of each block using the
  # "engineered algorithm" from:
  #   Cooper, Harvey & Kennedy, "A Simple, Fast Dominance Algorithm", 2001
  #   https://www.cs.tufts.edu/~nr/cs257/archive/keith-cooper/dom14.pdf

  class Dominators
    def initialize(fun)
      @blocks = fun.rpo
      return if @blocks.empty?

      # Map block → RPO index for fast comparison
      @rpo_index = {}
      @blocks.each_with_index { |b, i| @rpo_index[b.id] = i }

      # Build predecessor lists
      @preds = Hash.new { |h, k| h[k] = [] }
      @blocks.each do |block|
        block.insns.each do |insn|
          insn = insn.find
          case insn
          when Jump    then @preds[insn.target.target.id] << block
          when IfTrue  then @preds[insn.target.target.id] << block
          when IfFalse then @preds[insn.target.target.id] << block
          end
        end
      end

      # idom[block_id] = Block (immediate dominator)
      @idoms = {}
      root = @blocks[0]
      @idoms[root.id] = root  # root dominates itself (sentinel)

      # Iterate until convergence
      changed = true
      while changed
        changed = false
        @blocks.each do |block|
          next if block.equal?(root)
          preds = @preds[block.id]
          next if preds.empty?

          # Pick first processed predecessor
          new_idom = preds.find { |p| @idoms.key?(p.id) }
          next unless new_idom

          # Intersect with remaining processed predecessors
          preds.each do |p|
            next if p.equal?(new_idom)
            next unless @idoms.key?(p.id)
            new_idom = intersect(new_idom, p)
          end

          if @idoms[block.id] != new_idom
            @idoms[block.id] = new_idom
            changed = true
          end
        end
      end
    end

    # Return the immediate dominator of a block (nil for root)
    def idom(block)
      d = @idoms[block.id]
      (d && !d.equal?(block)) ? d : nil
    end

    # Does `a` dominate `b`? Walk idom chain from b upward.
    def dominates?(a, b)
      current = b
      while current
        return true if current.equal?(a)
        current = idom(current)
      end
      false
    end

    private

    def intersect(b1, b2)
      finger1 = b1
      finger2 = b2
      while !finger1.equal?(finger2)
        while @rpo_index[finger1.id] > @rpo_index[finger2.id]
          finger1 = @idoms[finger1.id]
        end
        while @rpo_index[finger2.id] > @rpo_index[finger1.id]
          finger2 = @idoms[finger2.id]
        end
      end
      finger1
    end
  end

  # ─── Function (the whole HIR graph) ────────────────────────────────

  class Function
    attr_reader :blocks, :name

    def initialize(name = "test")
      @name = name
      @blocks = []
      @next_id = 0
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

    # Infer the type of a newly created instruction
    def infer_type(insn)
      case insn
      when Const
        insn.type  # already set at creation
      when FixnumAdd, FixnumSub, FixnumMult
        lt = insn.left.find.type
        rt = insn.right.find.type
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
      when GuardType then insn.val.find.type & insn.guard_type
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
          resolved = insn.find
          case resolved
          when Jump    then worklist << resolved.target.target
          when IfTrue  then worklist << resolved.target.target
          when IfFalse then worklist << resolved.target.target
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
    def r(insn) = insn.find

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
      when LoadField  then "#{prefix}LoadField #{r(insn.recv)}, :#{insn.field_name}"
      when StoreField then "StoreField #{r(insn.recv)}, :#{insn.field_name}, #{r(insn.val)}"
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

          # Always emit a generic Send. type_specialize will lower it to
          # Fixnum ops if the types are known — just like real ZJIT.
          method = { opt_plus: :+, opt_minus: :-, opt_mult: :*, opt_lt: :<, opt_eq: :==, opt_gt: :> }[op]
          result = fun.push_insn(current_block, Send.new(left, method, [right], snap))
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
          insn = insn.find
          next push_insn_id(block, insn) unless insn.is_a?(Send)
          next push_insn_id(block, insn) unless FIXNUM_SEND_MAP.key?(insn.method_name)

          recv_type = insn.recv.find.type
          arg_type  = insn.args[0] ? insn.args[0].find.type : nil

          unless recv_type.fixnum? && arg_type&.fixnum?
            push_insn_id(block, insn); next
          end

          # Guard both operands
          gl = push_insn(block, GuardType.new(insn.recv.find, Types::Fixnum, insn.state))
          gl.type = insn.recv.find.type & Types::Fixnum
          gr = push_insn(block, GuardType.new(insn.args[0].find, Types::Fixnum, insn.state))
          gr.type = insn.args[0].find.type & Types::Fixnum

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
          insn.make_equal_to(result)
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
          insn = insn_id.find
          replacement = case insn

          # Guard elimination: if val already has the guarded type, forward
          when GuardType
            if insn.val.find.type <= insn.guard_type
              insn.make_equal_to(insn.val)
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
            val_type = insn.val.find.type
            if val_type <= Types::NilClass || val_type <= Types::FalseClass
              new_insn(Const.new(false, Types::FalseClass))
            elsif val_type <= Types::Fixnum || val_type <= Types::TrueClass || val_type <= Types::String
              new_insn(Const.new(true, Types::TrueClass))
            else
              insn
            end

          # Branch simplification: fold IfTrue/IfFalse on known booleans
          when IfTrue
            val_type = insn.val.find.type
            if val_type <= Types::TrueClass
              new_insn(Jump.new(insn.target))
            elsif val_type <= Types::FalseClass
              next  # never taken → drop
            else
              insn
            end

          when IfFalse
            val_type = insn.val.find.type
            if val_type <= Types::FalseClass
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
            insn.make_equal_to(replacement)
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
          insn = insn.find
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
        canonical = insn.find
        necessary << canonical.object_id
        canonical.operands.each do |op|
          next unless op.is_a?(Insn)
          worklist << op.find
        end
      end

      rpo.each do |block|
        block.insns.select! { |insn| necessary.include?(insn.object_id) }
      end
    end

    # ── global_value_numbering ─────────────────────────────────────
    # Dominator-tree-based GVN, following the Maxine-VM C1X approach:
    #   1. Compute dominators (Cooper/Harvey/Kennedy)
    #   2. Walk blocks in RPO; each block inherits its dominator's value map
    #   3. For each numberable instruction, findInsert in scoped map
    #   4. If found, make_equal_to the duplicate → the original

    def global_value_numbering
      doms = Dominators.new(self)
      block_order = rpo

      # ── Pre-compute write effects per block ──
      block_writes = {}  # Block.id -> write effect bits
      block_order.each do |block|
        writes = Eff::Empty
        block.insns.each { |insn| writes |= insn.find.effects.write }
        block_writes[block.id] = writes
      end

      # ── Compute accumulated write effects on all paths from idom to
      #    each block (union over all paths). Uses a fixpoint on RPO. ──
      #
      # For block B with idom D:
      #   path_writes[B] = union over predecessors P of:
      #     if P == D: block_writes[D]      (direct edge from dominator)
      #     else:      path_writes[P] | block_writes[P]
      #
      # This gives us the union of write effects along every path from
      # D to B, which tells us which value map entries to evict.

      preds = Hash.new { |h, k| h[k] = [] }
      block_order.each do |block|
        block.insns.each do |insn|
          resolved = insn.find
          case resolved
          when Jump    then preds[resolved.target.target.id] << block
          when IfTrue  then preds[resolved.target.target.id] << block
          when IfFalse then preds[resolved.target.target.id] << block
          end
        end
      end

      path_writes = {}  # Block.id -> accumulated write effects from idom
      block_order.each do |block|
        idom = doms.idom(block)
        unless idom
          path_writes[block.id] = Eff::Empty
          next
        end
        accumulated = Eff::Empty
        preds[block.id].each do |pred|
          if pred.equal?(idom)
            # Direct edge from dominator — only the dominator's own writes
            accumulated |= block_writes[idom.id]
          else
            # Transitive path: pred's accumulated path writes + pred's own writes
            accumulated |= (path_writes[pred.id] || Eff::Empty) | block_writes[pred.id]
          end
        end
        path_writes[block.id] = accumulated
      end

      # ── GVN walk ──
      value_maps = {}  # Block.id -> Hash { value_key => Insn }

      block_order.each do |block|
        # Inherit dominator's value map
        idom = doms.idom(block)
        parent_map = (idom && idom != block) ? value_maps[idom.id] : nil
        current_map = parent_map ? parent_map.dup : {}

        # Evict entries from inherited map that are invalidated by writes
        # on paths from idom to this block
        pw = path_writes[block.id] || Eff::Empty
        if pw & Eff::Memory != 0
          current_map.reject! { |_, insn| insn.effects.read & pw != 0 }
        end

        # Process instructions, evicting on writes and deduplicating
        old_insns = block.insns.dup
        block.insns.clear
        old_insns.each do |insn|
          canonical = insn.find

          # Intra-block eviction: writes kill overlapping reads
          write_effs = canonical.effects.write
          if write_effs & Eff::Memory != 0
            current_map.reject! { |_, mapped| mapped.effects.read & write_effs != 0 }
          end

          key = canonical.value_key
          if key
            existing = current_map[key]
            if existing && !existing.equal?(canonical)
              canonical.make_equal_to(existing)
              next  # drop duplicate from block
            else
              current_map[key] = canonical
            end
          end
          block.insns << insn
        end

        value_maps[block.id] = current_map
      end
    end

    # ── optimize (the pipeline) ──────────────────────────────────────
    # Runs each pass once, sequentially, just like real ZJIT.

    def optimize
      type_specialize
      fold_constants
      global_value_numbering
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
      last = last.find
      return false unless last.is_a?(Jump)

      target = last.target.target
      return false if target.equal?(block)          # can't absorb self
      return false if num_in_edges[target.id] != 1  # must be sole predecessor

      # Link block params → jump args via union-find
      target.params.each_with_index do |param, i|
        param.make_equal_to(last.target.args[i])
      end

      # Remove the Jump, append target's insns
      block.insns.pop
      target.insns.each { |insn| push_insn_id(block, insn) }
      target.insns.clear
      target.params.clear
      true
    end

    def fold_fixnum_bop(insn)
      lt = insn.left.find.type
      rt = insn.right.find.type
      return nil unless lt.has_const? && rt.has_const? && lt.fixnum? && rt.fixnum?
      result = case insn
               when FixnumAdd  then lt.const_val + rt.const_val
               when FixnumSub  then lt.const_val - rt.const_val
               when FixnumMult then lt.const_val * rt.const_val
               end
      new_insn(Const.new(result, Types::Fixnum.with_const(result)))
    end

    def fold_fixnum_pred(insn)
      lt = insn.left.find.type
      rt = insn.right.find.type
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
  require "json"
  require "fileutils"

  module InlineSnapshotFix
    PENDING_PATH = "tmp/inline_snapshots.pending.json"
    @pending = []

    class << self
      attr_reader :pending

      def normalize_expected(expected)
        expected.gsub(/^ {8}/, "").strip
      end

      def record_mismatch(file:, line:, actual:)
        @pending << { "file" => file, "line" => line, "actual" => actual }
      end

      def dump_pending!
        return if @pending.empty?

        FileUtils.mkdir_p(File.dirname(PENDING_PATH))
        existing = File.exist?(PENDING_PATH) ? JSON.parse(File.read(PENDING_PATH)) : []
        merged = {}
        (existing + @pending).each do |entry|
          merged["#{entry["file"]}:#{entry["line"]}"] = entry
        end

        rows = merged.values.sort_by { |entry| [entry["file"], entry["line"]] }
        File.write(PENDING_PATH, JSON.pretty_generate(rows))
        warn "\nInline snapshots pending: #{PENDING_PATH}"
      end

      def apply_pending!
        return unless File.exist?(PENDING_PATH)

        rows = JSON.parse(File.read(PENDING_PATH))
        rows_by_file = rows.group_by { |entry| entry["file"] }

        rows_by_file.each do |file, entries|
          lines = File.readlines(file, chomp: false)

          entries.sort_by { |entry| -entry["line"] }.each do |entry|
            replace_heredoc_body!(lines, entry["line"], entry["actual"])
          end

          File.write(file, lines.join)
          warn "Updated #{file} (#{entries.size} snapshots)"
        end

        File.delete(PENDING_PATH)
        warn "Applied and removed #{PENDING_PATH}"
      end

      private

      def replace_heredoc_body!(lines, line_no, actual)
        call_index = line_no - 1
        call_line = lines[call_index]
        raise "Missing snapshot call at line #{line_no}" unless call_line

        marker = call_line.match(/<<~['\"]?([A-Z_][A-Z0-9_]*)['\"]?/) or
          raise "No heredoc marker at line #{line_no}: #{call_line.inspect}"
        terminator = marker[1]

        body_start = call_index + 1
        body_end = body_start
        while body_end < lines.length && lines[body_end].strip != terminator
          body_end += 1
        end
        raise "No heredoc terminator #{terminator} after line #{line_no}" if body_end >= lines.length

        replacement = actual.end_with?("\n") ? actual : "#{actual}\n"
        lines[body_start...body_end] = replacement.lines
      end
    end

    def assert_hir(code, expected, optimize: true)
      actual = MiniZJIT.hir(code, optimize: optimize).strip
      expected = InlineSnapshotFix.normalize_expected(expected)
      return assert_equal(expected, actual, "HIR mismatch for: #{code}") if actual == expected

      loc = caller_locations(1, 1).first
      InlineSnapshotFix.record_mismatch(file: loc.path, line: loc.lineno, actual: actual)

      if ENV["FIX"] == "1"
        skip "Updated pending snapshot for #{loc.path}:#{loc.lineno}"
      else
        assert_equal expected, actual, "HIR mismatch for: #{code}"
      end
    end

    def assert_hir_text(actual, expected, message: "HIR mismatch")
      actual = actual.strip
      expected = InlineSnapshotFix.normalize_expected(expected)
      return assert_equal(expected, actual, message) if actual == expected

      loc = caller_locations(1, 1).first
      InlineSnapshotFix.record_mismatch(file: loc.path, line: loc.lineno, actual: actual)

      if ENV["FIX"] == "1"
        skip "Updated pending snapshot for #{loc.path}:#{loc.lineno}"
      else
        assert_equal expected, actual, message
      end
    end
  end

  Minitest.after_run do
    InlineSnapshotFix.dump_pending!
    InlineSnapshotFix.apply_pending! if ENV["FIX"] == "1"
  end

  class TypeTest < Minitest::Test
    include MiniZJIT

    # ── Bitset subtype checks ──

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

    def test_fixnum_subtype_of_integer
      assert Types::Fixnum <= Types::Integer
    end

    def test_integer_not_subtype_of_fixnum
      refute Types::Integer <= Types::Fixnum
    end

    def test_integer_subtype_of_numeric
      assert Types::Integer <= Types::Numeric
    end

    def test_cbool_subtype_of_any
      assert Types::CBool <= Types::Any
    end

    def test_cbool_not_subtype_of_basic_object
      refute Types::CBool <= Types::BasicObject
    end

    def test_nilclass_subtype_of_falsy
      assert Types::NilClass <= Types::Falsy
    end

    def test_falseclass_subtype_of_falsy
      assert Types::FalseClass <= Types::Falsy
    end

    def test_fixnum_not_subtype_of_falsy
      refute Types::Fixnum <= Types::Falsy
    end

    # ── Intersection (meet) via bitwise AND ──

    def test_meet_narrows
      assert_equal Types::Fixnum, (Types::Fixnum & Types::BasicObject)
    end

    def test_meet_integer_and_fixnum
      assert_equal Types::Fixnum, (Types::Integer & Types::Fixnum)
    end

    def test_meet_disjoint_is_empty
      assert_equal Types::Empty, (Types::Fixnum & Types::String)
    end

    def test_meet_cbool_and_basic_object_is_empty
      assert_equal Types::Empty, (Types::CBool & Types::BasicObject)
    end

    # ── Union (join) via bitwise OR ──

    def test_union_fixnum_and_nilclass
      union = Types::Fixnum | Types::NilClass
      assert Types::Fixnum <= union
      assert Types::NilClass <= union
      refute Types::String <= union
    end

    def test_union_preserves_const_if_same
      a = Types::Fixnum.with_const(1)
      b = Types::Fixnum.with_const(1)
      assert (a | b).has_const?
    end

    def test_union_drops_const_if_different
      a = Types::Fixnum.with_const(1)
      b = Types::Fixnum.with_const(2)
      refute (a | b).has_const?
    end

    # ── Specialization (constant info) ──

    def test_type_display_with_const
      assert_equal "Fixnum[42]", Types::Fixnum.with_const(42).to_s
    end

    def test_type_display_without_const
      assert_equal "Fixnum", Types::Fixnum.to_s
    end

    def test_const_type_subtype_of_unspecialized
      # Fixnum[42] is more specific → it IS a subtype of plain Fixnum
      assert Types::Fixnum.with_const(42) <= Types::Fixnum
    end

    def test_unspecialized_not_subtype_of_const
      # Plain Fixnum is NOT a subtype of Fixnum[42] — it could be any fixnum
      refute Types::Fixnum <= Types::Fixnum.with_const(42)
    end

    # ── Display ──

    def test_display_composite_types
      assert_equal "BasicObject", Types::BasicObject.to_s
      assert_equal "Any", Types::Any.to_s
      assert_equal "Empty", Types::Empty.to_s
      assert_equal "Integer", Types::Integer.to_s
      assert_equal "Falsy", Types::Falsy.to_s
    end

    def test_display_union_decomposition
      # A non-named union shows as pipe-separated leaves
      union = Types::Fixnum | Types::String
      assert_equal "Fixnum|String", union.to_s
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
    include InlineSnapshotFix

    # ── Unoptimized snapshots (before any passes) ──────────────────
    # The compiler emits generic Send instructions for arithmetic.
    # type_specialize lowers them to GuardType + Fixnum ops.

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
      # Compiler emits Send — type_specialize hasn't run yet
      assert_hir "1 + 2", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v2:Fixnum[2] = Const 2
          v4:BasicObject = Send v1, :+, v2
          Return v4
      HIR
    end

    def test_subtraction_unoptimized
      assert_hir "5 - 3", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[5] = Const 5
          v2:Fixnum[3] = Const 3
          v4:BasicObject = Send v1, :-, v2
          Return v4
      HIR
    end

    def test_comparison_unoptimized
      assert_hir "1 < 2", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v2:Fixnum[2] = Const 2
          v4:BasicObject = Send v1, :<, v2
          Return v4
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
          v4:BasicObject = Send v1, :+, v2
          Return v4
      HIR
    end

    def test_branch_unoptimized
      assert_hir "x = 1; if x > 0 then x + 1 else x - 1 end", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v2:Fixnum[0] = Const 0
          v4:BasicObject = Send v1, :>, v2
          v5:CBool = Test v4
          IfFalse v5, bb1(v0, v1)
          Jump bb2(v0, v1)
        bb1(v14:BasicObject, v15:Fixnum[1]):
          v16:Fixnum[1] = Const 1
          v18:BasicObject = Send v15, :-, v16
          Return v18
        bb2(v8:BasicObject, v9:Fixnum[1]):
          v10:Fixnum[1] = Const 1
          v12:BasicObject = Send v9, :+, v10
          Return v12
      HIR
    end

    # ── Optimized snapshots ────────────────────────────────────────
    # After: type_specialize → fold_constants → clean_cfg → eliminate_dead_code

    def test_fold_addition
      assert_hir "1 + 2", <<~HIR
        fn <compiled>:
        bb0:
          v9:Fixnum[3] = Const 3
          Return v9
      HIR
    end

    def test_fold_subtraction
      assert_hir "5 - 3", <<~HIR
        fn <compiled>:
        bb0:
          v9:Fixnum[2] = Const 2
          Return v9
      HIR
    end

    def test_fold_multiplication
      assert_hir "3 * 4", <<~HIR
        fn <compiled>:
        bb0:
          v9:Fixnum[12] = Const 12
          Return v9
      HIR
    end

    def test_fold_comparison_true
      assert_hir "1 < 2", <<~HIR
        fn <compiled>:
        bb0:
          v9:TrueClass = Const true
          Return v9
      HIR
    end

    def test_fold_comparison_false
      assert_hir "3 < 1", <<~HIR
        fn <compiled>:
        bb0:
          v9:FalseClass = Const false
          Return v9
      HIR
    end

    def test_fold_nested
      assert_hir "2 * 3 + 4", <<~HIR
        fn <compiled>:
        bb0:
          v16:Fixnum[10] = Const 10
          Return v16
      HIR
    end

    def test_fold_chain
      assert_hir "(1 + 2) * (3 + 4)", <<~HIR
        fn <compiled>:
        bb0:
          v23:Fixnum[21] = Const 21
          Return v23
      HIR
    end

    def test_fold_local_arithmetic
      assert_hir "x = 1; x + 2", <<~HIR
        fn <compiled>:
        bb0:
          v9:Fixnum[3] = Const 3
          Return v9
      HIR
    end

    def test_branch_fully_folded
      # type_specialize lowers Send to Fixnum ops, fold_constants folds
      # the comparison to true, simplifies IfFalse (never taken) + Jump
      # (always taken) into a single Jump, and clean_cfg absorbs the
      # target block. The whole thing collapses to a single block.
      assert_hir "x = 1; if x > 0 then x + 1 else x - 1 end", <<~HIR
        fn <compiled>:
        bb0:
          v32:Fixnum[2] = Const 2
          Return v32
      HIR
    end
  end

  # ── type_specialize tests ────────────────────────────────────────

  class TypeSpecializeTest < Minitest::Test
    include InlineSnapshotFix

    def test_send_lowered_to_fixnum_add
      # After full optimization, Send(:+) on Fixnum constants becomes
      # a folded Const. But we can see the strength reduction worked
      # because the result is correct (only possible via FixnumAdd).
      assert_hir "1 + 2", <<~HIR
        fn <compiled>:
        bb0:
          v9:Fixnum[3] = Const 3
          Return v9
      HIR
    end

    def test_send_not_lowered_for_strings
      # String receiver — type_specialize can't lower to Fixnum ops,
      # so Send survives.
      assert_hir '"hello".length', <<~HIR
        fn <compiled>:
        bb0:
          v1:String = Const "hello"
          v3:BasicObject = Send v1, :length
          Return v3
      HIR
    end
  end

  # ── fold_constants tests (guard elim + constant folding) ─────────

  class FoldConstantsTest < Minitest::Test
    include InlineSnapshotFix

    def test_guards_on_known_fixnums_eliminated
      # Both operands are Fixnum constants. fold_constants sees
      # GuardType on a value that is_a?(Fixnum) and drops it via
      # make_equal_to. Then folds the arithmetic. DCE cleans up.
      assert_hir "1 + 2", <<~HIR
        fn <compiled>:
        bb0:
          v9:Fixnum[3] = Const 3
          Return v9
      HIR
    end

    def test_branch_condition_folded
      # x = 1; x > 0 folds to Const(true). IfFalse on true is dropped,
      # Jump on true becomes unconditional. clean_cfg merges the blocks.
      assert_hir "x = 1; if x > 0 then x + 1 else x - 1 end", <<~HIR
        fn <compiled>:
        bb0:
          v32:Fixnum[2] = Const 2
          Return v32
      HIR
    end
  end

  # ── eliminate_dead_code tests ────────────────────────────────────

  class DCETest < Minitest::Test
    include InlineSnapshotFix

    def test_dead_consts_and_guards_removed
      # Before optimization: compiler emits Send (no guards yet)
      assert_hir "1 + 2", <<~HIR, optimize: false
        fn <compiled>:
        bb0:
          v0:BasicObject = PutSelf
          v1:Fixnum[1] = Const 1
          v2:Fixnum[2] = Const 2
          v4:BasicObject = Send v1, :+, v2
          Return v4
      HIR

      # After: type_specialize adds GuardType + FixnumAdd, fold_constants
      # folds everything, DCE sweeps dead Const/GuardType instructions.
      assert_hir "1 + 2", <<~HIR
        fn <compiled>:
        bb0:
          v9:Fixnum[3] = Const 3
          Return v9
      HIR
    end

    def test_send_not_removed_by_dce
      # Send has Any effects, so it is never a DCE candidate.
      assert_hir '"hello".length', <<~HIR
        fn <compiled>:
        bb0:
          v1:String = Const "hello"
          v3:BasicObject = Send v1, :length
          Return v3
      HIR
    end
  end

  # ── dominators tests ──────────────────────────────────────────────

  class DominatorsTest < Minitest::Test
    include MiniZJIT

    def test_single_block_dominates_itself
      fun = Function.new("test")
      bb0 = fun.new_block
      fun.push_insn(bb0, Return.new(fun.push_insn(bb0, Const.new(1, Types::Fixnum))))
      doms = Dominators.new(fun)
      assert_nil doms.idom(bb0), "root has no idom"
      assert doms.dominates?(bb0, bb0), "root dominates itself"
    end

    def test_linear_chain
      fun = Function.new("test")
      bb0 = fun.new_block
      bb1 = fun.new_block
      fun.push_insn(bb0, Jump.new(BranchEdge.new(bb1)))
      fun.push_insn(bb1, Return.new(fun.push_insn(bb1, Const.new(1, Types::Fixnum))))
      doms = Dominators.new(fun)
      assert_equal bb0, doms.idom(bb1)
      assert doms.dominates?(bb0, bb1)
      refute doms.dominates?(bb1, bb0)
    end

    def test_diamond
      fun = Function.new("test")
      bb0 = fun.new_block
      bb1 = fun.new_block
      bb2 = fun.new_block
      bb3 = fun.new_block
      cond = fun.push_insn(bb0, Const.new(true, Types::TrueClass))
      test = fun.push_insn(bb0, Test.new(cond))
      fun.push_insn(bb0, IfTrue.new(test, BranchEdge.new(bb1)))
      fun.push_insn(bb0, Jump.new(BranchEdge.new(bb2)))
      fun.push_insn(bb1, Jump.new(BranchEdge.new(bb3)))
      fun.push_insn(bb2, Jump.new(BranchEdge.new(bb3)))
      fun.push_insn(bb3, Return.new(fun.push_insn(bb3, Const.new(1, Types::Fixnum))))
      doms = Dominators.new(fun)
      # bb0 dominates everything
      assert doms.dominates?(bb0, bb1)
      assert doms.dominates?(bb0, bb2)
      assert doms.dominates?(bb0, bb3)
      # bb1 and bb2 don't dominate bb3 (both paths lead there)
      refute doms.dominates?(bb1, bb3)
      refute doms.dominates?(bb2, bb3)
      # bb3's idom is bb0
      assert_equal bb0, doms.idom(bb3)
    end
  end

  # ── GVN tests ────────────────────────────────────────────────────

  class GVNTest < Minitest::Test
    include MiniZJIT
    include InlineSnapshotFix

    def test_duplicate_fixnum_add_eliminated
      fun = Function.new("test")
      bb0 = fun.new_block
      a = fun.push_insn(bb0, Param.new(0, Types::Fixnum))
      b = fun.push_insn(bb0, Param.new(1, Types::Fixnum))
      snap = fun.push_insn(bb0, Snapshot.new({}, []))
      add1 = fun.push_insn(bb0, FixnumAdd.new(a, b, snap))
      add2 = fun.push_insn(bb0, FixnumAdd.new(a, b, snap))
      sum  = fun.push_insn(bb0, FixnumAdd.new(add1, add2, snap))
      fun.push_insn(bb0, Return.new(sum))
      fun.global_value_numbering
      fun.eliminate_dead_code
      assert_hir_text fun.to_s, <<~HIR
        fn test:
        bb0(v0:Fixnum, v1:Fixnum):
          v3:Fixnum = FixnumAdd v0, v1
          v5:Fixnum = FixnumAdd v3, v3
          Return v5
      HIR
    end

    def test_duplicate_const_eliminated
      fun = Function.new("test")
      bb0 = fun.new_block
      c1 = fun.push_insn(bb0, Const.new(42, Types::Fixnum.with_const(42)))
      c2 = fun.push_insn(bb0, Const.new(42, Types::Fixnum.with_const(42)))
      snap = fun.push_insn(bb0, Snapshot.new({}, []))
      add = fun.push_insn(bb0, FixnumAdd.new(c1, c2, snap))
      fun.push_insn(bb0, Return.new(add))
      fun.global_value_numbering
      fun.eliminate_dead_code
      assert_hir_text fun.to_s, <<~HIR
        fn test:
        bb0:
          v0:Fixnum[42] = Const 42
          v3:Fixnum = FixnumAdd v0, v0
          Return v3
      HIR
    end

    def test_gvn_across_dominator
      fun = Function.new("test")
      bb0 = fun.new_block
      bb1 = fun.new_block
      c0 = fun.push_insn(bb0, Const.new(42, Types::Fixnum.with_const(42)))
      fun.push_insn(bb0, Jump.new(BranchEdge.new(bb1)))
      c1 = fun.push_insn(bb1, Const.new(42, Types::Fixnum.with_const(42)))
      fun.push_insn(bb1, Return.new(c1))
      fun.global_value_numbering
      fun.eliminate_dead_code
      assert_hir_text fun.to_s, <<~HIR
        fn test:
        bb0:
          v0:Fixnum[42] = Const 42
          Jump bb1
        bb1:
          Return v0
      HIR
    end

    def test_duplicate_load_field_eliminated
      fun = Function.new("test")
      bb0 = fun.new_block
      obj = fun.push_insn(bb0, Param.new(0, Types::BasicObject))
      load1 = fun.push_insn(bb0, LoadField.new(obj, :x))
      load2 = fun.push_insn(bb0, LoadField.new(obj, :x))
      snap = fun.push_insn(bb0, Snapshot.new({}, []))
      add = fun.push_insn(bb0, FixnumAdd.new(load1, load2, snap))
      fun.push_insn(bb0, Return.new(add))
      fun.global_value_numbering
      fun.eliminate_dead_code
      assert_hir_text fun.to_s, <<~HIR
        fn test:
        bb0(v0:BasicObject):
          v1:BasicObject = LoadField v0, :x
          v4:Fixnum = FixnumAdd v1, v1
          Return v4
      HIR
    end

    def test_load_field_not_eliminated_across_store
      fun = Function.new("test")
      bb0 = fun.new_block
      obj = fun.push_insn(bb0, Param.new(0, Types::BasicObject))
      val = fun.push_insn(bb0, Const.new(99, Types::Fixnum.with_const(99)))
      load1 = fun.push_insn(bb0, LoadField.new(obj, :x))
      fun.push_insn(bb0, StoreField.new(obj, :x, val))
      load2 = fun.push_insn(bb0, LoadField.new(obj, :x))
      snap = fun.push_insn(bb0, Snapshot.new({}, []))
      add = fun.push_insn(bb0, FixnumAdd.new(load1, load2, snap))
      fun.push_insn(bb0, Return.new(add))
      fun.global_value_numbering
      assert_hir_text fun.to_s, <<~HIR
        fn test:
        bb0(v0:BasicObject):
          v1:Fixnum[99] = Const 99
          v2:BasicObject = LoadField v0, :x
          StoreField v0, :x, v1
          v4:BasicObject = LoadField v0, :x
          v6:Fixnum = FixnumAdd v2, v4
          Return v6
      HIR
    end

    def test_load_field_not_eliminated_across_send
      fun = Function.new("test")
      bb0 = fun.new_block
      obj = fun.push_insn(bb0, Param.new(0, Types::BasicObject))
      load1 = fun.push_insn(bb0, LoadField.new(obj, :x))
      snap = fun.push_insn(bb0, Snapshot.new({}, []))
      fun.push_insn(bb0, Send.new(obj, :mutate!, [], snap))
      load2 = fun.push_insn(bb0, LoadField.new(obj, :x))
      snap2 = fun.push_insn(bb0, Snapshot.new({}, []))
      add = fun.push_insn(bb0, FixnumAdd.new(load1, load2, snap2))
      fun.push_insn(bb0, Return.new(add))
      fun.global_value_numbering
      assert_hir_text fun.to_s, <<~HIR
        fn test:
        bb0(v0:BasicObject):
          v1:BasicObject = LoadField v0, :x
          v3:BasicObject = Send v0, :mutate!
          v4:BasicObject = LoadField v0, :x
          v6:Fixnum = FixnumAdd v1, v4
          Return v6
      HIR
    end

    def test_load_field_eliminated_across_dominator_no_write
      fun = Function.new("test")
      bb0 = fun.new_block
      bb1 = fun.new_block
      obj = fun.push_insn(bb0, Param.new(0, Types::BasicObject))
      load1 = fun.push_insn(bb0, LoadField.new(obj, :x))
      fun.push_insn(bb0, Jump.new(BranchEdge.new(bb1)))
      load2 = fun.push_insn(bb1, LoadField.new(obj, :x))
      fun.push_insn(bb1, Return.new(load2))
      fun.global_value_numbering
      fun.eliminate_dead_code
      assert_hir_text fun.to_s, <<~HIR
        fn test:
        bb0(v0:BasicObject):
          v1:BasicObject = LoadField v0, :x
          Jump bb1
        bb1:
          Return v1
      HIR
    end

    def test_load_field_not_eliminated_when_path_has_write
      fun = Function.new("test")
      bb0 = fun.new_block
      bb1 = fun.new_block
      bb2 = fun.new_block
      bb3 = fun.new_block
      obj = fun.push_insn(bb0, Param.new(0, Types::BasicObject))
      load1 = fun.push_insn(bb0, LoadField.new(obj, :x))
      cond = fun.push_insn(bb0, Const.new(true, Types::TrueClass))
      test = fun.push_insn(bb0, Test.new(cond))
      fun.push_insn(bb0, IfTrue.new(test, BranchEdge.new(bb1)))
      fun.push_insn(bb0, Jump.new(BranchEdge.new(bb2)))
      new_val = fun.push_insn(bb1, Const.new(42, Types::Fixnum.with_const(42)))
      fun.push_insn(bb1, StoreField.new(obj, :x, new_val))
      fun.push_insn(bb1, Jump.new(BranchEdge.new(bb3)))
      fun.push_insn(bb2, Jump.new(BranchEdge.new(bb3)))
      load2 = fun.push_insn(bb3, LoadField.new(obj, :x))
      fun.push_insn(bb3, Return.new(load2))
      fun.global_value_numbering
      assert_hir_text fun.to_s, <<~HIR
        fn test:
        bb0(v0:BasicObject):
          v1:BasicObject = LoadField v0, :x
          v2:TrueClass = Const true
          v3:CBool = Test v2
          IfTrue v3, bb1
          Jump bb2
        bb1:
          v6:Fixnum[42] = Const 42
          StoreField v0, :x, v6
          Jump bb3
        bb2:
          Jump bb3
        bb3:
          v10:BasicObject = LoadField v0, :x
          Return v10
      HIR
    end

    def test_gvn_does_not_unify_across_non_dominator
      fun = Function.new("test")
      bb0 = fun.new_block
      bb1 = fun.new_block
      bb2 = fun.new_block
      bb3 = fun.new_block
      a = fun.push_insn(bb0, Param.new(0, Types::Fixnum))
      b = fun.push_insn(bb0, Param.new(1, Types::Fixnum))
      cond = fun.push_insn(bb0, Const.new(true, Types::TrueClass))
      test = fun.push_insn(bb0, Test.new(cond))
      fun.push_insn(bb0, IfTrue.new(test, BranchEdge.new(bb1)))
      fun.push_insn(bb0, Jump.new(BranchEdge.new(bb2)))
      snap1 = fun.push_insn(bb1, Snapshot.new({}, []))
      add1 = fun.push_insn(bb1, FixnumAdd.new(a, b, snap1))
      fun.push_insn(bb1, Jump.new(BranchEdge.new(bb3, [add1])))
      snap2 = fun.push_insn(bb2, Snapshot.new({}, []))
      add2 = fun.push_insn(bb2, FixnumAdd.new(a, b, snap2))
      fun.push_insn(bb2, Jump.new(BranchEdge.new(bb3, [add2])))
      p0 = fun.push_insn(bb3, Param.new(:result, Types::Fixnum))
      fun.push_insn(bb3, Return.new(p0))
      fun.global_value_numbering
      assert_hir_text fun.to_s, <<~HIR
        fn test:
        bb0(v0:Fixnum, v1:Fixnum):
          v2:TrueClass = Const true
          v3:CBool = Test v2
          IfTrue v3, bb1
          Jump bb2
        bb1:
          v7:Fixnum = FixnumAdd v0, v1
          Jump bb3(v7)
        bb2:
          v10:Fixnum = FixnumAdd v0, v1
          Jump bb3(v10)
        bb3(v12:Fixnum):
          Return v12
      HIR
    end
  end

  # ── clean_cfg tests ─────────────────────────────────────────────

  class CleanCFGTest < Minitest::Test
    include InlineSnapshotFix

    def test_branch_blocks_absorbed
      # When the branch condition is folded to a constant, the dead
      # branch is dropped and the live branch is absorbed via clean_cfg,
      # collapsing 3 blocks into 1.
      assert_hir "x = 1; if x > 0 then x + 1 else x - 1 end", <<~HIR
fn <compiled>:
bb0:
  v32:Fixnum[2] = Const 2
  Return v32
      HIR
    end
  end
end
