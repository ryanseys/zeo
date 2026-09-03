# Bounded recursion 10_000 deep -- plain, mutual, through a lambda, and on a
# spawned thread -- answers rather than raising. CRuby's default VM stack
# takes this depth; zeo's frames are native frames on a 64 MiB stack
# (`zeo_rt::exec::MAIN_STACK_SIZE`), which the darwin link line sizes and a
# spawned thread carries elsewhere. `infinite_recursion_aborts.rb` pins the
# other side: past the stack, `SystemStackError` is rescuable.

def down(n) = n.zero? ? 0 : 1 + down(n - 1)
p down(10_000)

def ping(n) = n.zero? ? :ping : pong(n - 1)
def pong(n) = n.zero? ? :pong : ping(n - 1)
p ping(10_000)

count = nil
count = ->(n) { n.zero? ? 0 : 1 + count.(n - 1) }
p count.(10_000)

p Thread.new { down(10_000) }.value
__END__
10000
:ping
10000
10000
