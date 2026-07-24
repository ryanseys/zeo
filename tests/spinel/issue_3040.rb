p(nil.to_h == {})
p({} == nil.to_h)
p({} == {})
h = {"a" => 1}
p(h == {})
p({} == h)
p({}.eql?(nil.to_h))
p({} != nil.to_h)
