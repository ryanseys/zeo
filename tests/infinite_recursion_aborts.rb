# Unbounded recursion kills the process with a native stack overflow. Ruby
# raises `SystemStackError`, which is an ordinary rescuable exception -- the
# program below catches it and keeps running.
#
# zeo compiles a ruby method to a Rust function, so ruby's frames are the
# machine's frames and running out of them is a hardware fault, not a condition
# anything can observe. Nothing checks the remaining stack before recursing, so
# there is no point at which the error could be raised.
#
# Accidental infinite recursion is a routine bug -- a `method_missing` that
# forwards to itself, a `to_s` that interpolates the object, a cycle in a
# resolver -- and ruby's answer is a normal backtrace that names the method. An
# abort gives a signal exit status, no backtrace, and nothing for a test runner
# or a supervisor to catch. It is also unrescuable, so an `ensure` that would
# release a lock or a file never runs.
#
# Bounded deep recursion is fine and must stay fast: 5000 frames answers below.
#
# Fix shape: a stack-limit check on entry to a compiled method, the way CRuby
# checks `ruby_stack_check`. The cost has to be a comparison against a cached
# limit, not a syscall, or it taxes every call in every program.

def down(n) = n.zero? ? 0 : 1 + down(n - 1)
p down(5000)

def forever = forever
begin
  forever
rescue SystemStackError => e
  puts "rescued: #{e.class}"
end

# Mutual recursion reaches the same limit.
def ping(n) = pong(n)
def pong(n) = ping(n)
begin
  ping(1)
rescue SystemStackError
  puts "rescued mutual"
end

# A lambda recursing through a captured local, too.
loopy = nil
loopy = -> { loopy.call }
begin
  loopy.call
rescue SystemStackError
  puts "rescued lambda"
end

puts "still running"
