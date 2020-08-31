def fib(n)
    if n < 2
        return n
    end

    return fib(n-1) + fib(n-2)
end

start_time = Time.now.to_f
r = fib(38)
end_time = Time.now.to_f
time_ms = ((end_time - start_time) * 1000).to_i
puts "#{time_ms}ms"
