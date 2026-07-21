# Issue #704: begin/rescue used as an expression (puts arg).
# Three shapes:
#   - rescue path (raise + rescue + else + ensure)
#   - else path (no raise, rescue chain skipped)
#   - multi-class rescue (raise SubClass, match second clause)
puts (begin; raise "x"; rescue; "caught"; else; "no error"; ensure; "ensured"; end)
puts (begin; 42; rescue; "caught"; else; "no error"; end)
puts (begin; raise ArgumentError; rescue TypeError; "type"; rescue ArgumentError; "arg"; end)

# Bare begin..end as expression (no rescue / ensure).
puts (begin; 100; end)

# Single-class raise + rescue.
puts (begin; raise NotImplementedError; rescue NotImplementedError; "ni"; end)
