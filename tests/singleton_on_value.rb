# A singleton method belongs to the OBJECT, and in Ruby that includes any heap
# value -- csv defines `def NO_QUOTED_FIELDS.[](i)` on a bare Array.
NO_QUOTED = []
def NO_QUOTED.[](_index) = false
NO_QUOTED.freeze
p NO_QUOTED[0], NO_QUOTED[99]
p NO_QUOTED.size, NO_QUOTED.frozen?, NO_QUOTED.respond_to?(:[])

# It is per-OBJECT, so a sibling of the same class is untouched.
p [].respond_to?(:shout)
s = +"hello"
def s.shout = upcase + "!"
p s.shout, s.length, s.class
p (+"hello").respond_to?(:shout)

h = {}
h.define_singleton_method(:fetch_or) { |k| self[k] || :none }
p h.fetch_or(:x)
h[:x] = 1
p h.fetch_or(:x)
p h.singleton_methods

# An immediate has no identity to attach one to, which is why Ruby refuses.
begin
  1.define_singleton_method(:nope) { }
rescue TypeError => e
  puts e.message
end

# ...but nil/true/false each have exactly one instance, so their singleton
# class IS their class and Ruby accepts it there.
nil.define_singleton_method(:blank?) { true }
p nil.blank?
