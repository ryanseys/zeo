# #517. `module M; def self.X; ...; end; end` re-opened with a
# second `def self.X` previously emitted two C functions with the
# same name but possibly-conflicting return types. CRuby's
# semantics: last definition wins; spinel now replaces the
# @meth_* row in place when the same `<Mod>_cls_<X>` is seen
# again (sibling to the class-reopen fix #489).

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
