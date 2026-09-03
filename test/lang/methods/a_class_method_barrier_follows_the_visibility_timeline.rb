class C
  def self.helper(x) = x + 1
end

module M
  def bonus(x) = x + 100
end

# ONE call site, asked again after every change -- the site remembers its
# barrier verdict, so each answer has to be the one ruby gives at that moment.
def ask(n)
  C.helper(n)
rescue NoMethodError => e
  e.message
end

def ask_bonus(n)
  C.bonus(n)
rescue NoMethodError => e
  e.message
end

# A name Class supplies and C does not, so the barrier falls through.
def ask_frozen
  C.frozen?
rescue NoMethodError => e
  e.message
end

puts ask(1)
puts ask_bonus(1)
puts ask_frozen

C.private_class_method(:helper)
puts ask(2)

C.public_class_method(:helper)
puts ask(3)

C.define_singleton_method(:helper) { |x| x * 10 }
puts ask(4)

C.extend(M)
puts ask(5)
puts ask_bonus(5)

C.singleton_class.send(:private, :bonus)
puts ask_bonus(6)

C.singleton_class.send(:public, :bonus)
puts ask_bonus(7)

C.freeze
puts ask_frozen
puts ask(8)
__END__
2
undefined method 'bonus' for class C
false
private method 'helper' called for class C
4
40
50
105
private method 'bonus' called for class C
107
true
80
