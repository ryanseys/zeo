# #517. `module M; def self.X; ...; end; end` re-opened with a
# second `def self.X` previously emitted two C functions with the
# same name but possibly-conflicting return types. CRuby's
# semantics: the last definition wins, the same way a reopened class
# replaces an instance method.

module M
  def self.greet
    "first"
  end
end

module M
  def self.greet
    "second"
  end
  def self.only_in_reopen
    "extra"
  end
end

# Last definition wins.
puts M.greet
# Method unique to the reopen is also available.
puts M.only_in_reopen
__END__
second
extra
