# A `NoMethodError` names its receiver one of two ways, and which one is
# decided by whether the receiver has a SINGLETON CLASS: `an instance of K`
# without, `#<K:0xADDR>` with. Materializing one is the whole trigger --
# a bare `singleton_class` call flips it, removing the last `def obj.m`
# does not flip it back, and `freeze` never flips it.
#
# The address form is `rb_any_to_s`, so it ignores a `to_s` or an
# `inspect` the class defines. That matters beyond cosmetics: building
# this message must run no user code, which is what lets a blank-slate
# receiver report its own missing method instead of failing a second
# lookup while describing the first.
class K
  def to_s = "K-TO-S"
  def inspect = "K-INSPECT"
end
module M; end

def why(label)
  yield
rescue NoMethodError => e
  puts "#{label}: #{e.message}"
end

why("plain") { K.new.nope }
why("frozen") { K.new.freeze.nope }

k = K.new
def k.mine; end
why("own def") { k.nope }

e = K.new
e.extend(M)
why("extended") { e.nope }

s = K.new
s.singleton_class
why("singleton_class asked for") { s.nope }

r = K.new
def r.gone; end
r.singleton_class.send(:remove_method, :gone)
why("last singleton method removed") { r.nope }

str = +"text"
def str.mine; end
why("string") { str.nope }

arr = [1, 2]
def arr.mine; end
why("array") { arr.nope }

why("class") { K.nope }
why("module") { M.nope }
why("nil") { nil.nope }
why("true") { true.nope }
why("integer") { 5.nope }
why("main") { self.nope }
