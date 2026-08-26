# A user `def puts` on `Kernel` or `Object` answers every receiver-less
# `puts` in the program.
#
# `puts` is the one Kernel primitive with a direct fold at the call site,
# and the fold did not ask whether the program had redefined it -- so the
# override was registered, reachable through `send` and from inside a
# block, and skipped by every plain `puts` line. Wrong output, clean exit.

module Kernel
  def puts(*a)
    $stdout.write("K:" + a.join(",") + "\n")
  end
end

puts "a", "b"
send(:puts, "c")
[1].each { |x| puts x }

# A class of its own still wins over the universal, the way ruby's lookup
# orders them.
class Loud
  def puts(*a)
    $stdout.write("L:" + a.join(",") + "\n")
  end

  def go = puts("inside")
end
Loud.new.go
