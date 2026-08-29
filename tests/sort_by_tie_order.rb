# `sort_by` does NOT sort the way `sort` does. CRuby watches the keys go by
# and, when every one of them is a fixnum or a Float, drops `ruby_qsort` for
# `rb_uniform_intro_sort_2` -- its own introsort, which breaks a tie somewhere
# else entirely. One key outside that set (a String, a Bignum) puts the whole
# call back on the C library's `qsort_r`, so both orders are pinned below.
#
# The last two rows reach the branches n alone decides: 16 or fewer elements
# is an insertion sort, and an already-ordered input is left untouched.
#
# TWO goldens: the `qsort_r` rows are libc's answer, and glibc's qsort does
# not break a tie where BSD's does. `.expected` is macOS, `.linux.expected`
# linux, both recorded from the same pinned ruby.

def show(label)
  puts format("%-26s %s", label, (yield).inspect)
rescue StandardError => e
  puts format("%-26s %s: %s", label, e.class, e.message)
end

pairs = Array.new(60) { |i| [i % 3, i] }

show("fixnum keys") { pairs.sort_by { |x| x[0] }.first(8) }
show("float keys") { pairs.sort_by { |x| x[0].to_f }.first(6) }
show("mixed fixnum and float") do
  Array.new(40) { |i| [i.even? ? i % 4 : (i % 4).to_f, i] }.sort_by { |x| x[0] }.first(6)
end
show("string keys leave uniform") { pairs.sort_by { |x| x[0].to_s }.first(6) }
show("a bignum leaves uniform") do
  Array.new(30) { |i| [i % 3 == 0 ? 10**25 : i % 3, i] }.sort_by { |x| x[0] }.first(4)
end
show("every key equal") { Array.new(600) { |i| i }.sort_by { 0 }.first(6) }
show("descending keys") { Array.new(40) { |i| [i % 4, i] }.sort_by { |x| -x[0] }.first(5) }
show("insertion sort, n <= 16") { Array.new(15) { |i| [i % 2, i] }.sort_by { |x| x[0] } }
show("already ordered") { Array.new(40) { |i| [i / 10, i] }.sort_by { |x| x[0] }.first(5) }
show("a nan fails the sort") { [1.0, Float::NAN, 2.0].sort_by { |x| x } }
show("enumerator, not array") { pairs.each_with_index.sort_by { |x,| x[0] }.first(4) }
