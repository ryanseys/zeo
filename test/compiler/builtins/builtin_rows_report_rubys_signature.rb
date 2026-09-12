# `Method#parameters` and `#arity` on a BUILTIN row.
#
# CRuby reports NO parameter name on most of its C rows, so zeo's default is
# the anonymous descriptor its arity implies. The rows below are the ones ruby
# DOES name -- because it writes them in Ruby (Pathname), or because its C
# function declares a real signature -- and each carries a `ruby def` or a
# `params "..."` spelling in the DSL to say so.
#
# A Struct WRITER's parameter is named `_`, a reader has none.
#
# `require` is not asked about: the oracle runs under `bundler/setup`, whose
# `bundled_gems.rb` redefines both `Kernel#require` and `Kernel.require` with
# a parameter named `name`, so the recorded answer would be bundler's.

require "pathname" # the ruby half, which adds `Pathname.mktmpdir`
require "tmpdir"   # what adds `Dir.mktmpdir`

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e
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

# ---- Kernel: the rows ruby names.
%i[Float Integer Pathname clone pp warn].each do |m|
  um = Kernel.instance_method(m)
  puts "Kernel##{m}: #{um.parameters.inspect} / #{um.arity}"
end

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
__END__
Pathname#+: [[:req, :other]] / 1
Pathname#/: [[:req, :other]] / 1
Pathname#==: [[:req, :other]] / 1
Pathname#eql?: [[:req, :other]] / 1
Pathname#basename: [[:rest, :*], [:keyrest, :**], [:block, :&]] / -1
Pathname#children: [[:opt, :with_directory]] / -1
Pathname#chmod: [[:req, :mode]] / 1
Pathname#chown: [[:req, :owner], [:req, :group]] / 2
Pathname#cleanpath: [[:opt, :consider_symlink]] / -1
Pathname#each_child: [[:opt, :with_directory], [:block, :b]] / -1
Pathname#each_entry: [[:block, :block]] / 0
Pathname#find: [[:key, :ignore_error]] / -1
Pathname#fnmatch?: [[:req, :pattern], [:rest, :*], [:keyrest, :**], [:block, :&]] / -2
Pathname#glob: [[:rest, :args], [:keyrest, :kwargs]] / -1
Pathname#join: [[:rest, :args]] / -1
Pathname#mkpath: [[:key, :mode]] / -1
Pathname#open: [[:rest, :*], [:keyrest, :**], [:block, :&]] / -1
Pathname#opendir: [[:block, :block]] / 0
Pathname#read: [[:rest, :*], [:keyrest, :**], [:block, :&]] / -1
Pathname#relative_path_from: [[:req, :base_directory]] / 1
Pathname#rename: [[:req, :to]] / 1
Pathname#rmtree: [[:key, :noop], [:key, :verbose], [:key, :secure]] / -1
Pathname#sub_ext: [[:req, :repl]] / 1
Pathname#truncate: [[:req, :length]] / 1
Pathname#utime: [[:req, :atime], [:req, :mtime]] / 2
Pathname#write: [[:rest, :*], [:keyrest, :**], [:block, :&]] / -1
Pathname.glob: [[:rest, :args], [:keyrest, :kwargs]] / -1
Pathname.mktmpdir: [] / 0
Pathname.pwd: [] / 0
Pathname#chop_basename (private): [[:req, :path]] / 1
Pathname#plus (private): [[:req, :path1], [:req, :path2]] / 2
Pathname#prepend_prefix (private): [[:req, :prefix], [:req, :relpath]] / 2
Pathname#same_paths? (private): [[:req, :a], [:req, :b]] / 2
Pathname#split_names (private): [[:req, :path]] / 1
Pathname private set: [:add_trailing_separator, :chop_basename, :cleanpath_aggressive, :cleanpath_conservative, :del_trailing_separator, :has_trailing_separator?, :initialize, :plus, :prepend_prefix, :same_paths?, :split_names]
Pathname protected set: [:path]
Kernel#Float: [[:req, :arg], [:key, :exception]] / -2
Kernel#Integer: [[:req, :arg], [:opt, :base], [:key, :exception]] / -2
Kernel#Pathname: [[:req, :path]] / 1
Kernel#clone: [[:key, :freeze]] / -1
Kernel#pp: [[:rest, :objs]] / -1
Kernel#warn: [[:rest, :msgs], [:key, :uplevel], [:key, :category]] / -1
Dir#initialize: [[:req, :name], [:key, :encoding]]
Hash#initialize: [[:opt, :ifnone], [:key, :capacity], [:block, :block]]
Time#initialize: [[:opt, :year], [:opt, :mon], [:opt, :mday], [:opt, :hour], [:opt, :min], [:opt, :sec], [:opt, :zone], [:key, :in], [:key, :precision]]
Exception#initialize: [[:rest]] / -1
FrozenError#initialize: [[:rest]] / -1
Interrupt#initialize: [[:rest]] / -1
KeyError#initialize: [[:rest]] / -1
NameError#initialize: [[:rest]] / -1
NoMatchingPatternKeyError#initialize: [[:rest]] / -1
NoMethodError#initialize: [[:rest]] / -1
SignalException#initialize: [[:rest]] / -1
SyntaxError#initialize: [[:rest]] / -1
SystemCallError#initialize: [[:rest]] / -1
SystemExit#initialize: [[:rest]] / -1
UncaughtThrowError#initialize: [[:rest]] / -1
S#a=: [[:req, :_]]
S#a: []
D#x: []
String#index: [[[:rest]], -1]
Array#push: [[[:rest]], -1]
