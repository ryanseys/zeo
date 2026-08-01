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
# loaded, so they are requested unconditionally. So are the three stdlib files
# that ADD to IO -- zeo answers `raw`, `nonblock?` and `wait_readable` without
# a require, so the oracle has to have them loaded for the comparison to be
# about arity rather than about which file defines the method.
missing_features = {}
(features.uniq + %w[yaml weakref io/console io/nonblock io/wait]).each do |feature|
  require feature
rescue LoadError => e
  missing_features[feature] = e.message
end

# Names `Object.const_get` cannot parse. `ARGF` is an object; the class zeo
# models is its singleton.
RESOLVERS = {
  "ARGF.class" => -> { ARGF.class },
}.freeze

# `kind` is `i` for instance methods, `s` for singleton (`def self.`) methods.
def own_methods(klass, kind)
  if kind == "s"
    sing = klass.singleton_class
    names = sing.instance_methods(false) + sing.private_instance_methods(false) +
            sing.protected_instance_methods(false)
    names.uniq.map { |n| [n, sing.instance_method(n)] }
  else
    names = klass.instance_methods(false) + klass.private_instance_methods(false) +
            klass.protected_instance_methods(false)
    names.uniq.map { |n| [n, klass.instance_method(n)] }
  end
end

def visibility_of(klass, kind, name)
  target = kind == "s" ? klass.singleton_class : klass
  return "private" if target.private_instance_methods(false).include?(name)
  return "protected" if target.protected_instance_methods(false).include?(name)

  "public"
end

# `[[:req, :fmt], [:key, :buffer]]` -> `req:fmt,key:buffer`. A C function has no
# parameter names, so this degenerates to `rest` / `req,req` -- which is exactly
# the signal the drift test uses to decide the row carries no shape information.
def params_of(meth)
  parts = meth.parameters.map { |kind, name| name ? "#{kind}:#{name}" : kind.to_s }
  parts.empty? ? "-" : parts.join(",")
end

# An ancestor is either a plain module/class (`i:Enumerable`) or some class's
# singleton (`s:IO`), so instance and singleton resolution read the same way.
def ancestor_token(mod)
  if mod.singleton_class?
    attached = mod.respond_to?(:attached_object) ? mod.attached_object : nil
    return "s:#{attached}" if attached.is_a?(Module)

    name = mod.to_s[/\A#<Class:(.+)>\z/, 1]
    return "s:#{name}" if name
  end
  mod.name ? "i:#{mod.name}" : nil
end

rows = []
unavailable = []
# Every `(class, kind)` whose own methods have been dumped, so the ancestor
# pass below does not duplicate a manifest class.
dumped = {}
# Ancestor tokens seen in a chain, so their own methods can be dumped too.
seen_ancestors = {}

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
    chain = (kind == "s" ? klass.singleton_class : klass).ancestors
                                                         .filter_map { |m| ancestor_token(m) }
    rows << ["A", name, kind, *chain]
    chain.each { |token| seen_ancestors[token] = true }

    dumped[[name, kind]] = true
    dump_own(rows, name, kind, klass)
  end
end

# A method zeo declares may be OWNED by an ancestor the manifest never names --
# every `FFI::Pointer` accessor really lives on `FFI::AbstractMemory`. Without
# its own `M` rows the drift test cannot resolve those names at all and reads
# them as invented, so dump each ancestor's own methods too.
seen_ancestors.each_key do |token|
  kind, aname = token.split(":", 2)
  next if dumped[[aname, kind]]

  aklass = begin
    Object.const_get(aname)
  rescue NameError
    next
  end
  next unless aklass.is_a?(Module)

  dumped[[aname, kind]] = true
  dump_own(rows, aname, kind, aklass)
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
