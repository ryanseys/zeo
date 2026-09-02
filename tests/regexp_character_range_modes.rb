# Ruby reads a pattern in one of three character-range modes. In the default
# mode `\w`, `\d` and `\s` are ASCII-only while `\b`, `\B` and the POSIX
# brackets are Unicode; `(?a)` moves everything to ASCII, `(?u)` everything to
# Unicode, and `(?d)` is the default again. A bare option holds to the end of
# its enclosing group; `(?a:...)` scopes to the group. `\h` is always ASCII
# and `\p{...}` is always Unicode.
#
# Under `/i` a bare `\p{...}` case-folds, its negation folds as a negated
# class, and a ctype-derived ASCII member never folds outside ASCII (`ſ` and
# the Kelvin sign fold to `s` and `k`; `\w` does not reach them, `[a-z]` does).

def t(l) = puts("#{l.ljust(28)} #{begin; yield.inspect; rescue Exception => e; "#{e.class}: #{e.message}"; end}")
t("w cjk") { "日本語" =~ /\w/ }
t("W cjk") { "日本語" =~ /\W/ }
t("b cjk") { "λx" =~ /\b/ }
t("word cjk") { "日本語" =~ /[[:word:]]/ }
t("u w") { "日" =~ /(?u)\w/ }
t("u: w") { "日" =~ /(?u:\w)x/ }
t("a b") { "λx" =~ /(?a)\b/ }
t("a B") { "λx" =~ /(?a)\B/ }
t("a word") { "日" =~ /(?a)[[:word:]]/ }
t("a ^alpha") { "日" =~ /(?a)[[:^alpha:]]/ }
t("a pWord") { "日" =~ /(?a)\p{Word}/ }
t("s nbsp") { "\u00a0" =~ /\s/ }
t("space em") { "\u2003" =~ /[[:space:]]/ }
t("d w") { "日" =~ /(?d)\w/ }
t("digits") { "1２" =~ /\d+/ }
t("[^W]") { "x" =~ /[^\W]/ }
t("[W] cjk") { "日" =~ /[\W]/ }
t("[^w] cjk") { "日" =~ /[^\w]/ }
t("pUpper/i") { "é" =~ /\p{Upper}/i }
t("pLu/i") { "é" =~ /\p{Lu}/i }
t("pLower/i") { "É" =~ /\p{Lower}/i }
t("p^Lower/i") { "É" =~ /\p{^Lower}/i }
t("PLower/i") { "é" =~ /\P{Lower}/i }
t("pLt/i") { "ǆ" =~ /\p{Lt}/i }
t("(?i)pLower") { "É" =~ /(?i)\p{Lower}/ }
t("(?i)(?-i:pLower)") { "É" =~ /(?i)(?-i:\p{Lower})/ }
t("u w d w") { "日x" =~ /(?u)\w(?d)\w/ }
t("w/i long s") { "ſ" =~ /\w/i }
t("W/i long s") { "ſ" =~ /\W/i }
t("[a-z]/i long s") { "ſ" =~ /[a-z]/i }
t("k/i kelvin") { "\u212a" =~ /k/i }
t("h fullwidth") { "Ａ" =~ /\h/ }
t("xdigit fullwidth") { "Ａ" =~ /[[:xdigit:]]/ }
t("h/i long s") { "ſ" =~ /\h/i }
t("a h") { "日" =~ /(?a)\h/ }
t("(?a) alone") { "" =~ /(?a)/ }
t("-u") { Regexp.new("(?-u)\\w") }
t("x comment") { "a1" =~ /a # \w [ (
 \d/x }
t("names") { /(?<x>\w)(?a)(?<y>\d)/.names }
t("source") { /(?a)\w\b/.source }
t("inspect") { /(?u)\w/i.inspect }
t("p+quant") { "abc".scan(/\p{L}+/) }
t("pL/i neg class") { "É".scan(/[^\p{Lower}]/i) }
t("w in lookbehind") { "a1" =~ /(?<=\w)\d/ }
t("scan w cjk") { "日本 abc".scan(/\w+/) }
t("gsub W") { "日本 abc".gsub(/\W/, "_") }
t("split s") { "a b\u3000c".split(/\s/) }
t("b unicode gsub") { "日本語 です".gsub(/\b/, "|") }
t("union") { Regexp.union(/\w/, /(?a)\b/).source }
t("bare option then group") { "aXb" =~ /a(?i)x(?-i)b/ }
t("nested option") { "aXB" =~ /a(?i:x(?-i:B))/ }
