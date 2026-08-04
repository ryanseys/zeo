# `**nil` in a signature declares that the method takes NO keywords. Passing
# some is `ArgumentError: no keywords accepted`; zeo reports an arity error
# instead, having folded the keywords into a trailing positional Hash.
#
# The distinction is the point of the syntax (ruby 3.0): before it, a method
# with no keyword parameters silently accepted `f(1, b: 2)` as a Hash argument,
# and `**nil` is how you say that the old behaviour is not wanted. zeo's message
# -- "wrong number of arguments (given 2, expected 1)" -- describes exactly the
# conversion `**nil` exists to forbid, so the diagnosis points the reader at the
# wrong problem.
#
# Passing a real Hash positionally still has to work, which is the line below
# that must keep answering.

def nokw(a, **nil) = a

begin
  nokw(1, b: 2)
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end

p nokw(1)
p nokw({b: 2})

def nokw_splat(*a, **nil) = a
begin
  nokw_splat(1, b: 2)
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end
p nokw_splat(1, {b: 2})
