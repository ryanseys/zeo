# The boxed receiver's cast is built into a fixed buffer that both the call and
# g_self read. At 64 bytes a long class name truncated
# `(sp_Long... *)_t2.v.p` to `..._t2.v`, and the C build stopped on an
# identifier that does not exist. snprintf truncates silently, so nothing said
# so until the generated C would not compile.

class PolyDispatchReceiverWithAnExceedinglyLongClassNameForTheBuffer
  def initialize(n)
    @n = n
  end

  def size
    @n
  end

  def process(x)
    x + @n
  end
end

class AnotherPolyDispatchReceiverWhoseNameIsAlsoWellPastSixtyFourBytes
  def size
    2
  end

  def process(x)
    x * 2
  end
end

def sized(v) = v.size
def run(v, x) = v.process(x)

p sized(PolyDispatchReceiverWithAnExceedinglyLongClassNameForTheBuffer.new(7))
p sized(AnotherPolyDispatchReceiverWhoseNameIsAlsoWellPastSixtyFourBytes.new)
p sized([1, 2, 3])
p run(PolyDispatchReceiverWithAnExceedinglyLongClassNameForTheBuffer.new(3), 4)
p run(AnotherPolyDispatchReceiverWhoseNameIsAlsoWellPastSixtyFourBytes.new, 4)
