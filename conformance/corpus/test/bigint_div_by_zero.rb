# Bigint division / modulo by zero raises ZeroDivisionError.
#
# Bigint detection: `a = a * 2` inside a `while` loop promotes the
# local to bigint via scan_bigint_in_loop_node. Raising 1 by *2 50
# times overflows the mrb_int range so `a` is bigint-typed by the
# time we hit the / 0 expression.
#
# Runtime path: sp_bigint_div / sp_bigint_mod call into mini-gmp,
# which detects the zero divisor at sp_bigint.c:2718 and calls
# mrb_raise(mrb, E_ZERODIV_ERROR, "divided by 0"). The mrb_raise
# macro now dispatches to sp_bigint_raise_zerodiv (defined non-
# static in sp_runtime.h, linked from gen.c), which calls
# sp_raise_cls — same longjmp path as the int-div case.

a = 1
i = 0
while i < 50
  a = a * 2
  i = i + 1
end

# `a` is now 2^50, well above the mrb_int range — bigint-typed.

begin
  x = a / 0
  puts "no raise"
rescue ZeroDivisionError => e
  puts "caught bigint div: #{e}"
end

begin
  x = a % 0
  puts "no raise"
rescue ZeroDivisionError => e
  puts "caught bigint mod: #{e}"
end

# Bare rescue also catches.
begin
  a / 0
rescue => e
  puts "bare-rescue: #{e}"
end
