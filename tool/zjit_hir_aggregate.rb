#!/usr/bin/env ruby
# Aggregate samply profile samples by HIR opcode.
#
# Usage:
#   ruby tool/zjit_hir_aggregate.rb [profile.json]
#
# If no profile is given, looks for /tmp/zjit-profile.json or the most recent
# /tmp/zjit-*.json file.
#
# The script cross-references the profile JSON with the jitdump and HIR files
# to show which HIR opcodes consume the most time across all JIT-compiled methods.
#
# Output example:
#   Total JIT samples: 1234 (45.2% of all samples)
#
#   By HIR opcode (self samples in JIT code):
#     SendDirect         312  25.3%  ||||||||||||
#     CCall              198  16.0%  ||||||||
#     LoadField          156  12.6%  ||||||
#     GuardType          134  10.9%  |||||
#     ...

require 'json'

def find_profile
  if ARGV[0] && File.exist?(ARGV[0])
    return ARGV[0]
  end
  candidates = Dir["/tmp/zjit-*.json"].sort_by { |f| File.mtime(f) }
  candidates.last or abort "No profile found. Run tool/zjit_profile.sh first."
end

def find_hir_file(pid = nil)
  zjit_dir = File.expand_path("~/.zjit")
  if pid
    path = "#{zjit_dir}/hir-#{pid}.src"
    return path if File.exist?(path)
  end
  candidates = Dir["#{zjit_dir}/hir-*.src"].sort_by { |f| File.mtime(f) }
  candidates.last
end

def find_address_map(pid = nil)
  zjit_dir = File.expand_path("~/.zjit")
  if pid
    path = "#{zjit_dir}/hir-#{pid}.map"
    return path if File.exist?(path)
  end
  candidates = Dir["#{zjit_dir}/hir-*.map"].sort_by { |f| File.mtime(f) }
  candidates.last
end

# Extract the PID from the profile's jitdump lib reference
def extract_pid_from_profile(profile)
  profile["libs"]&.each do |lib|
    if lib["name"] =~ /jit-(\d+)\.dump/
      return $1.to_i
    end
  end
  thread = profile["threads"]&.find { |t| t["isMainThread"] } || profile["threads"]&.first
  thread&.dig("pid")&.to_i
end

# Parse the .map file (text format written by codegen alongside the HIR file)
# Format:
#   F func_name start_addr code_size
#     addr line
#     addr line
def parse_address_map(path)
  functions = {} # code_addr -> { name:, code_size:, debug_entries: [{addr:, line:}] }
  current_func = nil

  File.readlines(path).each do |line|
    line.chomp!
    if line.start_with?("F ")
      parts = line.split(" ")
      # F zjit::name 0xaddr size
      name = parts[1]
      code_addr = Integer(parts[2])
      code_size = Integer(parts[3])
      current_func = { name: name, code_addr: code_addr, code_size: code_size, debug_entries: [] }
      functions[code_addr] = current_func
    elsif line.strip =~ /^(0x[0-9a-f]+)\s+(\d+)$/ && current_func
      addr = Integer($1)
      lineno = Integer($2)
      current_func[:debug_entries] << { addr: addr, line: lineno }
    end
  end

  functions
end

# Parse HIR file to extract opcode from each line
def parse_hir_opcodes(path)
  lines = {} # line_number (1-based) -> opcode string
  File.readlines(path).each_with_index do |line, idx|
    lineno = idx + 1
    stripped = line.strip
    next if stripped.empty? || stripped.start_with?("fn ") || stripped.match?(/^bb\d+/)

    # Extract opcode: "v42:Fixnum = FixnumAdd v28, v29" -> "FixnumAdd"
    # or "CheckInterrupts" -> "CheckInterrupts"
    # or "Return v33" -> "Return"
    if stripped =~ /=\s+(\w+)/
      lines[lineno] = $1
    elsif stripped =~ /^(\w+)/
      lines[lineno] = $1
    end
  end
  lines
end

# Build a lookup: absolute_address -> { func_name, hir_line, hir_opcode }
def build_address_lookup(jitdump_funcs, hir_opcodes)
  # For each function, build sorted list of (addr, line) for binary search
  lookups = [] # [{range_start, range_end, line, func_name}]

  jitdump_funcs.each_value do |func|
    entries = func[:debug_entries].sort_by { |e| e[:addr] }
    entries.each_with_index do |entry, i|
      range_end = if i + 1 < entries.size
        entries[i + 1][:addr]
      else
        func[:code_addr] + func[:code_size]
      end
      lookups << {
        range_start: entry[:addr],
        range_end: range_end,
        line: entry[:line],
        func_name: func[:name],
      }
    end
  end

  lookups.sort_by! { |l| l[:range_start] }
  lookups
end

