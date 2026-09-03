# A `Data` subclass's constructor reports the two frames ruby reports, and its
# `Struct` twin still reports its one.
#
# It was two bugs, and the Struct half being right all along is what narrowed
# both.
#
# THE BODY'S LABEL said `Object#initialize`. A `def` inside a class whose
# superclass is a RUNTIME value (`D = Data.define(:m)`) is lifted to a
# top-level method and installed on the class afterwards, so the emitter bakes
# the only owner it can see. Ruby names the frame from the class's name AT
# RAISE TIME, which is not a compile-time fact at all -- the same anonymous
# class reports a bare `initialize` while held in a local and `K#initialize`
# once a constant assignment names it. So the INSTALLER, which knows the real
# class, hands the label to the next frame push.
#
# That handover is one-shot and replaces only the `Object#` fallback. Both
# narrowings are load-bearing: a top-level `def foo` called from inside such a
# method must keep its own label, and the same install path also carries a
# genuine `define_method` block, whose body keeps its `block in ...` label
# (`tests/define_method_body_frame_label.rb` caught that one).
#
# THE `D.new` FRAME was missing. Ruby pushes one because a Data class's `new`
# is a distinct cfunc -- the one that zips positionals against the member list
# -- where `Struct`'s is the ordinary `Class#new` and pushes none. The label
# names the class that OWNS the cfunc, so a subclass of `D` still reports
# `D.new` rather than its own name.
D = Data.define(:m)

class D2 < D
  def initialize(**kw)
    super
    @z = 1
  end
end

begin
  D2.new(m: 1)
rescue => e
  puts e.backtrace.first(2).map { |l| l.sub(/\A.*?([^\/]+\.rb)/, '\1') }
end

S = Struct.new(:a)
class S2 < S
  def initialize(*args)
    super
    raise "from the subclass"
  end
end
begin
  S2.new(1)
rescue => e
  puts e.backtrace.first(2).map { |l| l.sub(/\A.*?([^\/]+\.rb)/, '\1') }
end

# The sweep.

def frames(label, n = 2)
  yield
rescue => e
  puts "#{label}:"
  puts e.backtrace.first(n).map { |l| "  " + l.sub(%r{\A.*?([^/]+):}, '\1:') }
end

# A subclass of the Data subclass still names the class that owns `new`.
class D3 < D2
  def initialize(**kw)
    super
    raise "deeper"
  end
end
frames("Data sub-subclass") { D3.new(m: 1) }

# A Data class constructed DIRECTLY, with no subclass in between.
DD = Data.define(:v)
frames("Data direct, arity error") { DD.new(1, 2, 3) }

# A named runtime class reports its constant name; the SAME class held in a
# local stays anonymous and reports the bare method name.
Named = Class.new do
  def boom
    raise "named"
  end
end
frames("named runtime class", 1) { Named.new.boom }

anon = Class.new do
  def boom
    raise "anon"
  end
end
frames("anonymous runtime class", 1) { anon.new.boom }

# A subclass of a runtime class, written with `class ... < ...`.
class NamedSub < Named
  def other
    raise "sub"
  end
end
frames("subclass of a runtime class", 1) { NamedSub.new.other }

# The one-shot must not leak: a top-level `def` called from inside a runtime
# class's method keeps its OWN label.
def top_level_helper
  raise "helper"
end
Leaky = Class.new do
  def outer
    top_level_helper
  end
end
frames("top-level def called from a runtime method") { Leaky.new.outer }
__END__
a_struct_constructor_frame_names_the_subclass.rb:32:in 'D2#initialize'
a_struct_constructor_frame_names_the_subclass.rb:37:in 'D.new'
a_struct_constructor_frame_names_the_subclass.rb:46:in 'S2#initialize'
a_struct_constructor_frame_names_the_subclass.rb:50:in '<main>'
Data sub-subclass:
  a_struct_constructor_frame_names_the_subclass.rb:32:in 'D2#initialize'
  a_struct_constructor_frame_names_the_subclass.rb:67:in 'D3#initialize'
Data direct, arity error:
  a_struct_constructor_frame_names_the_subclass.rb:75:in 'DD.new'
  a_struct_constructor_frame_names_the_subclass.rb:75:in 'block in <main>'
named runtime class:
  a_struct_constructor_frame_names_the_subclass.rb:81:in 'Named#boom'
anonymous runtime class:
  a_struct_constructor_frame_names_the_subclass.rb:88:in 'boom'
subclass of a runtime class:
  a_struct_constructor_frame_names_the_subclass.rb:96:in 'NamedSub#other'
top-level def called from a runtime method:
  a_struct_constructor_frame_names_the_subclass.rb:104:in 'Object#top_level_helper'
  a_struct_constructor_frame_names_the_subclass.rb:108:in 'Leaky#outer'
