# When an `if` expression's value is consumed (assignment, return,
# ternary slot, etc.) and a branch holds multiple statements,
# compile_if_expr previously kept only the last statement of each
# branch — emitting `(cond ? lastexpr : ...)`. Any leading
# side-effect statements in the branch were silently dropped.
#
# Fix: detect multi-stmt branches, emit a real `if (cond) { ... }
# else { ... }` that compiles each leading statement for its side
# effect and assigns the final expression to a temp. Return the
# temp as the if-expression's value. Single-stmt / no-stmt branches
# keep using the original ternary fast path.
#
# Coverage:
# - Multi-stmt then- and else-branches assigned to a local
# - Elsif chain recursion (multi-stmt branches inside the else slot)
# - Nested if-expression used as a method argument
# - Mixed branch lengths (single-stmt then, multi-stmt else)
# - Empty/missing else (implicit nil → 0)
#
# Out of scope:
# - Poly type-unification of mixed concrete branches (int/string).
#   Spinel's inference picks one concrete type as the unified type
#   rather than `poly`, so `tmp = 1LL;` is emitted against a
#   `const char *` slot. The bug isn't in this PR's multi-stmt
#   restructuring — the ternary fast path has the same inference
#   issue. Separate fix.

x = if true
      puts "a"
      1
    else
      puts "z"
      0
    end
puts x

y = if false
      puts "skipped"
      99
    else
      puts "b"
      puts "c"
      42
    end
puts y

# Elsif chain recursion.
z = if false
      1
    elsif true
      puts "elsif"
      2
    else
      3
    end
puts z

# Nested if-expression as a method argument.
puts(if true
       puts "nested"
       10
     end)

# Mixed branch lengths.
m = if false
      99
    else
      puts "else-multi"
      100
    end
puts m

# Empty/missing else.
n = if true
      puts "no-else"
      77
    end
puts n
