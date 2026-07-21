# Comprehensive Method#arity / #parameters coverage, byte-diffed against the
# ruby oracle. Each line prints the code expression next to its result so a
# spinel-vs-oracle diff points straight at the failing method. Builtin arities
# are declared per-name at each method's `builtin_methods!` definition site
# (mirroring CRuby's `rb_define_method` argc), so this is their regression gate.

def show(label, val) = puts("#{label.ljust(34)} => #{val}")

# --- user def arity: every positional / rest / keyword / block shape ---------
def d0; end
def d1(a); end
def d2(a, b); end
def dopt(a, b = 1); end
def drest(*a); end
def dreq_rest(a, *b); end
def dpost(a, *b, c); end
def dkopt(a, b: 1); end
def dkreq(a, b:); end
def dkmix(a, b:, c: 1); end
def dkrest(a, **k); end
def dkreq_rest(a, b:, **k); end
def dkonly_req(b:); end
def dblk(a, &b); end
def dfwd(...); end
%i[d0 d1 d2 dopt drest dreq_rest dpost dkopt dkreq dkmix dkrest dkreq_rest
   dkonly_req dblk dfwd].each { |m| show("def #{m}", method(m).arity) }

show("method(:dkmix).parameters", method(:dkmix).parameters)
show("method(:dpost).parameters", method(:dpost).parameters)
show("method(:dblk).parameters",  method(:dblk).parameters)

# --- attr + struct + data accessors ------------------------------------------
class C
  attr_accessor :x
  attr_reader :y
  attr_writer :z
end
o = C.new
show("attr_reader x",  o.method(:x).arity)
show("attr_writer x=", o.method(:x=).arity)
show("attr_reader y",  o.method(:y).arity)
show("attr_writer z=", o.method(:z=).arity)
show("UnboundMethod x",  C.instance_method(:x).arity)
show("UnboundMethod x=", C.instance_method(:x=).arity)

S = Struct.new(:a, :b)
s = S.new(1, 2)
show("Struct reader a",  s.method(:a).arity)
show("Struct writer a=", s.method(:a=).arity)

D = Data.define(:m, :n)
dv = D.new(1, 2)
show("Data reader m", dv.method(:m).arity)

# --- Proc / lambda -----------------------------------------------------------
show("proc {|a,b|}.arity",   proc { |a, b| }.arity)
show("->(a,b){}.arity",      ->(a, b) {}.arity)
show("proc {|a,b=1|}.arity", proc { |a, b = 1| }.arity)
show("->(a,*b){}.arity",     ->(a, *b) {}.arity)
show("proc{|a,b|}.params",       proc { |a, b| }.parameters)
show("proc{|a,b|}.params(l:t)",  proc { |a, b| }.parameters(lambda: true))
show("->(a,b){}.params(l:f)",    ->(a, b) {}.parameters(lambda: false))
show("method(:d1).to_proc.arity",   method(:d1).to_proc.arity)
show("method(:d1).to_proc.lambda?", method(:d1).to_proc.lambda?)

