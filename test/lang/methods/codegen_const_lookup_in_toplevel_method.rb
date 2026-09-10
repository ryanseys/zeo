# Regression for the `String#index` nil-guard in
# `current_lexical_scope_name`. A toplevel `def show` body that
# reads a CONSTANT routes through `resolve_const_read_name`, which
# calls `current_lexical_scope_name`. Inside that helper,
# `@current_method_name.index("_cls_")` looks for the class-method
# name marker — for a toplevel def the marker isn't present, so
# `String#index` answers nil for a miss and not -1, so `if cls_idx >= 0`
# raises NoMethodError on it.

FOO = 42
def show; puts FOO; end
show
__END__
42
