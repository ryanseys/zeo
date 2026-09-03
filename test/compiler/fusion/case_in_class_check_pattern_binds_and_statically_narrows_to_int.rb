# `Integer => n` both binds `n` AND statically narrows its type for the
# rest of the arm's body -- `n + 1` should take the native `Int`
# arithmetic fast path, not a runtime Poly fallback.

case 5
in Integer => n
  puts n + 1
end
__END__
6
