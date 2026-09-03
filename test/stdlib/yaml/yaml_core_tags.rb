# Psych applies core and ruby tags: `!!str` forces string, `!!int`/
# `!!float` coerce a quoted scalar, `!!binary` base64-decodes, `!!set`
# is gated (Psych::Set needs permitting), `!!omap` loads as a Hash,
# `!ruby/symbol` is gated behind `permitted_classes: [Symbol]` (and
# converts when permitted, incl. under unsafe_load), and `!ruby/object:`
# is gated. zeo ignores the tags wholesale. (Found by the 2026-08-24
# probe sweep.)
require "yaml"
def show
  p yield
rescue Psych::DisallowedClass => e
  puts "#{e.class}: #{e.message}"
end
show { YAML.safe_load("v: !!str 123") }
show { YAML.safe_load(%(v: !!int "42")) }
show { YAML.safe_load("v: !!float 3") }
show { YAML.safe_load("v: !!binary aGVsbG8=") }
show { YAML.safe_load("v: !!set {a: null, b: null}") }
show { YAML.safe_load("v: !!omap [{a: 1}, {b: 2}]") }
show { YAML.safe_load("v: !ruby/symbol foo") }
show { YAML.safe_load("v: !ruby/symbol foo", permitted_classes: [Symbol]) }
show { YAML.safe_load("v: !ruby/object:OpenStruct {}") }
show { Psych.unsafe_load("v: !ruby/symbol s") }
__END__
{"v" => "123"}
{"v" => 42}
{"v" => 3.0}
{"v" => "hello"}
Psych::DisallowedClass: Tried to load unspecified class: Psych::Set
{"v" => {"a" => 1, "b" => 2}}
Psych::DisallowedClass: Tried to load unspecified class: Symbol
{"v" => :foo}
Psych::DisallowedClass: Tried to load unspecified class: OpenStruct
{"v" => :s}
