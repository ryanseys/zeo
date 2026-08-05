# The Ruby namespace + Ruby::Box census surface, disabled mode (no RUBY_BOX
# in the environment): identity constants alias the RUBY_* globals, the Box
# class carries its real rows, and the gate answers CRuby's disabled shape.
p Ruby.class
p Ruby.constants.sort
p [Ruby::VERSION == RUBY_VERSION, Ruby::ENGINE, Ruby::PLATFORM == RUBY_PLATFORM]
p [Ruby::COPYRIGHT == RUBY_COPYRIGHT, Ruby::DESCRIPTION == RUBY_DESCRIPTION,
   Ruby::RELEASE_DATE == RUBY_RELEASE_DATE]
p [Ruby::PATCHLEVEL == RUBY_PATCHLEVEL, Ruby::REVISION == RUBY_REVISION,
   Ruby::ENGINE_VERSION == RUBY_ENGINE_VERSION]
p Ruby::Box.superclass
p Ruby::Box.singleton_methods(false).sort
p Ruby::Box.instance_methods(false).sort
p Ruby::Box.constants.sort
p [Ruby::Box::Entry.class, Ruby::Box::Entry.superclass]
p Ruby::Box::Loader.class
p Ruby::Box.enabled?
p Ruby::Box.current
p Ruby::Box.instance_method(:initialize).arity
p Ruby::Box.instance_method(:eval).arity
p Ruby::Box.instance_method(:load_path).arity
p Ruby::Box.instance_method(:require).arity
