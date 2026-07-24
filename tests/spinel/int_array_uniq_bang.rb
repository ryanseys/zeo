# Array#uniq! on int_array — mutates in place. Returns the
# receiver (CRuby returns self when something was removed and nil
# otherwise; the typed int_array can't carry nil so we always
# return self).
a = [1, 2, 2, 3, 3, 3]
a.uniq!
puts a.inspect

b = [1, 2, 3]
b.uniq!
puts b.inspect

c = []
c.uniq!
puts c.inspect
