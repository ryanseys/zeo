# A class sits ABOVE Kernel in the ancestry, so its own method answers a bare
# call before a Kernel builtin of the same name does. Spinel reached the Kernel
# arms by position -- the implicit-self resolution sits far below them in both
# halves of the compiler -- so `def puts` in a class was emitted and never
# called.
class K
  def puts(v) = "class-puts:#{v}"
  def print(v) = "class-print:#{v}"
  def p(v) = "class-p:#{v}"
  def caller(v) = "class-caller:#{v}"
  def raise(v) = "class-raise:#{v}"
  def system(v) = "class-system:#{v}"
  def exit(v) = "class-exit:#{v}"
  def loop(v) = "class-loop:#{v}"
  def throw(v) = "class-throw:#{v}"
  def abort(v) = "class-abort:#{v}"
  def warn(v) = "class-warn:#{v}"

  def run
    say(puts(1))
    say(print(2))
    say(p(3))
    say(caller(4))
    say(raise(5))
    say(system(6))
    say(exit(7))
    say(loop(8))
    say(throw(9))
    say(abort(10))
    say(warn(11))
  end

  def say(s)
    STDOUT.write(s)
    STDOUT.write("\n")
  end
end

K.new.run

# an attr reader on the class answers a bare call the same way
class R
  attr_reader :format

  def initialize
    @format = "reader-format"
  end

  def run = format
end

STDOUT.write(R.new.run)
STDOUT.write("\n")

# class > top-level > Kernel, all three present
def puts(v) = "top-puts:#{v}"

class Three
  def puts(v) = "class-puts:#{v}"
  def own = puts(30)
  def notMine = caller(31)
end

def caller(v) = "top-caller:#{v}"

t = Three.new
STDOUT.write(t.own)
STDOUT.write("\n")
STDOUT.write(t.notMine)
STDOUT.write("\n")
STDOUT.write(puts(32))
STDOUT.write("\n")
