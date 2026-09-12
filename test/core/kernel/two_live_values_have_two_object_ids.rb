# `object_id` is an identity, so two live values of one kind have two of
# them -- for every heap kind, not only the ones with an ivar table.
p 2.times.map { Regexp.new("a") }.map(&:object_id).uniq.size
p 2.times.map { proc {} }.map(&:object_id).uniq.size
p 2.times.map { "x".match(/x/) }.map(&:object_id).uniq.size
p 2.times.map { [1].each }.map(&:object_id).uniq.size
p 2.times.map { Object.new }.map(&:object_id).uniq.size

r = /z/
p r.object_id == r.object_id
p r.object_id == r.dup.object_id
__END__
2
2
2
2
2
true
false
