# The `<<` merge key splices the referenced mapping(s) into the host
# mapping -- single alias, alias list, and inline mapping forms. zeo
# keeps a literal "<<" key (with the earlier alias bug feeding it nil).
# (Found by the 2026-08-24 probe sweep.)
require "yaml"
p YAML.safe_load("base: &b {x: 1, y: 2}\nchild:\n  <<: *b\n  y: 9", aliases: true)
p YAML.safe_load("a: &a {x: 1}\nb: &b {y: 2}\nc:\n  <<: [*a, *b]\n  z: 3", aliases: true)
p YAML.safe_load("c:\n  <<: {x: 1}\n  y: 2")
