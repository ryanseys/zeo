#!/usr/bin/env ruby
# frozen_string_literal: true

# The transitive method census: walk every `Module` reachable from `Object`'s
# constant tree and dump what it owns. Runs UNCHANGED under both engines --
# `ruby tools/method_census.rb` and `zeo -e "$(cat tools/method_census.rb)"` --
# so the two dumps diff directly.
#
#   cargo run -p xtask -- method-census    # records the oracle side
#   cargo nextest run -p zeo method_census # gates zeo against it
#
# Columns: qualified-name <TAB> kind <TAB> comma-joined sorted names.
#
#   i  public instance, own          I  public instance, inherited too
#   s  public singleton, own         S  public singleton, inherited too
#   p  private instance, own         c  constants, own
#
# The `I`/`S` pair is what makes the dump worth having: without it a missing
# `i` row cannot be told apart from a method zeo answers off a different class,
# and the two need completely different work. `walk` keys on `mod.name`, not
# `object_id` -- an engine that gives every class one id would otherwise stop
# after the first row.

DEPTH = 4

seen = {}
rows = []

walk = lambda do |mod, path, depth|
  return if depth > DEPTH

  key = mod.name || path
  return if seen[key]

  seen[key] = path
  rows << [path, mod]

  names = begin
    mod.constants
  rescue StandardError
    []
  end
  names.sort.each do |n|
    child = begin
      mod.const_get(n, false)
    rescue StandardError
      next
    end
    next unless child.is_a?(Module)

    # Descend only where the child is really NAMED here, which skips
    # `Object::Object` and every re-exported constant.
    child_path = path == "Object" ? n.to_s : "#{path}::#{n}"
    real = begin
      child.name
    rescue StandardError
      nil
    end
    next unless real == child_path

    walk.call(child, child_path, depth + 1)
  end
end

walk.call(Object, "Object", 0)

rows.sort_by! { |path, _| path }
rows.each do |path, mod|
  begin
    dump = {
      "i" => mod.instance_methods(false),
      "s" => mod.singleton_methods(false),
      "p" => mod.private_instance_methods(false),
      "c" => mod.constants(false),
      "I" => mod.instance_methods(true),
      "S" => mod.methods,
    }
  rescue StandardError => e
    puts "#{path}\tERR\t#{e.class}"
    next
  end
  dump.each { |kind, names| puts "#{path}\t#{kind}\t#{names.map(&:to_s).sort.join(',')}" }
end
