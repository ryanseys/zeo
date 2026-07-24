module M
  X = 7
  NAME = "spinel"
  module N
    Z = 1
  end
end

# literal symbol and string names resolve to the constant value
puts M.const_get(:X)
puts M.const_get(:NAME)
puts M.const_get("X")

# a literal but unresolved name raises NameError at runtime, matching CRuby:
# a valid constant name reports "uninitialized constant", qualified by a named
# module receiver; a name without a leading uppercase reports "wrong constant name".
begin; M.const_get(:Missing); rescue => e; puts "#{e.class}: #{e.message}"; end
begin; M.const_get(:lower); rescue => e; puts "#{e.class}: #{e.message}"; end
begin; Object.const_get(:Nope); rescue => e; puts "#{e.class}: #{e.message}"; end
begin; M::N.const_get(:Missing); rescue => e; puts "#{e.class}: #{e.message}"; end
