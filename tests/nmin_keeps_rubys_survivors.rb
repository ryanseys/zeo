# `min(n)`, `max(n)`, `min_by(n)` and `max_by(n)` are NOT sort-then-take.
# CRuby buffers 4n elements, QUICKSELECTS them down to n, and remembers the
# last pivot as a limit -- after which an element no better than the limit is
# dropped without being buffered at all. So among equal keys it is the SET of
# survivors that quickselect decides, not merely their order, and only the
# survivors are sorted at the end.
#
# The 4n buffer is why the count matters: at n=3 over 60 elements the filter
# runs several times, at n=20 once, and each run moves the limit.

def t(label)
  puts format("%-24s %s", label, (yield).inspect)
rescue StandardError => e
  puts format("%-24s %s: %s", label, e.class, e.message)
end

pairs = Array.new(60) { |i| [i % 3, i] }

t("min_by(3)") { pairs.min_by(3) { |x| x[0] } }
t("max_by(3)") { pairs.max_by(3) { |x| x[0] } }
t("min(3)") { pairs.min(3) { |a, b| a[0] <=> b[0] } }
t("max(3)") { pairs.max(3) { |a, b| a[0] <=> b[0] } }
t("min_by(1)") { pairs.min_by(1) { |x| x[0] } }
t("min_by(20)") { pairs.min_by(20) { |x| x[0] }.last(4) }
t("min_by(60)") { pairs.min_by(60) { |x| x[0] }.first(3) }
t("min_by past the end") { pairs.min_by(100) { |x| x[0] }.size }
t("min_by(0)") { pairs.min_by(0) { |x| x[0] } }
t("min(0)") { pairs.min(0) }
t("negative") { pairs.min_by(-1) { |x| x[0] } }

t("enumerator") { pairs.each_with_index.min_by(3) { |x,| x[0] } }
t("a range") { (1..40).min(3) { |a, b| (a % 4) <=> (b % 4) } }
t("distinct keys min") { [5, 3, 9, 1].min(2) }
t("distinct keys max") { [5, 3, 9, 1].max(2) }
t("all equal") { Array.new(30) { |i| [0, i] }.min_by(4) { |x| x[0] } }
t("strings") { %w[pear apple fig kiwi date].min_by(2, &:length) }
t("incomparable") { [1, "a"].min(1) }
t("nil from the block") { pairs.min(2) { |_a, _b| nil } }
