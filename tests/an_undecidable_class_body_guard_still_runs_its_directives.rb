# A class-body `if` whose condition is a real RUNTIME question -- reading ENV,
# say -- cannot be folded away, so both branches survive to run at the class's
# definition site. A `def` in one already had its own conditional registration
# and its own runtime emission; every OTHER definition-level directive had none,
# and reached codegen as "a definition-level construct used as a VALUE".
#
# ruby_parser closes with exactly this: `if ENV["RP_LINENO_DEBUG"] then class
# RubyLexer; alias old_lineno= lineno=; ...`, a debug hook nobody can decide at
# compile time.

module Extra
  def extra = :mixed_in
end

module Front
  def size = "front(#{super})"
end

class L
  def size = 1
  def wide = :wide
  def narrow = :narrow
end

# `ENV["ZEO_UNSET_AT_BUILD"]` is nil, so the branch does not run -- but it still
# has to COMPILE, and the class must come out untouched.
if ENV["ZEO_UNSET_AT_BUILD"] then
  class L
    alias old_size size
    include Extra
    prepend Front
    private :wide
    undef narrow
    module_function :wide
  end
end

p L.new.size
p [L.new.respond_to?(:old_size), L.include?(Extra), L.ancestors.first]
p [L.new.respond_to?(:wide), L.new.respond_to?(:narrow)]

# The same directives under a guard that is decidably TRUE take effect, so the
# two halves are not just "compiles and does nothing".
class R
  def size = 1
  def wide = :wide
end

if RUBY_VERSION then
  class R
    alias old_size size
    include Extra
    private :wide
  end
end

p [R.new.old_size, R.new.extra, R.include?(Extra)]
p [R.new.respond_to?(:wide), R.new.respond_to?(:wide, true)]

# A nested `if` inside the branch is rewritten the same way, at any depth.
class N
  def a = :a
end

if ENV["ZEO_UNSET_AT_BUILD"] then
  class N
    if ENV["ZEO_ALSO_UNSET"] then
      alias b a
    else
      alias c a
    end
  end
end

p [N.new.respond_to?(:b), N.new.respond_to?(:c)]

# ---------------------------------------------------------------------------
# A version gate spelled as an ANCHORED regexp is a build-time question, the
# same one `RUBY_VERSION < "2.0"` asks. ipaddress guards its 1.8 compatibility
# shims with `if RUBY_VERSION =~ /^1\.8/`, and left undecided the whole branch
# -- `class Hash; alias :key :index; end` -- had to compile.

if RUBY_VERSION =~ /^1\.8/
  class Hash
    alias key index
  end
end

p({ a: 1 }.respond_to?(:key))

# The anchor is what makes it decidable: `^` is a prefix test, `$` a suffix,
# both together equality, and no anchor at all a substring search.
p [(RUBY_VERSION =~ /^4\./) ? :yes : :no, (RUBY_VERSION =~ /^1\./) ? :yes : :no]
p [(RUBY_ENGINE =~ /^ruby$/) ? :yes : :no, (RUBY_ENGINE =~ /^rub$/) ? :yes : :no]
p [(RUBY_ENGINE =~ /ruby|jruby/) ? :yes : :no, (RUBY_ENGINE =~ /jruby|rbx/) ? :yes : :no]

# An escaped dot means a real dot -- `/^1\.8/` must not match "148".
p ["1.8.7" =~ /^1\.8/, "148" =~ /^1\.8/, "x1.8" =~ /^1\.8/]
