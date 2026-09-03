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

class R
  attr_reader :format

  def initialize
    @format = "reader-format"
  end

  def run = format
end

STDOUT.write(R.new.run)
STDOUT.write("\n")

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
__END__
class-puts:1
class-print:2
class-p:3
class-caller:4
class-raise:5
class-system:6
class-exit:7
class-loop:8
class-throw:9
class-abort:10
class-warn:11
reader-format
class-puts:30
top-caller:31
top-puts:32
