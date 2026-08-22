# The Ruby namespace + Ruby::Box census surface, disabled mode (no RUBY_BOX
# in the environment -- the harness sets the gate only for a source that
# ALLOCATES a box): identity constants alias the RUBY_* globals, the Box
# class carries its real rows, and the gate answers CRuby's disabled shape.
p Ruby.class
p Ruby.constants.sort
p [Ruby::VERSION == RUBY_VERSION, Ruby::ENGINE, Ruby::PLATFORM == RUBY_PLATFORM]
p [Ruby::COPYRIGHT == RUBY_COPYRIGHT, Ruby::DESCRIPTION == RUBY_DESCRIPTION,
   Ruby::RELEASE_DATE == RUBY_RELEASE_DATE]
p [Ruby::PATCHLEVEL == RUBY_PATCHLEVEL, Ruby::REVISION == RUBY_REVISION,
   Ruby::ENGINE_VERSION == RUBY_ENGINE_VERSION]
p Ruby::Box.superclass
# CRuby defines the gate-only rows (`main`/`root`/`master` and their
# predicates) ONLY under `RUBY_BOX=1`; zeo's are in the static table
# either way, so this pins the surface both modes share. See
# `tests/gaps/a_disabled_box_still_carries_its_gate_rows.rb`.
GATE_ONLY = %i[main master root main? master? root?]
p (Ruby::Box.singleton_methods(false) - GATE_ONLY).sort
p (Ruby::Box.instance_methods(false) - GATE_ONLY).sort
p Ruby::Box.constants.sort
p [Ruby::Box::Entry.class, Ruby::Box::Entry.superclass]
p Ruby::Box::Loader.class
p Ruby::Box.enabled?
p Ruby::Box.current
p Ruby::Box.instance_method(:initialize).arity
p Ruby::Box.instance_method(:eval).arity
p Ruby::Box.instance_method(:load_path).arity
p Ruby::Box.instance_method(:require).arity
