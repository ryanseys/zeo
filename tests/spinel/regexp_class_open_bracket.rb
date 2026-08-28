# A '[' inside a character class never stands for itself in CRuby: it opens a
# POSIX bracket, a collating element, an equivalence class, or a class nested
# in this one. Only the bracket was read here and the rest taken as members, so
# [[a][b]] compiled to a different pattern than the one written.
# Ported from mruby-regexp cafc53ae4.
def t(label)
  r = begin
    yield.inspect
  rescue => e
    e.class.to_s
  end
  puts label + " | " + r
end

t("nested")     { Regexp.new("[[a][b]]") =~ "a" }
t("nested2")    { Regexp.new("[a[b]") =~ "a" }
t("collating")  { Regexp.new("[[.a.]]") =~ "a" }
t("equivalent") { Regexp.new("[[=a=]]") =~ "a" }

# a bracket whose name does not close says so, where the class does close
t("bracket-end"){ Regexp.new("[[:alpha]") =~ "a" }

# and where the class does not close either, the older complaint stands
t("no-close")   { Regexp.new("[[:alpha") =~ "a" }

# the bracket itself is written with a backslash, in CRuby as well
t("escaped")    { Regexp.new("[\\[]").match("[")[0] }
t("escaped-b")  { Regexp.new("[a\\[b]").match("[")[0] }

# a complete POSIX bracket still reads
t("posix")      { Regexp.new("[[:alpha:]]").match("a")[0] }
t("posix-neg")  { Regexp.new("[[:^alpha:]]").match("1")[0] }
t("posix-mix")  { Regexp.new("[[:digit:]x]").match("x")[0] }
