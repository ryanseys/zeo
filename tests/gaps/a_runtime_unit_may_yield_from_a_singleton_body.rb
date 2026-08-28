# GAP: a `yield` inside a `def` inside a `class << self` body is refused as
# `Invalid yield` when the file is compiled at RUN time.
#
# NARROWED, all three measured against ruby 4.0.6:
#   * `def self.take(x) ... yield ... end`      -- loads and runs. FINE.
#   * `def take(x) ... yield ... end` +
#     `module_function :take`                   -- loads and runs. FINE.
#   * `class << self; def take(x); yield(x); end; end` -- SyntaxError.
#
# It is the RUN-TIME compile that refuses: the same file required statically
# compiles and runs. So this is not about `class << self`, and not about
# `yield` -- it is the two together on the path a computed `require` takes.
#
# WHERE IT COMES FROM: `eval.rs::invalid_yield` walks a snippet for a `yield`
# that has no block channel and stops the walk at `HirNode::DefMethod`. A def
# written in a singleton body IS a `DefMethod` (it carries the
# `SINGLETON_BODY_DEF` flag), so the walk ought to stop -- which means the
# yield is being reached by some route other than that def's own children.
# Finding the route is the work; the `continue` is already there.
#
# WHY IT MATTERS: it is what stands between `zeo gem install` -- which works
# -- and USING what it installed. Activating a gem makes rubygems require
# more of its own library at run time, and `gems/time/lib/time.rb:478` is
# exactly this shape, so `gem "x"; require "x"` dies on a file that has
# nothing to do with the gem.
DIR = File.expand_path("fixtures/singleton_yield", __dir__)
$LOAD_PATH.unshift(DIR)

name = "singleton_yield_target"
require name

p SingletonYield.take(2) { |v| v * 10 }
