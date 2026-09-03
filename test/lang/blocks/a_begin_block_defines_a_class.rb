# `BEGIN { ... }` runs before the main program, in the top-level scope and
# the top-level cref -- so a `class`, a `module` or a `def` written there is
# an ordinary top-level definition that simply happens earlier. sekrets and
# telesign both put their whole API inside one.
#
# The hoisting is what makes the last line work: `Early` is used above the
# BEGIN that defines it, and several BEGIN blocks run in source order among
# themselves.
class Recorder
  def self.method_added(name)
    puts "added #{name}"
  end
end

puts "main: #{Early.new.hi} / #{Late::WHO} / #{early_helper}"
puts "main: reopened #{Recorder.new.respond_to?(:from_begin)}"

BEGIN {
  puts "first begin"

  class Early
    def hi = "early hi"
  end

  module Late
    WHO = "late"
  end

  def early_helper = "helper"

  class Recorder
    def from_begin = :yes
  end
}

BEGIN {
  puts "second begin"
  Early.class_eval { def extra = "extra" }
}

puts "main: #{Early.new.extra}"
p [Early.instance_methods(false).sort, Late.constants, Late.class]

# The hook ordering both ways round. `Recorder`'s hook is installed in the
# MAIN body, so it never sees `from_begin` -- that ran first. `Watched`'s is
# installed inside a BEGIN, so it sees every main definition however early in
# the file it is written.
class Watched
  def main_one = 1
end

BEGIN {
  class Watched
    def self.method_added(name)
      puts "watched added #{name}"
    end
  end
}

class Watched
  def main_two = 2
end
__END__
first begin
second begin
main: early hi / late / helper
main: reopened true
main: extra
[[:extra, :hi], [:WHO], Module]
watched added main_one
watched added main_two
