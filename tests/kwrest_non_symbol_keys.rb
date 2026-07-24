# A `**kwrest` parameter captures keyword arguments with ANY key, not just
# symbols -- the options-hash idiom `method CONST => :value` (Thor's
# `map HELP_MAPPINGS => :help`) passes a non-symbol key, which real Ruby routes
# into `**kw` as a hash entry (a keyword parameter NAME is always a symbol, so a
# non-symbol key can only land in the rest). Distinct from an explicit `{...}`
# positional Hash, which the Ruby 3 keyword separation keeps out of `**kw`.
HELP = %w[-h --help]

def cmd(mappings = nil, **kw)
  "m=#{mappings.inspect} kw=#{kw.inspect}"
end

puts cmd HELP => :help        # non-symbol (array) key -> **kw
puts cmd(a: 1, b: 2)          # symbol keys -> **kw
puts cmd("s" => 1, x: 2)      # mixed keys -> one **kw hash
puts cmd({ z: 9 })            # explicit hash is POSITIONAL, not keywords

def only_kw(**opts)
  opts.inspect
end

puts only_kw HELP => :help
puts only_kw(a: 1)
