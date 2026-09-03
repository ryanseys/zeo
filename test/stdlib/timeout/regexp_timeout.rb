# `Regexp.timeout` is the process-wide match limit in seconds and `timeout:`
# a pattern's own; past either, a match raises `Regexp::TimeoutError`. The
# limit is a Float or nil, must be positive, and a pattern's own value is what
# `#timeout` reports (never the global default). The pattern below carries a
# backreference, which keeps ruby's own memoization from making it linear.

def t(l) = puts("#{l.ljust(20)} #{begin; yield.inspect; rescue Exception => e; "#{e.class}: #{e.message}"; end}")

t("default") { Regexp.timeout }
t("literal") { /a/.timeout }
t("negative") { Regexp.timeout = -1 }
t("zero") { Regexp.timeout = 0 }
t("negative float") { Regexp.timeout = -1.5 }
t("string") { Regexp.timeout = "x" }
t("object") { Regexp.timeout = Object.new }
t("integer") { Regexp.timeout = 2; Regexp.timeout }
t("rational") { Regexp.timeout = Rational(1, 2); Regexp.timeout }
t("nil") { Regexp.timeout = nil; Regexp.timeout }

t("per pattern") { Regexp.new("a", timeout: 2).timeout }
t("per pattern nil") { Regexp.new("a", timeout: nil).timeout }
t("with flags") { Regexp.new("a", "i", timeout: 1.5).inspect }
t("from a regexp") { Regexp.new(/a/, timeout: 2).timeout }
t("per pattern neg") { Regexp.new("a", timeout: -1) }
t("per pattern str") { Regexp.new("a", timeout: "x") }
t("unknown keyword") { Regexp.new("a", foo: 1) }
t("dup keeps") { Regexp.new("a", timeout: 3).dup.timeout }
t("quick match") { Regexp.new("a+", timeout: 3) =~ "baa" }

SLOW = "a" * 28 + "bb"
t("global raises") { Regexp.timeout = 0.05; /^(a*)*\1b$/ =~ SLOW }
t("match? raises") { /^(a*)*\1b$/.match?(SLOW) }
t("scan raises") { SLOW.scan(/^(a*)*\1b$/) }
t("gsub raises") { SLOW.gsub(/^(a*)*\1b$/, "") }
t("case raises") { case SLOW when /^(a*)*\1b$/ then 1 end }
t("rescued") { /^(a*)*\1b$/ =~ SLOW rescue $!.class.ancestors.include?(RegexpError) }
t("per pattern raises") { Regexp.timeout = nil; Regexp.new("^(a*)*\\1b$", timeout: 0.05) =~ SLOW }
t("global off") { Regexp.timeout = nil; /a+/ =~ "baa" }
__END__
default              nil
literal              nil
negative             ArgumentError: invalid timeout: -1
zero                 ArgumentError: invalid timeout: 0
negative float       ArgumentError: invalid timeout: -1.5
string               TypeError: no implicit conversion to float from string
object               TypeError: can't convert Object into Float
integer              2.0
rational             0.5
nil                  nil
per pattern          2.0
per pattern nil      nil
with flags           "/a/i"
from a regexp        2.0
per pattern neg      ArgumentError: invalid timeout: -1
per pattern str      TypeError: no implicit conversion to float from string
unknown keyword      ArgumentError: unknown keyword: :foo
dup keeps            3.0
quick match          1
global raises        Regexp::TimeoutError: regexp match timeout
match? raises        Regexp::TimeoutError: regexp match timeout
scan raises          Regexp::TimeoutError: regexp match timeout
gsub raises          Regexp::TimeoutError: regexp match timeout
case raises          Regexp::TimeoutError: regexp match timeout
rescued              true
per pattern raises   Regexp::TimeoutError: regexp match timeout
global off           1
