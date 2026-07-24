# A Hash read out of a container keeps #sort / #min / #max / #sort_by --
# the poly receiver must materialize its pairs, not degrade to NoMethodError/nil.
h = [{ 20 => 2, 3 => 1 }].first
p h.sort
p h.min
p h.max
p h.sort_by { |k, v| v }
p h.sort_by { |k, v| k }
