# A DECIDED DIVERGENCE, and the same one as `sort_with_comparator`. A block
# that takes ONE parameter still gets both elements, so it compares the first
# against nothing and answers a value unrelated to the pair. Every pair then
# ties or contradicts, and the order that comes out is the sort algorithm's,
# not the program's. zeo's stable sort keeps the input order; ruby hands the
# tie to the platform's `qsort_r`. Only line 7 shows it.
#
# --- ruby 4.0.6 answers line 7 ---
# ["ccc", "a", "bb"]

p [1, 5, 3].max { |x| -x }
p [3, 1, 2].sort { |x| 0 }
p [3, 1, 2].sort { 0 }
p [3, 1, 2].min { |x| 1 }
p [3, 1, 2].sort { |a, b| b <=> a }
p [3, 1, 2].max { |a, b| a <=> b }
p %w[bb a ccc].sort { |x| x.length }
p [2.5, 1.5].min { |x| -1 }
