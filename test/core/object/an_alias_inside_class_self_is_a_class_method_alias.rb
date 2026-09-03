# `alias new old` inside `class << self` aliases a SINGLETON method: it
# must resolve against the class-method table and register `new` as a class
# method, not an instance method.

module M
  def self.original = 42
  class << self
    alias renamed original
  end
end
puts M.renamed
puts M.respond_to?(:renamed)
puts M.singleton_methods.include?(:renamed)
__END__
42
true
true
