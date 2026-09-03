# The `succ` walk `String#upto` and a String/Symbol Range share
# (`rb_str_upto_each`): its single-character, all-digit and byte-ordered
# branches, plus what a `break` in each form answers.
def t(l); p [l, (yield)]; rescue Exception => e; p [l, e.class.name, e.message]; end
t("str range"){ ("a".."e").to_a }
t("str range x"){ ("a"..."e").to_a }
t("str 2ch"){ ("aa".."ac").to_a }
t("str y-ab"){ ("y".."ab").to_a }
t("digits"){ ("9".."11").to_a }
t("digits pad"){ ("08".."11").to_a }
t("digits x"){ ("9"..."11").to_a }
t("sym"){ (:a..:e).to_a }
t("sym2"){ (:aa..:ac).to_a }
t("upto"){ "a".upto("e").to_a }
t("upto excl"){ "a".upto("e", true).to_a }
t("upto digits"){ "9".upto("11").to_a }
t("upto pad"){ "08".upto("10").to_a }
t("upto rev"){ "e".upto("a").to_a }
t("upto same"){ "a".upto("a").to_a }
t("upto ret"){ "a".upto("c") { |s| s } }
t("include"){ ("a".."e").include?("bb") }
t("cover"){ ("a".."e").cover?("bb") }
t("step str"){ ("a".."z").step(5).to_a }
t("step sym"){ (:a..:j).step(3).to_a }
t("each break"){ ("a".."z").step(2) { |s| break s } }
t("upto break"){ "a".upto("z") { |s| break s } }
t("range size"){ ("a".."e").size }
t("min/max"){ [("a".."e").min, ("a".."e").max] }
t("succ carry"){ ("az".."bc").to_a }
__END__
["str range", ["a", "b", "c", "d", "e"]]
["str range x", ["a", "b", "c", "d"]]
["str 2ch", ["aa", "ab", "ac"]]
["str y-ab", []]
["digits", ["9", "10", "11"]]
["digits pad", ["08", "09", "10", "11"]]
["digits x", ["9", "10"]]
["sym", [:a, :b, :c, :d, :e]]
["sym2", [:aa, :ab, :ac]]
["upto", ["a", "b", "c", "d", "e"]]
["upto excl", ["a", "b", "c", "d"]]
["upto digits", ["9", "10", "11"]]
["upto pad", ["08", "09", "10"]]
["upto rev", []]
["upto same", ["a"]]
["upto ret", "a"]
["include", false]
["cover", true]
["step str", ["a", "f", "k", "p", "u", "z"]]
["step sym", [:a, :d, :g, :j]]
["each break", "a"]
["upto break", "a"]
["range size", nil]
["min/max", ["a", "e"]]
["succ carry", ["az", "ba", "bb", "bc"]]
