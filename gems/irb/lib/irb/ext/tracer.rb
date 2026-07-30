# frozen_string_literal: true
#
#   irb/lib/tracer.rb -
#   	by Keiju ISHITSUKA(keiju@ruby-lang.org)
#
# zeo: upstream opens with `require "tracer"` and, when that raises LoadError,
# warns on $stderr and `return`s before defining anything -- its
# `IRB::CallTracer < ::CallTracer` and its `WorkSpace#evaluate` override both
# come from the third-party tracer gem. Ruby 4.0.6 does not ship that gem and
# zeo installs none, so on this platform the file ALWAYS takes the LoadError
# branch and defines nothing. What is left here is that outcome.
#
# The warn and the `return` go with it. `IRB::Context#use_tracer=` reaches
# this file through a method-body `require_relative`, which whole-program AOT
# splices at LOAD time rather than on the call -- so keeping either would
# print the warning, and end the program, in every binary that requires irb
# at all, whether or not anything ever asked for the tracer.