def lookup_address(lookups, addr)
  # Binary search for the entry containing addr
  lo, hi = 0, lookups.size - 1
  result = nil
  while lo <= hi
    mid = (lo + hi) / 2
    entry = lookups[mid]
    if addr < entry[:range_start]
      hi = mid - 1
    elsif addr >= entry[:range_end]
      lo = mid + 1
    else
      result = entry
      break
    end
  end
  result
end

# Main
profile_path = find_profile
profile = JSON.parse(File.read(profile_path))
pid = extract_pid_from_profile(profile)
hir_path = find_hir_file(pid)
map_path = find_address_map(pid)

unless hir_path && map_path
  zjit_dir = File.expand_path("~/.zjit")
  abort "Missing HIR or address map for PID #{pid}. Run with --zjit --zjit-perf first.\n" \
        "Looked for: #{zjit_dir}/hir-#{pid}.src and #{zjit_dir}/hir-#{pid}.map"
end

$stderr.puts "Profile: #{profile_path}"
$stderr.puts "HIR:     #{hir_path}"
$stderr.puts "Map:     #{map_path}"
$stderr.puts ""

thread = profile["threads"].find { |t| t["isMainThread"] } || profile["threads"][0]

sa = thread["stringArray"]
ft = thread["frameTable"]
func_table = thread["funcTable"]
ns = thread["nativeSymbols"]
samples = thread["samples"]
stack_table = thread["stackTable"]

# Parse address map and HIR
jitdump_funcs = parse_address_map(map_path)
hir_opcodes = parse_hir_opcodes(hir_path)
lookups = build_address_lookup(jitdump_funcs, hir_opcodes)

# Find the jitdump lib index
jitdump_lib_idx = nil
profile["libs"].each_with_index do |lib, i|
  if lib["name"] =~ /jit-\d+\.dump/
    jitdump_lib_idx = i
    break
  end
end

# Find the base address of the jitdump lib
jitdump_base = 0
if jitdump_lib_idx
  # nativeSymbols have addresses relative to the lib base
  # We need to find the actual base from the first CODE_LOAD
  first_func = jitdump_funcs.values.first
  if first_func
    first_ns = (0...ns["length"]).find { |i| ns["libIndex"][i] == jitdump_lib_idx }
    if first_ns
      # The native symbol addr is relative to lib base
      # code_addr from jitdump is absolute
      jitdump_base = first_func[:code_addr] - ns["address"][first_ns]
    end
  end
end

# Walk all samples, resolve self frames to HIR opcodes
total_samples = samples["length"]
jit_samples = 0
opcode_counts = Hash.new(0)
func_opcode_counts = Hash.new { |h, k| h[k] = Hash.new(0) }

total_samples.times do |i|
  stack_idx = samples["stack"][i]

  # The self frame is the leaf of the stack
  frame_idx = stack_table["frame"][stack_idx]
  ns_idx = ft["nativeSymbol"][frame_idx]

  next unless ns_idx && ns_idx >= 0
  next unless ns["libIndex"][ns_idx] == jitdump_lib_idx

  jit_samples += 1

  # Compute absolute address
  relative_addr = ft["address"][frame_idx]
  abs_addr = jitdump_base + relative_addr

  entry = lookup_address(lookups, abs_addr)
  if entry
    opcode = hir_opcodes[entry[:line]] || "unknown(line:#{entry[:line]})"
    opcode_counts[opcode] += 1
    func_opcode_counts[entry[:func_name]][opcode] += 1
  else
    opcode_counts["(unmapped)"] += 1
  end
end

puts "Total samples: #{total_samples}"
puts "JIT self samples: #{jit_samples} (#{"%.1f" % (jit_samples * 100.0 / total_samples)}%)"
puts ""

if jit_samples == 0
  puts "No JIT samples found."
  exit
end

# Sort by count descending
sorted = opcode_counts.sort_by { |_, c| -c }
max_count = sorted.first[1]

puts "By HIR opcode (self samples in JIT code):"
puts "  #{"Opcode".ljust(30)} #{"Count".rjust(7)}  #{"Pct".rjust(6)}"
puts "  #{"-" * 30} #{"-" * 7}  #{"-" * 6}"
sorted.each do |opcode, count|
  pct = count * 100.0 / jit_samples
  bar = "|" * (count * 40 / max_count)
  puts "  #{opcode.ljust(30)} #{count.to_s.rjust(7)}  #{("%5.1f%%" % pct).rjust(6)}  #{bar}"
end

# Top functions by JIT self samples
puts ""
puts "Top functions by JIT self samples:"
func_totals = func_opcode_counts.transform_values { |opcodes| opcodes.values.sum }
func_totals.sort_by { |_, c| -c }.first(20).each do |func_name, count|
  pct = count * 100.0 / jit_samples
  top_opcodes = func_opcode_counts[func_name].sort_by { |_, c| -c }.first(3)
    .map { |op, c| "#{op}:#{c}" }.join(", ")
  puts "  #{("%5.1f%%" % pct).rjust(6)}  #{count.to_s.rjust(5)}  #{func_name}"
  puts "           #{top_opcodes}"
end
