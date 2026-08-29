# `RubyVM::InstructionSequence#label` on a Proc handle used to refuse: a
# block's frame name is a LEXICAL fact, and the Proc value carried only a
# location. Codegen stamps the name on the proc's shape now.
#
# `#label` counts the nesting; `#base_label` is the enclosing scope alone.
# Note ruby's iseq label does NOT qualify a method by its class, though its
# backtrace does -- `m`, never `Object#m`.

def show(x)
  h = RubyVM::InstructionSequence.of(x)
  puts(h ? "#{h.label}\t#{h.base_label}" : "nil")
end

show(proc { 1 })
show(lambda { 1 })
show(Proc.new { 1 })
[1].each { show(proc { 2 }) }
[1].each { [2].each { show(proc { 3 }) } }
[1].each { [2].each { [3].each { show(proc { 4 }) } } }

def m
  show(proc { 5 })
  [1].each { show(proc { 6 }) }
end
m

class C
  show(proc { 7 })

  def im
    show(proc { 8 })
  end

  def self.cm
    show(proc { 9 })
  end

  define_method(:dm) { show(proc { 10 }) }
end
C.new.im
C.cm
C.new.dm

module M
  show(proc { 11 })
end

class Outer
  class Inner
    def deep
      [1].each { [2].each { show(proc { 12 }) } }
    end
  end
end
Outer::Inner.new.deep

# A block handed over as `&blk` keeps the name of where it was WRITTEN.
def takes(&blk) = show(blk)
takes { 13 }

# A method handle answers its own bare name for both.
def a_method = 1
show(method(:a_method))
show(method(:a_method).unbind)

# A proc with no Ruby frame has no handle at all.
show(:upcase.to_proc)
