# A top-level class/module definition guarded by `defined?(Mod::VALUE_CONST)`
# where the constant is a plain VALUE (not a class) that the module's own body
# assigns. zeo must statically decide the guard (this is the rubygems idiom
# `unless defined?(Psych::VERSION); module Psych; ...`).
module Lib
  VERSION = "1.2.3"
end

unless defined?(Lib::VERSION)
  module Lib
    class Missing < StandardError; end
  end
end

puts(defined?(Lib::VERSION) ? "have-version" : "fallback")
puts Lib::VERSION
puts(defined?(Lib::Missing) ? "has-missing" : "no-missing")
__END__
have-version
1.2.3
no-missing
