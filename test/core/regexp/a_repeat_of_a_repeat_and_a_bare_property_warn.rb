# A pattern built at run time warns as ruby's parser reads it: a `\p` with no
# braces, and a repeat of a repeat, reduced the way Onigmo reduces it (so
# `a***` warns twice). The pattern is shown after `\u` and `\C-` expansion,
# with comments dropped, and left off once it is too long for the message.
# A static literal warns nothing.
$stderr.sync = true
qs = %w[? * + ?? *? +?]
patterns = qs.flat_map { |i| qs.map { |o| "(?:a#{i})#{o}" } }
patterns += [
  "a***", "a+*+", "a**+", "a***+", "a+?**", "(?:a+)?*", "a{0,}**", "a**{2}",
  "a{,2}**", "a{x}**", "a{2}?*", "(?:a*){2}", "(?:(?:a+)*)?", "(a*)*",
  "(?<n>a*)*", "(?i:a*)*", "(?>a*)*", "(?: a*)*", "(?:ab*)*", "(?:|a*)*",
  "(?:a*(?#c))*", "a\\K**", "\\A**", "\\u{61}**", "\\C-a**", "a/b**", "\t**",
  "\\p", "\\pL", "[\\p]", "\\P", "\\p{L}", "\\\\p", "\\p**",
  "(?x) a * * ", "(?x)a*#c\n*", "(?x)[#]**", "(?x)(?-x:# )**",
  "a" * 46 + "**", "a" * 47 + "**", "\\p" + "a" * 54, "\\p" + "a" * 55,
]
patterns.each do |pat|
  $stderr.puts "== #{pat.inspect}"
  Regexp.new(pat)
end
$stderr.puts "== extended flag"
Regexp.new("a* # c\n *", Regexp::EXTENDED)
$stderr.puts "== interpolated"
x = "c"
2.times { /#{x}**/ }
$stderr.puts "== static"
/e**/
$stderr.puts "== silenced"
$VERBOSE = nil
Regexp.new("f**")
p :done
__END__
:done
#@ stderr
== "(?:a?)?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '?': /(?:a?)?/
== "(?:a?)*"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '?' and '*' was replaced with '*' in regular expression: /(?:a?)*/
== "(?:a?)+"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '?' and '+' was replaced with '*' in regular expression: /(?:a?)+/
== "(?:a?)??"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '?' and '??' was replaced with '??' in regular expression: /(?:a?)??/
== "(?:a?)*?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '?' and '*?' was replaced with '*?' in regular expression: /(?:a?)*?/
== "(?:a?)+?"
== "(?:a*)?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /(?:a*)?/
== "(?:a*)*"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /(?:a*)*/
== "(?:a*)+"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /(?:a*)+/
== "(?:a*)??"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '*' and '??' was replaced with '+ and ??' in regular expression: /(?:a*)??/
== "(?:a*)*?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '*' and '*?' was replaced with '+ and ??' in regular expression: /(?:a*)*?/
== "(?:a*)+?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /(?:a*)+?/
== "(?:a+)?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '+' and '?' was replaced with '*' in regular expression: /(?:a+)?/
== "(?:a+)*"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '+' and '*' was replaced with '*' in regular expression: /(?:a+)*/
== "(?:a+)+"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '+': /(?:a+)+/
== "(?:a+)??"
== "(?:a+)*?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '+' and '*?' was replaced with '+ and ??' in regular expression: /(?:a+)*?/
== "(?:a+)+?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '+': /(?:a+)+?/
== "(?:a??)?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '??': /(?:a??)?/
== "(?:a??)*"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '??' and '*' was replaced with '*?' in regular expression: /(?:a??)*/
== "(?:a??)+"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '??' and '+' was replaced with '*?' in regular expression: /(?:a??)+/
== "(?:a??)??"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '??': /(?:a??)??/
== "(?:a??)*?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '??' and '*?' was replaced with '*?' in regular expression: /(?:a??)*?/
== "(?:a??)+?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '??' and '+?' was replaced with '*?' in regular expression: /(?:a??)+?/
== "(?:a*?)?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*?': /(?:a*?)?/
== "(?:a*?)*"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*?': /(?:a*?)*/
== "(?:a*?)+"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*?': /(?:a*?)+/
== "(?:a*?)??"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*?': /(?:a*?)??/
== "(?:a*?)*?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*?': /(?:a*?)*?/
== "(?:a*?)+?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*?': /(?:a*?)+?/
== "(?:a+?)?"
== "(?:a+?)*"
== "(?:a+?)+"
== "(?:a+?)??"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '+?' and '??' was replaced with '*?' in regular expression: /(?:a+?)??/
== "(?:a+?)*?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '+?' and '*?' was replaced with '*?' in regular expression: /(?:a+?)*?/
== "(?:a+?)+?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '+?': /(?:a+?)+?/
== "a***"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a***/
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a***/
== "a+*+"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '+' and '*' was replaced with '*' in regular expression: /a+*+/
== "a**+"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a**+/
== "a***+"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a***+/
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a***+/
== "a+?**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a+?**/
== "(?:a+)?*"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '+' and '?' was replaced with '*' in regular expression: /(?:a+)?*/
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /(?:a+)?*/
== "a{0,}**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a{0,}**/
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a{0,}**/
== "a**{2}"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a**{2}/
== "a{,2}**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a{,2}**/
== "a{x}**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a{x}**/
== "a{2}?*"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '?' and '*' was replaced with '*' in regular expression: /a{2}?*/
== "(?:a*){2}"
== "(?:(?:a+)*)?"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: nested repeat operator '+' and '*' was replaced with '*' in regular expression: /(?:(?:a+)*)?/
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /(?:(?:a+)*)?/
== "(a*)*"
== "(?<n>a*)*"
== "(?i:a*)*"
== "(?>a*)*"
== "(?: a*)*"
== "(?:ab*)*"
== "(?:|a*)*"
== "(?:a*(?#c))*"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /(?:a*)*/
== "a\\K**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a\K**/
== "\\A**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /\A**/
== "\\u{61}**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /\x61**/
== "\\C-a**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /\x01**/
== "a/b**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /a\/b**/
== "\t**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /\x09**/
== "\\p"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: invalid Unicode Property \p: /\p/
== "\\pL"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: invalid Unicode Property \p: /\pL/
== "[\\p]"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: invalid Unicode Property \p: /[\p]/
== "\\P"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: invalid Unicode Property \P: /\P/
== "\\p{L}"
== "\\\\p"
== "\\p**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: invalid Unicode Property \p: /\p**/
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /\p**/
== "(?x) a * * "
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /(?x) a * * /
== "(?x)a*#c\n*"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /(?x)a**/
== "(?x)[#]**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /(?x)[#]**/
== "(?x)(?-x:# )**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /(?x)(?-x:# )**/
== "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*': /aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa**/
== "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa**"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: regular expression has redundant nested repeat operator '*'
== "\\paaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: invalid Unicode Property \p: /\paaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/
== "\\paaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:20: warning: invalid Unicode Property \p
== extended flag
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:23: warning: regular expression has redundant nested repeat operator '*': /a*  */
== interpolated
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:26: warning: regular expression has redundant nested repeat operator '*': /c**/
core/regexp/a_repeat_of_a_repeat_and_a_bare_property_warn.rb:26: warning: regular expression has redundant nested repeat operator '*': /c**/
== static
== silenced
