# The `<<` merge key splices the referenced mapping(s) into the host
# mapping -- single alias, alias list, and inline mapping forms. zeo
# keeps a literal "<<" key (with the alias bug feeding it nil).
require "yaml"
p YAML.safe_load("base: &b {x: 1, y: 2}\nchild:\n  <<: *b\n  y: 9", aliases: true)
p YAML.safe_load("a: &a {x: 1}\nb: &b {y: 2}\nc:\n  <<: [*a, *b]\n  z: 3", aliases: true)
p YAML.safe_load("c:\n  <<: {x: 1}\n  y: 2")
__END__
{"base" => {"x" => 1, "y" => 2}, "child" => {"x" => 1, "y" => 9}}
{"a" => {"x" => 1}, "b" => {"y" => 2}, "c" => {"y" => 2, "x" => 1, "z" => 3}}
{"c" => {"x" => 1, "y" => 2}}
