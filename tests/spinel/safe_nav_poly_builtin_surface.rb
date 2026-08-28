# Every builtin name a `&.` can carry on a boxed receiver. The point is not the
# values: it is that the program BUILDS and that a nil receiver answers nil.
# The safe-navigation guard has to box a value arm the poly runtime renders as
# a raw C scalar, and which names those are used to be a hand-kept table in the
# emitter. It went stale within a day -- `begin`, `end`, `count` and `bytes`
# were missing and `infinite?` was listed with the wrong type -- and nothing in
# the gate could see it, because the failure is a C error in a program nobody
# had written yet. This file is that program.
VALS = [1, "abc", 1.5, [1], { "a" => 1 }]

# the Hash/Enumerable face names, whose arm converts the receiver before it
# re-dispatches: the conversion must sit INSIDE the nil guard, and must happen
# once (it used to be hoisted in front of the guard, and to convert its own
# conversion 63 times over)
def face_entries(v) = v&.entries
def face_to_a(v) = v&.to_a
def face_keys(v) = v&.keys
def face_values(v) = v&.values

p face_entries({ "a" => 1 })
p face_entries(nil)
p face_to_a({ "a" => 1 })
p face_to_a(nil)
p face_keys({ "a" => 1 })
p face_keys(nil)
p face_values({ "a" => 1 })
p face_values(nil)

def f_abs(v) = v&.abs
def f_begin(v) = v&.begin
def f_bit_length(v) = v&.bit_length
def f_bytes(v) = v&.bytes
def f_bytesize(v) = v&.bytesize
def f_capitalize(v) = v&.capitalize
def f_ceil(v) = v&.ceil
def f_chars(v) = v&.chars
def f_chomp(v) = v&.chomp
def f_chop(v) = v&.chop
def f_class(v) = v&.class
def f_compact(v) = v&.compact
def f_count(v) = v&.count
def f_denominator(v) = v&.denominator
def f_downcase(v) = v&.downcase
def f_each_char(v) = v&.each_char
def f_empty_p(v) = v&.empty?
def f_end(v) = v&.end
def f_end_with_p(v) = v&.end_with?("a")
def f_eql_p(v) = v&.eql?("a")
def f_equal_p(v) = v&.equal?("a")
def f_even_p(v) = v&.even?
def f_finite_p(v) = v&.finite?
def f_first(v) = v&.first
def f_flatten(v) = v&.flatten
def f_floor(v) = v&.floor
def f_frozen_p(v) = v&.frozen?
def f_hash(v) = v&.hash
def f_include_p(v) = v&.include?("a")
def f_infinite_p(v) = v&.infinite?
def f_inspect(v) = v&.inspect
def f_instance_of_p(v) = v&.instance_of?(String)
def f_integer_p(v) = v&.integer?
def f_is_a_p(v) = v&.is_a?(String)
def f_keys(v) = v&.keys
def f_kind_of_p(v) = v&.kind_of?(String)
def f_last(v) = v&.last
def f_lazy(v) = v&.lazy
def f_length(v) = v&.length
def f_lines(v) = v&.lines
def f_max(v) = v&.max
def f_min(v) = v&.min
def f_nan_p(v) = v&.nan?
def f_negative_p(v) = v&.negative?
def f_next(v) = v&.next
def f_nil_p(v) = v&.nil?
def f_numerator(v) = v&.numerator
def f_object_id(v) = v&.object_id
def f_odd_p(v) = v&.odd?
def f_ord(v) = v&.ord
def f_positive_p(v) = v&.positive?
def f_reverse(v) = v&.reverse
def f_round(v) = v&.round
def f_size(v) = v&.size
def f_sort(v) = v&.sort
def f_start_with_p(v) = v&.start_with?("a")
def f_step(v) = v&.step
def f_strip(v) = v&.strip
def f_succ(v) = v&.succ
def f_sum(v) = v&.sum
def f_to_a(v) = v&.to_a
def f_to_c(v) = v&.to_c
def f_to_f(v) = v&.to_f
def f_to_i(v) = v&.to_i
def f_to_r(v) = v&.to_r
def f_to_s(v) = v&.to_s
def f_to_sym(v) = v&.to_sym
def f_truncate(v) = v&.truncate
def f_uniq(v) = v&.uniq
def f_upcase(v) = v&.upcase
def f_values(v) = v&.values
def f_zero_p(v) = v&.zero?

