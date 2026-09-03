# Regression guard for the fix above: `||=`'s new leniency must NOT
# leak into `+=`/`&&=` -- confirmed against real `ruby` that those
# still raise on a genuinely undefined constant. An uncaught Ruby
# exception exits the compiled binary nonzero with a message on
# stderr (see `uncaught_raise_with_no_rescue_anywhere_exits_with_the_message`),
# not a Rust-level panic, so this asserts on the process output
# directly rather than `#[should_panic]`.

UNDEF += 1
__END__
#@ stderr
lang/variables/other_compound_assignment_operators_still_raise_on_an_undefined_constant.rb:9:in '<main>': uninitialized constant UNDEF (NameError)
#@ exit 1
