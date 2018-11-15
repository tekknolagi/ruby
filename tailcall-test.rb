def foo
    a = 1 + 1
end

def bar
    a = 3 + 4
    foo   
    foo   
    foo   
end

tailcall_source = <<-RB
def leaf
    true
end

def tco
    a = 1 + 1
    leaf
    leaf
end
RB

iseq = RubyVM::InstructionSequence.compile(tailcall_source, tailcall_optimization: true)
puts iseq.disasm
iseq.eval

trace = TracePoint.new(:call) do |tp|
    puts "#{tp.method_id} is a tailcall? #{tp.tailcall?}"
    puts '-' * 20 + '*'
end

trace.enable do
    bar
    tco
end


puts "phew didn't crash"
puts RubyVM::InstructionSequence.of(method(:bar)).disasm
exit(0)
puts RubyVM::InstructionSequence.compile("def a \n jojo\nend", tailcall_optimization:true).disasm
puts RubyVM::InstructionSequence.compile("def a \n jojo + 5\nend", tailcall_optimization:true).disasm
puts RubyVM::InstructionSequence.compile("def a \n  jojo(234 + 234)\nend", tailcall_optimization:true).disasm
