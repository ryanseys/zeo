# Every one of these ended a zeo process instead of raising.
#
# `io.wait(:read)` panicked: the Symbol lands in the timeout slot, which went
# through an "unchecked" float conversion that is a `panic!`. `f.read(10**18)`
# allocated eagerly, and an allocation failure is an ABORT, not a rescuable
# NoMemoryError. `$stdout.ioctl(TIOCGWINSZ, "")` handed the kernel a
# zero-capacity buffer to write eight bytes through -- heap corruption from
# pure Ruby. `io.goto(2**63 - 1, 0)` overflowed an i64.
#
# None of them is exotic; each is one wrong argument away from ordinary code.

require "io/console"
require "io/wait"

def refusal
  v = yield
  "no refusal: #{v.inspect}"
rescue Exception => e
  "#{e.class}: #{e.message}"
end

r, w = IO.pipe

# A timeout that is not a number.
puts refusal { r.wait_readable("1") }
puts refusal { w.wait_writable(:soon) }
puts refusal { r.wait(:read, "x") }
puts refusal { r.wait(:nope, 0.05) }
# Arguments sort by TYPE, so both orders name the same wait.
puts refusal { [r.wait(:read, 0.05), r.wait(0.05, :read)].inspect }
puts refusal { IO.select([r], nil, nil, "x") }

# A length no allocator can serve.
f = File.open("/etc/hosts")
puts refusal { f.read(10**18) }
puts refusal { f.readpartial(10**18) }
puts refusal { f.pread(10**18, 0) }
puts refusal { f.read(-1) }
puts refusal { f.readpartial(-1) }
f.close

# A coordinate the escape cannot carry.
puts refusal { w.goto(2**63 - 1, 0) }
puts refusal { w.goto(0, 2**40) }
puts refusal { w.goto_column(-2**31 - 1) }
puts refusal { w.cursor = [2**33, 0] }
puts refusal { w.winsize = [2**33, 80] }

# A negative coordinate is legal, and wraps the way NUM2UINT does.
w.goto(-1, -1)
w.goto_column(-1)
w.cursor = [-2, -3]
w.flush
p r.read_nonblock(80)

r.close
w.close
