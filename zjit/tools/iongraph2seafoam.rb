#!/usr/bin/env ruby
# frozen_string_literal: true

# Import a ZJIT iongraph dump (--zjit-dump-hir-iongraph) into a Seafoam graph and render it.
#
# Beyond the plain control/data graph, this reconstructs *split memory*: the effect lattice's
# leaves (`effectLeaves` in the dump) each get their own dependence chain, rooted in a token
# created at the start of the CFG. An instruction that reads a leaf consumes the current token
# for that leaf; an instruction that writes a leaf consumes the current token and produces a new
# one. Where control flow merges, the incoming tokens for a leaf meet in a memory phi. The
# resulting memory edges are exactly the dependences the effect system is asserting: two reads
# hanging off the same token are independent, while a write forces everything after it onto a new
# token.
#
# ZJIT gives its control flow instructions (Entries, EntryPoint, Jump, CondBranch, ...)
# `effects::Any`, which is a conservative "do not move anything across me" marker rather than a
# claim about a specific location. Taken literally it makes every block boundary a full barrier
# and every leaf phi at every merge, which drowns out the dependences worth looking at, so by
# default their effects are folded into the entry tokens. The instructions that leave the
# function (Return, SideExit, Throw) are instead read as memory sinks, consuming the final token
# of every chain so that the last write on each chain has a visible consumer. Pass
# --control-effects to see the graph the effect system literally describes.
#
# Leaves that no instruction in the function tells apart are threaded as a single chain, since
# their dependence graphs are identical by construction; --no-merge-chains keeps them separate.
# A chain that nothing in the function writes orders nothing - every reader sees the entry token -
# so it is left out unless --unwritten-chains asks for it.
#
# Usage:
#   ruby tools/iongraph2seafoam.rb DUMP.json [options]
#
# Requires seafoam (https://github.com/Shopify/seafoam) on the load path; see --seafoam.

require "json"
require "optparse"
require "set"
require "tmpdir"

