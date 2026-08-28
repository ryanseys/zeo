# `v&.each_slice(n) { }` / `v&.each_cons(n) { }`. The re-dispatch materializes
# the boxed receiver into a poly array and re-enters the call so the array
# emitters serve it -- and poking the receiver's cached type is not enough to
# make them: under the guard, the re-emission asks the inference again and
# re-establishes the receiver as poly, so they declined and the call fell
# through to NoMethodError. The node is pinned for the inference now.
def pick(n) = n > 0 ? [1, 2, 4, 5] : nil
def hash_or_nil(n) = n > 0 ? { "a" => 1, "b" => 2 } : nil

[1, 0].each do |k|
  v = pick(k)
  p v&.each_slice(2) { |s| s }
  p v&.each_cons(2) { |s| s }
  h = hash_or_nil(k)
  p h&.each_slice(1) { |s| s }
  p h&.each_cons(1) { |s| s }
end
