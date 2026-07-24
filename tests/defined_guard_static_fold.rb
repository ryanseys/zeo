# A `defined?(Const)` guard over a constant that provably can't exist folds
# the whole branch away at compile time -- the dead branch is never emitted,
# so its body may reference the missing constant (or otherwise-uncompilable
# code) without error, exactly as CRuby's reachability allows.
class Config
  VERSION = "2.5"
end

# Live value arm chosen; the dead arm's `Config::MISSING` never evaluates.
v = defined?(Config::MISSING) ? Config::MISSING : "1.0.0"
p v

# `&&` short-circuits on the missing left operand: the whole guard is
# statically false, so `Missing.enabled?` is never reached or emitted.
def toggle
  if defined?(MissingFeature) && MissingFeature.enabled?
    MissingFeature.enable
    "on"
  else
    "off"
  end
end
p toggle

# A truthy guard keeps its branch; the resolvable constant reads normally.
p(defined?(Config::VERSION) ? Config::VERSION : "none")

# `defined?` on a missing constant is `nil` in value position, too.
p defined?(Config::MISSING)
p defined?(NoSuchName)
p defined?(String)

# The full core Exception tree is nameable and rescuable, including the
# `Exception`-direct classes a bare `rescue` deliberately misses.
[SystemExit, SignalException, Interrupt, NoMemoryError, SecurityError,
 SystemStackError].each { |k| puts "#{k} < #{k.superclass}" }

begin
  raise SecurityError, "denied"
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
