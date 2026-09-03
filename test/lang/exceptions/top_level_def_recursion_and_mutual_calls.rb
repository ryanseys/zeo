def fib(n)
  n < 2 ? n : fib(n - 1) + fib(n - 2)
end
def announce(n)
  puts "fib(#{n}) = #{fib(n)}"
end
announce(10)
__END__
fib(10) = 55
