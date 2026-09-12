# Who owns `Enumerator::Lazy.new` and `Warning.warn`.
p Enumerator::Lazy.singleton_methods(false).include?(:new)
p Enumerator::Lazy.method(:new).owner.to_s
p Warning.singleton_methods(false).sort
p Warning.method(:warn).owner.to_s
__END__
false
"Class"
[:[], :[]=, :categories]
"Warning"
