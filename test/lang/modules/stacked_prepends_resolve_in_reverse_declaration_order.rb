# `prepend M1; prepend M2` -- M2 (most recently prepended) is closest,
# ahead of M1, ahead of the class's own definition.

module M1
  def label
    "m1"
  end
end
module M2
  def label
    "m2"
  end
end
class Stacked
  prepend M1
  prepend M2
  def label
    "own"
  end
end
puts Stacked.new.label
__END__
m2
