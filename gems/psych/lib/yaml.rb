# `yaml.rb` is a shim in ruby too: it loads psych and aliases the constant.
# Without it `require "yaml"` activated the native engine but never spliced
# psych's Ruby half, so `Psych::VERSION` and `Object#to_yaml` were missing
# under that spelling while `require "psych"` had them.
require "psych"
