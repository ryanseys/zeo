# Four divergences, and the mechanism behind each

[Compatibility](../reference/compatibility.md) states what zeo answers
differently. This page says why, for the four whose reason is structural
rather than a missing method.

Two come from the same decision: zeo splices every required file into its
requirer at compile time, so a program is one unit of compilation rather than
a sequence of loads. Two come from the singleton-class model.

## Where a nested `require` lands

A require the file's load reaches but that is not a top-level statement — one
under a conditional, in a `begin`, in a class body, in a block — is spliced at
the position of the **statement that holds it**:

```ruby
require_relative "../minitest"          # first
require_relative "spec"                 # second
require_relative "hell" if ENV["MT_HELL"]   # third, here -- not at the top
```

The guard itself is not evaluated at compile time (only the platform-detection
idioms `RUBY_ENGINE == "jruby"` and friends are), so a require in a branch that
never runs is still compiled in and still executes. That is usually invisible —
every extension links statically — but not always: `minitest/hell.rb` calls
`parallelize_me!` and warns about a missing optional gem, neither of which
CRuby does when `MT_HELL` is unset.

A file loaded this way runs its top level at program START, not on the call, so
a lazily-required file that only means to *warn and bail* on a missing optional
dependency does both at startup instead. `irb/ext/tracer.rb` is one; see
`crates/zeo/src/gems/bundled.rs` for where the bundled libraries come from,
and `cargo xtask deps` for how they get there.

## A class written in a `class << self` body

```ruby
module Color
  class << self
    class Visitor; end        # belongs to Color's SINGLETON class
    def paint = Visitor.new   # ...which is what makes this bare name resolve
  end
end
```

`Visitor` belongs to `Color.singleton_class`, exactly as a constant written
there does -- a `class X` IS a constant write with a body. The singleton
methods beside it see it by bare name, `Color.constants` is empty, and
`Color::Visitor` raises `NameError`. All of that now matches.

One thing does not: `Visitor.name`. Real Ruby answers
`"#<Class:0x00007f...>::Visitor"`, an address nobody can reproduce; zeo
answers `nil`, because the qualified name it builds is `#<Class:Color>::Visitor`
and `Module#name` reports nothing for a name no constant path can spell. The
class itself is identical either way.

A body compiled at RUN time (`eval` / `class_eval`) still hands both the
constant and the class to the enclosing module -- there is no compile-time
surrogate to file them on.

`test/lang/singleton/singleton_body_class_and_self_path.rb` pins the behaviour.

## A top-level `return` inside a required file

`return` at the top level ends the program. In a file the main script
`require`s, real Ruby ends only THAT file's load and carries on in the
requirer; Zeo splices required files into their requirer, so the `return`
reaches the top level of the whole program and ends it. Exit status stays 0 and
`at_exit` handlers still run, both matching a top-level `return` in the main
script.

## `extend` on a class, at runtime

`Klass.extend M` written as a runtime call — as opposed to `extend M` in the
class body — installs each of `M`'s methods as a class method of `Klass`. The
body runs with `self` bound to the class, so an implicit-self class-method call
and an `@ivar` write both land where Ruby says: `@x` is the class's own
class-level slot, the same one a `def self.x` reads. The singleton gem depends
on exactly this (`klass.extend SingletonClassMethods`, then
`klass.instance_eval { set_mutex(Thread::Mutex.new) }`).

The ancestry follows too: `M` is recorded on the receiver's singleton chain, so
`Klass.is_a?(M)` and `Klass.singleton_class.ancestors` both report it, and a
repeat `extend` leaves the module at the rank its first one gave it. The same
holds for a per-object `obj.extend(M)`: `obj.is_a?(M)` is true while
`obj.class` and every other instance of that class stay untouched.

The copies carry it the way Ruby's do: an object's `clone` copies the
singleton class (extended modules and `def obj.method` rows) and `dup` drops
it; a class's `dup` and `clone` both keep it, because `rb_mod_init_copy`
clones the singleton class either way. Pinned by
`test/lang/singleton/an_extended_receiver_copies_like_ruby.rb`.
