# A `&.` call has to reach the safe-nav guard. Three lowerings walk the
# receiver without looking at the operator -- the value-position iterator, the
# statement-position one, and the comprehension family -- so `v&.each { }` fed
# a nil to sp_poly_iter_check and raised where CRuby answers nil. The first two
# stand down until the guard has run; the guard re-enters them on the guarded
# temp, and THAT pass lowers normally.
#
# And the guard could only be a ternary. A lowering that hoists statements (the
# comprehension family builds a loop) put them in front of it, where they ran
# on the very nil it exists to stop, so the guard becomes a statement `if` when
# the value arm hoists.

def sn_0(v) = v&.each { |x| x }
p sn_0(nil)
def sn_1(v) = v&.each_entry { |x| x }
p sn_1(nil)
def sn_2(v) = v&.each_with_index { |x, i| x }
p sn_2(nil)
def sn_3(v) = v&.each_pair { |k, v| k }
p sn_3(nil)
def sn_4(v) = v&.each_key { |k| k }
p sn_4(nil)
def sn_5(v) = v&.each_value { |v| v }
p sn_5(nil)
def sn_6(v) = v&.reverse_each { |x| x }
p sn_6(nil)
def sn_7(v) = v&.map { |x| x }
p sn_7(nil)
def sn_8(v) = v&.collect { |x| x }
p sn_8(nil)
def sn_9(v) = v&.select { |x| true }
p sn_9(nil)
def sn_10(v) = v&.filter { |x| true }
p sn_10(nil)
def sn_11(v) = v&.reject { |x| false }
p sn_11(nil)
def sn_12(v) = v&.sort_by { |x| x }
p sn_12(nil)
def sn_13(v) = v&.min_by { |x| x }
p sn_13(nil)
def sn_14(v) = v&.max_by { |x| x }
p sn_14(nil)
def sn_15(v) = v&.group_by { |x| x }
p sn_15(nil)
def sn_16(v) = v&.partition { |x| true }
p sn_16(nil)
def sn_17(v) = v&.flat_map { |x| x }
p sn_17(nil)
def sn_18(v) = v&.each_slice(2) { |x| x }
p sn_18(nil)
def sn_19(v) = v&.each_cons(2) { |x| x }
p sn_19(nil)
def sn_20(v) = v&.sum { |x| x }
p sn_20(nil)
def sn_21(v) = v&.all? { |x| true }
p sn_21(nil)
def sn_22(v) = v&.any? { |x| true }
p sn_22(nil)
def sn_23(v) = v&.none? { |x| false }
p sn_23(nil)
def sn_24(v) = v&.one? { |x| true }
p sn_24(nil)
def sn_25(v) = v&.take_while { |x| true }
p sn_25(nil)
def sn_26(v) = v&.drop_while { |x| false }
p sn_26(nil)
def sn_27(v) = v&.each_with_object(0) { |a, b| a }
p sn_27(nil)
def sn_28(v) = v&.reduce { |a, b| a }
p sn_28(nil)
def sn_29(v) = v&.inject { |a, b| a }
p sn_29(nil)
def sn_30(v) = v&.filter_map { |x| x }
p sn_30(nil)
def sn_31(v) = v&.find_index { |x| true }
p sn_31(nil)
def sn_32(v) = v&.times { |x| x }
p sn_32(nil)
def sn_33(v) = v&.upto(3) { |x| x }
p sn_33(nil)
def sn_34(v) = v&.step(3) { |x| x }
p sn_34(nil)

# and a non-nil receiver still walks: the guard must not swallow the call
p [1, 2, 3]&.each { |x| x }
p [1, 2, 3]&.select { |x| x > 1 }
p [1, 2, 3]&.reject { |x| x > 1 }
p [1, 2, 3]&.reduce { |a, b| a + b }
p [1, 2, 3]&.group_by { |x| x.odd? }
p({ "a" => 1 }&.each_pair { |k, v| k })
p [1, 2, 3]&.map { |x| x * 2 }

# through a method, where the receiver is genuinely poly
def walk(v) = v&.select { |x| x > 1 }
p walk([1, 2, 3])
p walk(nil)
def total(v) = v&.reduce(0) { |a, b| a + b }
p total([1, 2, 3])
p total(nil)