module IonGraph2Seafoam
  # Seafoam node kinds, keyed by the leading word of an instruction's printed form.
  # Seafoam colors nodes by kind, so this is what gives the graph its shape at a glance.
  KIND_BY_PREFIX = {
    # Synthetic graph structure.
    "Entries" => "control",
    "EntryPoint" => "info",
    "PatchPoint" => "info",
    "Comment" => "info",
    # Values flowing in from outside the function.
    "Param" => "input",
    "LoadArg" => "input",
    "LoadSelf" => "input",
    "LoadEC" => "input",
    "LoadSP" => "input",
    "LoadPC" => "input",
    "GetEP" => "input",
    "Const" => "input",
    # Control flow.
    "Jump" => "control",
    "CondBranch" => "control",
    "Return" => "control",
    "SideExit" => "control",
    "Throw" => "control",
    "CheckInterrupts" => "sync",
    # Anything that can run arbitrary Ruby.
    "Send" => "call",
    "SendForward" => "call",
    "SendDirect" => "call",
    "SendWithoutBlock" => "call",
    "InvokeSuper" => "call",
    "InvokeSuperForward" => "call",
    "InvokeBlock" => "call",
    "InvokeBlockIfunc" => "call",
    "CCall" => "call",
    "CCallWithFrame" => "call",
    "CCallVariadic" => "call",
    "PushInlineFrame" => "call",
    "PopInlineFrame" => "call",
    # Allocation.
    "NewArray" => "alloc",
    "NewHash" => "alloc",
    "NewRange" => "alloc",
    "NewRangeFixnum" => "alloc",
    "ArrayDup" => "alloc",
    "HashDup" => "alloc",
    "ObjectAlloc" => "alloc",
    "ObjectAllocClass" => "alloc",
    "StringCopy" => "alloc",
    "StringIntern" => "alloc",
    "ToRegexp" => "alloc",
    "WriteBarrier" => "alloc",
    # Memory.
    "LoadField" => "memory",
    "StoreField" => "memory",
    "GetIvar" => "memory",
    "SetIvar" => "memory",
    "DefinedIvar" => "memory",
    "GetGlobal" => "memory",
    "SetGlobal" => "memory",
    "GetClassVar" => "memory",
    "SetClassVar" => "memory",
    "SetLocal" => "memory",
    "GetLocal" => "memory",
    "GetConstant" => "memory",
    "GetConstantPath" => "memory",
    "ArrayLength" => "memory",
    "ArrayAref" => "memory",
    "ArrayAset" => "memory",
  }.freeze

  # Seafoam's graphviz writer styles edges by kind: "data" is a solid green line, "control" a
  # thick red one, and "info" a dashed blue one, which we borrow for memory dependences.
  DATA_EDGE = { kind: "data" }.freeze
  CONTROL_EDGE = { kind: "control" }.freeze

  # Control flow instructions whose `effects::Any` is a placeholder rather than a real access.
  # `Entries` and `EntryPoint` mark where the frame begins, which the entry tokens already model.
  CONTROL_BARRIERS = %w[Entries EntryPoint Jump CondBranch Unreachable].freeze
  # Instructions that leave the function. Each is treated as a memory sink: it reads every chain,
  # so the last write on each one has a visible consumer rather than trailing off the bottom of
  # the graph, and it writes none, since a token it produced could have no reader. Their declared
  # `effects::Any` says both, but the write half only adds a dangling token.
  EXITS = %w[Return SideExit Throw].freeze
  # Instructions drawn as floating nodes: duplicated beside each user instead of occupying a
  # place in the block. `Const` takes no operands and has no effects, so it orders nothing and
  # its position in the instruction stream carries no information. Every constant prints under
  # this one opcode, the kind of value being a variant of its payload (Const CShape(0x100009),
  # Const CPtr(0x...), Const Value(nil)).
  FLOATING = %w[Const].freeze

  # Hex addresses in labels vary run to run and say nothing about dependence. Method entry
  # pointers are dropped along with their separator so the remaining argument list still reads.
  ADDRESS_PATTERNS = [/,\s*cme:0x[0-9a-f]+/, /@0x[0-9a-f]+/].freeze

  # One component of split memory: the leaves whose dependence chains are indistinguishable.
  Chain = Struct.new(:name, :leaves) do
    def touches?(leaf_names)
      leaves.intersect?(leaf_names)
    end
  end

  Options = Struct.new(
    :pass, :list_passes, :leaves, :memory, :data, :control, :blocks,
    :data_labels, :float_constants, :control_effects, :compact_labels, :merge_chains, :unwritten_chains, :format, :out, :open_with, :seafoam_lib, :dot, :json,
    keyword_init: true
  )

  class << self
    def parse_options(argv)
      options = Options.new(
        pass: nil, list_passes: false, leaves: nil, memory: true, data: true, control: true,
        blocks: true, data_labels: true, float_constants: true, control_effects: false, compact_labels: true, merge_chains: true, unwritten_chains: false, format: "svg", out: nil, open_with: nil,
        seafoam_lib: File.expand_path("~/Documents/code/seafoam/lib"), dot: false, json: false
      )

      parser = OptionParser.new do |o|
        o.banner = "Usage: iongraph2seafoam.rb DUMP.json [options]"
        o.on("--pass NAME", "Pass to render: a name, an index, or 'first'/'last' (default: last)") { |v| options.pass = v }
        o.on("--list-passes", "List the passes in the dump and exit") { options.list_passes = true }
        o.on("--leaves LIST", "Only thread these effect leaves, comma separated (default: all)") do |v|
          options.leaves = v.split(",").map(&:strip).reject(&:empty?)
        end
        o.on("--[no-]memory", "Draw split-memory dependence edges (default: on)") { |v| options.memory = v }
        o.on("--[no-]data", "Draw SSA data edges (default: on)") { |v| options.data = v }
        o.on("--[no-]control", "Draw control flow edges between blocks (default: on)") { |v| options.control = v }
        o.on("--[no-]data-labels", "Name each data edge after the operand it carries, vNNN " \
                                   "(default: on)") { |v| options.data_labels = v }
        o.on("--[no-]float-constants", "Draw #{FLOATING.join("/")} beside each user rather than in " \
                                       "the block (default: on)") { |v| options.float_constants = v }
        o.on("--[no-]blocks", "Draw basic blocks as clusters (default: on)") { |v| options.blocks = v }
        o.on("--[no-]control-effects", "Honour the effects::Any on control flow instructions " \
                                       "(#{CONTROL_BARRIERS.join(", ")}) instead of folding them " \
                                       "into the entry tokens, and on the exits " \
                                       "(#{EXITS.join(", ")}) instead of reading them as memory " \
                                       "sinks (default: off)") do |v|
          options.control_effects = v
        end
        o.on("--[no-]compact-labels", "Strip hex addresses from labels (default: on)") { |v| options.compact_labels = v }
        o.on("--[no-]merge-chains", "Thread leaves that no instruction distinguishes as one " \
                                    "chain (default: on)") { |v| options.merge_chains = v }
        o.on("--[no-]unwritten-chains", "Draw chains that nothing writes, whose reads are all of " \
                                        "the entry token and so order nothing (default: off)") do |v|
          options.unwritten_chains = v
        end
        o.on("--format FORMAT", "Output format for dot: svg, pdf, png (default: svg)") { |v| options.format = v }
        o.on("--out PATH", "Where to write the rendered graph") { |v| options.out = v }
        o.on("--open [APP]", "Open the result when done, optionally with a named application") do |v|
          options.open_with = v || true
        end
        o.on("--dot", "Write Graphviz DOT source instead of rendering") { options.dot = true }
        o.on("--json", "Write the Seafoam graph as JSON instead of rendering") { options.json = true }
        o.on("--seafoam PATH", "Path to seafoam's lib directory") { |v| options.seafoam_lib = v }
        o.on("-h", "--help") do
          puts o
          exit
        end
      end

      rest = parser.parse(argv)
      if rest.size != 1
        warn parser.help
        exit 1
      end
      [rest.first, options]
    end

    def run(argv)
      file, options = parse_options(argv)
      dump = JSON.parse(File.read(file))
      passes = dump.fetch("passes")

      if options.list_passes
        passes.each_with_index { |pass, index| puts format("%2d  %s", index, pass.fetch("name")) }
        return
      end

      pass = select_pass(passes, options.pass)
      load_seafoam(options.seafoam_lib)
      graph = Builder.new(pass, options).build
      name = "#{dump.fetch("name")} (#{pass.fetch("name")})"
      emit(graph, name, options)
    end

    def select_pass(passes, requested)
      case requested
      when nil, "last" then passes.last
      when "first" then passes.first
      when /\A\d+\z/
        index = requested.to_i
        passes.fetch(index) { abort "No pass at index #{index}; the dump has #{passes.size}" }
      else
        # Passes repeat across fixpoint iterations, so take the last run of the named pass.
        passes.select { |pass| pass.fetch("name") == requested }.last ||
          abort("No pass named #{requested.inspect}. Try --list-passes")
      end
    end

    def load_seafoam(lib)
      $LOAD_PATH.unshift(lib) if lib && File.directory?(lib)
      require "seafoam"
    rescue LoadError
      abort "Could not load seafoam. Clone https://github.com/Shopify/seafoam and pass --seafoam PATH/lib"
    end

    def emit(graph, name, options)
      if options.json
        write_to(options.out) { |stream| Seafoam::JSONWriter.new(stream).write(name, graph) }
        return
      end

      if options.dot
        write_to(options.out) { |stream| write_dot(stream, graph, options) }
        return
      end

      out = options.out || File.join(Dir.tmpdir, "#{sanitize(name)}.#{options.format}")
      render(graph, out, options)
      puts out
      open_result(out, options.open_with) if options.open_with
    end

    def write_dot(stream, graph, options)
      Seafoam::GraphvizWriter.new(stream).write_graph(graph, false, options.blocks)
    end

    def write_to(path)
      if path
        File.open(path, "w") { |stream| yield stream }
        puts path
      else
        yield $stdout
      end
    end

    def render(graph, out, options)
      IO.popen(["dot", "-T#{options.format}", "-o", out], "w") do |stream|
        write_dot(stream, graph, options)
      end
      abort "dot failed" unless $?.success?
    end

    def open_result(out, app)
      if app == true
        system("open", out) || system("xdg-open", out)
      else
        system("open", "-a", app, out) || system(app, out)
      end
    end

    def sanitize(name)
      name.gsub(/[^A-Za-z0-9_.-]+/, "_")
    end
  end

  # Turns one iongraph pass into a Seafoam graph.
  class Builder
    def initialize(pass, options)
      @pass = pass
      @options = options
      @blocks = pass.fetch("mir").fetch("blocks")
      @graph = Seafoam::Graph.new(name: pass.fetch("name"))
      # Seafoam labels every node with its id, so keep synthetic ids in the same range as the
      # instruction ids rather than off in a high numbered block of their own.
      @next_synthetic_id = @blocks.flat_map { |block| block.fetch("instructions").map { |insn| insn.fetch("id") } }.max.to_i
      # Memory edges are collected per (from, to) pair so that an instruction reading five
      # leaves from one token gets one labelled edge rather than five parallel ones.
      @memory_edges = {}
      @block_node_ids = {}
      @leaves = resolve_leaves
    end

    def build
      create_instruction_nodes
      create_data_edges if @options.data
      create_control_edges if @options.control
      SplitMemory.new(self, @blocks, chains, @options).build if @options.memory && !@leaves.empty?
      flush_memory_edges
      create_blocks if @options.blocks
      @graph
    end

    attr_reader :graph

    # Whether this instruction's declared effects should be ignored. See CONTROL_BARRIERS.
    def barrier?(insn)
      return false if @options.control_effects

      CONTROL_BARRIERS.include?(insn.fetch("opcode")[/\A\w+/])
    end

    # Whether this instruction ends the function and so reads every chain. See EXITS.
    def sink?(insn)
      return false if @options.control_effects

      EXITS.include?(insn.fetch("opcode")[/\A\w+/])
    end

    # Whether this instruction's declared effects are taken at face value. A sink's reads are
    # imposed rather than declared, and they cover every leaf, so counting them would mark every
    # leaf as touched and written and defeat both chain merging and the unwritten-chain prune.
    def declared_effects?(insn)
      !barrier?(insn) && !sink?(insn)
    end

    def node_for(id)
      @graph.nodes[id]
    end

    # Record a memory dependence. Parallel dependences between the same two nodes are merged into
    # one edge carrying both labels.
    def add_memory_edge(from, to, label)
      return if from.nil? || to.nil?

      (@memory_edges[[from.id, to.id]] ||= []).push(label)
    end

    def create_synthetic_node(label, block_id, kind: "virtual")
      id = (@next_synthetic_id += 1)
      node = @graph.create_node(id, label: label, kind: kind, synthetic: true, synthetic_class: kind)
      (@block_node_ids[block_id] ||= []).push(id) if block_id
      node
    end

    private

    # The dump publishes the lattice leaves; fall back to whatever the instructions mention in
    # case we are reading a dump from a build without `effectLeaves`.
    def resolve_leaves
      published = @pass["effectLeaves"] || touched_leaves.sort
      requested = @options.leaves
      if requested
        unknown = requested - published
        abort "Unknown effect leaves: #{unknown.join(", ")}. Known: #{published.join(", ")}" unless unknown.empty?
      end

      # A leaf that no instruction reads or writes has an empty chain, so its token would just
      # dangle in the entry block.
      (requested || published) & touched_leaves.to_a
    end

    # Split memory has one component per chain. A chain is a set of leaves whose read/write
    # pattern over the whole function is identical, so threading them separately would draw the
    # same graph several times over; the chain is named after the leaves it stands for, joined
    # with `+`. Only leaves are ever threaded: the dump decomposes unions such as Memory into
    # their leaves before we see them, and a chain groups leaves this function happens not to
    # distinguish, which need not be a union of the lattice.
    def chains
      groups = if @options.merge_chains
        @leaves.group_by { |leaf| leaf_signature(leaf) }.values
      else
        @leaves.map { |leaf| [leaf] }
      end
      # `+` rather than `|`: a merged chain is a coalesced set of leaves, not a lattice union.
      built = groups.map { |leaves| Chain.new(leaves.join("+"), leaves.to_set) }
      return built if @options.unwritten_chains

      built.select { |chain| chain.touches?(written_leaves) }
    end

    def written_leaves
      @written_leaves ||= effectful_instructions
        .flat_map { |insn| Array((insn["effects"] || {})["write"]) }.to_set
    end

    # What every instruction does to this leaf, in program order.
    def leaf_signature(leaf)
      effectful_instructions.map do |insn|
        effects = insn["effects"] || {}
        access = Array(effects["read"]).include?(leaf) ? "r" : ""
        access += "w" if Array(effects["write"]).include?(leaf)
        access.empty? ? nil : [insn.fetch("id"), access]
      end.compact
    end

    def effectful_instructions
      @effectful_instructions ||= @blocks.flat_map { |block| block.fetch("instructions") }
        .select { |insn| declared_effects?(insn) }
    end

    def touched_leaves
      @touched_leaves ||= @blocks.flat_map { |block|
        block.fetch("instructions").select { |insn| declared_effects?(insn) }.flat_map do |insn|
          effects = insn["effects"] || {}
          Array(effects["read"]) + Array(effects["write"])
        end
      }.to_set
    end

    def create_instruction_nodes
      @blocks.each do |block|
        ids = (@block_node_ids[block.fetch("id")] ||= [])
        block.fetch("instructions").each do |insn|
          id = insn.fetch("id")
          @graph.create_node(id, node_props(insn))
          ids.push(id)
        end
      end
    end

    def node_props(insn)
      opcode = insn.fetch("opcode")
      opcode = ADDRESS_PATTERNS.inject(opcode) { |text, pattern| text.gsub(pattern, "") } if @options.compact_labels
      type = insn["type"].to_s
      label = type.empty? ? opcode : "#{opcode}\n#{type}"
      props = { label: label, kind: kind_for(opcode), opcode: opcode, type: type, effects: insn["effects"] }
      # A constant has no effects and no operands, so where it sits in the block says nothing.
      # Seafoam's `inlined` draws such a node once per user, as a small oval beside the consumer
      # and outside the block, which keeps a shared constant from stretching edges across the
      # graph. See FLOATING in this file's header.
      props[:inlined] = true if @options.float_constants && FLOATING.include?(opcode[/\A\w+/])
      props
    end

    def kind_for(opcode)
      prefix = opcode[/\A[A-Za-z_][A-Za-z0-9_]*/]
      return "other" if prefix.nil?

      KIND_BY_PREFIX.fetch(prefix) do
        # Guards are worth calling out as a family rather than one by one.
        next "guard" if prefix.start_with?("Guard", "Check", "AdjustBounds")

        "calc"
      end
    end

    # SSA operands. Each edge is named after the value it carries, so it can be matched against
    # the `vNNN` operands printed in the node's own label. An operand an instruction uses twice
    # draws one edge: a second copy would carry the same name and so say nothing more. Inputs
    # naming a Snapshot are dropped: snapshots are not serialized as instructions, so there is no
    # node to point at.
    def create_data_edges
      each_instruction do |insn, _block|
        to = node_for(insn.fetch("id"))
        insn.fetch("inputs").uniq.each do |input_id|
          from = node_for(input_id)
          next unless from

          props = DATA_EDGE.dup
          props[:label] = "v#{input_id}" if @options.data_labels
          @graph.create_edge(from, to, props)
        end
      end
    end

    # HIR control flow lives in the block structure rather than in edges, so join each block's
    # terminator to the first instruction of each successor.
    def create_control_edges
      by_id = @blocks.to_h { |block| [block.fetch("id"), block] }
      @blocks.each do |block|
        terminator = block.fetch("instructions").last
        next if terminator.nil?

        from = node_for(terminator.fetch("id"))
        block.fetch("successors").each do |successor_id|
          successor = by_id[successor_id]
          next if successor.nil?

          entry = successor.fetch("instructions").first
          next if entry.nil?

          to = node_for(entry.fetch("id"))
          @graph.create_edge(from, to, CONTROL_EDGE.dup) if from && to
        end
      end
    end

    def flush_memory_edges
      @memory_edges.each do |(from_id, to_id), labels|
        @graph.create_edge(
          @graph.nodes[from_id], @graph.nodes[to_id],
          kind: "info", label: labels.uniq.join(", "), memory: true
        )
      end
    end

    def create_blocks
      @blocks.each do |block|
        id = block.fetch("id")
        @graph.create_block(id, @block_node_ids[id] || [])
      end
    end

    def each_instruction
      @blocks.each do |block|
        block.fetch("instructions").each { |insn| yield insn, block }
      end
    end
  end

  # Threads one dependence chain per effect lattice leaf through the CFG.
  #
  # Each leaf behaves like an SSA value of its own: the entry block defines it, a write
  # redefines it, and a merge point phis the incoming definitions together. Blocks arrive in
  # reverse post order, so a loop header can be reached before its back edge has been walked;
  # those headers get a phi up front whose inputs are filled in once every predecessor is done.
  class SplitMemory
    Phi = Struct.new(:node, :block_id, :chain)

    def initialize(builder, blocks, chains, options)
      @builder = builder
      @blocks = blocks
      @chains = chains
      @options = options
      @in_state = {}
      @out_state = {}
      @pending_phis = []
    end

    def build
      by_id = @blocks.to_h { |block| [block.fetch("id"), block] }
      @blocks.each { |block| process_block(block) }
      resolve_pending_phis(by_id)
    end

    private

    def process_block(block)
      id = block.fetch("id")
      state = entry_state(block)
      @in_state[id] = state.dup

      block.fetch("instructions").each do |insn|
        if @builder.sink?(insn)
          apply_sink(insn, state)
        elsif !barrier?(insn)
          apply_effects(insn, state)
        end
      end

      @out_state[id] = state
    end

    # Control flow instructions claim `effects::Any`, which would make every block boundary a
    # full barrier and phi every leaf at every merge. See CONTROL_BARRIERS.
    def barrier?(insn)
      @builder.barrier?(insn)
    end

    def apply_effects(insn, state)
      effects = insn["effects"] || {}
      read = Array(effects["read"]).to_set
      written = Array(effects["write"]).to_set
      node = @builder.node_for(insn.fetch("id"))
      return if node.nil?

      reads = @chains.select { |chain| chain.touches?(read) }
      writes = @chains.select { |chain| chain.touches?(written) }

      # A read consumes the current token; a write consumes it and produces a fresh one.
      (reads | writes).each { |chain| @builder.add_memory_edge(state[chain], node, chain.name) }
      writes.each { |chain| state[chain] = node }
    end

    # An exit consumes the final token of every chain and produces none. See EXITS.
    def apply_sink(insn, state)
      node = @builder.node_for(insn.fetch("id"))
      return unless node

      @chains.each { |chain| @builder.add_memory_edge(state[chain], node, chain.name) }
    end

    def entry_state(block)
      id = block.fetch("id")
      predecessors = block.fetch("predecessors")

      # The start of the CFG is where split memory comes from: one token per leaf.
      return initial_state(id) if predecessors.empty?

      return @out_state.fetch(predecessors.first).dup if predecessors.size == 1

      merge_state(id, predecessors)
    end

    def initial_state(block_id)
      @chains.to_h do |chain|
        [chain, @builder.create_synthetic_node("Memory\n#{chain.name}", block_id)]
      end
    end

    def merge_state(block_id, predecessors)
      visited, unvisited = predecessors.partition { |pred| @out_state.key?(pred) }

      @chains.to_h do |chain|
        incoming = visited.map { |pred| @out_state.fetch(pred)[chain] }.uniq

        if unvisited.empty? && incoming.size == 1
          # Every predecessor agrees, so there is nothing to merge.
          [chain, incoming.first]
        else
          phi = @builder.create_synthetic_node("Phi\n#{chain.name}", block_id)
          incoming.each { |def_node| @builder.add_memory_edge(def_node, phi, chain.name) }
          @pending_phis.push(Phi.new(phi, block_id, chain)) unless unvisited.empty?
          [chain, phi]
        end
      end
    end

    # Fill in the back edge inputs of loop header phis, now that every block has an out state.
    def resolve_pending_phis(by_id)
      @pending_phis.each do |phi|
        by_id.fetch(phi.block_id).fetch("predecessors").each do |pred|
          out = @out_state[pred]
          next if out.nil?

          @builder.add_memory_edge(out[phi.chain], phi.node, phi.chain.name)
        end
      end
    end
  end
end

IonGraph2Seafoam.run(ARGV) if $PROGRAM_NAME == __FILE__
