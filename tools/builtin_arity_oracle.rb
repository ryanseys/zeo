#!/usr/bin/env ruby
# frozen_string_literal: true

# Dumps CRuby's own arity/visibility/parameter data for every class zeo
# declares, so the drift test in `crates/zeo/tests/builtin_arity.rs` can run
# with no ruby installed.
#
# Do not run this directly -- `cargo run -p xtask -- arity-oracle` resolves the
# pinned oracle through mise, feeds the class manifest on stdin, and writes the
# sorted result to `conformance/builtin-arity.tsv`.
#
# Why a dump at all: CRuby's reported arity is an artifact of how each method
# happens to be implemented. A C function declares `argc`, so it can only ever
# report N or -1 -- it cannot express "1 required plus 1 optional". A method
# written in Ruby keeps its real signature and reports -2 for the same shape.
# The `impl` column records which, because that one bit is the whole difference
# between the two numbers and it cannot be derived from the signature.

# The manifest: one `C` row per class zeo declares.
#   C <const> <ruby-name> <is_module> <feature-or-dash>
classes = []
features = []
$stdin.each_line do |line|
  next unless line.start_with?("C\t")

  _, const, name, is_module, feature = line.chomp.split("\t")
  classes << { const: const, name: name, module: is_module == "true", feature: feature }
  features << feature unless feature == "-"
end

# `Object.const_get` cannot see a gated class before its `require`. YAML and
# WeakRef additionally alias constants that only appear once their own file is
# loaded, so they are requested unconditionally. So is every stdlib file that
# ADDS to a class zeo answers WITHOUT a require -- `io/console`, `io/nonblock`
# and `io/wait` for `raw`/`nonblock?`/`wait_readable`, `objspace` for
# `ObjectSpace.memsize_of` and friends, `digest/bubblebabble` for
# `Digest.bubblebabble`. Without them loaded the comparison would be about
# which file defines the method rather than about its arity.
missing_features = {}
extra = %w[yaml weakref io/console io/nonblock io/wait objspace digest/bubblebabble]
(features.uniq + extra).each do |feature|
  require feature
rescue LoadError => e
  missing_features[feature] = e.message
end

# Names `Object.const_get` cannot parse. `ARGF` is an object; the class zeo
# models is its singleton.
RESOLVERS = {
  "ARGF.class" => -> { ARGF.class },
}.freeze

