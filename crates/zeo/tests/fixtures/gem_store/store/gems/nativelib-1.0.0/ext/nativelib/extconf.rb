# frozen_string_literal: true

# The fixture store's buildable gem. Deliberately tiny: what is on trial is
# the PATH -- gemspec `extensions` -> extconf -> Makefile -> compile -> link
# -> dlopen -> `Init_nativelib` -> a method Ruby can call -- not any part of
# the C API.
require "mkmf"

create_makefile("nativelib/nativelib")
