def foo
    a = 1 + 1
end

def bar
    a = 3 + 4
    foo
    foo
    foo
end

iseq = RubyVM::InstructionSequence.compile(<<-RB, nil, nil, __LINE__+1, tailcall_optimization: true)
def leaf
    true
end

def tco
    a = 1 + 1
    leaf
end
RB
puts iseq.disasm
iseq.eval

trace = TracePoint.new(:call) do |tp|
    puts "line #{tp.lineno} #{tp.method_id} is a tailcall? #{tp.tailcall?}"
    puts '-' * 20 + '*'
end

trace.enable do
    tco
end


puts "phew didn't crash"
exit(0)
puts RubyVM::InstructionSequence.compile("def a \n jojo\nend", tailcall_optimization:true).disasm
puts RubyVM::InstructionSequence.compile("def a \n jojo + 5\nend", tailcall_optimization:true).disasm
puts RubyVM::InstructionSequence.compile("def a \n  jojo(234 + 234)\nend", tailcall_optimization:true).disasm
