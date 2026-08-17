# `delete` on a boxed receiver dispatches on what the receiver turns out to
# be: a Hash gives the deleted value, an Array the object, a String the
# stripped copy. It used to commit to String#delete and stringify the
# receiver (#3806).
opts = { n: 1, map: { 'a' => 7 }, list: [1, 2, 3], text: 'banana' }
h = opts[:map]
p h.delete('a')
p h

a = opts[:list]
p a.delete(2)
p a

s = opts[:text]
p s.delete('a')

k = opts[:n]
m = { 1 => 'one' }
mm = [m, nil].first
p mm.delete(k)
