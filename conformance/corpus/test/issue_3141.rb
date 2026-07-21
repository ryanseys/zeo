p [123].any? { _1 }
p [1,2,3].any? { |x| x > 2 }
p [1,2,3].all? { _1 }
p [1,2,3].none? { _1 }
p [1,2,3].one? { _1 }
p [nil, nil].any? { _1 }
p [0, 0].all? { _1 }
p [].any? { _1 }
p ["a", nil].any? { _1 }
p [1,2,3].any? { |x| x.even? }
p ["x", "y"].all? { |s| s }
