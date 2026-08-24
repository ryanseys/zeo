# `Method#parameters` and `#arity` on a BUILTIN row.
#
# CRuby reports NO parameter name on most of its C rows, so zeo's default is
# the anonymous descriptor its arity implies. The rows below are the ones ruby
# DOES name -- because it writes them in Ruby (Pathname), or because its C
# function declares a real signature -- and each carries a `ruby def` or a
# `params "..."` spelling in the DSL to say so.
#
# Two rows are asymmetric on purpose:
#   * `Kernel#require` names its parameter and `Kernel.require` does not,
#     because CRuby defines the two separately.
#   * a Struct WRITER's parameter is named `_`, a reader has none.

require "pathname" # a no-op in zeo and in ruby 4.0: the class is already there
require "tmpdir"   # what adds `Pathname.mktmpdir`

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

# ---- Pathname: ruby writes it in Ruby, so every row names its parameters.
%i[+ / == eql? basename children chmod chown cleanpath each_child each_entry
   find fnmatch? glob join mkpath open opendir read relative_path_from rename
   rmtree sub_ext truncate utime write].each do |m|
  um = Pathname.instance_method(m)
  puts "Pathname##{m}: #{um.parameters.inspect} / #{um.arity}"
end
%i[glob mktmpdir pwd].each do |m|
  um = Pathname.method(m)
  puts "Pathname.#{m}: #{um.parameters.inspect} / #{um.arity}"
end
# The private helpers are named too, and stay private.
%i[chop_basename plus prepend_prefix same_paths? split_names].each do |m|
  um = Pathname.instance_method(m)
  puts "Pathname##{m} (private): #{um.parameters.inspect} / #{um.arity}"
end
show("Pathname private set") { Pathname.private_instance_methods(false).sort }
show("Pathname protected set") { Pathname.protected_instance_methods(false).sort }

# ---- Kernel: the seven rows ruby names.
%i[Float Integer Pathname clone pp require warn].each do |m|
  um = Kernel.instance_method(m)
  puts "Kernel##{m}: #{um.parameters.inspect} / #{um.arity}"
end
show("Kernel.require") { [Kernel.method(:require).parameters, Kernel.method(:require).arity] }

# ---- Three builtins whose C function declares a real signature.
show("Dir#initialize") { Dir.instance_method(:initialize).parameters }
show("Hash#initialize") { Hash.instance_method(:initialize).parameters }
show("Time#initialize") { Time.instance_method(:initialize).parameters }

# ---- Every exception class that declares its own `initialize` reports
# `[[:rest]]` and -1: CRuby declares each one `argc = -1`.
[Exception, FrozenError, Interrupt, KeyError, NameError,
 NoMatchingPatternKeyError, NoMethodError, SignalException, SyntaxError,
 SystemCallError, SystemExit, UncaughtThrowError].each do |k|
  um = k.instance_method(:initialize)
  puts "#{k}#initialize: #{um.parameters.inspect} / #{um.arity}"
end

# ---- A Struct accessor.
S = Struct.new(:a, :b)
show("S#a=") { S.instance_method(:a=).parameters }
show("S#a") { S.instance_method(:a).parameters }
D = Data.define(:x)
show("D#x") { D.instance_method(:x).parameters }

# ---- An UNNAMED row still answers the anonymous descriptor, which is ruby's
# own answer for a C method that declares only a count.
show("String#index") { [String.instance_method(:index).parameters, String.instance_method(:index).arity] }
show("Array#push") { [Array.instance_method(:push).parameters, Array.instance_method(:push).arity] }
