# The 64 MiB main stack (`zeo_rt::MAIN_STACK_SIZE`) is zeo's own guarantee,
# past what CRuby's 1 MiB VM stack takes (it overflows near 11_000), so no
# oracle golden can pin it: 100_000 frames sits at half the measured
# ceiling (200_000 answered, 400_000 raised). On macOS this is the PROCESS
# main thread's stack, sized by the link line; on Linux the spawned one's.

def down(n) = n.zero? ? 0 : 1 + down(n - 1)
p down(100_000)
p Thread.current.equal?(Thread.main)
__END__
100000
true
