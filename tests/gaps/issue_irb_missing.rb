# irb isn't vendored under gems/, so `require "irb"` raises LoadError. It is
# not a vendoring job: every irb since 1.15 reads Ruby source through a
# RUBY-LEVEL Prism -- `Prism.lex_compat` in `ruby-lex.rb` and
# `nesting_parser.rb`, a `Prism::Visitor` subclass in `color.rb` -- and zeo
# exposes no such API (it embeds prism as a Rust crate, for its own front
# end). Ripper, irb's older lexer, is absent for the same reason. reline, the
# line editor irb reads through, IS vendored and works.
require "irb"
p defined?(IRB)