# Every helper lives here rather than at the top level, because a top-level
# `def` IS a private instance method of Object -- and Object is a class the
# manifest asks about, so the tool would dump itself.
module Dump
  # Modules a locally-installed GEM patched into a core class. The manifest
  # reaches them only as a side effect of asking about that gem's own classes
  # (`FFI::ModernForkTracking` rides in with `FFI::Pointer`), and they describe
  # the machine the dump ran on rather than ruby -- so they are skipped both as
  # an ancestry entry and as the owner a method resolves to.
  GEM_PATCHES = %w[FFI::ModernForkTracking].freeze

  module_function

  # The definition `owner` itself contributes, looking past any gem patch that
  # prepended itself over the real one.
  def unpatched(meth)
    meth = meth.super_method while meth && GEM_PATCHES.include?(meth.owner.to_s)
    meth
  end

  # `kind` is `i` for instance methods, `s` for singleton (`def self.`) methods.
  def own_methods(klass, kind)
    target = kind == "s" ? klass.singleton_class : klass
    names = target.instance_methods(false) + target.private_instance_methods(false) +
            target.protected_instance_methods(false)
    names.uniq.filter_map do |n|
      meth = unpatched(target.instance_method(n))
      [n, meth] if meth
    end
  end

  def visibility_of(klass, kind, name)
    target = kind == "s" ? klass.singleton_class : klass
    return "private" if target.private_instance_methods(false).include?(name)
    return "protected" if target.protected_instance_methods(false).include?(name)

    "public"
  end

  # `[[:req, :fmt], [:key, :buffer]]` -> `req:fmt,key:buffer`. A C function has
  # no parameter names, so this degenerates to `rest` / `req,req` -- which is
  # exactly the signal the drift test uses to decide the row carries no shape
  # information.
  def params_of(meth)
    parts = meth.parameters.map { |kind, name| name ? "#{kind}:#{name}" : kind.to_s }
    parts.empty? ? "-" : parts.join(",")
  end

  # An ancestor is either a plain module/class (`i:Enumerable`) or some class's
  # singleton (`s:IO`), so instance and singleton resolution read the same way.
  #
  # Answers the token AND the module whose own methods `dump_own` should read,
  # because the token cannot always be resolved back by name: `StringIO` really
  # inherits `<<` from `IO::generic_writable`, a constant `Object.const_get`
  # refuses to parse.
  def ancestor_entry(mod)
    return nil if GEM_PATCHES.include?(mod.to_s)

    if mod.singleton_class?
      attached = mod.respond_to?(:attached_object) ? mod.attached_object : nil
      return ["s:#{attached}", attached] if attached.is_a?(Module)

      name = mod.to_s[/\A#<Class:(.+)>\z/, 1]
      if name
        owner = begin
          Object.const_get(name)
        rescue NameError
          nil
        end
        return ["s:#{name}", owner]
      end
    end
    mod.name ? ["i:#{mod.name}", mod] : nil
  end

  def dump_own(rows, name, kind, klass)
    own_methods(klass, kind).each do |mname, meth|
      rows << [
        "M", name, kind, mname.to_s, meth.arity.to_s,
        visibility_of(klass, kind, mname),
        meth.source_location ? "ruby" : "c",
        params_of(meth),
      ]
    end
  end
end

rows = []
unavailable = []
# Every `(class, kind)` whose own methods have been dumped, so the ancestor
# pass below does not duplicate a manifest class.
dumped = {}
# Ancestor token -> the module to read own methods from, so an ancestor the
# manifest never names still gets `M` rows. Keeps the MODULE, not just the
# token: see `ancestor_entry`.
seen_ancestors = {}

classes.each do |entry|
  name = entry[:name]
  klass = begin
    if RESOLVERS.key?(name)
      RESOLVERS[name].call
    else
      Object.const_get(name)
    end
  rescue NameError, NoMethodError
    reason = entry[:feature] != "-" && missing_features.key?(entry[:feature]) ? "require-failed" : "const-missing"
    unavailable << [name, reason, entry[:feature]]
    next
  end

  next unless klass.is_a?(Module)

  %w[i s].each do |kind|
    chain = []
    (kind == "s" ? klass.singleton_class : klass).ancestors.each do |m|
      token, owner = Dump.ancestor_entry(m)
      next unless token

      chain << token
      seen_ancestors[token] ||= owner
    end
    rows << ["A", name, kind, *chain]

    dumped[[name, kind]] = true
    Dump.dump_own(rows, name, kind, klass)
  end
end

# A method zeo declares may be OWNED by an ancestor the manifest never names --
# every `FFI::Pointer` accessor really lives on `FFI::AbstractMemory`. Without
# its own `M` rows the drift test cannot resolve those names at all and reads
# them as invented, so dump each ancestor's own methods too.
seen_ancestors.each do |token, aklass|
  kind, aname = token.split(":", 2)
  next if dumped[[aname, kind]]
  next unless aklass.is_a?(Module)

  dumped[[aname, kind]] = true
  Dump.dump_own(rows, aname, kind, aklass)
end

out = $stdout
out.puts "#! zeo builtin arity oracle -- @generated by `cargo run -p xtask -- arity-oracle`"
out.puts "#! ruby\t#{RUBY_DESCRIPTION}"
out.puts "#! classes\t#{classes.size}\tresolved\t#{classes.size - unavailable.size}\tunavailable\t#{unavailable.size}"
out.puts "#! sections\tA=ancestry  M=own-method  !=unavailable"

unavailable.sort.each { |name, reason, feature| out.puts ["!", name, reason, feature].join("\t") }
# Sort by the full row so the file is byte-identical across machines and the
# diff of a regeneration shows only real changes.
rows.sort_by { |r| r.map(&:to_s) }.each { |r| out.puts r.join("\t") }
