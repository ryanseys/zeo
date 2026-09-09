# Psych's emitter styles: a multiline string dumps as a literal block
# (`|` / `|-`), binary as `!binary`, unicode plain (unquoted), infinity
# as `.inf`, Time/Date as native scalars, a Range with its `!ruby/range`
# tag, a nil key as `! ''`, a leading-space string double-quoted; the
# `indentation:`/`line_width:`/`header:` options are honored; and two
# references to ONE object dump as an anchor and alias (`&1`/`*1`). zeo
# quotes-and-inlines everything and ignores the options.
require "yaml"
require "date"
p YAML.dump({ "s" => "l1\nl2\n" })
p YAML.dump({ "s" => "l1\nl2" })
p YAML.dump("\xFF\x00".b)
p YAML.dump("héllo")
p YAML.dump(" pad")
p YAML.dump(Float::INFINITY)
p YAML.dump(Time.utc(2001, 2, 3, 4, 5, 6))
p YAML.dump(Date.new(2001, 2, 3))
p YAML.dump(1..3)
p YAML.dump({ nil => 1 })
p YAML.dump({ "a" => { "b" => 1 } }, indentation: 4)
p YAML.dump({ "s" => ("word " * 20).strip }, line_width: 20).lines.length > 2
p YAML.dump({ "a" => 1 }, header: true)
shared = [1]
p YAML.dump({ "a" => shared, "b" => shared })
__END__
"---\ns: |\n  l1\n  l2\n"
"---\ns: |-\n  l1\n  l2\n"
"--- !binary |-\n  /wA=\n"
"--- héllo\n"
"--- \" pad\"\n"
"--- .inf\n"
"--- 2001-02-03 04:05:06.000000000 Z\n"
"--- 2001-02-03\n"
"--- !ruby/range\nbegin: 1\nend: 3\nexcl: false\n"
"---\n! '': 1\n"
"---\na:\n    b: 1\n"
true
"%YAML 1.1\n---\na: 1\n"
"---\na: &1\n- 1\nb: *1\n"
