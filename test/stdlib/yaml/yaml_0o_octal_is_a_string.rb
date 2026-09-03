# YAML's octal scalar is `017`; the Ruby-spelling `0o17` is a plain
# STRING to psych. zeo parses it as octal 15. (Found by the 2026-08-24
# probe sweep.)
require "yaml"
p YAML.safe_load("w: 0o17")
p YAML.safe_load("v: 017")
__END__
{"w" => "0o17"}
{"v" => 15}
