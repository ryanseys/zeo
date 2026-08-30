def frames
  yield
rescue => e
  puts e.backtrace.first(3).map { |l| l.sub(/\A.*:\d+:in /, "") }
end

# `class << obj` on a plain object. Its `def` is a real method, so ruby names
# the frame after it -- not after the scope the body was written in.
o = Object.new
class << o
  def sm
    [1].each { raise "x" }
  end
end
frames { o.sm }

puts "--- def obj.m ---"
p2 = Object.new
def p2.dm
  [1].each { raise "x" }
end
frames { p2.dm }

puts "--- def self.meta inside class << obj ---"
p3 = Object.new
class << p3
  def self.meta = [1].each { raise "x" }
end
frames { p3.singleton_class.meta }

puts "--- a define_method body really is a block ---"
class D
  define_method(:dm) { [1].each { raise "x" } }
end
frames { D.new.dm }

puts "--- def self.x in a class body is qualified ---"
class L
  def self.sm2 = [1].each { raise "x" }
end
frames { L.sm2 }

puts "--- the iseq label reads the same string ---"
q = Object.new
def q.labelled = RubyVM::InstructionSequence.of(method(:labelled)).label
p q.labelled
