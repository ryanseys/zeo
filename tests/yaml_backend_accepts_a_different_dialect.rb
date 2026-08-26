# DECIDED DIVERGENCE. Zeo parses YAML with `yaml-rust2` where CRuby uses
# libyaml, and the two do not draw the line between a legal and an illegal
# document in exactly the same place. The rows below are every difference
# found by sweeping the truncations and byte shapes in
# `tests/yaml_parser_edge_cases.rb` -- which is where every row the two
# engines AGREE on lives, all of them exact.
#
# The differences are all at the PARSER, not at the semantics: what a
# document means is zeo's own code and matches psych exactly. Closing these
# means forking a YAML parser, which buys a handful of error classes on
# input no program means to write.
#
# Six shapes, and they group into three:
#
#   * yaml-rust2 ACCEPTS what libyaml refuses -- a lone `: `, a bare `...`,
#     a tab-indented mapping, an invalid UTF-8 byte, an embedded NUL.
#   * yaml-rust2 REFUSES what libyaml accepts -- an alias used as a key.
#   * yaml-rust2 keeps a byte-order mark as part of the first key.
#
# DEEP NESTING is the same story with a happier ending: both engines
# refuse it and neither ends the process, they just disagree about where
# and with which class. yaml-rust2 bounds its own recursion and reports a
# SyntaxError; libyaml runs until ruby raises SystemStackError. Nothing to
# fix -- the backend already does the bounding a recursive descent would
# otherwise need its own ceiling for.

require "yaml"

def show(name)
  r = yield
  puts "#{name}\t#{r.inspect}"
rescue Exception => e
  puts "#{name}\t#{e.class}"
end

show("a lone colon") { YAML.unsafe_load(": ") }
show("a bare document end") { YAML.unsafe_load("...") }
show("a tab alone") { YAML.unsafe_load("\t") }
show("a tab-indented mapping") { YAML.unsafe_load("\ta: 1") }
show("a nested tab indent") { YAML.unsafe_load("a:\n\tb: 1") }
show("an invalid utf-8 byte") { YAML.unsafe_load("v: \"\xFF\"".b) }
show("an embedded nul") { YAML.unsafe_load("v: a\0b") }
show("an alias as a key") { YAML.unsafe_load("a: &x k\n*x: 1") }
show("a byte-order mark") { YAML.unsafe_load("\uFEFFv: 1") }
show("an empty literal block") { YAML.unsafe_load("|\n") }
show("an omap of scalars") { YAML.unsafe_load("v: !!omap\n- 1\n- 2")["v"] }

# Deep nesting: refused by both, with different classes and at a different
# depth. The value of the row is that NEITHER ends the process.
[500, 2_000, 100_000].each do |n|
  show("nested #{n} deep") { YAML.unsafe_load("[" * n + "]" * n).class }
end