p f_abs(nil)
p f_begin(nil)
p f_bit_length(nil)
p f_bytes(nil)
p f_bytesize(nil)
p f_capitalize(nil)
p f_ceil(nil)
p f_chars(nil)
p f_chomp(nil)
p f_chop(nil)
p f_class(nil)
p f_compact(nil)
p f_count(nil)
p f_denominator(nil)
p f_downcase(nil)
p f_each_char(nil)
p f_empty_p(nil)
p f_end(nil)
p f_end_with_p(nil)
p f_eql_p(nil)
p f_equal_p(nil)
p f_even_p(nil)
p f_finite_p(nil)
p f_first(nil)
p f_flatten(nil)
p f_floor(nil)
p f_frozen_p(nil)
p f_hash(nil)
p f_include_p(nil)
p f_infinite_p(nil)
p f_inspect(nil)
p f_instance_of_p(nil)
p f_integer_p(nil)
p f_is_a_p(nil)
p f_keys(nil)
p f_kind_of_p(nil)
p f_last(nil)
p f_lazy(nil)
p f_length(nil)
p f_lines(nil)
p f_max(nil)
p f_min(nil)
p f_nan_p(nil)
p f_negative_p(nil)
p f_next(nil)
p f_nil_p(nil)
p f_numerator(nil)
p f_object_id(nil)
p f_odd_p(nil)
p f_ord(nil)
p f_positive_p(nil)
p f_reverse(nil)
p f_round(nil)
p f_size(nil)
p f_sort(nil)
p f_start_with_p(nil)
p f_step(nil)
p f_strip(nil)
p f_succ(nil)
p f_sum(nil)
p f_to_a(nil)
p f_to_c(nil)
p f_to_f(nil)
p f_to_i(nil)
p f_to_r(nil)
p f_to_s(nil)
p f_to_sym(nil)
p f_truncate(nil)
p f_uniq(nil)
p f_upcase(nil)
p f_values(nil)
p f_zero_p(nil)

# the value calls never run; they are what makes each parameter poly
if false
  p f_abs(VALS[0])
  p f_begin(VALS[0])
  p f_bit_length(VALS[0])
  p f_bytes(VALS[0])
  p f_bytesize(VALS[0])
  p f_capitalize(VALS[0])
  p f_ceil(VALS[0])
  p f_chars(VALS[0])
  p f_chomp(VALS[0])
  p f_chop(VALS[0])
  p f_class(VALS[0])
  p f_compact(VALS[0])
  p f_count(VALS[0])
  p f_denominator(VALS[0])
  p f_downcase(VALS[0])
  p f_each_char(VALS[0])
  p f_empty_p(VALS[0])
  p f_end(VALS[0])
  p f_end_with_p(VALS[0])
  p f_eql_p(VALS[0])
  p f_equal_p(VALS[0])
  p f_even_p(VALS[0])
  p f_finite_p(VALS[0])
  p f_first(VALS[0])
  p f_flatten(VALS[0])
  p f_floor(VALS[0])
  p f_frozen_p(VALS[0])
  p f_hash(VALS[0])
  p f_include_p(VALS[0])
  p f_infinite_p(VALS[0])
  p f_inspect(VALS[0])
  p f_instance_of_p(VALS[0])
  p f_integer_p(VALS[0])
  p f_is_a_p(VALS[0])
  p f_keys(VALS[0])
  p f_kind_of_p(VALS[0])
  p f_last(VALS[0])
  p f_lazy(VALS[0])
  p f_length(VALS[0])
  p f_lines(VALS[0])
  p f_max(VALS[0])
  p f_min(VALS[0])
  p f_nan_p(VALS[0])
  p f_negative_p(VALS[0])
  p f_next(VALS[0])
  p f_nil_p(VALS[0])
  p f_numerator(VALS[0])
  p f_object_id(VALS[0])
  p f_odd_p(VALS[0])
  p f_ord(VALS[0])
  p f_positive_p(VALS[0])
  p f_reverse(VALS[0])
  p f_round(VALS[0])
  p f_size(VALS[0])
  p f_sort(VALS[0])
  p f_start_with_p(VALS[0])
  p f_step(VALS[0])
  p f_strip(VALS[0])
  p f_succ(VALS[0])
  p f_sum(VALS[0])
  p f_to_a(VALS[0])
  p f_to_c(VALS[0])
  p f_to_f(VALS[0])
  p f_to_i(VALS[0])
  p f_to_r(VALS[0])
  p f_to_s(VALS[0])
  p f_to_sym(VALS[0])
  p f_truncate(VALS[0])
  p f_uniq(VALS[0])
  p f_upcase(VALS[0])
  p f_values(VALS[0])
  p f_zero_p(VALS[0])
end