# --- builtin arity: one labeled line per method, across many classes ---------
{
  "Integer#+"          => 1.method(:+),
  "Integer#divmod"     => 1.method(:divmod),
  "Integer#gcd"        => 1.method(:gcd),
  "Integer#abs"        => 1.method(:abs),
  "Integer#bit_length" => 1.method(:bit_length),
  "Integer#<=>"        => 1.method(:<=>),
  "Integer#[]"         => 1.method(:[]),
  "Integer#digits"     => 1.method(:digits),
  "Integer#to_s"       => 1.method(:to_s),
  "Integer#pow"        => 1.method(:pow),
  "Integer#<<"         => 1.method(:<<),
  "Integer#coerce"     => 1.method(:coerce),
  "Float#round"        => 1.0.method(:round),
  "Float#ceil"         => 1.0.method(:ceil),
  "Float#nan?"         => 1.0.method(:nan?),
  "Float#divmod"       => 1.0.method(:divmod),
  "Float#step"         => 1.0.method(:step),
  "Float#to_i"         => 1.0.method(:to_i),
  "String#length"      => "s".method(:length),
  "String#upcase"      => "s".method(:upcase),
  "String#+"           => "s".method(:+),
  "String#*"           => "s".method(:*),
  "String#gsub"        => "s".method(:gsub),
  "String#sub"         => "s".method(:sub),
  "String#index"       => "s".method(:index),
  "String#center"      => "s".method(:center),
  "String#tr"          => "s".method(:tr),
  "String#count"       => "s".method(:count),
  "String#replace"     => "s".method(:replace),
  "String#insert"      => "s".method(:insert),
  "String#start_with?" => "s".method(:start_with?),
  "String#<<"          => "s".method(:<<),
  "String#<=>"         => "s".method(:<=>),
  "Array#size"         => [].method(:size),
  "Array#<<"           => [].method(:<<),
  "Array#push"         => [].method(:push),
  "Array#first"        => [].method(:first),
  "Array#map"          => [].method(:map),
  "Array#join"         => [].method(:join),
  "Array#+"            => [].method(:+),
  "Array#index"        => [].method(:index),
  "Array#include?"     => [].method(:include?),
  "Array#insert"       => [].method(:insert),
  "Array#dig"          => [].method(:dig),
  "Array#zip"          => [].method(:zip),
  "Array#rotate"       => [].method(:rotate),
  "Array#fill"         => [].method(:fill),
  "Array#reduce"       => [].method(:reduce),
  "Hash#size"          => {}.method(:size),
  "Hash#merge"         => {}.method(:merge),
  "Hash#fetch"         => {}.method(:fetch),
  "Hash#key?"          => {}.method(:key?),
  "Hash#dig"           => {}.method(:dig),
  "Hash#store"         => {}.method(:store),
  "Hash#invert"        => {}.method(:invert),
  "Range#include?"     => (1..2).method(:include?),
  "Range#cover?"       => (1..2).method(:cover?),
  "Range#step"         => (1..2).method(:step),
  "Range#sum"          => (1..2).method(:sum),
  "Range#first"        => (1..2).method(:first),
  "Range#exclude_end?" => (1..2).method(:exclude_end?),
  "Symbol#to_s"        => :x.method(:to_s),
  "Symbol#length"      => :x.method(:length),
  "Symbol#<=>"         => :x.method(:<=>),
  "Symbol#succ"        => :x.method(:succ),
  "Rational#+"         => Rational(1, 2).method(:+),
  "Rational#numerator" => Rational(1, 2).method(:numerator),
  "Complex#+"          => Complex(1, 2).method(:+),
  "Complex#abs"        => Complex(1, 2).method(:abs),
  "Complex#real"       => Complex(1, 2).method(:real),
  "Regexp#match"       => /a/.method(:match),
  "Regexp#source"      => /a/.method(:source),
  "Regexp#names"       => /a/.method(:names),
  "MatchData#[]"       => "abc".match(/(b)/).method(:[]),
  "MatchData#captures" => "abc".match(/(b)/).method(:captures),
  "NilClass#to_s"      => nil.method(:to_s),
  "NilClass#nil?"      => nil.method(:nil?),
  "TrueClass#&"        => true.method(:&),
  "Integer#clamp"      => 1.method(:clamp),
  "Integer#between?"   => 1.method(:between?),
  "Array#min"          => [1].method(:min),
  "Array#sort_by"      => [1].method(:sort_by),
  "Array#group_by"     => [1].method(:group_by),
  "Array#each_with_index"  => [1].method(:each_with_index),
  "Array#each_with_object" => [1].method(:each_with_object),
  "Array#partition"    => [1].method(:partition),
  "Array#tally"        => [1].method(:tally),
  "Time#year"          => Time.at(0).method(:year),
  "Time#+"             => Time.at(0).method(:+),
  "Time#strftime"      => Time.at(0).method(:strftime),
  "Time#<=>"           => Time.at(0).method(:<=>),
}.each { |label, m| show(label, m.arity) }
