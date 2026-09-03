# MRO here is `Pre, Own, Inc, Object` -- `Own`'s own `label` calls
# `super` and reaches `Inc` (the next ancestor after `Own`); `Pre`'s
# `label` calls `super` and reaches `Own`.

module Pre
  def label
    "pre(#{super})"
  end
end
module Inc
  def label
    "inc"
  end
end
class Own
  prepend Pre
  include Inc
  def label
    "own(#{super})"
  end
end
puts Own.new.label
__END__
pre(own(inc))
